// SPDX-License-Identifier: MIT OR Apache-2.0

//! DXF reader (design D05 §3).
//!
//! An own reader of ASCII group-code pairs, so that every count is bounded
//! (CH-09). Only the `HEADER` (for `$INSUNITS`), `BLOCKS` and `ENTITIES`
//! sections are read. Curves keep their kind: arcs stay circles, ellipses
//! stay ellipses, splines stay B-splines. Each layer becomes one wire body.

use crate::conic::{arc, line, transform};
use crate::report::{Import2D, ImportReport, Limits, Options, ReadError, Unit};
use fluut_geom::bspline::basis::{SMALL, ders_basis_funs_n, find_span};
use fluut_geom::{Affine, BSplineCurve3, Curve3, Interval, Point3, Vec3};
use fluut_topo::{WireBody, WireCurve};
use glam::{DMat3, DVec3};
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

#[cfg(test)]
mod tests;

/// Reads a DXF file into one wire body per layer, in the order in which the
/// layers first produce a curve.
///
/// `opts.scale` is mm per drawing unit. DXF carries no reliable unit
/// (spec 02 §2.6.2), so the caller must supply it; with `None` the curves
/// stay in drawing units. `$INSUNITS` is returned in `report.unit` only as a
/// suggestion.
pub fn read_dxf(bytes: &[u8], opts: &Options) -> Result<Import2D, ReadError> {
    let limits = opts.limits;
    if bytes.len() > limits.max_bytes {
        return Err(ReadError::new(format!(
            "file is {} bytes; the limit is {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(ReadError::new(
            "binary DXF is not supported; save the drawing as ASCII DXF",
        ));
    }
    let text = String::from_utf8_lossy(bytes);
    let mut report = ImportReport::default();
    let dxf = parse(&text, &limits, &mut report)?;
    report.unit = dxf.insunits.and_then(|code| {
        let u = insunits(code);
        if u.is_none() && code != 0 {
            report.warn(format!("$INSUNITS {code} is not a unit FluutCAM knows"));
        }
        u
    });
    let s = opts.scale.unwrap_or(1.0);
    if !(s.is_finite() && s > 0.0) {
        return Err(ReadError::new(format!("bad scale {s}")));
    }
    let mut out = Out {
        layers: Vec::new(),
        index: BTreeMap::new(),
        curves: 0,
        items: 0,
        limits,
        report,
    };
    let root = Affine::scale(s, s, s);
    for e in &dxf.entities {
        out.emit(e, &root, None, 0, &dxf)?;
    }
    let Out {
        layers, mut report, ..
    } = out;
    let wires: Vec<WireBody> = layers
        .into_iter()
        .map(|(name, curves)| WireBody {
            name,
            tol_model: opts.tol_model,
            curves,
        })
        .collect();
    report.count_chains(&wires, opts.tol_model);
    Ok(Import2D { wires, report })
}

/// The unit of a `$INSUNITS` code, for the codes FluutCAM can name.
pub fn insunits(code: i64) -> Option<Unit> {
    match code {
        1 => Some(Unit::In),
        2 => Some(Unit::Ft),
        4 => Some(Unit::Mm),
        5 => Some(Unit::Cm),
        6 => Some(Unit::M),
        13 => Some(Unit::Um),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Group-code pairs and entities

#[derive(Debug, Clone, Copy)]
struct Pair<'a> {
    code: i32,
    value: &'a str,
    line: u32,
}

struct Pairs<'a> {
    lines: std::str::Split<'a, char>,
    line: u32,
    peeked: Option<Pair<'a>>,
}

impl<'a> Pairs<'a> {
    fn new(text: &'a str) -> Pairs<'a> {
        Pairs {
            lines: text.split('\n'),
            line: 0,
            peeked: None,
        }
    }

    fn next_line(&mut self) -> Option<&'a str> {
        let l = self.lines.next()?;
        self.line += 1;
        Some(l.strip_suffix('\r').unwrap_or(l))
    }

    /// The next pair; `Ok(None)` at the end of the text.
    fn next(&mut self) -> Result<Option<Pair<'a>>, ReadError> {
        if let Some(p) = self.peeked.take() {
            return Ok(Some(p));
        }
        let code_line = loop {
            match self.next_line() {
                None => return Ok(None),
                // Blank lines between pairs (some writers end with one).
                Some(l) if l.trim().is_empty() => continue,
                Some(l) => break l,
            }
        };
        let line = self.line;
        let code: i32 = code_line.trim().parse().map_err(|_| {
            ReadError::at(line, format!("group code expected, found {code_line:?}"))
        })?;
        let value = self
            .next_line()
            .ok_or_else(|| ReadError::at(line, "file ends after a group code"))?;
        Ok(Some(Pair {
            code,
            value: value.trim(),
            line,
        }))
    }

    fn push_back(&mut self, p: Pair<'a>) {
        self.peeked = Some(p);
    }
}

