// SPDX-License-Identifier: MIT OR Apache-2.0

//! The definition library (D06 §4.1): posts, machines, templates and tool
//! libraries from the built-in, user and project layers.
//!
//! A reference resolves to the highest version that it accepts; when two
//! layers hold the same id and version, the higher layer (project over user
//! over built-in) wins. Ids in the `fluut/` namespace come only from the
//! built-in layer.

use crate::defs::{Def, DefError, DefRef, DefStore, MemStore};
use crate::diag::Diagnostic;
use crate::machine::Machine;
use crate::tools::{ToolLibrary, ToolStore};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Library layers, lowest first.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LayerKind {
    /// Embedded in the binary, read-only.
    BuiltIn,
    /// The user's data directory.
    User,
    /// Beside the project file.
    Project,
}

impl LayerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LayerKind::BuiltIn => "builtin",
            LayerKind::User => "user",
            LayerKind::Project => "project",
        }
    }
}

/// What a definition file defines, from its `format` key.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DefKind {
    Post,
    Machine,
    Template,
}

impl DefKind {
    pub const ALL: [DefKind; 3] = [DefKind::Post, DefKind::Machine, DefKind::Template];

    /// The `format` value, the folder and the file suffix.
    pub fn format(self) -> &'static str {
        match self {
            DefKind::Post => "fluut.post/1",
            DefKind::Machine => "fluut.machine/1",
            DefKind::Template => "fluut.template/1",
        }
    }

    pub fn folder(self) -> &'static str {
        match self {
            DefKind::Post => "posts",
            DefKind::Machine => "machines",
            DefKind::Template => "templates",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            DefKind::Post => ".post.toml",
            DefKind::Machine => ".machine.toml",
            DefKind::Template => ".template.toml",
        }
    }
}

/// One definition in the library and where it came from.
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    pub kind: DefKind,
    pub layer: LayerKind,
    /// File path, or the built-in name.
    pub source: String,
    pub def: Arc<Def>,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    entries: Vec<Entry>,
    /// Tool libraries in layer order, then load order.
    tool_libs: Vec<(LayerKind, ToolLibrary)>,
}

fn load_err(code: &str, source: &str, msg: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::error(code, format!("{source}: {msg}"))
}

impl Library {
    pub fn new() -> Library {
        Library::default()
    }

    /// Adds a definition from its file text. The `format` must be `kind`'s
    /// and `fluut/` ids must come from the built-in layer.
    pub fn add(
        &mut self,
        kind: DefKind,
        layer: LayerKind,
        source: &str,
        text: &str,
    ) -> Result<(), Diagnostic> {
        let def = Def::from_toml(text).map_err(|e| load_err("def.load", source, e))?;
        match def.table.get("format").and_then(|v| v.as_str()) {
            Some(f) if f == kind.format() => {}
            f => {
                return Err(load_err(
                    "def.format",
                    source,
                    format!(
                        "`format` is {}, expected `{}`",
                        f.map_or("missing".to_owned(), |f| format!("`{f}`")),
                        kind.format()
                    ),
                ));
            }
        }
        check_namespace(&def.id, layer, source)?;
        self.entries.push(Entry {
            kind,
            layer,
            source: source.to_owned(),
            def: Arc::new(def),
        });
        Ok(())
    }

    /// Adds a tool library.
    pub fn add_tools(&mut self, layer: LayerKind, lib: ToolLibrary) -> Result<(), Diagnostic> {
        check_namespace(&lib.id, layer, &lib.id)?;
        self.tool_libs.push((layer, lib));
        Ok(())
    }

