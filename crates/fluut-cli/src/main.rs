// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut`: calculate and post FluutCAM projects from the command line.
//!
//! ```text
//! fluut calc <project> [--tools <file>]...
//! fluut sim <project> [--tools <file>]... [--error <mm>]
//! fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <d>] [--time <t>]
//! fluut run <script> [<project>] [--tools <file>]... [--date <d>] [--time <t>] [-- <args>...]
//! fluut update-check [--channel stable|beta]
//! ```
//!
//! `calc` prints each toolpath's estimated machining time. `sim`
//! calculates, then simulates each Setup and verifies it (D11): it prints
//! the findings and fails when one is an error.
//!
//! `run` runs a script headless (spec 09 §9.2, D12 §4). It may read and
//! write the project's folder and the paths given after `--`, and import
//! modules from the project's `scripts/`. Its exit status is
//! `fluut.exit(n)`, 0, or 1 when it fails.
//!
//! `update-check` asks for the signed release list once, because you typed
//! it (spec 06 §6.4). Nothing runs it automatically, and it downloads and
//! installs nothing. Exit status: 0 when up to date or an update exists, 1
//! when the check failed, 3 when this build has no update key. The channel is
//! the one in the settings (`stable` unless chosen otherwise) unless
//! `--channel` says otherwise (D17).
//!
//! Exit status: 0 on success, 1 when a diagnostic is an error, 2 on bad
//! arguments.

use fluut_core::settings::{Settings, settings_path};
use fluut_core::update::{self, Channel, Outcome, UpdateError};
use fluut_core::{JobReport, JobState, PostOptions, Session, Targets};
use fluut_model::{Diagnostic, Severity};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  fluut calc <project> [--tools <file>]...
  fluut sim <project> [--tools <file>]... [--error <mm>]
  fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>]
  fluut run <script> [<project>] [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>] [-- <args>...]
  fluut update-check [--channel stable|beta]";

#[derive(Debug, Default)]
struct Args {
    command: String,
    project: PathBuf,
    tools: Vec<PathBuf>,
    post: Option<String>,
    output: Option<PathBuf>,
    date: Option<String>,
    time: Option<String>,
    /// Simulation error target, mm.
    error: Option<f64>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        command: it.next().ok_or("no command")?,
        ..Args::default()
    };
    let mut project = None;
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--tools" => a.tools.push(value("--tools")?.into()),
            "--post" => a.post = Some(value("--post")?),
            "-o" | "--output" => a.output = Some(value("-o")?.into()),
            "--date" => a.date = Some(value("--date")?),
            "--time" => a.time = Some(value("--time")?),
            "--error" => {
                let v = value("--error")?;
                a.error = Some(
                    v.parse()
                        .ok()
                        .filter(|e: &f64| *e > 0.0)
                        .ok_or(format!("--error needs a positive number, not {v}"))?,
                );
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if project.is_none() => project = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    a.project = project.ok_or("no project file")?;
    Ok(a)
}