/// One entity: its kind, its pairs in order, and the `VERTEX` entities of a
/// `POLYLINE`.
#[derive(Debug, Clone)]
struct Ent<'a> {
    kind: &'a str,
    line: u32,
    codes: Vec<(i32, &'a str)>,
    children: Vec<Ent<'a>>,
}

/// Why an entity gave no curves: malformed (`Bad`, the entity is dropped
/// and reported) or a limit reached (`Stop`, the import fails).
#[derive(Debug)]
enum Fail {
    Bad(String),
    Stop(ReadError),
}

impl From<ReadError> for Fail {
    fn from(e: ReadError) -> Fail {
        Fail::Stop(e)
    }
}

fn bad(msg: impl Into<String>) -> Fail {
    Fail::Bad(msg.into())
}

impl<'a> Ent<'a> {
    fn get(&self, code: i32) -> Option<&'a str> {
        self.codes.iter().find(|c| c.0 == code).map(|c| c.1)
    }

    fn num(&self, code: i32, default: f64, max: f64) -> Result<f64, Fail> {
        match self.get(code) {
            None => Ok(default),
            Some(v) => number(v, max).ok_or_else(|| bad(format!("bad number {v:?} (code {code})"))),
        }
    }

    fn int(&self, code: i32, default: i64) -> Result<i64, Fail> {
        match self.get(code) {
            None => Ok(default),
            Some(v) => v
                .parse::<i64>()
                .map_err(|_| bad(format!("bad integer {v:?} (code {code})"))),
        }
    }

    /// The point in codes `base`, `base + 10`, `base + 20`; missing
    /// coordinates are 0.
    fn point(&self, base: i32, max: f64) -> Result<Point3, Fail> {
        Ok(Point3::new(
            self.num(base, 0.0, max)?,
            self.num(base + 10, 0.0, max)?,
            self.num(base + 20, 0.0, max)?,
        ))
    }

    /// The extrusion direction (codes 210–230), default +Z.
    fn extrusion(&self, max: f64) -> Result<Vec3, Fail> {
        Ok(Vec3::new(
            self.num(210, 0.0, max)?,
            self.num(220, 0.0, max)?,
            self.num(230, 1.0, max)?,
        ))
    }

    /// Repeated points: each `x_code` starts a point, and the `x_code + 10`
    /// and `x_code + 20` that follow fill it. Other codes are passed to
    /// `other` with the index of the current point.
    fn points(
        &self,
        x_code: i32,
        limits: &Limits,
        mut other: impl FnMut(i32, &str, usize) -> Result<(), Fail>,
    ) -> Result<Vec<Point3>, Fail> {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for &(c, v) in &self.codes {
            let coord = if c == x_code {
                if pts.len() >= limits.max_points {
                    return Err(bad(format!("more than {} points", limits.max_points)));
                }
                pts.push([0.0; 3]);
                Some(0)
            } else if c == x_code + 10 {
                Some(1)
            } else if c == x_code + 20 {
                Some(2)
            } else {
                None
            };
            match (coord, pts.last_mut()) {
                (Some(k), Some(p)) => {
                    p[k] = number(v, limits.max_coord)
                        .ok_or_else(|| bad(format!("bad number {v:?} (code {c})")))?;
                }
                (Some(_), None) => {}
                (None, _) => other(c, v, pts.len().wrapping_sub(1))?,
            }
        }
        Ok(pts
            .into_iter()
            .map(|p| Point3::new(p[0], p[1], p[2]))
            .collect())
    }
}

