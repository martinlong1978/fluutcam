// SPDX-License-Identifier: MIT OR Apache-2.0

//! The update check in the app (spec 06 §6.4, plan 2026-11-03).
//!
//! The check is **opt-in**: the user is asked once on first start, the
//! answer is stored in the settings, and without a yes nothing is requested
//! unless the user presses "Check now". An automatic check runs at most once
//! a day. Each check runs on its own short-lived thread, never on the UI
//! thread, and the result comes back through a channel the UI polls. The
//! calculation worker is not used, so a slow network never delays a job.
//!
//! Like `session`, this module is where the app calls `fluut-core`.

use fluut_core::settings::{Settings, UpdateSettings, settings_path};
use fluut_core::update::{self, Channel, Offer, Outcome, UpdateError, UreqFetch};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{SystemTime, UNIX_EPOCH};

/// What the last check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// No check has run in this session.
    Idle,
    Checking,
    UpToDate,
    Available(Offer),
    /// This build has no update key.
    NoKey,
    Failed(String),
}

/// How a check is done; tests give one that never uses the network.
pub type Checker = Arc<dyn Fn(Channel) -> Result<Outcome, UpdateError> + Send + Sync>;

/// Unix time now, seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Settings, the first-start question, the throttle and the running check.
pub struct Updates {
    settings: Settings,
    /// Where the settings are saved; `None` keeps them in memory only.
    path: Option<PathBuf>,
    checker: Checker,
    clock: fn() -> u64,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// `false` for a window that never asks and never checks (tests).
    active: bool,
    /// This build can verify a manifest. Without a key the question would
    /// promise something the build cannot do, so it is not asked.
    has_key: bool,
    state: UpdateState,
    rx: Option<Receiver<Result<Outcome, UpdateError>>>,
}

impl std::fmt::Debug for Updates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updates")
            .field("settings", &self.settings)
            .field("active", &self.active)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Updates {
    /// No question, no checks, nothing stored: the state of a window built
    /// for tests.
    pub fn off() -> Updates {
        Updates::with(
            Settings::default(),
            None,
            Arc::new(|_| Err(UpdateError::NoKey)),
            now,
            Arc::new(|| {}),
            false,
        )
    }

    /// The real thing: settings from the data directory and the network
    /// check of this build. `wake` repaints the window when a check ends.
    pub fn load(wake: impl Fn() + Send + Sync + 'static) -> Updates {
        let path = settings_path();
        let settings = path.as_deref().map(Settings::load_from).unwrap_or_default();
        let checker: Checker = Arc::new(|c| update::check(&UreqFetch::default(), c));
        Updates::with(settings, path, checker, now, Arc::new(wake), true)
    }

    /// Everything given; the tests' entry point.
    pub fn with(
        settings: Settings,
        path: Option<PathBuf>,
        checker: Checker,
        clock: fn() -> u64,
        wake: Arc<dyn Fn() + Send + Sync>,
        active: bool,
    ) -> Updates {
        Updates {
            settings,
            path,
            checker,
            clock,
            wake,
            active,
            has_key: update::has_key(),
            state: UpdateState::Idle,
            rx: None,
        }
    }

    /// Pretends the build has (or lacks) an update key, for tests.
    pub fn set_has_key(&mut self, has_key: bool) {
        self.has_key = has_key;
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    /// The check is turned on for this window.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// This build can verify a release list.
    pub fn has_key(&self) -> bool {
        self.has_key
    }

    pub fn is_checking(&self) -> bool {
        self.rx.is_some()
    }

    /// The user closed the "Getting started" card; it stays closed at the
    /// next start.
    pub fn guide_dismissed(&self) -> bool {
        self.settings.guide.dismissed
    }

    /// Remembers whether the "Getting started" card was closed.
    pub fn set_guide_dismissed(&mut self, dismissed: bool) {
        if self.settings.guide.dismissed != dismissed {
            self.settings.guide.dismissed = dismissed;
            self.save();
        }
    }

    /// The automatic-check choice as stored.
    pub fn auto_check(&self) -> bool {
        self.settings.update.auto_check
    }

    /// The release channel as stored.
    pub fn channel(&self) -> Channel {
        self.settings.update.channel
    }

    /// Chooses the release channel. A result of the old channel is dropped,
    /// because it says nothing about the new one.
    pub fn set_channel(&mut self, channel: Channel) {
        if self.settings.update.channel != channel {
            self.settings.update.channel = channel;
            if !self.is_checking() {
                self.state = UpdateState::Idle;
            }
            self.save();
        }
    }

    /// The first-start question is due: active, a key in the build, and not
    /// answered yet.
    pub fn needs_prompt(&self) -> bool {
        self.active && self.has_key && self.settings.update.needs_prompt()
    }

    /// The answer to the first-start question, or the settings toggle. A
    /// dismissed question is a `false`.
    pub fn answer(&mut self, yes: bool) {
        self.settings.update.answer(yes);
        self.save();
    }

    /// Starts a check the user asked for. Allowed whatever the automatic
    /// setting is. Ignored while one runs.
    pub fn check_now(&mut self) {
        if self.active {
            self.start();
        }
    }

    /// Called every frame: collects a finished check and starts an
    /// automatic one when it is due (opted in, a day since the last, and
    /// the question is not open).
    pub fn tick(&mut self) {
        self.poll();
        if self.active
            && self.rx.is_none()
            && !self.needs_prompt()
            && self.settings.update.auto_check_due((self.clock)())
        {
            self.start();
        }
    }

    fn start(&mut self) {
        if self.rx.is_some() {
            return;
        }
        // Noted before the request, so a failing check is not retried until
        // tomorrow.
        UpdateSettings::record_check(&mut self.settings.update, (self.clock)());
        self.save();
        self.state = UpdateState::Checking;
        let (tx, rx) = channel();
        let checker = Arc::clone(&self.checker);
        let channel = self.settings.update.channel;
        let wake = Arc::clone(&self.wake);
        let spawned = std::thread::Builder::new()
            .name("fluut-update".into())
            .spawn(move || {
                // The UI may be gone by now.
                let _ = tx.send(checker(channel));
                wake();
            });
        match spawned {
            Ok(_) => self.rx = Some(rx),
            Err(e) => self.state = UpdateState::Failed(e.to_string()),
        }
    }

    /// Takes the result of a finished check. Returns `true` if the state
    /// changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => {
                Err(UpdateError::Fetch("the check stopped unexpectedly".into()))
            }
        };
        self.rx = None;
        self.state = match result {
            Ok(Outcome::UpToDate) => UpdateState::UpToDate,
            Ok(Outcome::Available(o)) => UpdateState::Available(o),
            Err(UpdateError::NoKey) => UpdateState::NoKey,
            Err(e) => UpdateState::Failed(e.to_string()),
        };
        true
    }

    fn save(&self) {
        if let Some(p) = &self.path {
            // A settings file that cannot be written must not stop the app;
            // the choice then lasts for this session.
            let _ = self.settings.save_to(p);
        }
    }
}

#[cfg(test)]
mod tests;
