// SPDX-License-Identifier: MIT OR Apache-2.0

//! The project document (spec 02 §2.1, D06 §1, §7.2).
//!
//! A project holds geometry sources, Setups and their toolpaths. Every
//! Setup field is optional: an absent field is inherited from `based_on`
//! or takes its default ([`crate::setup`]). Setups are shared `Arc`s, so a
//! cloned project (an undo entry) costs only the Setups that change
//! (CH-02). [`read`] and [`write`] round-trip a canonical file byte for
//! byte.

mod read;
mod write;

#[cfg(test)]
mod tests;

pub use read::{LoadError, Loaded, read};
pub use write::write;

pub(crate) use read::{Cx, SETUP_KEYS, TOOLPATH_KEYS, check_ids};
pub(crate) use write::{
    setup as write_setup, setup_value, source as write_source, toolpath as write_toolpath,
};

use crate::params::{Num, Params, ToolpathTypeId};
use std::fmt;
use std::sync::Arc;

/// File format id and version (D06 §7.2). The writer writes
/// `fluut.project/1`, or `fluut.project/1.1` when the project lists tool
/// libraries (D06 §7.2.1); the reader accepts minors up to
/// [`PROJECT_MINOR`] and runs the migrations of older minors.
pub const PROJECT_FORMAT: &str = "fluut.project";
pub const PROJECT_MAJOR: u32 = 1;
pub const PROJECT_MINOR: u32 = 1;

#[derive(Clone, PartialEq, Debug)]
pub struct Project {
    pub name: String,
    /// Display and entry unit; the file always holds mm (D15 §1.2).
    pub units: DisplayUnit,
    /// Tool library files the project uses besides its `tools/` folder
    /// (D06 §7.2.1), as written: relative to the project file with `/`,
    /// or absolute. In load order; empty is not written.
    pub tool_libraries: Vec<String>,
    pub geometry: Vec<GeometrySource>,
    /// In machining order.
    pub setups: Vec<Arc<Setup>>,
    /// Saved camera views, kept as written (D13 owns their keys).
    pub views: Vec<toml::Table>,
}

impl Project {
    pub fn new(name: &str) -> Project {
        Project {
            name: name.to_owned(),
            units: DisplayUnit::Mm,
            tool_libraries: Vec::new(),
            geometry: Vec::new(),
            setups: Vec::new(),
            views: Vec::new(),
        }
    }

    pub fn setup(&self, id: &str) -> Option<&Setup> {
        self.setups.iter().find(|s| s.id == id).map(|s| &**s)
    }

    pub fn setup_index(&self, id: &str) -> Option<usize> {
        self.setups.iter().position(|s| s.id == id)
    }

    /// The Setup holding toolpath `id`, and the toolpath.
    pub fn toolpath(&self, id: &str) -> Option<(&Setup, &Toolpath)> {
        self.setups
            .iter()
            .find_map(|s| s.toolpaths.iter().find(|t| t.id == id).map(|t| (&**s, t)))
    }

    /// The toolpath, unsharing its Setup.
    pub fn toolpath_mut(&mut self, id: &str) -> Option<&mut Toolpath> {
        let i = self
            .setups
            .iter()
            .position(|s| s.toolpaths.iter().any(|t| t.id == id))?;
        Arc::make_mut(&mut self.setups[i])
            .toolpaths
            .iter_mut()
            .find(|t| t.id == id)
    }

    /// The Setup, unshared.
    pub fn setup_mut(&mut self, id: &str) -> Option<&mut Setup> {
        let i = self.setup_index(id)?;
        Some(Arc::make_mut(&mut self.setups[i]))
    }

    pub fn source(&self, id: &str) -> Option<&GeometrySource> {
        self.geometry.iter().find(|g| g.id == id)
    }