/// A finite number of magnitude at most `max`.
fn number(v: &str, max: f64) -> Option<f64> {
    v.parse::<f64>()
        .ok()
        .filter(|x| x.is_finite() && x.abs() <= max)
}

struct Block<'a> {
    base: Point3,
    entities: Vec<Ent<'a>>,
}

struct Dxf<'a> {
    insunits: Option<i64>,
    /// By upper-case name (block names are case-insensitive).
    blocks: BTreeMap<String, Block<'a>>,
    entities: Vec<Ent<'a>>,
}

fn parse<'a>(
    text: &'a str,
    limits: &Limits,
    report: &mut ImportReport,
) -> Result<Dxf<'a>, ReadError> {
    let mut p = Pairs::new(text);
    let mut dxf = Dxf {
        insunits: None,
        blocks: BTreeMap::new(),
        entities: Vec::new(),
    };
    let mut items = 0usize;
    let mut seen_eof = false;
    while let Some(pair) = p.next()? {
        match (pair.code, pair.value) {
            (0, "SECTION") => {
                let name = match p.next()? {
                    Some(Pair { code: 2, value, .. }) => value,
                    Some(other) => {
                        p.push_back(other);
                        ""
                    }
                    None => break,
                };
                match name {
                    "HEADER" => dxf.insunits = header(&mut p)?.or(dxf.insunits),
                    "BLOCKS" => blocks(&mut p, &mut dxf.blocks, limits, &mut items, report)?,
                    "ENTITIES" => {
                        let (ents, end) =
                            entities(&mut p, &["ENDSEC"], limits, &mut items, report)?;
                        dxf.entities.extend(ents);
                        if end.is_none() {
                            report.warn("file ends inside the ENTITIES section");
                        }
                    }
                    _ => skip_section(&mut p)?,
                }
            }
            (0, "EOF") => {
                seen_eof = true;
                break;
            }
            _ => {}
        }
    }
    if !seen_eof {
        report.warn("no EOF marker; the file may be truncated");
    }
    Ok(dxf)
}

fn skip_section(p: &mut Pairs<'_>) -> Result<(), ReadError> {
    while let Some(pair) = p.next()? {
        if pair.code == 0 && (pair.value == "ENDSEC" || pair.value == "EOF") {
            if pair.value == "EOF" {
                p.push_back(pair);
            }
            break;
        }
    }
    Ok(())
}

/// `$INSUNITS` from the header section.
fn header(p: &mut Pairs<'_>) -> Result<Option<i64>, ReadError> {
    let mut var = "";
    let mut units = None;
    while let Some(pair) = p.next()? {
        match pair.code {
            0 => {
                if pair.value == "EOF" {
                    p.push_back(pair);
                }
                break;
            }
            9 => var = pair.value,
            70 if var == "$INSUNITS" => units = pair.value.parse().ok(),
            _ => {}
        }
    }
    Ok(units)
}

