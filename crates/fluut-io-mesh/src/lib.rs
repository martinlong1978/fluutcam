// SPDX-License-Identifier: MIT OR Apache-2.0

//! STL import (design D05 §2, spec 02 §2.6.2): binary and ASCII STL into a
//! welded [`MeshBody`] with adjacency, and a report of what the mesh lacks.
//!
//! Input is untrusted (spec 06 §6.3): the reader returns errors and never
//! panics, and sizes and counts are bounded by [`Limits`]. The binary
//! triangle count is checked against the file size before anything is
//! allocated.

#[cfg(test)]
mod tests;

use fluut_geom::{Point3, Vec3};
use fluut_topo::MeshBody;
use fluut_topo::mesh::{EdgeCounts, adjacency, boundary_loops, open_edges, weld_points};
use thiserror::Error;

/// Bounds on untrusted input. Exceeding one is an error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Largest file accepted, in bytes.
    pub max_bytes: usize,
    /// Most triangles accepted (D05 §2.2).
    pub max_triangles: usize,
    /// Longest ASCII token, in bytes.
    pub max_token: usize,
    /// Largest absolute coordinate accepted, in file units. Triangles beyond
    /// it are dropped.
    pub max_coord: f64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_bytes: 84 + 50 * 50_000_000,
            max_triangles: 50_000_000,
            max_token: 256,
            max_coord: 1e9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Millimetres per file unit. STL has no unit, so the caller supplies it
    /// (spec 02 §2.6.2); `None` keeps file units.
    pub scale: Option<f64>,
    /// Modelling tolerance in mm: vertices closer than this are welded.
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

/// What the import found about the mesh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshReport {
    pub binary: bool,
    /// Names after `solid` (ASCII), in file order.
    pub solids: Vec<String>,
    /// Triangles in the file.
    pub triangles_read: u32,
    /// Dropped: area below `tol_model²`, corners welded together, or
    /// coordinates not finite or out of range.
    pub dropped: u32,
    /// Triangles whose file normal agrees or disagrees with their vertex
    /// order (zero normals do not vote).
    pub normals_agree: u32,
    pub normals_disagree: u32,
    /// Every triangle was reversed because most file normals disagreed with
    /// the vertex order.
    pub flipped: bool,
    /// Edges used by one triangle, and the loops they form (holes).
    pub open_edges: u32,
    pub boundary_loops: u32,
    /// Edges used by more than two triangles.
    pub non_manifold_edges: u32,
    /// Edges shared by two triangles that run the same way along them
    /// (inconsistent orientation).
    pub inconsistent_edges: u32,
    pub warnings: Vec<String>,
}

impl MeshReport {
    /// True if the mesh is closed, manifold and consistently oriented.
    pub fn is_closed(&self) -> bool {
        self.open_edges == 0 && self.non_manifold_edges == 0 && self.inconsistent_edges == 0
    }

    fn warn(&mut self, msg: String) {
        if self.warnings.len() < 100 {
            self.warnings.push(msg);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportMesh {
    pub body: MeshBody,
    pub report: MeshReport,
}

/// A file that cannot be read. `line` is from 1, for ASCII files.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("{}{msg}", line.map(|l| format!("line {l}: ")).unwrap_or_default())]
pub struct ReadError {
    pub line: Option<u32>,
    pub msg: String,
}

fn err(msg: impl Into<String>) -> ReadError {
    ReadError {
        line: None,
        msg: msg.into(),
    }
}

fn err_at(line: u32, msg: impl Into<String>) -> ReadError {
    ReadError {
        line: Some(line),
        msg: msg.into(),
    }
}

/// One triangle as read: file normal and corners.
type RawTri = (Vec3, [Point3; 3]);

/// True if `bytes` has the size of a binary STL with the count at byte 80
/// (D05 §2.1). The `solid` keyword is not used: many binary exporters write
/// it into the header.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.len() >= 84 && bytes.len() as u64 == 84 + 50 * u64::from(count(bytes))
}

fn count(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]])
}

