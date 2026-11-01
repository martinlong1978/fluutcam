// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local crash reports (design D14 §6, spec 06 §6.4).
//!
//! A panic hook writes a text report (message, location, backtrace,
//! version, operating system) to the crash directory. It holds no project
//! data. At the next start the program tells the user about unread reports
//! and offers to show them. **Nothing is ever sent**: sending is off until
//! the product owner decides on an endpoint and the opt-in flow.

use crate::build_info;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Overrides the data directory (tests, portable installs).
pub const DATA_DIR_ENV: &str = "FLUUT_DATA_DIR";

/// The user data directory for `os` (`std::env::consts::OS`); `env` reads
/// environment variables.
pub fn data_dir_from(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = var(DATA_DIR_ENV) {
        return Some(d);
    }
    match os {
        "windows" => var("LOCALAPPDATA").map(|d| d.join("FluutCAM")),
        "macos" => var("HOME").map(|h| h.join("Library/Application Support/FluutCAM")),
        _ => var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .map(|d| d.join("fluutcam")),
    }
}

/// Where crash reports go: `<data dir>/crashes`.
pub fn crash_dir() -> Option<PathBuf> {
    data_dir_from(std::env::consts::OS, |k| std::env::var_os(k)).map(|d| d.join("crashes"))
}

/// What a crash report says about the panic.
#[derive(Clone, Debug, Default)]
pub struct PanicFacts {
    pub message: String,
    /// `file:line:column`.
    pub location: Option<String>,
    pub thread: Option<String>,
    pub backtrace: String,
}

/// The text of a report.
pub fn format_report(app: &str, unix_time: u64, f: &PanicFacts) -> String {
    format!(
        "FluutCAM crash report\n\
         This file stays on your computer. Nothing in it has been sent anywhere,\n\
         and it holds no project data.\n\
         \n\
         Application: {app}\n\
         Version: {}\n\
         Operating system: {} {}\n\
         Time (Unix seconds): {unix_time}\n\
         Thread: {}\n\
         Panic: {}\n\
         Location: {}\n\
         \n\
         Backtrace:\n{}\n",
        build_info::version_string(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        f.thread.as_deref().unwrap_or("unnamed"),
        f.message,
        f.location.as_deref().unwrap_or("unknown"),
        f.backtrace.trim_end(),
    )
}

/// Writes `text` as a new `crash-<time>-<pid>.txt` in `dir`.
pub fn write_report(dir: &Path, unix_time: u64, text: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("crash-{unix_time}-{}.txt", std::process::id()));
    std::fs::write(&path, text)?;
    Ok(path)
}

fn is_report(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.starts_with("crash-") && n.ends_with(".txt") && !n.ends_with(".seen.txt")
    })
}

/// Reports in `dir` nobody has been told about yet, oldest name first.
pub fn pending(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_report(p))
        .collect();
    v.sort();
    v
}

/// Marks a report as seen by renaming it to `crash-….seen.txt`, so the user
/// is not asked twice. The file stays for the user to keep or delete.
pub fn mark_seen(path: &Path) -> io::Result<PathBuf> {
    let seen = path.with_extension("seen.txt");
    std::fs::rename(path, &seen)?;
    Ok(seen)
}

/// Tells the user about pending reports and, when `ask` is set, offers to
/// print them. All reports are marked seen afterwards. Returns how many
/// there were.
pub fn offer_to_show(
    dir: &Path,
    ask: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> io::Result<usize> {
    let reports = pending(dir);
    if reports.is_empty() {
        return Ok(0);
    }
    writeln!(
        out,
        "fluut: the last run crashed. {} report(s) saved in {}.\n\
         Nothing was sent. You can read or delete the files.",
        reports.len(),
        dir.display()
    )?;
    if ask {
        write!(out, "Show the report now? [y/N] ")?;
        out.flush()?;
        let mut answer = String::new();
        input.read_line(&mut answer)?;
        if answer.trim().eq_ignore_ascii_case("y") {
            for r in &reports {
                writeln!(
                    out,
                    "\n--- {} ---\n{}",
                    r.display(),
                    std::fs::read_to_string(r)?
                )?;
            }
        }
    }
    for r in &reports {
        mark_seen(r)?;
    }
    Ok(reports.len())
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Installs the panic hook for the program `app`. The report goes to `dir`;
/// the previous hook still runs, so the usual panic message is still shown.
pub fn install_in(app: &'static str, dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let message = match info.payload().downcast_ref::<&str>() {
            Some(s) => (*s).to_owned(),
            None => info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "(non-text panic payload)".to_owned()),
        };
        let facts = PanicFacts {
            message,
            location: info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            thread: std::thread::current().name().map(str::to_owned),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        let t = unix_time();
        // A failing write must not turn into a second panic.
        if let Ok(path) = write_report(&dir, t, &format_report(app, t, &facts)) {
            eprintln!(
                "{app}: a crash report was saved to {}. It stays on this computer.",
                path.display()
            );
        }
    }));
}