fn blocks<'a>(
    p: &mut Pairs<'a>,
    out: &mut BTreeMap<String, Block<'a>>,
    limits: &Limits,
    items: &mut usize,
    report: &mut ImportReport,
) -> Result<(), ReadError> {
    while let Some(pair) = p.next()? {
        if pair.code != 0 {
            continue;
        }
        match pair.value {
            "BLOCK" => {
                let head = read_codes(p, pair, limits)?;
                let (entities, _) = entities(p, &["ENDBLK", "ENDSEC"], limits, items, report)?;
                // ENDBLK's own pairs are read as the next entity's prefix and
                // skipped by the loop.
                let base = head.point(10, limits.max_coord).unwrap_or(Point3::ORIGIN);
                let name = head.get(2).unwrap_or("").to_uppercase();
                out.insert(name, Block { base, entities });
            }
            "ENDSEC" => break,
            "EOF" => {
                p.push_back(pair);
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

/// The pairs of the entity that `start` opens, up to the next code 0.
fn read_codes<'a>(
    p: &mut Pairs<'a>,
    start: Pair<'a>,
    limits: &Limits,
) -> Result<Ent<'a>, ReadError> {
    let mut e = Ent {
        kind: start.value,
        line: start.line,
        codes: Vec::new(),
        children: Vec::new(),
    };
    while let Some(pair) = p.next()? {
        if pair.code == 0 {
            p.push_back(pair);
            break;
        }
        if e.codes.len() >= 4 * limits.max_points + 64 {
            return Err(ReadError::at(
                start.line,
                format!("{} has too many values", e.kind),
            ));
        }
        e.codes.push((pair.code, pair.value));
    }
    Ok(e)
}

/// Entities up to one of the `end` markers (returned, consumed) or the end
/// of the file (`None`). `VERTEX` entities are attached to the `POLYLINE`
/// before them; `SEQEND` closes it.
fn entities<'a>(
    p: &mut Pairs<'a>,
    end: &[&str],
    limits: &Limits,
    items: &mut usize,
    report: &mut ImportReport,
) -> Result<(Vec<Ent<'a>>, Option<&'a str>), ReadError> {
    let mut out: Vec<Ent<'a>> = Vec::new();
    let mut open_polyline = false;
    while let Some(pair) = p.next()? {
        if pair.code != 0 {
            continue;
        }
        if end.contains(&pair.value) {
            return Ok((out, Some(pair.value)));
        }
        if pair.value == "EOF" {
            p.push_back(pair);
            return Ok((out, None));
        }
        *items += 1;
        if *items > limits.max_items {
            return Err(ReadError::at(
                pair.line,
                format!("more than {} entities", limits.max_items),
            ));
        }
        let e = read_codes(p, pair, limits)?;
        match e.kind {
            "VERTEX" if open_polyline => {
                if let Some(parent) = out.last_mut() {
                    if parent.children.len() >= limits.max_points {
                        return Err(ReadError::at(e.line, "POLYLINE has too many vertices"));
                    }
                    parent.children.push(e);
                }
            }
            "VERTEX" => report.warn(format!("line {}: VERTEX outside a POLYLINE", e.line)),
            "SEQEND" => open_polyline = false,
            "ATTRIB" => report.ignore("ATTRIB"),
            _ => {
                open_polyline = e.kind == "POLYLINE";
                out.push(e);
            }
        }
    }
    Ok((out, None))
}

// ---------------------------------------------------------------------------
// Entities to curves

struct Out {
    layers: Vec<(String, Vec<WireCurve>)>,
    index: BTreeMap<String, usize>,
    curves: usize,
    items: usize,
    limits: Limits,
    report: ImportReport,
}

const IGNORED: &[&str] = &[
    "TEXT",
    "MTEXT",
    "DIMENSION",
    "ARC_DIMENSION",
    "LARGE_RADIAL_DIMENSION",
    "HATCH",
    "MPOLYGON",
    "POINT",
    "LEADER",
    "MLEADER",
    "MULTILEADER",
    "TOLERANCE",
    "IMAGE",
    "WIPEOUT",
    "VIEWPORT",
    "ATTDEF",
    "OLE2FRAME",
    "XLINE",
    "RAY",
    "SOLID",
    "TRACE",
    "SHAPE",
    "LIGHT",
    "SUN",
    "TABLE",
    "UNDERLAY",
    "PDFUNDERLAY",
    "DWFUNDERLAY",
    "DGNUNDERLAY",
];

impl Out {
    /// Adds `c` mapped by `m` to `layer`, or counts it as dropped.
    fn push(&mut self, layer: &str, c: Option<WireCurve>, m: &Affine) -> Result<(), ReadError> {
        let Some(c) = c.and_then(|c| transform(&c, m)) else {
            self.report.dropped += 1;
            return Ok(());
        };
        self.curves += 1;
        if self.curves > self.limits.max_curves {
            return Err(ReadError::new(format!(
                "more than {} curves",
                self.limits.max_curves
            )));
        }
        let i = match self.index.get(layer) {
            Some(&i) => i,
            None => {
                self.layers.push((layer.to_owned(), Vec::new()));
                self.index.insert(layer.to_owned(), self.layers.len() - 1);
                self.layers.len() - 1
            }
        };
        self.layers[i].1.push(c);
        Ok(())
    }

    /// Adds the curves of `e`, mapped by `xf`. `layer0` is the layer that
    /// entities on layer `0` take inside a block.
    fn emit(
        &mut self,
        e: &Ent<'_>,
        xf: &Affine,
        layer0: Option<&str>,
        depth: usize,
        dxf: &Dxf<'_>,
    ) -> Result<(), ReadError> {
        self.items += 1;
        if self.items > self.limits.max_items {
            return Err(ReadError::new(format!(
                "more than {} entities after block expansion",
                self.limits.max_items
            )));
        }
        if e.get(67) == Some("1") {
            self.report.ignore("paper space");
            return Ok(());
        }
        let own = e.get(8).unwrap_or("0");
        let layer = match layer0 {
            Some(l) if own == "0" => l,
            _ => own,
        };
        match self.entity(e, layer, xf, depth, dxf) {
            Ok(()) => Ok(()),
            Err(Fail::Bad(msg)) => {
                self.report.dropped += 1;
                self.report
                    .warn(format!("line {}: {}: {msg}; dropped", e.line, e.kind));
                Ok(())
            }
            Err(Fail::Stop(err)) => Err(err),
        }
    }

    /// The curves of one entity.
    fn entity(
        &mut self,
        e: &Ent<'_>,
        layer: &str,
        xf: &Affine,
        depth: usize,
        dxf: &Dxf<'_>,
    ) -> Result<(), Fail> {
        let max = self.limits.max_coord;
        match e.kind {
            "LINE" => {
                let c = line(e.point(10, max)?, e.point(11, max)?);
                self.push(layer, c, xf)?;
            }
            "CIRCLE" | "ARC" => {
                let m = ocs(e.extrusion(max)?)?.then(xf);
                let c = e.point(10, max)?;
                let r = e.num(40, 0.0, max)?;
                let range = if e.kind == "CIRCLE" {
                    Interval { lo: 0.0, hi: TAU }
                } else {
                    let a0 = e.num(50, 0.0, max)?.to_radians();
                    let sweep = (e.num(51, 0.0, max)?.to_radians() - a0).rem_euclid(TAU);
                    Interval {
                        lo: a0,
                        hi: a0 + sweep,
                    }
                };
                let a = arc(c, Vec3::new(r, 0.0, 0.0), Vec3::new(0.0, r, 0.0), range);
                self.push(layer, a, &m)?;
            }
            "ELLIPSE" => {
                let c = e.point(10, max)?;
                let major = e.point(11, max)?.to_vec();
                let n = e
                    .extrusion(max)?
                    .normalized()
                    .ok_or_else(|| bad("zero extrusion"))?;
                let ratio = e.num(40, 1.0, max)?;
                let lo = e.num(41, 0.0, max)?;
                let mut hi = e.num(42, TAU, max)?;
                if hi <= lo {
                    hi += TAU * (((lo - hi) / TAU).floor() + 1.0);
                }
                let minor = n.get().cross(major) * ratio;
                let a = arc(c, major, minor, Interval { lo, hi });
                self.push(layer, a, xf)?;
            }
            "LWPOLYLINE" => {
                let m = ocs(e.extrusion(max)?)?.then(xf);
                let mut bulges: Vec<f64> = Vec::new();
                let pts = e.points(10, &self.limits, |c, v, i| {
                    if c == 42 && i != usize::MAX {
                        bulges.resize(i + 1, 0.0);
                        bulges[i] =
                            number(v, max).ok_or_else(|| bad(format!("bad bulge {v:?}")))?;
                    }
                    Ok(())
                })?;
                let z = e.num(38, 0.0, max)?;
                let closed = e.int(70, 0)? & 1 != 0;
                let verts: Vec<(Point3, f64)> = pts
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let b = bulges.get(i).copied().unwrap_or(0.0);
                        (Point3::new(p.x(), p.y(), z), b)
                    })
                    .collect();
                self.polyline(layer, &verts, closed, &m)?;
            }
            "POLYLINE" => self.old_polyline(e, layer, xf)?,
            "SPLINE" => {
                let c = spline(e, &self.limits)?;
                self.push(layer, Some(c), xf)?;
            }
            "INSERT" => self.insert(e, layer, xf, depth, dxf)?,
            k if IGNORED.contains(&k) => self.report.ignore(k),
            k => self.report.unsupported(k),
        }
        Ok(())
    }

    /// Lines and bulge arcs through `verts` (point, bulge to the next point).
    fn polyline(
        &mut self,
        layer: &str,
        verts: &[(Point3, f64)],
        closed: bool,
        m: &Affine,
    ) -> Result<(), Fail> {
        let n = verts.len();
        let segs = if closed && n > 1 && verts[0].0 != verts[n - 1].0 {
            n
        } else {
            n.saturating_sub(1)
        };
        for i in 0..segs {
            let (p0, b) = verts[i];
            let p1 = verts[(i + 1) % n].0;
            self.push(layer, bulge_segment(p0, p1, b), m)?;
        }
        Ok(())
    }

    /// `POLYLINE` with its `VERTEX` children: 2-D (in OCS, with bulges) or
    /// 3-D (a line chain). Meshes are unsupported.
    fn old_polyline(&mut self, e: &Ent<'_>, layer: &str, xf: &Affine) -> Result<(), Fail> {
        let max = self.limits.max_coord;
        let flags = e.int(70, 0)?;
        if flags & (16 | 64) != 0 {
            self.report.unsupported("POLYLINE mesh");
            return Ok(());
        }
        let closed = flags & 1 != 0;
        let three_d = flags & 8 != 0;
        let z = e.point(10, max)?.z();
        let mut verts = Vec::with_capacity(e.children.len());
        for v in &e.children {
            // Spline frame control points are not on the curve.
            if v.int(70, 0)? & 16 != 0 {
                continue;
            }
            let p = v.point(10, max)?;
            if three_d {
                verts.push((p, 0.0));
            } else {
                verts.push((Point3::new(p.x(), p.y(), z), v.num(42, 0.0, max)?));
            }
        }
        let m = if three_d {
            *xf
        } else {
            ocs(e.extrusion(max)?)?.then(xf)
        };
        self.polyline(layer, &verts, closed, &m)
    }

    /// A block reference: the block's entities mapped by the insert's
    /// placement, for each copy of a column and row array.
    fn insert(
        &mut self,
        e: &Ent<'_>,
        layer: &str,
        xf: &Affine,
        depth: usize,
        dxf: &Dxf<'_>,
    ) -> Result<(), Fail> {
        let max = self.limits.max_coord;
        let name = e.get(2).unwrap_or("");
        let Some(block) = dxf.blocks.get(&name.to_uppercase()) else {
            self.report
                .warn(format!("line {}: INSERT of unknown block {name:?}", e.line));
            return Ok(());
        };
        if depth + 1 > self.limits.max_depth {
            self.report.warn(format!(
                "line {}: block {name:?} nested deeper than {}; not expanded",
                e.line, self.limits.max_depth
            ));
            return Ok(());
        }
        let at = e.point(10, max)?;
        let (sx, sy, sz) = (
            e.num(41, 1.0, max)?,
            e.num(42, 1.0, max)?,
            e.num(43, 1.0, max)?,
        );
        let rot = e.num(50, 0.0, max)?.to_radians();
        let cols = e.int(70, 1)?.max(1);
        let rows = e.int(71, 1)?.max(1);
        let (dc, dr) = (e.num(44, 0.0, max)?, e.num(45, 0.0, max)?);
        if cols.saturating_mul(rows) as u64 > self.limits.max_items as u64 {
            return Err(ReadError::at(e.line, format!("INSERT array of {cols} × {rows}")).into());
        }
        let place = ocs(e.extrusion(max)?)?;
        let (s, c) = libm::sincos(rot);
        let rz = Affine::from_mat3_translation(
            DMat3::from_cols(DVec3::new(c, s, 0.0), DVec3::new(-s, c, 0.0), DVec3::Z),
            Vec3::ZERO,
        );
        let base =
            Affine::translation(Point3::ORIGIN - block.base).then(&Affine::scale(sx, sy, sz));
        let after = rz
            .then(&Affine::translation(at.to_vec()))
            .then(&place)
            .then(xf);
        for r in 0..rows {
            for k in 0..cols {
                let offset = Vec3::new(k as f64 * dc, r as f64 * dr, 0.0);
                let m = base.then(&Affine::translation(offset)).then(&after);
                for child in &block.entities {
                    self.emit(child, &m, Some(layer), depth + 1, dxf)?;
                }
            }
        }
        Ok(())
    }
}