    /// Every id in the document: sources, Setups, toolpaths.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.geometry
            .iter()
            .map(|g| g.id.as_str())
            .chain(self.setups.iter().map(|s| s.id.as_str()))
            .chain(
                self.setups
                    .iter()
                    .flat_map(|s| s.toolpaths.iter().map(|t| t.id.as_str())),
            )
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DisplayUnit {
    Mm,
    Inch,
}

impl DisplayUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            DisplayUnit::Mm => "mm",
            DisplayUnit::Inch => "inch",
        }
    }

    pub fn parse(s: &str) -> Option<DisplayUnit> {
        match s {
            "mm" => Some(DisplayUnit::Mm),
            "inch" => Some(DisplayUnit::Inch),
            _ => None,
        }
    }

    /// mm per display unit.
    pub fn mm(self) -> f64 {
        match self {
            DisplayUnit::Mm => 1.0,
            DisplayUnit::Inch => 25.4,
        }
    }
}

/// Converts an inspector entry to mm (D15 §1.2): a plain number is in the
/// project unit; `mm` or `in` after the number wins. Converted once, at entry.
pub fn length_entry(text: &str, unit: DisplayUnit) -> Option<f64> {
    let t = text.trim();
    let (num, scale) = if let Some(n) = t.strip_suffix("mm") {
        (n, 1.0)
    } else if let Some(n) = t.strip_suffix("in") {
        (n, 25.4)
    } else {
        (t, unit.mm())
    };
    let v: f64 = num.trim().parse().ok()?;
    let mm = v * scale;
    mm.is_finite().then_some(mm)
}

/// Kind of geometry file (spec 02 §2.1).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SourceKind {
    Step,
    Stl,
    Dxf,
    Svg,
    Bitmap,
}

impl SourceKind {
    pub const ALL: [SourceKind; 5] = [
        SourceKind::Step,
        SourceKind::Stl,
        SourceKind::Dxf,
        SourceKind::Svg,
        SourceKind::Bitmap,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Step => "step",
            SourceKind::Stl => "stl",
            SourceKind::Dxf => "dxf",
            SourceKind::Svg => "svg",
            SourceKind::Bitmap => "bitmap",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct GeometrySource {
    pub id: String,
    /// Relative to the project file.
    pub path: String,
    pub kind: SourceKind,
    /// `sha256:<hex>` of the file when it was imported (spec 02 §2.6.3).
    pub hash: Option<String>,
    /// Required for stl and dxf.
    pub unit: Option<String>,
    pub bodies: Vec<BodyEntry>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct BodyEntry {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SetupKind {
    ThreeAxis,
    Rotary,
    Indexed,
}

impl SetupKind {
    pub const ALL: [SetupKind; 3] = [SetupKind::ThreeAxis, SetupKind::Rotary, SetupKind::Indexed];

    pub fn as_str(self) -> &'static str {
        match self {
            SetupKind::ThreeAxis => "three_axis",
            SetupKind::Rotary => "rotary",
            SetupKind::Indexed => "indexed",
        }
    }

    /// Rotary and indexed Setups turn about the rotary axis.
    pub fn is_rotary(self) -> bool {
        self != SetupKind::ThreeAxis
    }
}

impl fmt::Display for SetupKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A signed model axis.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AxisDir {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl AxisDir {
    pub const ALL: [AxisDir; 6] = [
        AxisDir::PosX,
        AxisDir::NegX,
        AxisDir::PosY,
        AxisDir::NegY,
        AxisDir::PosZ,
        AxisDir::NegZ,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AxisDir::PosX => "+X",
            AxisDir::NegX => "-X",
            AxisDir::PosY => "+Y",
            AxisDir::NegY => "-Y",
            AxisDir::PosZ => "+Z",
            AxisDir::NegZ => "-Z",
        }
    }

    /// Unit vector in the model frame.
    pub fn vector(self) -> [f64; 3] {
        match self {
            AxisDir::PosX => [1.0, 0.0, 0.0],
            AxisDir::NegX => [-1.0, 0.0, 0.0],
            AxisDir::PosY => [0.0, 1.0, 0.0],
            AxisDir::NegY => [0.0, -1.0, 0.0],
            AxisDir::PosZ => [0.0, 0.0, 1.0],
            AxisDir::NegZ => [0.0, 0.0, -1.0],
        }
    }
}

/// A direction picked from the model (spec 03 §3.9.3): a signed model axis,
/// a face (planar normal, or cylinder or cone axis) or an edge (line).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AxisPick {
    Axis(AxisDir),
    Face(String),
    Edge(String),
}

impl From<AxisDir> for AxisPick {
    fn from(a: AxisDir) -> AxisPick {
        AxisPick::Axis(a)
    }
}

/// How model axes map to machine axes (spec 03 §3.9.3).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Orientation {
    /// `three_axis`: +Z and +X of the work frame.
    ThreeAxis {
        tool_axis: AxisPick,
        x_axis: AxisPick,
    },
    /// `rotary` and `indexed`: the rotary axis line and the model direction
    /// that points to +Z of W at A = 0 (default +Z).
    Rotary {
        rotary_axis: AxisPick,
        a_zero_toward: Option<AxisPick>,
    },
}

impl Default for Orientation {
    fn default() -> Self {
        Orientation::ThreeAxis {
            tool_axis: AxisPick::Axis(AxisDir::PosZ),
            x_axis: AxisPick::Axis(AxisDir::PosX),
        }
    }
}

/// Body selection, scale and mirror of a Setup (spec 02 §2.1).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct GeometrySel {
    /// Body ids; `None` = every body of every source.
    pub bodies: Option<Vec<String>>,
    /// Per-axis scale about the model origin.
    pub scale: Option<[Num; 3]>,
    pub mirror: Option<[bool; 3]>,
}

