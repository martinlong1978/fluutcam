// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut`: calculate and post FluutCAM projects from the command line.
//!
//! ```text
//! fluut calc <project> [--tools <file>]...
//! fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <d>] [--time <t>]
//! ```
//!
//! Exit status: 0 on success, 1 when a diagnostic is an error, 2 on bad
//! arguments.

use fluut_core::{JobReport, JobState, PostOptions, Session, Targets};
use fluut_model::{Diagnostic, Severity};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  fluut calc <project> [--tools <file>]...
  fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>]";

#[derive(Debug, Default)]
struct Args {
    command: String,
    project: PathBuf,
    tools: Vec<PathBuf>,
    post: Option<String>,
    output: Option<PathBuf>,
    date: Option<String>,
    time: Option<String>,
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
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if project.is_none() => project = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    a.project = project.ok_or("no project file")?;
    Ok(a)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    match args.peek().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("fluut {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
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
        "post" => post(&a),
        c => {
            eprintln!("fluut: unknown command {c}\n{USAGE}");
            ExitCode::from(2)
        }
    }
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
    match &r.stats {
        Some(st) => println!(
            "{} \"{name}\": {state}; cut {:.1} mm, rapid {:.1} mm",
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
    let store = fluut_post::builtin::builtin_store();
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
        assert_eq!(
            numbered(std::path::Path::new("d/out.ngc"), 2),
            PathBuf::from("d/out.2.ngc")
        );
    }
}
