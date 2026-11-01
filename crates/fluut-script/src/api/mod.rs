// SPDX-License-Identifier: MIT OR Apache-2.0

//! The object API (D12 §3, D12b): handles over the run state, mapped onto
//! `fluut-model` edits and `fluut-core` session calls.
//!
//! A handle holds the run state and the generation of the session it was
//! made from; a handle from a project that was since replaced (`open`,
//! `new_project`) fails. Handles to Setups and toolpaths hold ids and read
//! the current document on every access (D12 §3.1).

mod library;
mod model;
mod project;
mod setup;
mod toolpath;

use crate::engine::Control;
use crate::host::{Level, NO_UI, UiRequest, UiResponse};
use crate::perm::{Access, Permissions, read_limited};
use fluut_core::Session;
use fluut_core::session::OpenOptions;
use fluut_model::Diagnostic;
use fluut_model::edit::Edit;
use rhai::{Array, Dynamic, Engine, EvalAltResult, FLOAT, INT, Map, Position};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub use model::{BodyH, FaceH};
pub(crate) use project::ProjectH;

pub(crate) type Res<T> = Result<T, Box<EvalAltResult>>;

pub(crate) fn fail<T>(msg: impl Into<String>) -> Res<T> {
    Err(Box::new(EvalAltResult::ErrorRuntime(
        msg.into().into(),
        Position::NONE,
    )))
}

/// The last calculation of a toolpath in this run.
#[derive(Clone, Debug)]
pub(crate) struct Calc {
    pub failed: bool,
    /// Result hash when it was calculated.
    pub hash: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Everything a run shares between its handles.
#[derive(Debug)]
pub(crate) struct State {
    pub session: Option<Session>,
    /// Bumped when the session is replaced.
    pub generation: u64,
    pub perm: Permissions,
    pub open: OpenOptions,
    pub args: Vec<String>,
    pub version: String,
    /// Undo label of the run.
    pub label: String,
    pub group_open: bool,
    pub date: String,
    pub time: String,
    pub calc: BTreeMap<String, Calc>,
}

impl State {
    /// Opens the run's undo group on first use (D12 §3.4).
    pub fn apply(&mut self, edit: Edit) -> Res<()> {
        let label = self.label.clone();
        let open = self.group_open;
        let s = self.session_mut()?;
        if !open {
            s.begin_group(&label);
        }
        let r = s.apply(edit);
        self.group_open = true;
        r.map(|_| ()).or_else(|e| fail(e.0))
    }

    /// Opens the undo group for calls that apply edits themselves.
    pub fn ensure_group(&mut self) -> Res<()> {
        if !self.group_open {
            let label = self.label.clone();
            self.session_mut()?.begin_group(&label);
            self.group_open = true;
        }
        Ok(())
    }

    pub fn session(&self) -> Res<&Session> {
        match &self.session {
            Some(s) => Ok(s),
            None => fail("no project is open (fluut.open or fluut.new_project)"),
        }
    }

    pub fn session_mut(&mut self) -> Res<&mut Session> {
        match &mut self.session {
            Some(s) => Ok(s),
            None => fail("no project is open (fluut.open or fluut.new_project)"),
        }
    }

    /// Replaces the session, closing the old one's undo group.
    pub fn replace_session(&mut self, s: Session) {
        if let (Some(old), true) = (&mut self.session, self.group_open) {
            old.end_group();
        }
        self.group_open = false;
        self.session = Some(s);
        self.generation += 1;
        self.calc.clear();
    }

    /// Checks a path against the permissions, relative to the project
    /// folder.
    pub fn path(&self, path: &str, access: Access) -> Res<PathBuf> {
        let base = self
            .session
            .as_ref()
            .filter(|s| s.path.is_some())
            .map(|s| s.dir.clone());
        self.perm.check(path, base.as_deref(), access).or_else(fail)
    }
}

/// The run state and control, cloned into every handle.
#[derive(Clone, Debug)]
pub struct Rt {
    pub(crate) state: Arc<Mutex<State>>,
    pub(crate) ctl: Arc<Control>,
}

impl Rt {
    pub(crate) fn lock(&self) -> Res<MutexGuard<'_, State>> {
        match self.state.lock() {
            Ok(g) => Ok(g),
            Err(_) => fail("internal error: the run state is poisoned"),
        }
    }

    /// The state, checking that `generation` is still the open project.
    pub(crate) fn at(&self, generation: u64) -> Res<MutexGuard<'_, State>> {
        let g = self.lock()?;
        if g.generation != generation {
            return fail("this object belongs to a project that is no longer open");
        }
        Ok(g)
    }

    fn log(&self, level: Level, msg: &str) {
        self.ctl.host().log(level, msg);
    }
}

/// The `fluut` global.
#[derive(Clone, Debug)]
pub struct FluutH {
    rt: Rt,
}

/// `fluut.library`.
#[derive(Clone, Debug)]
pub struct LibraryH {
    rt: Rt,
    generation: u64,
}

/// `fluut.ui`.
#[derive(Clone, Debug)]
pub struct UiH {
    rt: Rt,
}

