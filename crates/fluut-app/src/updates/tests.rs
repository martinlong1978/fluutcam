// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_core::update::Offer;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The clock the tests move. Tests that use it run one after another.
static CLOCK: AtomicU64 = AtomicU64::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

fn clock() -> u64 {
    CLOCK.load(Ordering::SeqCst)
}

fn offer() -> Offer {
    Offer {
        version: "2.0.0".into(),
        date: "2026-12-01".into(),
        notes_url: "https://example.org/notes".into(),
        download_page: "https://example.org/dl".into(),
        artefacts: Vec::new(),
    }
}

/// A checker that counts its calls and answers `result`.
fn counting(result: Result<Outcome, UpdateError>) -> (Checker, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&n);
    let checker: Checker = Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        result.clone()
    });
    (checker, n)
}

fn updates(checker: Checker, settings: Settings, path: Option<PathBuf>) -> Updates {
    let mut u = Updates::with(settings, path, checker, clock, Arc::new(|| {}), true);
    u.set_has_key(true);
    u
}

/// Waits for the check thread, as the UI does over several frames.
fn settle(u: &mut Updates) {
    let t = Instant::now();
    while u.is_checking() {
        u.poll();
        assert!(t.elapsed() < Duration::from_secs(10), "check did not end");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn opted_in() -> Settings {
    let mut s = Settings::default();
    s.update.answer(true);
    s
}

fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fluut-updates-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("settings.toml")
}

#[test]
fn off_never_asks_and_never_checks() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut u = Updates::off();
    assert!(!u.needs_prompt());
    u.tick();
    u.check_now();
    assert!(!u.is_checking());
    assert_eq!(*u.state(), UpdateState::Idle);
}

#[test]
fn a_new_user_is_asked_and_nothing_is_checked_meanwhile() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let (checker, calls) = counting(Ok(Outcome::UpToDate));
    let mut u = updates(checker, Settings::default(), None);
    assert!(u.needs_prompt());
    assert!(!u.auto_check(), "opt-in: the default is off");
    for _ in 0..5 {
        u.tick();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*u.state(), UpdateState::Idle);
}

#[test]
fn a_build_without_a_key_does_not_ask() {
    let (checker, _) = counting(Ok(Outcome::UpToDate));
    let mut u = updates(checker, Settings::default(), None);
    u.set_has_key(false);
    assert!(!u.needs_prompt());
}