/// Stock (spec 03 §3.9.6).
#[derive(Clone, PartialEq, Debug)]
pub enum StockSpec {
    /// A box: the model bounds in the aligned frame grown by `margin = [x,
    /// y, z]` (both X sides, both Y sides, the top), or an explicit `size`
    /// centred on the model in X and Y with its bottom at the model bottom.
    Block {
        margin: Option<[Num; 3]>,
        size: Option<[Num; 3]>,
    },
    /// Coaxial with the rotary axis.
    Cylinder { radius: Num, length: Num },
    /// A body of the model (a casting, a prior-operation part).
    Body { body: String },
    /// The material left by the parent Setup (`based_on`).
    FromPrevious,
}

impl StockSpec {
    pub fn kind(&self) -> &'static str {
        match self {
            StockSpec::Block { .. } => "block",
            StockSpec::Cylinder { .. } => "cylinder",
            StockSpec::Body { .. } => "body",
            StockSpec::FromPrevious => "from_previous",
        }
    }
}

impl Default for StockSpec {
    fn default() -> Self {
        StockSpec::Block {
            margin: None,
            size: None,
        }
    }
}

/// A corner of the stock seen from above: front is −Y, left is −X.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Corner {
    FrontLeft,
    FrontRight,
    BackLeft,
    BackRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::FrontLeft,
        Corner::FrontRight,
        Corner::BackLeft,
        Corner::BackRight,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Corner::FrontLeft => "front_left",
            Corner::FrontRight => "front_right",
            Corner::BackLeft => "back_left",
            Corner::BackRight => "back_right",
        }
    }
}

/// X of the work zero on the rotary axis.
#[derive(Clone, PartialEq, Debug)]
pub enum AxisX {
    StockLeft,
    StockRight,
    /// An X in the aligned frame.
    At(Num),
}

/// Work zero (spec 03 §3.9.3).
#[derive(Clone, PartialEq, Debug)]
pub enum ZeroSpec {
    StockTopCorner(Corner),
    StockBottomCorner(Corner),
    StockTopCentre,
    /// A point in the model frame M.
    Point([Num; 3]),
    /// Rotary and indexed Setups: Y and Z on the rotary axis, X free.
    OnAxis {
        x: AxisX,
    },
}

impl Default for ZeroSpec {
    fn default() -> Self {
        ZeroSpec::StockTopCorner(Corner::FrontLeft)
    }
}

/// The Setup clearance: a plane `offset` above the stock top, or a
/// cylinder about the rotary axis.
#[derive(Clone, PartialEq, Debug)]
pub enum ClearanceSpec {
    Plane { offset: Option<Num> },
    Cylinder { radius: Num },
}

impl ClearanceSpec {
    pub const DEFAULT_OFFSET: f64 = 10.0;

    /// The plane offset above the stock top (default 10 mm).
    pub fn plane_offset(&self) -> Option<f64> {
        match self {
            ClearanceSpec::Plane { offset } => {
                Some(offset.map_or(ClearanceSpec::DEFAULT_OFFSET, |n| n.v))
            }
            ClearanceSpec::Cylinder { .. } => None,
        }
    }
}

