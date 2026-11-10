// SPDX-License-Identifier: MIT OR Apache-2.0

//! File permissions of a run (spec 09 §9.3, D12 §1, D12b §8) and the
//! module resolver that lets `import` read the library's `scripts/` only.

use rhai::{Engine, EvalAltResult, Module, ModuleResolver, Position, Scope, Shared};
use std::path::{Component, Path, PathBuf};

/// Where a run may read and write files.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Permissions {
    /// The project folder: read and write.
    pub project_dir: Option<PathBuf>,
    /// Definition library folders: read only; `import` looks in their
    /// `scripts/`.
    pub library_dirs: Vec<PathBuf>,
    /// Files or folders picked in a dialog or passed as headless
    /// arguments: read and write.
    pub allowed_paths: Vec<PathBuf>,
    /// The app's dialogs are available.
    pub ui: bool,
}

/// Read or write access.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
}

/// Largest file a script may read (CSV) or import as a module.
pub const MAX_READ_BYTES: u64 = 64 << 20;

impl Permissions {
    /// Checks `path` (relative to `base`, the current project folder) and
    /// returns it absolute and normalised, or the reason it is denied.
    pub fn check(
        &self,
        path: &str,
        base: Option<&Path>,
        access: Access,
    ) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            match base.or(self.project_dir.as_deref()) {
                // The folder of a project opened by a bare file name is empty:
                // it is the current folder.
                Some(b) if b.as_os_str().is_empty() => absolute(Path::new(".")).join(p),
                Some(b) => absolute(b).join(p),
                None => {
                    return Err(format!(
                        "`{path}` is relative and the project has no folder"
                    ));
                }
            }
        };
        let real = real_path(&abs).ok_or_else(|| format!("`{path}` is not a valid path"))?;
        let mut roots: Vec<&PathBuf> = self.project_dir.iter().collect();
        roots.extend(&self.allowed_paths);
        if access == Access::Read {
            roots.extend(&self.library_dirs);
        }
        let ok = roots
            .iter()
            .filter_map(|r| real_path(&absolute(r)))
            .any(|r| real.starts_with(&r));
        if ok {
            Ok(real)
        } else {
            let what = match access {
                Access::Read => "read",
                Access::Write => "write",
            };
            Err(format!(
                "permission denied: the script may not {what} `{}`",
                abs.display()
            ))
        }
    }
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `p` without `.` and `..`, with the symbolic links of its existing part
/// resolved. `None` when `..` climbs above the root.
pub fn real_path(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    // Resolve links in the longest existing prefix; the rest does not
    // exist yet, so it holds none.
    let mut rest = Vec::new();
    let mut head = out.clone();
    loop {
        if let Ok(r) = head.canonicalize() {
            let mut r = r;
            for c in rest.iter().rev() {
                r.push(c);
            }
            return Some(r);
        }
        let name = head.file_name()?.to_owned();
        rest.push(name);
        if !head.pop() {
            return Some(out);
        }
    }
}

/// `import "name" as m;` loads `<library>/scripts/name.rhai` (D12 §1).
#[derive(Clone, Debug)]
pub struct LibraryResolver {
    dirs: Vec<PathBuf>,
}

impl LibraryResolver {
    pub fn new(library_dirs: &[PathBuf]) -> LibraryResolver {
        LibraryResolver {
            dirs: library_dirs.to_vec(),
        }
    }

    /// The module file for `name`, if it is a valid name found in a library.
    pub fn find(&self, name: &str) -> Result<PathBuf, String> {
        let valid = !name.is_empty()
            && name.len() <= 200
            && name
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != "..")
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.'));
        if !valid {
            return Err(format!(
                "`{name}` is not a library module name (letters, digits, `_`, `-`, `/`; no `..`)"
            ));
        }
        for d in &self.dirs {
            let root = d.join("scripts");
            let f = root.join(format!("{name}.rhai"));
            let (Ok(root), Ok(real)) = (root.canonicalize(), f.canonicalize()) else {
                continue;
            };
            if real.starts_with(&root) && real.is_file() {
                return Ok(real);
            }
        }
        Err(format!("module `{name}` is not in the library's scripts"))
    }
}

impl ModuleResolver for LibraryResolver {
    fn resolve(
        &self,
        engine: &Engine,
        _source: Option<&str>,
        path: &str,
        pos: Position,
    ) -> Result<Shared<Module>, Box<EvalAltResult>> {
        let not_found = |msg: String| -> Box<EvalAltResult> {
            Box::new(EvalAltResult::ErrorModuleNotFound(msg, pos))
        };
        let f = self.find(path).map_err(not_found)?;
        let text = read_limited(&f).map_err(not_found)?;
        let mut ast = engine
            .compile(&text)
            .map_err(|e| Box::new(EvalAltResult::ErrorInModule(path.into(), e.into(), pos)))?;
        ast.set_source(path);
        let m = Module::eval_ast_as_new(Scope::new(), &ast, engine)
            .map_err(|e| Box::new(EvalAltResult::ErrorInModule(path.into(), e, pos)))?;
        Ok(m.into())
    }
}

/// Reads a text file of at most [`MAX_READ_BYTES`].
pub fn read_limited(f: &Path) -> Result<String, String> {
    let len = std::fs::metadata(f)
        .map_err(|e| format!("{}: {e}", f.display()))?
        .len();
    if len > MAX_READ_BYTES {
        return Err(format!(
            "{} is larger than {} MiB",
            f.display(),
            MAX_READ_BYTES >> 20
        ));
    }
    let b = std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
    String::from_utf8(b).map_err(|_| format!("{} is not UTF-8 text", f.display()))
}