/// Compares dotted versions (`"1.2"` < `"1.10"`); missing parts are 0.
pub fn version_at_least(have: &str, need: &str) -> Result<bool, String> {
    let parse = |s: &str| -> Result<Vec<u64>, String> {
        s.trim()
            .split('.')
            .map(|p| {
                p.parse::<u64>()
                    .map_err(|_| format!("`{s}` is not a version (like `1.2`)"))
            })
            .collect()
    };
    let (a, b) = (parse(have)?, parse(need)?);
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return Ok(x > y);
        }
    }
    Ok(true)
}

fn diag_map(d: &Diagnostic) -> Dynamic {
    let mut m = Map::new();
    m.insert("code".into(), d.code.clone().into());
    m.insert(
        "severity".into(),
        match d.severity {
            fluut_model::Severity::Error => "error",
            fluut_model::Severity::Warning => "warning",
        }
        .into(),
    );
    m.insert("message".into(), d.message.clone().into());
    m.insert(
        "field".into(),
        d.field.clone().map_or(Dynamic::UNIT, Into::into),
    );
    Dynamic::from_map(m)
}

pub(crate) fn diag_array(ds: &[Diagnostic]) -> Array {
    ds.iter().map(diag_map).collect()
}

/// `"a; b"` of the error diagnostics, for a failed call.
pub(crate) fn errors_text(ds: &[Diagnostic]) -> String {
    let e: Vec<String> = ds.iter().map(|d| d.to_string()).collect();
    e.join("; ")
}

fn to_text(d: &Dynamic) -> String {
    if d.is_string() {
        d.clone().into_string().unwrap_or_default()
    } else {
        d.to_string()
    }
}

/// Parses CSV text: rows of cells, quotes with `""` escapes.
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => {
                if c == '\n' {
                    line += 1;
                }
                cell.push(c);
            }
            (false, '"') if cell.is_empty() => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                line += 1;
            }
            (false, c) => cell.push(c),
        }
    }
    if quoted {
        return Err(format!("line {line}: unterminated quote"));
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}

fn csv_cell(s: &str) -> Dynamic {
    let t = s.trim();
    if t.is_empty() {
        return Dynamic::UNIT;
    }
    match t.parse::<f64>() {
        Ok(x) if x.is_finite() && !t.chars().any(char::is_alphabetic) => (x as FLOAT).into(),
        _ => s.to_owned().into(),
    }
}

impl FluutH {
    pub fn new(rt: Rt) -> FluutH {
        FluutH { rt }
    }

    fn project(&mut self) -> Res<ProjectH> {
        let g = self.rt.lock()?;
        g.session()?;
        Ok(ProjectH::new(self.rt.clone(), g.generation))
    }

    fn open(&mut self, path: &str) -> Res<ProjectH> {
        let mut g = self.rt.lock()?;
        if g.perm.ui {
            return fail("fluut.open is for headless runs; in the app, open the project first");
        }
        let p = g.path(path, Access::Read)?;
        let opts = g.open.clone();
        let s = self
            .rt
            .ctl
            .host_call(|| Session::open_with(&p, &opts))
            .or_else(|d| fail(errors_text(&d)))?;
        for w in &s.warnings {
            self.rt.log(Level::Warn, &w.to_string());
        }
        g.perm.project_dir = Some(s.dir.clone());
        g.replace_session(s);
        Ok(ProjectH::new(self.rt.clone(), g.generation))
    }

    fn new_project(&mut self, units: &str) -> Res<ProjectH> {
        let Some(u) = fluut_model::project::DisplayUnit::parse(units) else {
            return fail(format!("`{units}` is not mm or inch"));
        };
        let mut p = fluut_model::project::Project::new("");
        p.units = u;
        let s = Session::new(
            p,
            PathBuf::new(),
            fluut_core::ModelStore::new(),
            fluut_core::builtin_library(),
        );
        let mut g = self.rt.lock()?;
        g.replace_session(s);
        Ok(ProjectH::new(self.rt.clone(), g.generation))
    }

    fn require(&mut self, need: &str) -> Res<()> {
        let have = self.rt.lock()?.version.clone();
        match version_at_least(&have, need) {
            Ok(true) => Ok(()),
            Ok(false) => fail(format!(
                "this script needs FluutCAM {need} or later; this is {have}"
            )),
            Err(e) => fail(e),
        }
    }

    fn read_csv(&mut self, path: &str) -> Res<Array> {
        let p = self.rt.lock()?.path(path, Access::Read)?;
        let text = read_limited(&p).or_else(fail)?;
        let rows = parse_csv(&text).or_else(|e| fail(format!("{path}: {e}")))?;
        let mut it = rows.into_iter();
        let Some(header) = it.next() else {
            return Ok(Array::new());
        };
        let mut out = Array::new();
        for r in it {
            let m: Map = header
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let v = r.get(i).map_or(Dynamic::UNIT, |c| csv_cell(c));
                    (h.trim().into(), v)
                })
                .collect();
            out.push(Dynamic::from_map(m));
        }
        Ok(out)
    }

    fn exit(&mut self, code: INT) -> Res<()> {
        if !(0..=255).contains(&code) {
            return fail(format!("exit code {code} is not 0–255"));
        }
        self.rt.ctl.exit(code);
        Err(Box::new(EvalAltResult::Exit(code.into(), Position::NONE)))
    }
}