    /// Loads `posts/*.post.toml`, `machines/*.machine.toml`,
    /// `templates/*.template.toml` and `tools/*.toml` under `dir`, in file
    /// name order. Missing folders are skipped; a bad file is reported and
    /// left out.
    pub fn load_dir(&mut self, dir: &Path, layer: LayerKind) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for kind in DefKind::ALL {
            for f in files(&dir.join(kind.folder()), kind.suffix()) {
                let src = f.display().to_string();
                match std::fs::read_to_string(&f) {
                    Ok(text) => {
                        if let Err(d) = self.add(kind, layer, &src, &text) {
                            out.push(d);
                        }
                    }
                    Err(e) => out.push(load_err("def.load", &src, e)),
                }
            }
        }
        out.extend(self.load_tools_dir(dir, layer));
        out
    }

    /// Loads `tools/*.toml` under `dir`, in file name order.
    pub fn load_tools_dir(&mut self, dir: &Path, layer: LayerKind) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for f in files(&dir.join("tools"), ".toml") {
            out.extend(self.load_tools_file(&f, layer));
        }
        out
    }

    /// Loads the tool library files a project lists (`tool_libraries`,
    /// D06 §7.2.1), in order. References are paths relative to `dir` (the
    /// project folder) or absolute. A reference that is not a plain file
    /// path, is listed twice, or names a missing file is a warning and is
    /// skipped; a library with the id and version of one already loaded in
    /// `layer` replaces it, with a warning.
    pub fn load_tool_references(
        &mut self,
        dir: &Path,
        refs: &[String],
        layer: LayerKind,
    ) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for r in refs {
            if let Err(why) = check_tool_ref(r) {
                out.push(Diagnostic::warning(
                    "project.tools.rejected",
                    format!("tool library reference `{r}` is not used: {why}"),
                ));
                continue;
            }
            let f = resolve_tool_ref(dir, r);
            if !seen.insert(normal(&absolute(&f))) {
                out.push(Diagnostic::warning(
                    "project.tools.duplicate",
                    format!("tool library `{r}` is listed twice; it is loaded once"),
                ));
                continue;
            }
            if !f.is_file() {
                out.push(Diagnostic::warning(
                    "project.tools.missing",
                    format!(
                        "tool library `{r}` was not found ({}); its tools are not available",
                        f.display()
                    ),
                ));
                continue;
            }
            out.extend(self.load_reference(&f, r, layer));
        }
        out
    }

    fn load_reference(&mut self, f: &Path, r: &str, layer: LayerKind) -> Vec<Diagnostic> {
        let src = f.display().to_string();
        let parsed = std::fs::read_to_string(f)
            .map_err(|e| e.to_string())
            .and_then(|t| ToolLibrary::parse(&t));
        match parsed {
            Ok((lib, w)) => {
                let mut out: Vec<Diagnostic> = w
                    .into_iter()
                    .map(|w| Diagnostic::warning("model.tools.warning", format!("{src}: {w}")))
                    .collect();
                if self
                    .tool_libs
                    .iter()
                    .any(|(l, x)| *l == layer && x.id == lib.id && x.version() == lib.version())
                {
                    out.push(Diagnostic::warning(
                        "project.tools.replaces",
                        format!(
                            "tool library `{r}` has the id and version of one already loaded \
                             for this project (`{}`); it replaces that one",
                            lib.id
                        ),
                    ));
                }
                if let Err(d) = self.add_tools(layer, lib) {
                    out.push(d);
                }
                out
            }
            Err(e) => vec![load_err("model.tools.load", &src, e)],
        }
    }

    /// Drops the tool libraries of `layer` (before they are loaded again).
    pub fn remove_tool_libs(&mut self, layer: LayerKind) {
        self.tool_libs.retain(|(l, _)| *l != layer);
    }

    /// Loads one tool library file. Unknown keys are warnings.
    pub fn load_tools_file(&mut self, f: &Path, layer: LayerKind) -> Vec<Diagnostic> {
        let src = f.display().to_string();
        let parsed = std::fs::read_to_string(f)
            .map_err(|e| e.to_string())
            .and_then(|t| ToolLibrary::parse(&t));
        match parsed {
            Ok((lib, w)) => {
                let mut out: Vec<Diagnostic> = w
                    .into_iter()
                    .map(|w| Diagnostic::warning("model.tools.warning", format!("{src}: {w}")))
                    .collect();
                if let Err(d) = self.add_tools(layer, lib) {
                    out.push(d);
                }
                out
            }
            Err(e) => vec![load_err("model.tools.load", &src, e)],
        }
    }

    /// The entry `r` resolves to.
    pub fn find(&self, kind: DefKind, r: &DefRef) -> Option<&Entry> {
        self.of(kind)
            .filter(|e| e.def.id == r.id && r.version.matches(e.def.version))
            .max_by_key(|e| (e.def.version, e.layer))
    }

    /// Definitions of one kind, lowest layer first.
    fn of(&self, kind: DefKind) -> impl Iterator<Item = &Entry> {
        let mut v: Vec<&Entry> = self.entries.iter().filter(|e| e.kind == kind).collect();
        v.sort_by_key(|e| e.layer);
        v.into_iter()
    }

    /// A store of one kind for the generic resolver.
    pub fn store(&self, kind: DefKind) -> KindStore<'_> {
        KindStore { lib: self, kind }
    }

    /// A standalone store of one kind (for callers that also borrow the
    /// session mutably).
    pub fn mem_store(&self, kind: DefKind) -> MemStore {
        let mut s = MemStore::new();
        for e in self.of(kind) {
            s.insert_arc(e.def.clone());
        }
        s
    }

    /// Resolves and reads a machine.
    pub fn machine(&self, r: &DefRef) -> Result<Machine, DefError> {
        Machine::resolve(&self.store(DefKind::Machine), r)
    }

    /// Every tool library, higher layers replacing lower ones with the same
    /// id and version.
    pub fn tools(&self) -> ToolStore {
        let mut libs: Vec<&(LayerKind, ToolLibrary)> = self.tool_libs.iter().collect();
        libs.sort_by_key(|(l, _)| *l);
        let mut s = ToolStore::new();
        for (_, lib) in libs {
            s.insert(lib.clone());
        }
        s
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

/// Longest tool library reference, in bytes.
pub const MAX_TOOL_REF: usize = 1024;

/// Why `r` cannot be a tool library reference: references are file paths,
/// never URLs or network paths, so nothing is fetched and no network
/// share is contacted when a project is opened (D06 §7.2.1).
pub fn check_tool_ref(r: &str) -> Result<(), &'static str> {
    if r.trim().is_empty() {
        return Err("it is empty");
    }
    if r.len() > MAX_TOOL_REF {
        return Err("it is too long");
    }
    if r.chars().any(char::is_control) {
        return Err("it contains a control character");
    }
    if r.starts_with("\\\\") || r.starts_with("//") {
        return Err("network paths are not allowed");
    }
    if let Some((scheme, _)) = r.split_once(':') {
        // `C:\x` and `C:/x` are drive paths; `https://x` and `file:x` are not.
        let drive = scheme.len() == 1 && scheme.chars().all(|c| c.is_ascii_alphabetic());
        let word = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
        if !drive && word {
            return Err("URLs are not allowed, only file paths");
        }
    }
    Ok(())
}