/// Reads an STL file into one welded mesh body named after the first solid
/// (`"stl"` for binary files and unnamed solids).
pub fn read_stl(bytes: &[u8], opts: &Options) -> Result<ImportMesh, ReadError> {
    let limits = opts.limits;
    if bytes.len() > limits.max_bytes {
        return Err(err(format!(
            "file is {} bytes; the limit is {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    let scale = opts.scale.unwrap_or(1.0);
    if !(scale.is_finite() && scale > 0.0) {
        return Err(err(format!("bad scale {scale}")));
    }
    let mut report = MeshReport::default();
    let raw = if is_binary(bytes) {
        report.binary = true;
        read_binary(bytes, &limits)?
    } else {
        read_ascii(bytes, &limits, &mut report)?
    };
    report.triangles_read = raw.len() as u32;
    let name = report
        .solids
        .iter()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "stl".to_owned());
    let body = build(raw, scale, opts.tol_model, &limits, &mut report);
    Ok(ImportMesh {
        body: MeshBody { name, ..body },
        report,
    })
}

fn read_binary(bytes: &[u8], limits: &Limits) -> Result<Vec<RawTri>, ReadError> {
    let n = count(bytes) as usize;
    if n > limits.max_triangles {
        return Err(err(format!(
            "{n} triangles; the limit is {}",
            limits.max_triangles
        )));
    }
    let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let v = |o: usize| Vec3::new(f64::from(f(o)), f64::from(f(o + 4)), f64::from(f(o + 8)));
    // `is_binary` checked the size, so every record is in range.
    Ok((0..n)
        .map(|i| {
            let o = 84 + 50 * i;
            let p = |k: usize| Point3(v(o + 12 + 12 * k).0);
            (v(o), [p(0), p(1), p(2)])
        })
        .collect())
}

/// ASCII tokens with their line numbers.
struct Tokens<'a> {
    lines: std::iter::Enumerate<std::str::Lines<'a>>,
    words: std::str::SplitAsciiWhitespace<'a>,
    line: u32,
    max: usize,
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<Option<&'a str>, ReadError> {
        loop {
            if let Some(w) = self.words.next() {
                if w.len() > self.max {
                    return Err(err_at(
                        self.line,
                        format!("token longer than {} bytes", self.max),
                    ));
                }
                return Ok(Some(w));
            }
            let Some((i, l)) = self.lines.next() else {
                return Ok(None);
            };
            self.line = i as u32 + 1;
            self.words = l.split_ascii_whitespace();
        }
    }

    /// The rest of the current line, as one string.
    fn rest_of_line(&mut self) -> String {
        let rest: Vec<&str> = self.words.by_ref().collect();
        rest.join(" ")
    }

    fn expect(&mut self, word: &str) -> Result<(), ReadError> {
        match self.next()? {
            Some(w) if w.eq_ignore_ascii_case(word) => Ok(()),
            Some(w) => Err(err_at(self.line, format!("expected `{word}`, found `{w}`"))),
            None => Err(err_at(
                self.line,
                format!("file ends where `{word}` was expected"),
            )),
        }
    }

    fn number(&mut self) -> Result<f64, ReadError> {
        match self.next()? {
            // Non-finite values parse and are dropped with their triangle.
            Some(w) => w
                .parse::<f64>()
                .map_err(|_| err_at(self.line, format!("number expected, found `{w}`"))),
            None => Err(err_at(self.line, "file ends where a number was expected")),
        }
    }

    fn vec3(&mut self) -> Result<Vec3, ReadError> {
        Ok(Vec3::new(self.number()?, self.number()?, self.number()?))
    }
}

fn read_ascii(
    bytes: &[u8],
    limits: &Limits,
    report: &mut MeshReport,
) -> Result<Vec<RawTri>, ReadError> {
    let text = String::from_utf8_lossy(bytes);
    let mut t = Tokens {
        lines: text.lines().enumerate(),
        words: "".split_ascii_whitespace(),
        line: 0,
        max: limits.max_token,
    };
    let mut out: Vec<RawTri> = Vec::new();
    let mut in_solid = false;
    while let Some(w) = t.next()? {
        let kw = w.to_ascii_lowercase();
        match (kw.as_str(), in_solid) {
            ("solid", false) => {
                report.solids.push(t.rest_of_line());
                in_solid = true;
            }
            ("endsolid", true) => {
                t.rest_of_line();
                in_solid = false;
            }
            ("facet", true) => {
                if out.len() >= limits.max_triangles {
                    return Err(err_at(
                        t.line,
                        format!("more than {} triangles", limits.max_triangles),
                    ));
                }
                t.expect("normal")?;
                let n = t.vec3()?;
                t.expect("outer")?;
                t.expect("loop")?;
                let mut p = [Point3::ORIGIN; 3];
                for q in &mut p {
                    t.expect("vertex")?;
                    *q = Point3(t.vec3()?.0);
                }
                t.expect("endloop")?;
                t.expect("endfacet")?;
                out.push((n, p));
            }
            _ if out.is_empty() && report.solids.is_empty() => {
                return Err(err_at(
                    t.line,
                    "not an STL file: the size does not match a binary STL and the text does not start with `solid`",
                ));
            }
            _ => return Err(err_at(t.line, format!("unexpected `{w}`"))),
        }
    }
    if report.solids.is_empty() {
        return Err(err("empty file"));
    }
    if in_solid {
        report.warn("no `endsolid`; the file may be truncated".into());
    }
    Ok(out)
}

/// Welds, drops degenerate triangles, orients by the vote, and builds the
/// adjacency.
fn build(
    raw: Vec<RawTri>,
    scale: f64,
    tol: f64,
    limits: &Limits,
    report: &mut MeshReport,
) -> MeshBody {
    let max = limits.max_coord;
    let mut corners: Vec<Point3> = Vec::with_capacity(raw.len() * 3);
    let mut normals: Vec<Vec3> = Vec::with_capacity(raw.len());
    for (n, p) in &raw {
        if p.iter()
            .all(|q| q.is_finite() && q.0.abs().max_element() <= max)
        {
            corners.extend(p.iter().map(|q| Point3(q.0 * scale)));
            normals.push(if n.is_finite() { *n } else { Vec3::ZERO });
        } else {
            report.dropped += 1;
        }
    }
    if report.dropped > 0 {
        report.warn(format!(
            "{} triangles with coordinates that are not finite or beyond {max}",
            report.dropped
        ));
    }
    drop(raw);
    let rep = weld_points(&corners, tol);
    // Representatives are the earliest of their group, so numbering them in
    // order of index keeps the order of first appearance.
    let mut index = vec![u32::MAX; corners.len()];
    let mut positions = Vec::new();
    for (i, &r) in rep.iter().enumerate() {
        if r as usize == i {
            index[i] = positions.len() as u32;
            positions.push(corners[i]);
        }
    }
    let min_area = tol * tol;
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(normals.len());
    for (k, n) in normals.iter().enumerate() {
        let t = [0, 1, 2].map(|j| index[rep[3 * k + j] as usize]);
        let [a, b, c] = t.map(|i| positions[i as usize]);
        let cross = (b - a).cross(c - a);
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] || 0.5 * cross.length() < min_area {
            report.dropped += 1;
            continue;
        }
        let d = cross.dot(*n);
        if d > 0.0 {
            report.normals_agree += 1;
        } else if d < 0.0 {
            report.normals_disagree += 1;
        }
        tris.push(t);
    }
    if report.normals_disagree > report.normals_agree {
        for t in &mut tris {
            t.swap(1, 2);
        }
        report.flipped = true;
    }
    let (
        neighbours,
        EdgeCounts {
            open,
            non_manifold,
            same_direction,
        },
    ) = adjacency(&tris);
    report.open_edges = open;
    report.boundary_loops = boundary_loops(&open_edges(&tris));
    report.non_manifold_edges = non_manifold;
    report.inconsistent_edges = same_direction;
    MeshBody {
        name: String::new(),
        tol_model: tol,
        positions,
        tris,
        neighbours,
    }
}
