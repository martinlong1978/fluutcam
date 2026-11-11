// SPDX-License-Identifier: MIT OR Apache-2.0

//! Project operations for the application (plan 2026-11-04-app-project-ops):
//! start a project, save it elsewhere, import a model, add a Setup and add a
//! toolpath, each with the defaults of spec 03 §3.9 and D15.
//!
//! Every operation that changes the project is one undo step. None of them
//! asks anything: the caller supplies paths, kinds and names.

use crate::session::{Session, builtin_library};
use crate::store::ModelStore;
use fluut_cam::geometry::{BodyGeom, SetupGeometry};
use fluut_model::defs::DefRef;
use fluut_model::edit::Edit;
use fluut_model::hash::sha256_hex;
use fluut_model::library::{DefKind, LayerKind, check_tool_ref, resolve_tool_ref, tool_ref_for};
use fluut_model::params::{Num, ToolpathTypeId};
use fluut_model::persist;
use fluut_model::project::{
    AxisX, BodyEntry, ClearanceSpec, DisplayUnit, GeometrySource, Project, Setup, SetupKind,
    SourceKind, StockSpec, Toolpath, ZeroSpec,
};
use fluut_model::setup::resolve_setup;
use fluut_model::{Diagnostic, PValue, Severity};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Margin of a new three_axis block stock: both X sides, both Y sides and
/// the top, mm (spec 02 §2.1 example).
pub const DEFAULT_STOCK_MARGIN: [i64; 3] = [2, 2, 1];

/// Material of a new Setup: the one the starter tools have cutting data for.
pub const DEFAULT_MATERIAL: &str = "MDF";

/// Machine of a new three_axis Setup.
pub const MACHINE_THREE_AXIS: &str = "fluut/generic-3axis-router@1";

/// Machine of a new rotary or indexed Setup.
pub const MACHINE_ROTARY: &str = "fluut/generic-4axis-router@1";

/// Extra radius and length of a new cylinder stock around the model, mm.
const CYLINDER_ALLOWANCE: f64 = 1.0;

/// Clearance of a new rotary Setup beyond the stock radius, mm.
const CYLINDER_CLEARANCE: f64 = 10.0;

/// A machine a Setup can name.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MachineChoice {
    /// `id@major`, as a Setup's `machine` holds it.
    pub reference: String,
    pub name: String,
    /// It has a rotary axis.
    pub rotary: bool,
}

/// What an import added.
#[derive(Clone, PartialEq, Debug)]
pub struct ImportReport {
    /// The new source's id.
    pub source: String,
    /// Its body ids.
    pub bodies: Vec<String>,
    /// The file was copied into the project folder.
    pub copied: bool,
    /// Load warnings and notes (an assumed unit).
    pub diagnostics: Vec<Diagnostic>,
}

fn fail<T>(code: &str, msg: impl Into<String>) -> Result<T, Vec<Diagnostic>> {
    Err(vec![Diagnostic::error(code, msg)])
}

fn abs(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The path with symbolic links and `..` resolved when it exists.
fn real(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| abs(p))
}

/// The first `prefix<n>` that is not an id of the project.
fn free_id(p: &Project, prefix: &str) -> String {
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|id| !p.ids().any(|x| x == id))
        .unwrap_or_default()
}

/// `a/b/c.step` with `/` separators.
fn slashed(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Copies a folder tree, leaving files that exist at the destination alone.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_tree(&e.path(), &dest)?;
        } else if !dest.exists() {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

fn io_diag(code: &str, what: &str, e: &std::io::Error) -> Diagnostic {
    Diagnostic::error(code, format!("{what}: {e}"))
}

/// The geometry kind of a file, from its extension.
pub fn source_kind(path: &Path) -> Option<SourceKind> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match ext.as_str() {
        "step" | "stp" => SourceKind::Step,
        "stl" => SourceKind::Stl,
        "dxf" => SourceKind::Dxf,
        "svg" => SourceKind::Svg,
        _ => return None,
    })
}