/// The object coordinate system of extrusion `n` (arbitrary-axis algorithm):
/// OCS coordinates to WCS.
fn ocs(n: Vec3) -> Result<Affine, Fail> {
    let n = n.normalized().ok_or_else(|| bad("zero extrusion"))?.get();
    let limit = 1.0 / 64.0;
    let w = if n.x().abs() < limit && n.y().abs() < limit {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let ax = w
        .cross(n)
        .normalized()
        .ok_or_else(|| bad("bad extrusion"))?
        .get();
    let ay = n.cross(ax);
    Ok(Affine::from_mat3_translation(
        DMat3::from_cols(ax.0, ay.0, n.0),
        Vec3::ZERO,
    ))
}

/// The segment from `p0` to `p1` (same `z`) with `bulge = tan(θ/4)`, where
/// `θ` is the included angle, positive counter-clockwise about +Z.
pub(crate) fn bulge_segment(p0: Point3, p1: Point3, bulge: f64) -> Option<WireCurve> {
    if bulge.abs() < 1e-12 {
        return line(p0, p1);
    }
    let d = p1 - p0;
    let c = libm::hypot(d.x(), d.y());
    if !(c > 0.0 && bulge.is_finite()) {
        return None;
    }
    let left = Vec3::new(-d.y() / c, d.x() / c, 0.0);
    let mid = p0.lerp(p1, 0.5);
    let centre = mid + left * (0.5 * c * (1.0 - bulge * bulge) / (2.0 * bulge));
    let theta = 4.0 * libm::atan(bulge.abs());
    let x = p0 - centre;
    let z = if bulge > 0.0 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(0.0, 0.0, -1.0)
    };
    let y = z.cross(x);
    arc(centre, x, y, Interval { lo: 0.0, hi: theta })
}

