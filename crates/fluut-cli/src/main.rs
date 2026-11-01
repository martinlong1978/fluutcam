// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut`: calculate and post FluutCAM projects from the command line.
//!
//! ```text
//! fluut calc <project> [--tools <file>]...
//! fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <d>] [--time <t>]
//! fluut run <script> [<project>] [--tools <file>]... [--date <d>] [--time <t>] [-- <args>...]
//! ```
//!
//! `run` runs a script headless (spec 09 §9.2, D12 §4). It may read and
//! write the project's folder and the paths given after `--`, and import
//! modules from the project's `scripts/`. Its exit status is
//! `fluut.exit(n)`, 0, or 1 when it fails.
//!
//! Exit status: 0 on success, 1 when a diagnostic is an error, 2 on bad
//! arguments.

use fluut_core::{JobReport, JobState, PostOptions, Session, Targets};
use fluut_model::{Diagnostic, Severity};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  fluut calc <project> [--tools <file>]...
  fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>]
  fluut run <script> [<project>] [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>] [-- <args>...]";

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
            println!("fluut {}", fluut_core::build_info::version_string());
            return ExitCode::SUCCESS;
        }
        _ => {}
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
    let project_dir = session.as_ref().map(|s| absolute(&s.dir));
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
        assert_eq!(
            numbered(std::path::Path::new("d/out.ngc"), 2),
            PathBuf::from("d/out.2.ngc")
        );
    }
}