/// The file a reference names: `r` joined to the project folder `dir`
/// unless it is absolute.
pub fn resolve_tool_ref(dir: &Path, r: &str) -> PathBuf {
    let p = Path::new(r);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

/// The reference to write for the library file `file` in a project whose
/// file is in `dir`: relative to `dir` with `/` when the two share a
/// folder below the root, else the absolute path. Purely lexical.
pub fn tool_ref_for(dir: &Path, file: &Path) -> String {
    let abs_dir = normal(&absolute(dir));
    let abs_file = normal(&absolute(file));
    let slashed = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let a: Vec<Component> = abs_dir.components().collect();
    let b: Vec<Component> = abs_file.components().collect();
    let same = |x: &Component, y: &Component| {
        if cfg!(windows) {
            x.as_os_str().eq_ignore_ascii_case(y.as_os_str())
        } else {
            x == y
        }
    };
    let common = a.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    let shared = a
        .iter()
        .take(common)
        .any(|c| matches!(c, Component::Normal(_)));
    if !shared {
        return slashed(&abs_file);
    }
    let mut rel = PathBuf::new();
    for _ in common..a.len() {
        rel.push("..");
    }
    for c in &b[common..] {
        rel.push(c.as_os_str());
    }
    slashed(&rel)
}

/// Whether reference `r` of a project in `dir` names the file `file`
/// (purely lexical, as [`tool_ref_for`]).
pub fn names_tool_file(dir: &Path, r: &str, file: &Path) -> bool {
    let a = normal(&absolute(&resolve_tool_ref(dir, r)));
    let b = normal(&absolute(file));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `p` without `.`, with `..` applied where it can be (lexical).
fn normal(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

fn check_namespace(id: &str, layer: LayerKind, source: &str) -> Result<(), Diagnostic> {
    if id.starts_with("fluut/") && layer != LayerKind::BuiltIn {
        return Err(load_err(
            "def.namespace.reserved",
            source,
            format!(
                "`{id}`: the `fluut/` namespace is for built-in definitions; \
                 use `user/` or `project/`"
            ),
        ));
    }
    Ok(())
}

fn files(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(suffix))
        })
        .collect();
    v.sort();
    v
}

/// One kind of the library as a [`DefStore`].
#[derive(Debug, Clone, Copy)]
pub struct KindStore<'a> {
    lib: &'a Library,
    kind: DefKind,
}

impl DefStore for KindStore<'_> {
    fn lookup(&self, r: &DefRef) -> Result<Arc<Def>, DefError> {
        self.lib
            .find(self.kind, r)
            .map(|e| e.def.clone())
            .ok_or_else(|| DefError::NotFound(r.to_string()))
    }
}