/// Installs the crash-report hook and tells the user about reports from an
/// earlier crash (D14 §6). Nothing is sent anywhere.
fn crash_reports() {
    use std::io::IsTerminal;
    let Some(dir) = fluut_core::crash::install("fluut") else {
        return;
    };
    let ask = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let _ = fluut_core::crash::offer_to_show(
        &dir,
        ask,
        &mut std::io::stdin().lock(),
        &mut std::io::stderr(),
    );
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    match args.peek().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("fluut {}", fluut_core::build_info::version_string());
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    crash_reports();
    if args.peek().map(String::as_str) == Some("update-check") {
        args.next();
        let saved = settings_path()
            .as_deref()
            .map(Settings::load_from)
            .unwrap_or_default()
            .update
            .channel;
        let check = |c| update::check(&update::UreqFetch::default(), c);
        let code = update_check(
            args,
            saved,
            check,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        return ExitCode::from(code);
    }
    if args.peek().map(String::as_str) == Some("run") {
        args.next();
        return match parse_run(args) {
            Ok(r) => run_script(&r),
            Err(e) => {
                eprintln!("fluut: {e}\n{USAGE}");
                ExitCode::from(2)
            }
        };
    }
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("fluut: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match a.command.as_str() {
        "calc" => calc(&a),
        "sim" => sim(&a),
        "post" => post(&a),
        c => {
            eprintln!("fluut: unknown command {c}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Arguments of `fluut run`.
#[derive(Debug, Default)]
struct RunArgs {
    script: PathBuf,
    project: Option<PathBuf>,
    tools: Vec<PathBuf>,
    date: Option<String>,
    time: Option<String>,
    /// After `--`: `fluut.args`, and paths the script may use.
    args: Vec<String>,
}

fn parse_run(mut it: impl Iterator<Item = String>) -> Result<RunArgs, String> {
    let mut a = RunArgs::default();
    let mut script = None;
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--" => {
                a.args = it.by_ref().collect();
                break;
            }
            "--tools" => a.tools.push(value("--tools")?.into()),
            "--project" => a.project = Some(value("--project")?.into()),
            "--date" => a.date = Some(value("--date")?),
            "--time" => a.time = Some(value("--time")?),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if script.is_none() => script = Some(PathBuf::from(s)),
            s if a.project.is_none() => a.project = Some(PathBuf::from(s)),
            s => {
                return Err(format!(
                    "unexpected argument {s}; script arguments go after --"
                ));
            }
        }
    }
    a.script = script.ok_or("no script file")?;
    Ok(a)
}

fn run_script(a: &RunArgs) -> ExitCode {
    let source = match std::fs::metadata(&a.script).and_then(|m| {
        if m.len() > fluut_script::run::MAX_SOURCE_BYTES as u64 {
            Err(std::io::Error::other("the script is too large"))
        } else {
            std::fs::read_to_string(&a.script)
        }
    }) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fluut: cannot read {}: {e}", a.script.display());
            return ExitCode::from(1);
        }
    };
    let open = fluut_core::OpenOptions {
        tool_files: a.tools.clone(),
        ..fluut_core::OpenOptions::default()
    };
    let session = match &a.project {
        None => None,
        Some(p) => match Session::open_with(p, &open) {
            Ok(s) => {
                print_diags(&s.warnings);
                Some(s)
            }
            Err(d) => {
                print_diags(&d);
                return ExitCode::from(1);
            }
        },
    };
    // A project given as a bare file name has an empty folder: that is the
    // current folder.
    let project_dir = session.as_ref().map(|s| {
        absolute(if s.dir.as_os_str().is_empty() {
            std::path::Path::new(".")
        } else {
            &s.dir
        })
    });
    let (date, time) = now_utc();
    let name = a
        .script
        .file_name()
        .map_or_else(|| "script".into(), |n| n.to_string_lossy().into_owned());
    let opts = fluut_script::RunOptions {
        name: name.clone(),
        permissions: fluut_script::Permissions {
            library_dirs: project_dir.iter().cloned().collect(),
            project_dir,
            allowed_paths: a
                .args
                .iter()
                .map(|x| absolute(std::path::Path::new(x)))
                .collect(),
            ui: false,
        },
        open,
        args: a.args.clone(),
        date: a.date.clone().unwrap_or(date),
        time: a.time.clone().unwrap_or(time),
        ..fluut_script::RunOptions::default()
    };
    let host = std::sync::Arc::new(fluut_script::Console::echo());
    let out = fluut_script::run(&source, session, opts, host);
    if let Err(e) = &out.result {
        match e.line {
            Some(l) => eprintln!("fluut: {name}:{l}: {}", e.message),
            None => eprintln!("fluut: {name}: {}", e.message),
        }
    }
    ExitCode::from(out.exit_code.clamp(0, 255) as u8)
}

