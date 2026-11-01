// SPDX-License-Identifier: MIT OR Apache-2.0

//! The project document (spec 02 §2.1, D06 §1, §7.2).
//!
//! M3 holds the minimal project: geometry sources, `three_axis` Setups with
//! the fields of D06 §1.1, and toolpaths with their parameter groups.
//! [`read`] and [`write`] round-trip a canonical file byte for byte.

mod read;
mod write;

#[cfg(test)]
mod tests;

pub use read::{LoadError, Loaded, read};
pub use write::write;

use crate::params::{Num, Params, ToolpathTypeId};
use std::fmt;

/// File format id and major version (D06 §7.2).
pub const PROJECT_FORMAT: &str = "fluut.project";
pub const PROJECT_MAJOR: u32 = 1;

#[derive(Clone, PartialEq, Debug)]
pub struct Project {
    /// As read, e.g. `fluut.project/1`; written back unchanged.
    pub format: String,
    pub name: String,
    /// Display and entry unit; the file always holds mm (D15 §1.2).
    pub units: DisplayUnit,
    pub geometry: Vec<GeometrySource>,
    pub setups: Vec<Setup>,
}

impl Project {
    pub fn new(name: &str) -> Project {
        Project {
            format: format!("{PROJECT_FORMAT}/{PROJECT_MAJOR}"),
            name: name.to_owned(),
            units: DisplayUnit::Mm,
            geometry: Vec::new(),
            setups: Vec::new(),
        }
    }

    pub fn setup(&self, id: &str) -> Option<&Setup> {
        self.setups.iter().find(|s| s.id == id)
    }

    /// The Setup holding toolpath `id`, and the toolpath.
    pub fn toolpath(&self, id: &str) -> Option<(&Setup, &Toolpath)> {
        self.setups
            .iter()
            .find_map(|s| s.toolpaths.iter().find(|t| t.id == id).map(|t| (s, t)))
    }

    pub fn toolpath_mut(&mut self, id: &str) -> Option<&mut Toolpath> {
        self.setups
            .iter_mut()
            .find_map(|s| s.toolpaths.iter_mut().find(|t| t.id == id))
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

/// Which model directions become +Z and +X of the work frame (spec 03
/// §3.9.3). M3 takes signed model axes only; picks from faces come later.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Orientation {
    pub tool_axis: AxisDir,
    pub x_axis: AxisDir,
}

impl Default for Orientation {
    fn default() -> Self {
        Orientation {
            tool_axis: AxisDir::PosZ,
            x_axis: AxisDir::PosX,
        }
    }
}

/// Stock (spec 03 §3.9.6). M3: a block from the model bounds plus margins:
/// `margin = [x, y, z]` grows both X sides, both Y sides and the top.
#[derive(Clone, PartialEq, Debug)]
pub enum StockSpec {
    Block { margin: [Num; 3] },
}

impl Default for StockSpec {
    fn default() -> Self {
        StockSpec::Block {
            margin: [Num::int(0); 3],
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

/// Work zero of a `three_axis` Setup (spec 03 §3.9.3).
#[derive(Clone, PartialEq, Debug)]
pub enum ZeroSpec {
    StockTopCorner(Corner),
    StockBottomCorner(Corner),
    StockTopCentre,
    /// A point in the model frame M.
    Point([Num; 3]),
}

impl Default for ZeroSpec {
    fn default() -> Self {
        ZeroSpec::StockTopCorner(Corner::FrontLeft)
    }
}

/// Setup clearance plane: `offset` above the stock top.
#[derive(Clone, PartialEq, Debug)]
pub struct ClearanceSpec {
    pub offset: Num,
}

impl ClearanceSpec {
    pub const DEFAULT_OFFSET: f64 = 10.0;
}

#[derive(Clone, PartialEq, Debug)]
pub struct Setup {
    pub id: String,
    pub name: String,
    pub kind: SetupKind,
    pub machine: Option<String>,
    pub material: Option<String>,
    pub work_offset: Option<String>,
    /// Body ids; `None` = every body of every source.
    pub bodies: Option<Vec<String>>,
    pub orientation: Option<Orientation>,
    pub stock: Option<StockSpec>,
    pub zero: Option<ZeroSpec>,
    pub clearance: Option<ClearanceSpec>,
    pub toolpaths: Vec<Toolpath>,
}

impl Setup {
    pub fn new(id: &str, name: &str) -> Setup {
        Setup {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: SetupKind::ThreeAxis,
            machine: None,
            material: None,
            work_offset: None,
            bodies: None,
            orientation: None,
            stock: None,
            zero: None,
            clearance: None,
            toolpaths: Vec::new(),
        }
    }

    pub fn work_offset(&self) -> &str {
        self.work_offset.as_deref().unwrap_or("G54")
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Toolpath {
    pub id: String,
    pub name: String,
    /// `library#tool` (spec 02 §2.1).
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
}

impl fmt::Display for SetupKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