#[test]
fn no_stays_off_and_is_remembered_across_restarts() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let path = temp_path("no");
    let (checker, calls) = counting(Ok(Outcome::UpToDate));
    let mut u = updates(checker.clone(), Settings::default(), Some(path.clone()));
    u.answer(false);
    assert!(!u.needs_prompt());
    u.tick();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // A new start reads the file: asked, off.
    let mut again = updates(checker, Settings::load_from(&path), Some(path.clone()));
    assert!(!again.needs_prompt());
    assert!(!again.auto_check());
    again.tick();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn yes_checks_at_once_off_the_ui_thread_and_then_once_a_day() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let calls = Arc::new(AtomicUsize::new(0));
    let thread = Arc::new(Mutex::new(None));
    let (c, t) = (Arc::clone(&calls), Arc::clone(&thread));
    let checker: Checker = Arc::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        *t.lock().unwrap() = std::thread::current().name().map(str::to_owned);
        Ok(Outcome::Available(offer()))
    });
    let path = temp_path("yes");
    let mut u = updates(checker, Settings::default(), Some(path.clone()));
    u.answer(true);
    u.tick();
    assert!(u.is_checking() || *u.state() != UpdateState::Idle);
    settle(&mut u);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*u.state(), UpdateState::Available(offer()));
    assert_eq!(thread.lock().unwrap().as_deref(), Some("fluut-update"));
    // The same day: no more checks, however often the UI ticks.
    for _ in 0..5 {
        u.tick();
    }
    CLOCK.store(1_000_000 + 86_399, Ordering::SeqCst);
    u.tick();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The time of the check was stored, so a restart does not check again.
    let mut again = updates(
        Arc::new(|_| panic!("checked again after a restart")),
        Settings::load_from(&path),
        Some(path.clone()),
    );
    again.tick();
    // A day later it is due.
    CLOCK.store(1_000_000 + 86_400, Ordering::SeqCst);
    u.tick();
    settle(&mut u);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_manual_check_runs_even_when_automatic_checks_are_off() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let (checker, calls) = counting(Ok(Outcome::UpToDate));
    let mut u = updates(checker, Settings::default(), None);
    u.answer(false);
    u.check_now();
    settle(&mut u);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*u.state(), UpdateState::UpToDate);
    // It did not turn the automatic check on.
    assert!(!u.auto_check());
    u.tick();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_second_request_while_checking_is_ignored() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let release = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (r, c) = (Arc::clone(&release), Arc::clone(&calls));
    let checker: Checker = Arc::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        while !r.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(Outcome::UpToDate)
    });
    let mut u = updates(checker, opted_in(), None);
    u.check_now();
    u.check_now();
    u.tick();
    assert_eq!(*u.state(), UpdateState::Checking);
    // The UI is not blocked: polling returns at once.
    assert!(!u.poll());
    release.store(true, Ordering::SeqCst);
    settle(&mut u);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_check_shows_why_and_is_not_retried_the_same_day() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let (checker, calls) = counting(Err(UpdateError::Fetch("timed out".into())));
    let mut u = updates(checker, opted_in(), None);
    u.tick();
    settle(&mut u);
    let UpdateState::Failed(why) = u.state() else {
        panic!("expected a failure, got {:?}", u.state());
    };
    assert!(why.contains("timed out"), "{why}");
    u.tick();
    u.tick();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_build_without_a_key_reports_it() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let (checker, _) = counting(Err(UpdateError::NoKey));
    let mut u = updates(checker, Settings::default(), None);
    u.set_has_key(false);
    u.check_now();
    settle(&mut u);
    assert_eq!(*u.state(), UpdateState::NoKey);
}

#[test]
fn a_check_that_panics_is_a_failure_not_a_hang() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let checker: Checker = Arc::new(|_| panic!("test panic in the check thread"));
    let mut u = updates(checker, opted_in(), None);
    u.check_now();
    settle(&mut u);
    assert!(matches!(u.state(), UpdateState::Failed(_)));
}

#[test]
fn the_window_is_woken_when_a_check_ends() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let woken = Arc::new(AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    let (checker, _) = counting(Ok(Outcome::UpToDate));
    let mut u = Updates::with(
        opted_in(),
        None,
        checker,
        clock,
        Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        }),
        true,
    );
    u.check_now();
    settle(&mut u);
    let t = Instant::now();
    while woken.load(Ordering::SeqCst) == 0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(woken.load(Ordering::SeqCst), 1);
}

#[test]
fn the_check_gets_the_chosen_channel_and_a_change_is_kept_and_clears_the_result() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    CLOCK.store(1_000_000, Ordering::SeqCst);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = Arc::clone(&seen);
    let checker: Checker = Arc::new(move |c| {
        s.lock().unwrap().push(c);
        Ok(Outcome::UpToDate)
    });
    let path = temp_path("channel");
    let mut u = updates(checker, opted_in(), Some(path.clone()));
    assert_eq!(u.channel(), Channel::Stable);
    u.check_now();
    settle(&mut u);
    assert_eq!(*u.state(), UpdateState::UpToDate);
    // Another channel: the old answer says nothing about it.
    u.set_channel(Channel::Beta);
    assert_eq!(*u.state(), UpdateState::Idle);
    u.check_now();
    settle(&mut u);
    assert_eq!(*seen.lock().unwrap(), [Channel::Stable, Channel::Beta]);
    // Kept for the next start.
    assert_eq!(Settings::load_from(&path).update.channel, Channel::Beta);
    let _ = std::fs::remove_file(&path);
}