fn absolute(p: &std::path::Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn print_diags(d: &[Diagnostic]) -> bool {
    for x in d {
        eprintln!("{x}");
    }
    d.iter().any(|x| x.severity == Severity::Error)
}

fn open(a: &Args) -> Result<Session, ExitCode> {
    match Session::open(&a.project, &a.tools) {
        Ok(s) => {
            if print_diags(&s.warnings) {
                return Err(ExitCode::from(1));
            }
            Ok(s)
        }
        Err(d) => {
            print_diags(&d);
            Err(ExitCode::from(1))
        }
    }
}

fn report(s: &Session, r: &JobReport) {
    let name = s
        .project
        .toolpath(&r.toolpath)
        .map_or("", |(_, t)| t.name.as_str());
    let state = match r.state {
        JobState::Done { cached: false } => "calculated",
        JobState::Done { cached: true } => "up to date",
        JobState::Failed => "failed",
        JobState::Skipped => "disabled",
    };
    let time = s
        .time_estimate(&r.toolpath)
        .map_or_else(String::new, |t| format!(", time {}", clock(t)));
    match &r.stats {
        Some(st) => println!(
            "{} \"{name}\": {state}; cut {:.1} mm, rapid {:.1} mm{time}",
            r.toolpath, st.cut_length, st.rapid_length
        ),
        None => println!("{} \"{name}\": {state}", r.toolpath),
    }
}

fn calc(a: &Args) -> ExitCode {
    let mut s = match open(a) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let reports = s.calculate(&Targets::All);
    let mut failed = false;
    for r in &reports {
        report(&s, r);
        failed |= print_diags(&r.diagnostics);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `m:ss`, or `h:mm:ss` from an hour, rounded to the second.
fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn sim(a: &Args) -> ExitCode {
    let mut s = match open(a) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let mut failed = false;
    for r in &s.calculate(&Targets::All) {
        report(&s, r);
        failed |= print_diags(&r.diagnostics);
    }
    if failed {
        return ExitCode::from(1);
    }
    let settings = fluut_core::SimSettings {
        error: a.error.unwrap_or(fluut_core::SimSettings::default().error),
        ..fluut_core::SimSettings::default()
    };
    let ids: Vec<(String, String)> = s
        .project
        .setups
        .iter()
        .map(|x| (x.id.clone(), x.name.clone()))
        .collect();
    for (id, name) in ids {
        let t = std::time::Instant::now();
        match s.simulate(&id, &settings, &fluut_core::fluut_sim::Quiet) {
            Ok(r) => {
                let sim = &r.sim;
                let rest = sim.rest.unwrap_or_default();
                println!(
                    "{id} \"{name}\": simulated at {} mm in {:.2} s; {} toolpaths, time {}; rest {:.1} mm³ on the part (up to {:.3} mm), {:.1} mm³ off it",
                    sim.res,
                    t.elapsed().as_secs_f64(),
                    sim.toolpaths.len(),
                    clock(sim.duration()),
                    rest.on_part,
                    rest.max,
                    rest.off_part,
                );
                failed |= print_diags(&r.diagnostics);
            }
            Err(d) => failed |= print_diags(&d),
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn post(a: &Args) -> ExitCode {
    let (Some(id), Some(out)) = (&a.post, &a.output) else {
        eprintln!("fluut: post needs --post <id> and -o <file>\n{USAGE}");
        return ExitCode::from(2);
    };
    let mut s = match open(a) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let (date, time) = now_utc();
    let opts = PostOptions {
        date: a.date.clone().unwrap_or(date),
        time: a.time.clone().unwrap_or(time),
        notes: String::new(),
    };
    // Built-in posts, then the project's `posts/` (D06 §4.1).
    let store = s.library.mem_store(fluut_model::library::DefKind::Post);
    match s.post(id, &store, &opts) {
        Ok((output, reports)) => {
            for r in &reports {
                report(&s, r);
                print_diags(&r.diagnostics);
            }
            for w in &output.warnings {
                eprintln!("warning: post: {w}");
            }
            for (i, text) in output.files.iter().enumerate() {
                let path = if i == 0 {
                    out.clone()
                } else {
                    numbered(out, i + 1)
                };
                if let Err(e) = std::fs::write(&path, text) {
                    eprintln!("fluut: cannot write {}: {e}", path.display());
                    return ExitCode::from(1);
                }
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(d) => {
            print_diags(&d);
            ExitCode::from(1)
        }
    }
}

/// `out.ngc` → `out.2.ngc` for the files after the first.
fn numbered(p: &std::path::Path, n: usize) -> PathBuf {
    let stem = p
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let name = match p.extension() {
        Some(e) => format!("{stem}.{n}.{}", e.to_string_lossy()),
        None => format!("{stem}.{n}"),
    };
    p.with_file_name(name)
}

/// The current UTC date and time, for the output header.
fn now_utc() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        format!("{y:04}-{m:02}-{d:02}"),
        format!("{:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments() {
        let a = parse(
            [
                "post",
                "p.fluut",
                "--post",
                "fluut/grbl",
                "-o",
                "x.nc",
                "--tools",
                "t.toml",
            ]
            .into_iter()
            .map(String::from),
        )
        .unwrap();
        assert_eq!(a.post.as_deref(), Some("fluut/grbl"));
        assert_eq!(a.tools, [PathBuf::from("t.toml")]);
        assert!(parse(["calc"].into_iter().map(String::from)).is_err());
        assert!(parse(["calc", "a", "b"].into_iter().map(String::from)).is_err());
        assert!(parse(["calc", "a", "--x"].into_iter().map(String::from)).is_err());
        let a = parse(
            ["sim", "p.fluut", "--error", "0.05"]
                .into_iter()
                .map(String::from),
        );
        assert_eq!(a.unwrap().error, Some(0.05));
        assert!(parse(["sim", "p", "--error", "-1"].into_iter().map(String::from)).is_err());
        assert_eq!(clock(59.6), "1:00");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(
            numbered(std::path::Path::new("d/out.ngc"), 2),
            PathBuf::from("d/out.2.ngc")
        );
    }
}

/// `fluut update-check`: one check the user asked for, printed. Returns the
/// exit status.
fn update_check(
    mut args: impl Iterator<Item = String>,
    saved: Channel,
    check: impl FnOnce(Channel) -> Result<Outcome, UpdateError>,
    out: &mut impl Write,
    err: &mut impl Write,
) -> u8 {
    let mut channel = saved;
    while let Some(a) = args.next() {
        if a == "--channel" {
            match args.next().as_deref().and_then(Channel::parse) {
                Some(c) => channel = c,
                None => {
                    let _ = writeln!(err, "fluut: --channel needs stable or beta\n{USAGE}");
                    return 2;
                }
            }
        } else {
            let _ = writeln!(err, "fluut: unexpected argument {a}\n{USAGE}");
            return 2;
        }
    }
    let version = fluut_core::build_info::VERSION;
    match check(channel) {
        Ok(Outcome::UpToDate) => {
            let _ = writeln!(out, "FluutCAM {version} is up to date.");
            0
        }
        Ok(Outcome::Available(o)) => {
            let _ = writeln!(
                out,
                "Version {} is available ({}). You have {version}.\nRelease notes: {}\nDownload page: {}",
                o.version, o.date, o.notes_url, o.download_page
            );
            0
        }
        Err(UpdateError::NoKey) => {
            let _ = writeln!(err, "fluut: {}", UpdateError::NoKey);
            3
        }
        Err(e) => {
            let _ = writeln!(err, "fluut: update check failed: {e}");
            1
        }
    }
}

#[cfg(test)]
mod update_check_tests {
    use super::*;
    use fluut_core::update::Offer;

    fn run(args: &[&str], result: Result<Outcome, UpdateError>) -> (u8, String, String, bool) {
        let (code, out, err, asked) = run_in(args, Channel::Stable, result);
        (code, out, err, asked.is_some())
    }

    fn run_in(
        args: &[&str],
        saved: Channel,
        result: Result<Outcome, UpdateError>,
    ) -> (u8, String, String, Option<Channel>) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut asked = None;
        let code = update_check(
            args.iter().map(|s| (*s).to_owned()),
            saved,
            |c| {
                asked = Some(c);
                result
            },
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            asked,
        )
    }

    fn offer() -> Offer {
        Offer {
            version: "9.9.9".into(),
            date: "2030-01-01".into(),
            notes_url: "https://example.org/notes".into(),
            download_page: "https://example.org/downloads".into(),
            artefacts: Vec::new(),
        }
    }

    #[test]
    fn up_to_date_is_printed_and_succeeds() {
        let (code, out, err, _) = run(&[], Ok(Outcome::UpToDate));
        assert_eq!(code, 0);
        assert!(out.contains("is up to date"), "{out}");
        assert!(err.is_empty());
    }

    #[test]
    fn an_update_prints_the_version_and_both_links_and_succeeds() {
        let (code, out, _, _) = run(&[], Ok(Outcome::Available(offer())));
        assert_eq!(code, 0);
        assert!(
            out.contains("Version 9.9.9 is available (2030-01-01)"),
            "{out}"
        );
        assert!(out.contains("Release notes: https://example.org/notes"));
        assert!(out.contains("Download page: https://example.org/downloads"));
    }

    #[test]
    fn no_key_exits_with_3_and_says_so() {
        let (code, out, err, _) = run(&[], Err(UpdateError::NoKey));
        assert_eq!(code, 3);
        assert!(out.is_empty());
        assert!(err.contains("no update key in this build"), "{err}");
    }

    #[test]
    fn a_failed_check_exits_with_1() {
        let (code, _, err, _) = run(&[], Err(UpdateError::Fetch("timed out".into())));
        assert_eq!(code, 1);
        assert!(err.contains("timed out"), "{err}");
    }

    #[test]
    fn the_channel_is_the_saved_one_unless_the_flag_says_otherwise() {
        let ok = Ok(Outcome::UpToDate);
        let (_, _, _, ch) = run_in(&[], Channel::Stable, ok.clone());
        assert_eq!(ch, Some(Channel::Stable));
        let (_, _, _, ch) = run_in(&[], Channel::Beta, ok.clone());
        assert_eq!(ch, Some(Channel::Beta));
        let (_, _, _, ch) = run_in(&["--channel", "stable"], Channel::Beta, ok.clone());
        assert_eq!(ch, Some(Channel::Stable));
        let (_, _, _, ch) = run_in(&["--channel", "beta"], Channel::Stable, ok);
        assert_eq!(ch, Some(Channel::Beta));
    }

    #[test]
    fn a_bad_channel_is_refused_before_any_request() {
        for args in [&["--channel", "nightly"][..], &["--channel"][..]] {
            let (code, _, err, asked) = run(args, Ok(Outcome::UpToDate));
            assert_eq!(code, 2, "{args:?}");
            assert!(err.contains("--channel needs stable or beta"), "{err}");
            assert!(!asked);
        }
    }

    #[test]
    fn arguments_are_refused_before_any_request() {
        let (code, _, err, asked) = run(&["--now"], Ok(Outcome::UpToDate));
        assert_eq!(code, 2);
        assert!(err.contains("unexpected argument --now"));
        assert!(!asked);
    }
}