/// A `SPLINE`: from its control points, or interpolated through its fit
/// points when it has none.
fn spline(e: &Ent<'_>, limits: &Limits) -> Result<WireCurve, Fail> {
    let max = limits.max_coord;
    let degree = e.int(71, 3)?;
    let flags = e.int(70, 0)?;
    let mut knots = Vec::new();
    let mut weights = Vec::new();
    let ctrl = e.points(10, limits, |c, v, _| {
        let target = match c {
            40 => &mut knots,
            41 => &mut weights,
            _ => return Ok(()),
        };
        if target.len() >= limits.max_points + 32 {
            return Err(bad("too many knots or weights"));
        }
        target.push(number(v, max).ok_or_else(|| bad(format!("bad number {v:?} (code {c})")))?);
        Ok(())
    })?;
    let curve = if ctrl.is_empty() {
        let fit = e.points(11, limits, |_, _, _| Ok(()))?;
        interpolate(&fit).ok_or_else(|| bad("fit points do not define a curve"))?
    } else {
        if !(1..=25).contains(&degree) {
            return Err(bad(format!("degree {degree}")));
        }
        let rational = flags & 4 != 0 || weights.iter().any(|&w| w != 1.0);
        let w = (rational && weights.len() == ctrl.len()).then_some(weights);
        BSplineCurve3::new(degree as u8, knots, ctrl, w).map_err(|err| bad(err.to_string()))?
    };
    let range = curve.domain();
    Ok(WireCurve {
        curve: Curve3::BSpline(Arc::new(curve)),
        range,
    })
}