impl UiH {
    fn ask(&mut self, req: UiRequest) -> Res<UiResponse> {
        if !self.rt.lock()?.perm.ui {
            return fail(NO_UI);
        }
        let host = self.rt.ctl.host().clone();
        self.rt.ctl.host_call(|| host.ui_request(req)).or_else(fail)
    }
}

/// Registers the API on `e` (D12b).
pub fn register(e: &mut Engine, rt: &Rt) {
    e.register_type_with_name::<FluutH>("Fluut");
    e.register_type_with_name::<LibraryH>("Library");
    e.register_type_with_name::<UiH>("Ui");
    let f = FluutH::new(rt.clone());
    let ctl = rt.ctl.clone();
    #[allow(deprecated)] // volatile, not deprecated (rhai docs)
    e.on_var(move |name, _, cx| {
        if let Err(n) = ctl.check_memory(cx.scope().iter_raw().map(|(_, _, v)| v)) {
            return Err(Box::new(EvalAltResult::ErrorDataTooLarge(
                format!("Size of all variables ({n} cells)"),
                Position::NONE,
            )));
        }
        Ok((name == "fluut").then(|| Dynamic::from(f.clone())))
    });

    e.register_get("version", |h: &mut FluutH| -> Res<String> {
        Ok(h.rt.lock()?.version.clone())
    });
    e.register_fn("require", FluutH::require);
    e.register_get("project", FluutH::project);
    e.register_get("has_project", |h: &mut FluutH| -> Res<bool> {
        Ok(h.rt.lock()?.session.is_some())
    });
    e.register_fn("open", FluutH::open);
    e.register_fn("new_project", FluutH::new_project);
    e.register_fn("log", |h: &mut FluutH, m: Dynamic| {
        h.rt.log(Level::Info, &to_text(&m))
    });
    e.register_fn("warn", |h: &mut FluutH, m: Dynamic| {
        h.rt.log(Level::Warn, &to_text(&m))
    });
    e.register_get("args", |h: &mut FluutH| -> Res<Array> {
        Ok(h.rt.lock()?.args.iter().map(|a| a.clone().into()).collect())
    });
    e.register_fn("exit", FluutH::exit);
    // The language's own `exit` ends the run the same way.
    let r = rt.clone();
    e.register_fn("exit", move |code: INT| FluutH::new(r.clone()).exit(code));
    let r = rt.clone();
    e.register_fn("exit", move || FluutH::new(r.clone()).exit(0));
    e.register_fn("read_csv", FluutH::read_csv);
    e.register_get("library", |h: &mut FluutH| -> Res<LibraryH> {
        let g = h.rt.lock()?;
        g.session()?;
        Ok(LibraryH {
            rt: h.rt.clone(),
            generation: g.generation,
        })
    });
    e.register_get("ui", |h: &mut FluutH| UiH { rt: h.rt.clone() });

    e.register_fn("message", |u: &mut UiH, text: &str| -> Res<()> {
        u.ask(UiRequest::Message(text.into())).map(|_| ())
    });
    e.register_fn(
        "ask_number",
        |u: &mut UiH, label: &str, default: Dynamic, unit: &str| -> Res<FLOAT> {
            let default = default
                .as_float()
                .or_else(|_| default.as_int().map(|i| i as FLOAT))
                .or_else(|_| fail("the default must be a number"))?;
            match u.ask(UiRequest::AskNumber {
                label: label.into(),
                default,
                unit: unit.into(),
            })? {
                UiResponse::Number(x) => Ok(x),
                _ => fail("the dialog gave no number"),
            }
        },
    );
    e.register_fn(
        "ask_choice",
        |u: &mut UiH, label: &str, options: Array| -> Res<String> {
            let options: Vec<String> = options.iter().map(to_text).collect();
            match u.ask(UiRequest::AskChoice {
                label: label.into(),
                options: options.clone(),
            })? {
                UiResponse::Choice(c) if options.contains(&c) => Ok(c),
                _ => fail("the dialog gave no choice"),
            }
        },
    );
    e.register_fn("pick_faces", |u: &mut UiH, prompt: &str| -> Res<Array> {
        match u.ask(UiRequest::PickFaces(prompt.into()))? {
            UiResponse::Faces(f) => Ok(f.into_iter().map(Into::into).collect()),
            _ => fail("the dialog gave no faces"),
        }
    });
    e.register_fn("pick_file", |u: &mut UiH, filter: &str| -> Res<String> {
        match u.ask(UiRequest::PickFile(filter.into()))? {
            UiResponse::File(p) => {
                let s = p.to_string_lossy().into_owned();
                u.rt.lock()?.perm.allowed_paths.push(p);
                Ok(s)
            }
            _ => fail("the dialog gave no file"),
        }
    });

    project::register(e);
    setup::register(e);
    toolpath::register(e);
    model::register(e);
    library::register(e);
}