/// The built-in tool a new toolpath of `kind` starts with (plan decision 5).
pub fn default_tool(kind: ToolpathTypeId) -> &'static str {
    use ToolpathTypeId as T;
    match kind {
        T::ManualNc => "",
        T::Face => "fluut/starter#flat-12",
        T::Chamfer2d | T::Engrave => "fluut/starter#vbit-60",
        T::Drill => "fluut/starter#drill-5",
        T::Parallel
        | T::Contour
        | T::Spiral
        | T::Scallop
        | T::SteepAndShallow
        | T::RotaryAlongX
        | T::RotaryAroundA
        | T::RotarySpiral => "fluut/starter#ball-6",
        _ => "fluut/starter#flat-6",
    }
}

impl Session {
    /// Every machine of the library that resolves, once per id and major
    /// version, in id order.
    pub fn machine_choices(&self) -> Vec<MachineChoice> {
        let mut seen = BTreeSet::new();
        for e in self.library.entries() {
            if e.kind == DefKind::Machine {
                seen.insert((e.def.id.clone(), e.def.version.major));
            }
        }
        seen.into_iter()
            .filter_map(|(id, major)| {
                let reference = format!("{id}@{major}");
                let m = self
                    .library
                    .machine(&DefRef::parse(&reference).ok()?)
                    .ok()?;
                Some(MachineChoice {
                    name: if m.fields.name.is_empty() {
                        id
                    } else {
                        m.fields.name.clone()
                    },
                    rotary: m.has_rotary(),
                    reference,
                })
            })
            .collect()
    }

    /// Starts a project at `path` (`.fluut` is added when there is no
    /// extension) and writes it, so the file exists and is valid. The name
    /// is the file name; definitions beside the file are loaded.
    pub fn new_project(path: &Path, units: DisplayUnit) -> Result<Session, Vec<Diagnostic>> {
        let mut path = path.to_path_buf();
        if path.extension().is_none() {
            path.set_extension("fluut");
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            return fail(
                "model.project.save",
                format!("{} is not a file name", path.display()),
            );
        };
        let mut project = Project::new(&stem);
        project.units = units;
        let dir = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let mut library = builtin_library();
        let warnings = library.load_dir(&dir, LayerKind::Project);
        let mut s = Session::new(project, dir, ModelStore::new(), library);
        s.warnings = warnings;
        s.path = Some(path);
        s.save().map_err(|d| vec![d])?;
        Ok(s)
    }

    /// Whether the project differs from the last save (or from the file as
    /// opened). Undoing back to the saved state makes it clean again.
    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// Saves the project as `path` and makes that its file. In another
    /// folder, the geometry files and the project-layer definition folders
    /// are copied there first, so paths stay as they are; a geometry path
    /// that leaves the project folder is changed to a copy beside the new
    /// file (which restarts the undo history). The undo history is kept
    /// otherwise.
    pub fn save_as(&mut self, path: &Path) -> Result<(), Diagnostic> {
        let mut path = path.to_path_buf();
        if path.extension().is_none() {
            path.set_extension("fluut");
        }
        let new_dir = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let moved = real(&new_dir) != real(&self.dir);
        if moved {
            self.copy_beside(&new_dir)?;
        }
        if moved {
            self.rebase_tool_references(&new_dir);
        }
        let saved = persist::save(&path, &self.project).map_err(|e| {
            Diagnostic::error("model.project.save", format!("{}: {e}", path.display()))
        })?;
        drop(saved);
        self.path = Some(path);
        self.dir = new_dir;
        self.saved = self.project.clone();
        Ok(())
    }

    /// Rewrites the relative tool library references to name the same
    /// files from `new_dir`. The libraries themselves stay where they are:
    /// they are shared, not part of the project folder (D06 §7.2.1).
    fn rebase_tool_references(&mut self, new_dir: &Path) {
        let old = self.dir.clone();
        for r in &mut self.project.tool_libraries {
            if check_tool_ref(r).is_ok() && !Path::new(r.as_str()).is_absolute() {
                *r = tool_ref_for(new_dir, &resolve_tool_ref(&old, r));
            }
        }
        self.tool_refs = self.project.tool_libraries.clone();
    }

