// SPDX-License-Identifier: MIT OR Apache-2.0

//! Options, limits, results and errors shared by the DXF and SVG readers
//! (D05 §1).

use fluut_geom::Point3;
use fluut_topo::WireBody;
use fluut_topo::mesh::weld_points;
use std::collections::BTreeMap;
use thiserror::Error;

/// Bounds on untrusted input (spec 06 §6.3). Exceeding one is an error,
/// except `max_depth`, which skips the nested part and reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Largest file accepted, in bytes.
    pub max_bytes: usize,
    /// Most curves in the result, after block and `use` expansion.
    pub max_curves: usize,
    /// Most entities or elements visited, counting every expanded copy. This
    /// bounds the time spent on block fan-outs that produce few curves (about
    /// 0.5 µs per visit, so the default is a few seconds at worst).
    pub max_items: usize,
    /// Deepest nesting of `INSERT`s, or of `use` and groups in SVG.
    pub max_depth: usize,
    /// Most control or fit points in one spline, and points in one polyline
    /// or path.
    pub max_points: usize,
    /// Largest absolute coordinate accepted, in file units.
    pub max_coord: f64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_bytes: 256 << 20,
            max_curves: 2_000_000,
            max_items: 5_000_000,
            max_depth: 16,
            max_points: 1_000_000,
            max_coord: 1e9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Millimetres per drawing unit. Required for DXF (spec 02 §2.6.2); for
    /// SVG, `None` takes the unit from the document.
    pub scale: Option<f64>,
    /// Modelling tolerance of the bodies, in mm: curve ends closer than this
    /// meet.
    pub tol_model: f64,
    pub limits: Limits,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            scale: None,
            tol_model: 1e-4,
            limits: Limits::default(),
        }
    }
}

/// A length unit a file may suggest, and the units a project may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unit {
    Um,
    Mm,
    Cm,
    M,
    In,
    Ft,
    /// SVG pixel, 1/96 inch.
    Px,
}

impl Unit {
    pub const ALL: [Unit; 7] = [
        Unit::Um,
        Unit::Mm,
        Unit::Cm,
        Unit::M,
        Unit::In,
        Unit::Ft,
        Unit::Px,
    ];

    /// Millimetres per unit.
    pub fn mm(self) -> f64 {
        match self {
            Unit::Um => 0.001,
            Unit::Mm => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
            Unit::In => 25.4,
            Unit::Ft => 304.8,
            Unit::Px => 25.4 / 96.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Um => "um",
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::M => "m",
            Unit::In => "in",
            Unit::Ft => "ft",
            Unit::Px => "px",
        }
    }

    pub fn parse(s: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|u| u.as_str() == s)
    }
}

/// What an import found besides the geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportReport {
    /// The unit the file declares (`$INSUNITS`, SVG root size), shown to the
    /// user as a suggestion.
    pub unit: Option<Unit>,
    /// Entities or elements skipped on purpose (text, dimensions, images), by
    /// name.
    pub ignored: BTreeMap<String, u32>,
    /// Entities or elements with geometry that cannot be read (solids, meshes,
    /// unknown kinds), by name.
    pub unsupported: BTreeMap<String, u32>,
    /// Curves dropped because they were degenerate or malformed.
    pub dropped: u32,
    /// Chains of curves meeting end to end (within `tol_model`).
    pub chains: u32,
    /// Chains that close.
    pub closed_chains: u32,
    /// Curve ends that meet no other curve end.
    pub open_ends: u32,
    /// Problems worth showing, in file order (at most [`MAX_WARNINGS`]; the
    /// rest are counted in `more_warnings`).
    pub warnings: Vec<String>,
    pub more_warnings: u32,
}

/// Most warnings kept in a report.
pub const MAX_WARNINGS: usize = 100;

impl ImportReport {
    pub fn warn(&mut self, msg: impl Into<String>) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(msg.into());
        } else {
            self.more_warnings += 1;
        }
    }

    pub fn ignore(&mut self, what: &str) {
        *self.ignored.entry(what.to_owned()).or_default() += 1;
    }

    pub fn unsupported(&mut self, what: &str) {
        *self.unsupported.entry(what.to_owned()).or_default() += 1;
    }

    /// Fills the chain counts from the curve ends of `wires`.
    pub fn count_chains(&mut self, wires: &[WireBody], tol: f64) {
        let ends: Vec<Point3> = wires
            .iter()
            .flat_map(|w| w.curves.iter())
            .flat_map(|c| [c.curve.eval(c.range.lo), c.curve.eval(c.range.hi)])
            .collect();
        let node = weld_points(&ends, tol);
        let mut degree = vec![0u32; ends.len()];
        for &n in &node {
            degree[n as usize] += 1;
        }
        // Union-find over end nodes: each curve joins its two ends.
        let mut parent: Vec<u32> = (0..ends.len() as u32).collect();
        fn find(p: &mut [u32], mut i: u32) -> u32 {
            while p[i as usize] != i {
                p[i as usize] = p[p[i as usize] as usize];
                i = p[i as usize];
            }
            i
        }
        for c in node.as_chunks::<2>().0 {
            let (a, b) = (find(&mut parent, c[0]), find(&mut parent, c[1]));
            parent[a.max(b) as usize] = a.min(b);
        }
        let mut open_in = BTreeMap::<u32, bool>::new();
        for (i, &n) in node.iter().enumerate() {
            if n as usize == i {
                let root = find(&mut parent, n);
                let open = open_in.entry(root).or_default();
                *open |= degree[i] != 2;
            }
        }
        self.chains = open_in.len() as u32;
        self.closed_chains = open_in.values().filter(|&&o| !o).count() as u32;
        self.open_ends = degree.iter().filter(|&&d| d == 1).count() as u32;
    }
}

/// The result of reading a DXF or SVG file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Import2D {
    pub wires: Vec<WireBody>,
    pub report: ImportReport,
}

/// A file that cannot be read at all. `line` is from 1, when known.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("{}{msg}", line.map(|l| format!("line {l}: ")).unwrap_or_default())]
pub struct ReadError {
    pub line: Option<u32>,
    pub msg: String,
}

impl ReadError {
    pub fn new(msg: impl Into<String>) -> ReadError {
        ReadError {
            line: None,
            msg: msg.into(),
        }
    }

    pub fn at(line: u32, msg: impl Into<String>) -> ReadError {
        ReadError {
            line: Some(line),
            msg: msg.into(),
        }
    }
}
