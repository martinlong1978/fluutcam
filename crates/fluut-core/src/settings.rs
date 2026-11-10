// SPDX-License-Identifier: MIT OR Apache-2.0

//! User settings kept between runs: `<data dir>/settings.toml`.
//!
//! Today this holds the update-check choice (spec 06 §6.4): whether the
//! user was asked, the answer, and when the automatic check last ran. The
//! answer is **no** until the user says yes. A missing or damaged file gives
//! the defaults.

use crate::crash;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// The automatic check runs at most this often, seconds (one day).
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// The update-check part of the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// The user has answered the first-start question.
    pub asked: bool,
    /// Check automatically (at most once a day). Off until the user says yes.
    pub auto_check: bool,
    /// Unix time of the last check, automatic or asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_check: Option<u64>,
}

impl UpdateSettings {
    /// The first-start question has not been answered yet.
    pub fn needs_prompt(&self) -> bool {
        !self.asked
    }

    /// Stores the answer to "Check for updates automatically?". Also used by
    /// the settings toggle.
    pub fn answer(&mut self, yes: bool) {
        self.asked = true;
        self.auto_check = yes;
    }

    /// An automatic check is due at `now` (Unix seconds): the user said yes
    /// and the last check was a day or more ago. A last check in the future
    /// (the clock was set back) counts as due, so the check cannot stall.
    pub fn auto_check_due(&self, now: u64) -> bool {
        self.asked
            && self.auto_check
            && self
                .last_check
                .is_none_or(|t| t > now || now - t >= CHECK_INTERVAL_SECS)
    }

    /// Notes a check at `now`.
    pub fn record_check(&mut self, now: u64) {
        self.last_check = Some(now);
    }
}

/// The first-project guide part of the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuideSettings {
    /// The user closed the "Getting started" card. Shown until then.
    pub dismissed: bool,
}

/// All settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub update: UpdateSettings,
    pub guide: GuideSettings,
}

impl Settings {
    /// Reads the file; a missing or damaged one gives the defaults.
    pub fn load_from(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Writes the file through a temporary file, so a crash cannot leave a
    /// half-written one.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

/// Where the settings are: `<data dir>/settings.toml`.
pub fn settings_path() -> Option<PathBuf> {
    crash::data_dir_from(std::env::consts::OS, |k| std::env::var_os(k))
        .map(|d| d.join("settings.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fluut-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("settings.toml")
    }

    #[test]
    fn update_check_is_off_and_unasked_by_default() {
        let s = Settings::default();
        assert!(!s.update.auto_check);
        assert!(!s.update.asked);
        assert!(s.update.needs_prompt());
        // Never due before the user has said yes.
        assert!(!s.update.auto_check_due(1_000_000));
    }

    #[test]
    fn a_no_is_remembered_and_never_due() {
        let mut u = UpdateSettings::default();
        u.answer(false);
        assert!(!u.needs_prompt());
        assert!(!u.auto_check);
        assert!(!u.auto_check_due(u64::MAX));
    }

    #[test]
    fn a_yes_is_due_at_once_then_once_a_day() {
        let mut u = UpdateSettings::default();
        u.answer(true);
        assert!(u.auto_check_due(1_000));
        u.record_check(1_000);
        assert!(!u.auto_check_due(1_000));
        assert!(!u.auto_check_due(1_000 + CHECK_INTERVAL_SECS - 1));
        assert!(u.auto_check_due(1_000 + CHECK_INTERVAL_SECS));
    }

    #[test]
    fn turning_it_off_stops_the_checks() {
        let mut u = UpdateSettings::default();
        u.answer(true);
        u.answer(false);
        assert!(!u.auto_check_due(u64::MAX));
    }

    #[test]
    fn a_clock_set_back_does_not_stall_the_check() {
        let mut u = UpdateSettings::default();
        u.answer(true);
        u.record_check(5_000_000);
        assert!(u.auto_check_due(1_000));
    }

    #[test]
    fn the_guide_is_shown_until_dismissed_and_the_choice_is_kept() {
        let path = temp_file("guide");
        let mut s = Settings::default();
        assert!(!s.guide.dismissed);
        s.guide.dismissed = true;
        s.save_to(&path).unwrap();
        assert!(Settings::load_from(&path).guide.dismissed);
        // A file from before the guide existed still loads.
        std::fs::write(
            &path,
            "[update]
asked = true
",
        )
        .unwrap();
        let old = Settings::load_from(&path);
        assert!(old.update.asked && !old.guide.dismissed);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let path = temp_file("roundtrip");
        let mut s = Settings::default();
        s.update.answer(true);
        s.update.record_check(1234);
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), s);
        // A first save without a last check also round-trips.
        let mut t = Settings::default();
        t.update.answer(false);
        t.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), t);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_or_damaged_file_gives_the_defaults() {
        let path = temp_file("damaged");
        assert_eq!(Settings::load_from(&path), Settings::default());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "update = [not toml").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        // Unknown keys are ignored; missing ones are defaulted.
        std::fs::write(&path, "future = 1\n[update]\nasked = true\n").unwrap();
        let s = Settings::load_from(&path);
        assert!(s.update.asked && !s.update.auto_check);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