    /// Copies what the project needs from its folder to `new_dir`.
    fn copy_beside(&mut self, new_dir: &Path) -> Result<(), Diagnostic> {
        let err = |what: String, e: &std::io::Error| io_diag("model.project.save", &what, e);
        std::fs::create_dir_all(new_dir).map_err(|e| err(new_dir.display().to_string(), &e))?;
        for i in 0..self.project.geometry.len() {
            let g = &self.project.geometry[i];
            let rel = PathBuf::from(&g.path);
            let from = self.dir.join(&rel);
            let escapes =
                rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir));
            let dest_rel = if escapes {
                unique_name(new_dir, &from)
            } else {
                rel.clone()
            };
            let dest = new_dir.join(&dest_rel);
            if from.is_file() && !dest.exists() {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| err(parent.display().to_string(), &e))?;
                }
                std::fs::copy(&from, &dest).map_err(|e| err(dest.display().to_string(), &e))?;
            }
            if escapes {
                self.project.geometry[i].path = slashed(&dest_rel);
            }
        }
        for folder in ["tools", "machines", "posts", "templates"] {
            let from = self.dir.join(folder);
            if from.is_dir() {
                copy_tree(&from, &new_dir.join(folder))
                    .map_err(|e| err(from.display().to_string(), &e))?;
            }
        }
        Ok(())
    }

    /// Adds the geometry file `file` as a new source, with the unit assumed
    /// for STL, DXF and SVG files (the project's). See
    /// [`Session::import_model_unit`].
    pub fn import_model(&mut self, file: &Path) -> Result<ImportReport, Vec<Diagnostic>> {
        self.import_model_unit(file, None)
    }

    /// Adds the geometry file `file` (STEP, STL, DXF or SVG by extension)
    /// as a new source, one undo step. The file is read first; nothing
    /// changes unless it gives bodies. A file inside the project folder is
    /// referenced where it is, any other is copied into the folder (plan
    /// decision 1). `unit` (`mm`, `in`, …) is the unit of an STL or DXF
    /// file; `None` assumes the project's.
    pub fn import_model_unit(
        &mut self,
        file: &Path,
        unit: Option<&str>,
    ) -> Result<ImportReport, Vec<Diagnostic>> {
        if self.path.is_none() {
            return fail(
                "model.import.no_project",
                "save the project first: geometry paths are stored relative to its folder",
            );
        }
        let Some(kind) = source_kind(file) else {
            return fail(
                "model.import.kind",
                format!("{}: not a STEP, STL, DXF or SVG file", file.display()),
            );
        };
        let file = abs(file);
        let bytes = std::fs::read(&file).map_err(|e| {
            vec![io_diag(
                "model.import.read",
                &format!("cannot read {}", file.display()),
                &e,
            )]
        })?;
        let mut notes = Vec::new();
        let unit = match kind {
            SourceKind::Stl | SourceKind::Dxf => Some(unit.map_or_else(
                || {
                    let u = match self.project.units {
                        DisplayUnit::Mm => "mm",
                        DisplayUnit::Inch => "in",
                    };
                    notes.push(Diagnostic::warning(
                        "model.import.unit_assumed",
                        format!(
                            "{} carries no reliable unit; assuming {u}, the project unit",
                            file.display()
                        ),
                    ));
                    u.to_owned()
                },
                str::to_owned,
            )),
            _ => unit.map(str::to_owned),
        };
        let id = free_id(&self.project, "g");
        let mut src = GeometrySource {
            id: id.clone(),
            // Absolute for the trial load: `dir.join` keeps an absolute path.
            path: file.to_string_lossy().into_owned(),
            kind,
            hash: None,
            unit,
            bodies: Vec::new(),
        };
        let mut fresh = ModelStore::new();
        let diags = fresh.load(&src, &self.dir);
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return Err(diags);
        }
        let Some(loaded) = fresh.source(&id).cloned() else {
            return fail(
                "model.import.empty",
                format!("{} gave no geometry", file.display()),
            );
        };
        if loaded.bodies.is_empty() {
            let mut d = diags;
            d.push(Diagnostic::error(
                "model.import.empty",
                format!(
                    "{} has no {} to machine",
                    file.display(),
                    if kind == SourceKind::Stl {
                        "triangles"
                    } else {
                        "solids or curves"
                    }
                ),
            ));
            return Err(d);
        }
        for b in &loaded.bodies {
            if let Some(other) = self
                .project
                .geometry
                .iter()
                .find(|g| g.bodies.iter().any(|e| e.id == b.id))
            {
                return fail(
                    "model.import.body_ids",
                    format!(
                        "body `{}` of {} has the id of a body of `{}`; two sources with the \
                         same body ids are not supported yet",
                        b.id,
                        file.display(),
                        other.id
                    ),
                );
            }
        }
        // Place the file: referenced inside the project folder, else copied.
        let dir_real = real(&self.dir);
        let (rel, copied) = match real(&file).strip_prefix(&dir_real) {
            Ok(r) => (slashed(r), false),
            Err(_) => {
                let (name, existed) = copy_name(&self.dir, &file, &bytes);
                if !existed {
                    std::fs::write(self.dir.join(&name), &bytes).map_err(|e| {
                        vec![io_diag(
                            "model.import.copy",
                            &format!("cannot copy into {}", self.dir.display()),
                            &e,
                        )]
                    })?;
                }
                (name, !existed)
            }
        };
        src.path = rel.clone();
        src.hash = Some(loaded.hash.clone());
        src.bodies = loaded
            .bodies
            .iter()
            .map(|b| {
                let name = match &b.geom {
                    BodyGeom::Brep(x) => x.name.clone(),
                    BodyGeom::Wire(w) => w.name.clone(),
                };
                BodyEntry {
                    id: b.id.clone(),
                    name: (!name.is_empty()).then_some(name),
                }
            })
            .collect();
        let bodies: Vec<String> = src.bodies.iter().map(|b| b.id.clone()).collect();
        if let Err(e) = self.apply(Edit::add_geometry(None, &src)) {
            if copied {
                let _ = std::fs::remove_file(self.dir.join(&rel));
            }
            return fail("model.import.apply", e.to_string());
        }
        self.store.insert(&id, loaded);
        let mut diagnostics = notes;
        diagnostics.extend(diags);
        Ok(ImportReport {
            source: id,
            bodies,
            copied,
            diagnostics,
        })
    }

    /// Adds a Setup named `name` with the defaults of plan decision 4, one
    /// undo step. With `based_on`, the Setup inherits everything but its
    /// kind (spec 03 §3.9.4) and takes the parent's stock as `from_previous`.
    /// Returns the new Setup's id.
    pub fn add_setup(
        &mut self,
        kind: SetupKind,
        based_on: Option<&str>,
        name: &str,
    ) -> Result<String, Diagnostic> {
        let id = free_id(&self.project, "s");
        let mut s = Setup::new(&id, name);
        match based_on {
            Some(parent) => {
                let p = resolve_setup(&self.project, parent)?;
                if p.kind.is_rotary() != kind.is_rotary() {
                    return Err(Diagnostic::error(
                        "setup.add.kind",
                        format!(
                            "a {kind} Setup cannot be based on the {} Setup `{parent}`: \
                             orientation and zero could not be inherited",
                            p.kind
                        ),
                    ));
                }
                s.based_on = Some(parent.to_owned());
                if p.kind != kind {
                    s.kind = Some(kind);
                }
                s.stock = Some(StockSpec::FromPrevious);
            }
            None => {
                s.kind = Some(kind);
                s.material = Some(DEFAULT_MATERIAL.to_owned());
                if kind.is_rotary() {
                    s.machine = Some(MACHINE_ROTARY.to_owned());
                    self.rotary_defaults(&mut s)?;
                } else {
                    s.machine = Some(MACHINE_THREE_AXIS.to_owned());
                    s.stock = Some(StockSpec::Block {
                        margin: Some(DEFAULT_STOCK_MARGIN.map(Num::int)),
                        size: None,
                    });
                    s.zero = Some(ZeroSpec::default());
                }
            }
        }
        self.apply(Edit::add_setup(None, &s))
            .map_err(|e| Diagnostic::error("setup.add", e.to_string()))?;
        Ok(id)
    }

    /// Cylinder stock, zero and clearance for a rotary or indexed Setup,
    /// from the bounds of the model about the +X axis (the default
    /// orientation).
    fn rotary_defaults(&self, s: &mut Setup) -> Result<(), Diagnostic> {
        let g = SetupGeometry {
            bodies: self.store.bodies().cloned().collect(),
        };
        let b = g.bounds(1e-3);
        if g.bodies.is_empty() || b.is_empty() {
            return Err(Diagnostic::error(
                "setup.add.no_model",
                "import a model before adding a rotary or indexed Setup: its stock is sized from it",
            ));
        }
        let (y0, y1, z0, z1) = (b.min.y(), b.max.y(), b.min.z(), b.max.z());
        let r = [y0, y1]
            .iter()
            .flat_map(|y| [z0, z1].map(|z| libm::hypot(*y, z)))
            .fold(0.0, f64::max);
        let radius = r + CYLINDER_ALLOWANCE;
        let length = b.max.x() - b.min.x() + 2.0 * CYLINDER_ALLOWANCE;
        s.stock = Some(StockSpec::Cylinder {
            radius: Num::float(radius),
            length: Num::float(length),
        });
        s.zero = Some(ZeroSpec::OnAxis {
            x: AxisX::StockLeft,
        });
        s.clearance = Some(ClearanceSpec::Cylinder {
            radius: Num::float(radius + CYLINDER_CLEARANCE),
        });
        Ok(())
    }

    /// Adds a toolpath of `kind` named `name` at the end of `setup`, with
    /// the default tool ([`default_tool`]) and every parameter at its D15
    /// default, one undo step. Returns the new toolpath's id.
    pub fn add_toolpath(
        &mut self,
        setup: &str,
        kind: ToolpathTypeId,
        name: &str,
    ) -> Result<String, Diagnostic> {
        if self.project.setup(setup).is_none() {
            return Err(Diagnostic::error(
                "setup.missing",
                format!("Setup `{setup}` is not in the project"),
            ));
        }
        let id = free_id(&self.project, "t");
        let tool = default_tool(kind);
        let tool = if self.tools.get(tool).is_some() {
            tool
        } else {
            ""
        };
        let t = Toolpath::new(&id, name, kind, tool);
        self.apply(Edit::add_toolpath(setup, None, &t))
            .map_err(|e| Diagnostic::error("toolpath.add", e.to_string()))?;
        Ok(id)
    }
}