/// [`install_in`] with the default [`crash_dir`]; does nothing when the
/// data directory is unknown. Returns the directory.
pub fn install(app: &'static str) -> Option<PathBuf> {
    let dir = crash_dir()?;
    install_in(app, dir.clone());
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| m.get(k).map(OsString::from)
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fluut-crash-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn data_dir_per_platform() {
        let e = env(&[("LOCALAPPDATA", "/L"), ("HOME", "/h")]);
        assert_eq!(data_dir_from("windows", &e), Some("/L/FluutCAM".into()));
        assert_eq!(
            data_dir_from("macos", &e),
            Some("/h/Library/Application Support/FluutCAM".into())
        );
        assert_eq!(
            data_dir_from("linux", &e),
            Some("/h/.local/share/fluutcam".into())
        );
        let x = env(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")]);
        assert_eq!(data_dir_from("linux", &x), Some("/x/fluutcam".into()));
        assert_eq!(data_dir_from("linux", env(&[])), None);
    }

    #[test]
    fn override_wins_on_every_platform() {
        let e = env(&[(DATA_DIR_ENV, "/data"), ("HOME", "/h")]);
        assert_eq!(data_dir_from("macos", &e), Some("/data".into()));
        assert_eq!(data_dir_from("windows", env(&[(DATA_DIR_ENV, "")])), None);
    }

    #[test]
    fn report_names_version_system_panic_and_backtrace() {
        let t = format_report(
            "fluut",
            42,
            &PanicFacts {
                message: "boom".into(),
                location: Some("a.rs:1:2".into()),
                thread: Some("main".into()),
                backtrace: "frame 0\nframe 1\n".into(),
            },
        );
        assert!(t.starts_with("FluutCAM crash report\n"));
        assert!(t.contains("Application: fluut\n"));
        assert!(t.contains(&format!("Version: {}\n", build_info::version_string())));
        assert!(t.contains(&format!(
            "Operating system: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
        assert!(t.contains("Time (Unix seconds): 42\n"));
        assert!(t.contains("Thread: main\nPanic: boom\nLocation: a.rs:1:2\n"));
        assert!(t.ends_with("Backtrace:\nframe 0\nframe 1\n"));
        assert!(t.contains("no project data"));
    }

    #[test]
    fn unknown_facts_are_spelled_out() {
        let t = format_report("fluutcam", 0, &PanicFacts::default());
        assert!(t.contains("Thread: unnamed\n"));
        assert!(t.contains("Location: unknown\n"));
    }

    #[test]
    fn pending_lists_unseen_reports_only() {
        let dir = temp("pending");
        let a = write_report(&dir, 1, "a").unwrap();
        write_report(&dir, 2, "b").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert_eq!(pending(&dir).len(), 2);
        let seen = mark_seen(&a).unwrap();
        assert!(seen.to_string_lossy().ends_with(".seen.txt"));
        assert!(seen.is_file() && !a.exists());
        assert_eq!(pending(&dir).len(), 1);
        assert!(pending(&dir.join("missing")).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_pending_prints_nothing() {
        let dir = temp("none");
        let mut out = Vec::new();
        assert_eq!(
            offer_to_show(&dir, true, &mut io::empty(), &mut out).unwrap(),
            0
        );
        assert!(out.is_empty());
    }

    #[test]
    fn offer_prints_the_report_on_yes_and_marks_it_seen() {
        let dir = temp("yes");
        write_report(&dir, 1, "REPORT TEXT").unwrap();
        let mut out = Vec::new();
        let n = offer_to_show(&dir, true, &mut "y\n".as_bytes(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(n, 1);
        assert!(out.contains("Nothing was sent"));
        assert!(out.contains("REPORT TEXT"));
        assert!(pending(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn offer_keeps_the_text_back_on_no_or_when_not_asking() {
        for (ask, input) in [(true, "n\n"), (false, "y\n")] {
            let dir = temp("no");
            write_report(&dir, 1, "REPORT TEXT").unwrap();
            let mut out = Vec::new();
            offer_to_show(&dir, ask, &mut input.as_bytes(), &mut out).unwrap();
            assert!(!String::from_utf8(out).unwrap().contains("REPORT TEXT"));
            assert!(pending(&dir).is_empty(), "told once");
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}