/// A fixture that turns with the rotary axis of a rotary or indexed Setup
/// (spec 03 section 3.9.8). Positions are in the Setup's work frame W,
/// whose X is the rotary axis; sizes are in mm.
#[derive(Clone, PartialEq, Debug)]
pub enum Fixture {
    /// A cylinder about the axis against the stock's start face (its lowest
    /// X), reaching `length` outward.
    Chuck { diameter: Num, length: Num },
    /// A cylinder, or a cone when `tip_diameter` is given, about the axis
    /// against the stock's end face (its highest X), reaching `length`
    /// outward. `diameter` is the width at the far end, `tip_diameter` the
    /// width at the stock.
    Tailstock {
        diameter: Num,
        length: Num,
        tip_diameter: Option<Num>,
    },
    /// A box with corners `min` and `max` in W.
    Clamp { min: [Num; 3], max: [Num; 3] },
}

impl Fixture {
    pub const KINDS: [&'static str; 3] = ["chuck", "tailstock", "clamp"];

    pub fn kind(&self) -> &'static str {
        match self {
            Fixture::Chuck { .. } => "chuck",
            Fixture::Tailstock { .. } => "tailstock",
            Fixture::Clamp { .. } => "clamp",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AngleMode {
    Absolute,
    Relative,
}

impl AngleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AngleMode::Absolute => "absolute",
            AngleMode::Relative => "relative",
        }
    }
}

/// Rotary position of an indexed Setup, degrees (spec 03 §3.9.5).
#[derive(Clone, PartialEq, Debug)]
pub struct IndexAngle {
    pub mode: AngleMode,
    pub value: Num,
}

/// Machining region (spec 02 §2.1): toolpaths do not cut below `floor_z`
/// (in W).
#[derive(Clone, PartialEq, Debug)]
pub struct Region {
    pub floor_z: Num,
}

/// A Setup as stored: `None` fields are inherited from `based_on` or take
/// their defaults (spec 03 §3.9.4).
#[derive(Clone, PartialEq, Debug)]
pub struct Setup {
    pub id: String,
    pub name: String,
    pub based_on: Option<String>,
    pub kind: Option<SetupKind>,
    pub machine: Option<String>,
    pub material: Option<String>,
    pub work_offset: Option<String>,
    pub geometry: Option<GeometrySel>,
    pub orientation: Option<Orientation>,
    pub index_angle: Option<IndexAngle>,
    pub stock: Option<StockSpec>,
    pub zero: Option<ZeroSpec>,
    pub region: Option<Region>,
    pub clearance: Option<ClearanceSpec>,
    /// Fixtures that turn with A; inherited as a whole list.
    pub fixtures: Option<Vec<Fixture>>,
    pub toolpaths: Vec<Toolpath>,
}

impl Setup {
    pub fn new(id: &str, name: &str) -> Setup {
        Setup {
            id: id.to_owned(),
            name: name.to_owned(),
            based_on: None,
            kind: None,
            machine: None,
            material: None,
            work_offset: None,
            geometry: None,
            orientation: None,
            index_angle: None,
            stock: None,
            zero: None,
            region: None,
            clearance: None,
            fixtures: None,
            toolpaths: Vec::new(),
        }
    }

    pub fn toolpath(&self, id: &str) -> Option<&Toolpath> {
        self.toolpaths.iter().find(|t| t.id == id)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Toolpath {
    pub id: String,
    pub name: String,
    /// `library#tool` or `library@version#tool` (spec 02 §2.1). Empty
    /// until a tool is chosen (the key is then not written); validation
    /// reports it.
    pub tool: String,
    pub enabled: Option<bool>,
    pub template: Option<String>,
    pub params: Params,
}

impl Toolpath {
    pub fn new(id: &str, name: &str, kind: ToolpathTypeId, tool: &str) -> Toolpath {
        Toolpath {
            id: id.to_owned(),
            name: name.to_owned(),
            tool: tool.to_owned(),
            enabled: None,
            template: None,
            params: Params::new(kind),
        }
    }

    pub fn kind(&self) -> ToolpathTypeId {
        self.params.kind()
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// The tool reference, or `""` when none is set.
    pub fn tool_ref(&self) -> &str {
        &self.tool
    }
}