/// The cubic (or lower, for fewer points) B-spline through `points`, with
/// chord-length parameters and averaged knots (*The NURBS Book* §9.2.1).
/// Consecutive duplicates are skipped. `None` for fewer than two distinct
/// points.
pub(crate) fn interpolate(points: &[Point3]) -> Option<BSplineCurve3> {
    let mut q: Vec<Point3> = Vec::with_capacity(points.len());
    for &p in points {
        if q.last() != Some(&p) {
            q.push(p);
        }
    }
    let n = q.len().checked_sub(1)?;
    if n == 0 {
        return None;
    }
    let p = n.min(3);
    let chords: Vec<f64> = q.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f64 = chords.iter().sum();
    if !(total.is_finite() && total > 0.0) {
        return None;
    }
    let mut ub = vec![0.0; n + 1];
    for k in 1..n {
        ub[k] = ub[k - 1] + chords[k - 1] / total;
    }
    ub[n] = 1.0;
    let mut knots = vec![0.0; p + 1];
    for j in 1..=n - p {
        knots.push(ub[j..j + p].iter().sum::<f64>() / p as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    // Banded system N·P = Q: row k has non-zeros in columns span−p..=span,
    // within k−p..=k+p. Gaussian elimination without pivoting keeps the band
    // (the matrix is totally positive).
    let w = 2 * p + 1;
    let at = |i: usize, j: usize| i * w + (j + p - i);
    let mut a = vec![0.0; (n + 1) * w];
    for (k, &u) in ub.iter().enumerate() {
        let span = find_span(n, p, u, &knots);
        let basis = ders_basis_funs_n::<SMALL>(span, u, p, 0, &knots);
        for (j, &value) in basis[0][..=p].iter().enumerate() {
            let col = span - p + j;
            if col + p < k || col > k + p {
                return None;
            }
            a[at(k, col)] = value;
        }
    }
    let mut rhs: Vec<Vec3> = q.iter().map(|x| x.to_vec()).collect();
    for k in 0..=n {
        let pivot = a[at(k, k)];
        if !pivot.is_finite() || pivot.abs() <= 1e-300 {
            return None;
        }
        for i in k + 1..=(k + p).min(n) {
            let f = a[at(i, k)] / pivot;
            if f == 0.0 {
                continue;
            }
            for j in k..=(k + p).min(n) {
                a[at(i, j)] -= f * a[at(k, j)];
            }
            let r = rhs[k];
            rhs[i] -= r * f;
        }
    }
    let mut poles = vec![Vec3::ZERO; n + 1];
    for k in (0..=n).rev() {
        let mut s = rhs[k];
        for j in k + 1..=(k + p).min(n) {
            s -= poles[j] * a[at(k, j)];
        }
        poles[k] = s / a[at(k, k)];
    }
    let poles = poles.into_iter().map(|v| Point3(v.0)).collect();
    BSplineCurve3::new(p as u8, knots, poles, None).ok()
}
