// SPDX-License-Identifier: MIT OR Apache-2.0

//! One run of a script (spec 09 §9.2, D12 §3.4, §4): checks the version
//! header, runs the script on a session, and closes the run's undo group
//! so the whole run is one undo step.

use crate::api::{self, Rt, State, version_at_least};
use crate::engine::{Control, Limits, Stop, make_engine};
use crate::host::ScriptHost;
use crate::perm::Permissions;
use fluut_core::Session;
use fluut_core::session::OpenOptions;
use fluut_model::document::Outcome;
use rhai::{EvalAltResult, ParseError, Position};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Largest script text accepted.
pub const MAX_SOURCE_BYTES: usize = 4 << 20;

/// What a run may do and see.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// Script file name, for messages and the undo label.
    pub name: String,
    pub limits: Limits,
    pub permissions: Permissions,
    /// Definition layers for projects the script opens.
    pub open: OpenOptions,
    /// `fluut.args` (headless).
    pub args: Vec<String>,
    /// `fluut.version`; the build's version unless a test sets it.
    pub version: String,
    /// Default date and time in post headers (`yyyy-mm-dd`, `hh:mm:ss`).
    pub date: String,
    pub time: String,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions {
            name: "script".into(),
            limits: Limits::default(),
            permissions: Permissions::default(),
            open: OpenOptions::default(),
            args: Vec::new(),
            version: env!("CARGO_PKG_VERSION").into(),
            date: String::new(),
            time: String::new(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ErrorKind {
    /// The text is not a valid script (including `eval`).
    Parse,
    /// The `//! fluut >= x` header asks for a newer FluutCAM.
    Version,
    /// An error the script raised or did not catch.
    Runtime,
    /// Operations, depth, size, variables or modules.
    Limit,
    /// The script-time limit.
    TimeLimit,
    Cancelled,
}

/// Why a run failed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScriptError {
    pub kind: ErrorKind,
    pub message: String,
    /// 1-based line and column, when known.
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ScriptError {}

/// The end of a run.
#[derive(Debug)]
pub struct RunOutcome {
    /// The session, with the run's edits; replaced when the script opened
    /// or created a project.
    pub session: Option<Session>,
    pub result: Result<(), ScriptError>,
    /// `fluut.exit(n)`, else 0, or 1 when the run failed.
    pub exit_code: i32,
    /// The run's undo step, when it changed the project.
    pub undo: Option<Outcome>,
}

fn at(pos: Position) -> (Option<usize>, Option<usize>) {
    (pos.line(), pos.position())
}

fn parse_error(e: &ParseError) -> ScriptError {
    let (line, column) = at(e.position());
    ScriptError {
        kind: ErrorKind::Parse,
        message: e.to_string(),
        line,
        column,
    }
}

/// The innermost error of a chain of function and module calls.
fn innermost(e: &EvalAltResult) -> &EvalAltResult {
    match e {
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _)
        | EvalAltResult::ErrorInModule(_, inner, _) => innermost(inner),
        e => e,
    }
}

fn eval_error(e: &EvalAltResult, ctl: &Control) -> ScriptError {
    use EvalAltResult as E;
    let kind = match innermost(e) {
        E::ErrorParsing(..) => ErrorKind::Parse,
        E::ErrorTooManyOperations(..)
        | E::ErrorTooManyVariables(..)
        | E::ErrorTooManyModules(..)
        | E::ErrorStackOverflow(..)
        | E::ErrorDataTooLarge(..) => ErrorKind::Limit,
        E::ErrorTerminated(..) => match ctl.stopped() {
            Some(Stop::TimeLimit) => ErrorKind::TimeLimit,
            _ => ErrorKind::Cancelled,
        },
        _ => ErrorKind::Runtime,
    };
    let (line, column) = at(e.position());
    ScriptError {
        kind,
        message: e.to_string(),
        line,
        column,
    }
}

/// The version a `//! fluut >= x` first line asks for.
pub fn required_version(source: &str) -> Result<Option<String>, String> {
    let first = source.lines().next().unwrap_or("").trim();
    let Some(rest) = first.strip_prefix("//!") else {
        return Ok(None);
    };
    let rest = rest.trim();
    let Some(rest) = rest.strip_prefix("fluut") else {
        return Ok(None);
    };
    match rest.trim().strip_prefix(">=") {
        Some(v) if !v.trim().is_empty() => Ok(Some(v.trim().to_owned())),
        _ => Err(format!(
            "line 1: `{first}` is not a version header (`//! fluut >= 1.2`)"
        )),
    }
}

fn fail(kind: ErrorKind, message: String) -> Result<(), ScriptError> {
    Err(ScriptError {
        kind,
        message,
        line: Some(1),
        column: None,
    })
}

/// Runs `source` on `session` (`None`: no project open). The run's edits
/// are one undo step; a failed run keeps the edits made before the error,
/// so the app can offer "undo script changes" (D12 §3.4).
pub fn run(
    source: &str,
    session: Option<Session>,
    opts: RunOptions,
    host: Arc<dyn ScriptHost>,
) -> RunOutcome {
    let pre = check_header(source, &opts.version);
    if let Err(e) = pre {
        return RunOutcome {
            session,
            result: Err(e),
            exit_code: 1,
            undo: None,
        };
    }
    let ctl = Arc::new(Control::new(host, &opts.limits));
    let state = Arc::new(Mutex::new(State {
        session,
        generation: 0,
        perm: opts.permissions.clone(),
        open: opts.open,
        args: opts.args,
        version: opts.version,
        label: format!("Script {}", opts.name),
        group_open: false,
        date: opts.date,
        time: opts.time,
        calc: BTreeMap::new(),
    }));
    let rt = Rt {
        state: state.clone(),
        ctl: ctl.clone(),
    };
    let mut engine = make_engine(&opts.limits, &opts.permissions, ctl.clone());
    api::register(&mut engine, &rt);
    let result = match engine.compile(source) {
        Err(e) => Err(parse_error(&e)),
        Ok(mut ast) => {
            ast.set_source(opts.name.as_str());
            match engine.run_ast(&ast) {
                Ok(()) => Ok(()),
                Err(e) if matches!(innermost(&e), EvalAltResult::Exit(..)) => Ok(()),
                Err(e) if ctl.exit_code().is_some() => {
                    // A limit or error after `exit` was decided still exits.
                    let _ = e;
                    Ok(())
                }
                Err(e) => Err(eval_error(&e, &ctl)),
            }
        }
    };
    drop(engine);
    drop(rt);
    let (session, undo) = match state.lock() {
        Ok(mut g) => {
            let open = g.group_open;
            let mut s = g.session.take();
            let undo = match (&mut s, open) {
                (Some(s), true) => s.end_group(),
                _ => None,
            };
            (s, undo)
        }
        Err(_) => (None, None),
    };
    let exit_code = match (&result, ctl.exit_code()) {
        (Err(_), _) => 1,
        (Ok(()), Some(c)) => c as i32,
        (Ok(()), None) => 0,
    };
    RunOutcome {
        session,
        result,
        exit_code,
        undo,
    }
}

fn check_header(source: &str, version: &str) -> Result<(), ScriptError> {
    if source.len() > MAX_SOURCE_BYTES {
        return fail(
            ErrorKind::Limit,
            format!("the script is larger than {} MiB", MAX_SOURCE_BYTES >> 20),
        );
    }
    match required_version(source) {
        Err(m) => fail(ErrorKind::Parse, m),
        Ok(None) => Ok(()),
        Ok(Some(need)) => match version_at_least(version, &need) {
            Ok(true) => Ok(()),
            Ok(false) => fail(
                ErrorKind::Version,
                format!("this script needs FluutCAM {need} or later; this is {version}"),
            ),
            Err(m) => fail(ErrorKind::Parse, format!("line 1: {m}")),
        },
    }
}