impl Session {
    /// Copies toolpath `id` to just after itself, named `name`, one undo
    /// step. Returns the copy's id.
    pub fn duplicate_toolpath(&mut self, id: &str, name: &str) -> Result<String, Diagnostic> {
        let new_id = free_id(&self.project, "t");
        let Some((_, t)) = self.project.toolpath(id) else {
            return Err(Diagnostic::error(
                "toolpath.missing",
                format!("toolpath `{id}` is not in the project"),
            ));
        };
        let quoted = PValue::Enum(name.to_owned()).to_toml();
        let edit = Edit::Group {
            label: format!("Duplicate {id}"),
            edits: vec![
                Edit::DuplicateAs {
                    toolpath: id.to_owned(),
                    new_id: new_id.clone(),
                    new_type: t.kind().as_str().to_owned(),
                },
                Edit::set_toolpath_field(&new_id, "name", Some(&quoted)),
            ],
        };
        self.apply(edit)
            .map_err(|e| Diagnostic::error("toolpath.duplicate", e.to_string()))?;
        Ok(new_id)
    }
}

/// A file name for a copy of `from` in `dir` that nothing uses.
fn unique_name(dir: &Path, from: &Path) -> PathBuf {
    let stem = from
        .file_stem()
        .map_or_else(|| "file".to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = from
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut name = format!("{stem}{ext}");
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("{stem}-{n}{ext}");
        n += 1;
    }
    PathBuf::from(name)
}

/// The name for a copy of `file` in `dir`, and whether a file with the same
/// content is already there under it.
fn copy_name(dir: &Path, file: &Path, bytes: &[u8]) -> (String, bool) {
    let hash = sha256_hex(bytes);
    let stem = file
        .file_stem()
        .map_or_else(|| "model".to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = file
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut name = format!("{stem}{ext}");
    let mut n = 2;
    loop {
        match std::fs::read(dir.join(&name)) {
            Err(_) => return (name, false),
            Ok(b) if sha256_hex(&b) == hash => return (name, true),
            Ok(_) => {
                name = format!("{stem}-{n}{ext}");
                n += 1;
            }
        }
    }
}
