// SPDX-License-Identifier: MIT OR Apache-2.0

//! SVG reader (design D05 §4).
//!
//! The XML tree comes from `roxmltree`, which bounds entity expansion and
//! never loads external entities. Path data,
//! transform lists and lengths have their own parsers ([`path`]). Shapes
//! and paths are mapped through the composed transforms and the document's
//! unit and Y flip, keeping arcs as circles and ellipses.

mod path;
#[cfg(test)]
mod tests;

use crate::conic::{line, transform};
use crate::report::{Import2D, ImportReport, Limits, Options, ReadError, Unit};
use fluut_geom::{Affine, Point3, Vec3};
use fluut_topo::{WireBody, WireCurve};
use glam::{DMat3, DVec3};
use path::{Builder, Scanner, ellipse, numbers, parse_path, rect};
use roxmltree::{Document, Node, NodeId, ParsingOptions};
use std::collections::BTreeMap;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
/// Deepest element nesting walked. `use` nesting has its own limit
/// ([`Limits::max_depth`]).
const MAX_NESTING: usize = 256;
/// Millimetres per CSS pixel.
const PX_MM: f64 = 25.4 / 96.0;

/// Elements skipped on purpose and counted in the report.
const IGNORED: &[&str] = &[
    "text",
    "image",
    "foreignObject",
    "filter",
    "style",
    "script",
    "clipPath",
    "mask",
    "marker",
    "pattern",
    "linearGradient",
    "radialGradient",
    "audio",
    "video",
    "canvas",
    "iframe",
];

/// Elements skipped without a report: no geometry and nothing lost.
const SILENT: &[&str] = &["defs", "symbol", "title", "desc", "metadata", "view"];

/// Reads an SVG file into one wire body (`name` "svg"), in mm with Y up.
///
/// With `opts.scale` set, user units are taken as that many mm and the
/// root size is not used; otherwise the size and `viewBox` give the unit,
/// with user units of 1 px (1/96 in) when there is no `viewBox`. The unit the
/// document declares is returned in `report.unit`.
pub fn read_svg(bytes: &[u8], opts: &Options) -> Result<Import2D, ReadError> {
    let limits = opts.limits;
    if bytes.len() > limits.max_bytes {
        return Err(ReadError::new(format!(
            "file is {} bytes; the limit is {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)
        .map_err(|e| ReadError::new(format!("not UTF-8 text (byte {})", e.valid_up_to())))?;
    let popts = ParsingOptions {
        // Illustrator writes DOCTYPEs. `roxmltree` bounds entity expansion
        // and resolves no external entities.
        allow_dtd: true,
        nodes_limit: u32::try_from(limits.max_items).unwrap_or(u32::MAX),
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(text, popts)
        .map_err(|e| ReadError::at(e.pos().row, e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" || !svg_ns(root) {
        return Err(ReadError::new("not an SVG document"));
    }
    let mut report = ImportReport::default();
    let (doc_xf, unit) = document_transform(root, opts.scale, &mut report)?;
    report.unit = Some(unit);
    let mut ids: BTreeMap<&str, NodeId> = BTreeMap::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        if let Some(id) = n.attribute("id") {
            ids.entry(id).or_insert(n.id());
        }
    }
    let mut w = Walker {
        doc: &doc,
        ids,
        limits,
        curves: Vec::new(),
        items: 0,
        uses: Vec::new(),
        report,
    };
    w.children(root, &doc_xf, 0)?;
    let Walker {
        curves, mut report, ..
    } = w;
    let wires = if curves.is_empty() {
        Vec::new()
    } else {
        vec![WireBody {
            name: "svg".into(),
            tol_model: opts.tol_model,
            curves,
        }]
    };
    report.count_chains(&wires, opts.tol_model);
    Ok(Import2D { wires, report })
}

fn svg_ns(n: Node<'_, '_>) -> bool {
    matches!(n.tag_name().namespace(), None | Some(SVG_NS))
}

/// A length with its unit, in px; `None` for percentages, font-relative
/// units and junk.
fn length_px(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s
        .find(|c: char| (c.is_ascii_alphabetic() && c != 'e' && c != 'E') || c == '%')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let mut sc = Scanner::new(num);
    let v = sc.number()?;
    if !sc.at_end() {
        return None;
    }
    let k = match unit {
        "" | "px" => 1.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "Q" => 96.0 / 101.6,
        "in" => 96.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        _ => return None,
    };
    Some(v * k)
}

/// The unit a root `width` declares.
fn declared_unit(s: &str) -> Unit {
    let s = s.trim();
    if s.ends_with("mm") || s.ends_with('Q') {
        Unit::Mm
    } else if s.ends_with("cm") {
        Unit::Cm
    } else if s.ends_with("in") || s.ends_with("pt") || s.ends_with("pc") {
        Unit::In
    } else {
        Unit::Px
    }
}

/// User units to mm with Y up, and the unit the document declares.
fn document_transform(
    root: Node<'_, '_>,
    scale: Option<f64>,
    report: &mut ImportReport,
) -> Result<(Affine, Unit), ReadError> {
    let vb = root.attribute("viewBox").and_then(|v| match numbers(v) {
        Some(n) if n.len() == 4 && n[2] > 0.0 && n[3] > 0.0 => Some([n[0], n[1], n[2], n[3]]),
        _ => {
            report.warn(format!("viewBox {v:?} is not valid; ignored"));
            None
        }
    });
    let size = |name: &str, report: &mut ImportReport| {
        let v = root.attribute(name)?;
        let px = length_px(v).filter(|&x| x > 0.0);
        if px.is_none() {
            report.warn(format!("root {name} {v:?} has no absolute size; ignored"));
        }
        px
    };
    let (w_px, h_px) = (size("width", report), size("height", report));
    let unit = root.attribute("width").map_or(Unit::Px, declared_unit);
    if let Some(s) = scale {
        if !(s.is_finite() && s > 0.0) {
            return Err(ReadError::new(format!("bad scale {s}")));
        }
        // User units as given; Y flipped about the viewBox's bottom edge.
        let (x0, bottom) = vb.map_or((0.0, 0.0), |b| (b[0], b[1] + b[3]));
        let m = Affine::translation(Vec3::new(-x0, -bottom, 0.0)).then(&Affine::scale(s, -s, 1.0));
        return Ok((m, unit));
    }
    // User units to viewport px.
    let (to_vp, vp_h) = match vb {
        Some([x, y, w, h]) => {
            let (vw, vh) = match (w_px, h_px) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) => (a, a * h / w),
                (None, Some(b)) => (b * w / h, b),
                (None, None) => (w, h),
            };
            let (sx, sy) = (vw / w, vh / h);
            let par = root
                .attribute("preserveAspectRatio")
                .unwrap_or("xMidYMid meet");
            let mut words = par.split_whitespace();
            let align = words.next().unwrap_or("xMidYMid");
            let slice = words.next() == Some("slice");
            let m = if align == "none" {
                Affine::translation(Vec3::new(-x, -y, 0.0)).then(&Affine::scale(sx, sy, 1.0))
            } else {
                let s = if slice { sx.max(sy) } else { sx.min(sy) };
                let frac = |key: &str| {
                    if align.contains(&format!("{key}Min")) {
                        0.0
                    } else if align.contains(&format!("{key}Max")) {
                        1.0
                    } else {
                        0.5
                    }
                };
                let tx = frac("x") * (vw - w * s);
                let ty = frac("Y") * (vh - h * s);
                Affine::translation(Vec3::new(-x, -y, 0.0))
                    .then(&Affine::scale(s, s, 1.0))
                    .then(&Affine::translation(Vec3::new(tx, ty, 0.0)))
            };
            (m, Some(vh))
        }
        None => (Affine::identity(), h_px),
    };
    // Viewport px to mm, Y up: y' = (height − y)·k.
    let flip = Affine::scale(PX_MM, -PX_MM, 1.0).then(&Affine::translation(Vec3::new(
        0.0,
        vp_h.unwrap_or(0.0) * PX_MM,
        0.0,
    )));
    Ok((to_vp.then(&flip), unit))
}

/// A transform list (SVG 2 §8.5), or `None` if malformed.
fn parse_transform(s: &str) -> Option<Affine> {
    let mut sc = Scanner::new(s);
    let mut total = Affine::identity();
    sc.skip_separators();
    while !sc.at_end() {
        let start = sc.i;
        while sc.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            sc.i += 1;
        }
        let name = s.get(start..sc.i)?;
        sc.skip_ws();
        if sc.peek() != Some(b'(') {
            return None;
        }
        sc.i += 1;
        sc.skip_ws();
        let mut args = Vec::new();
        while sc.number_next() {
            args.push(sc.number()?);
            if args.len() > 6 {
                return None;
            }
        }
        if sc.peek() != Some(b')') {
            return None;
        }
        sc.i += 1;
        sc.skip_separators();
        let m2 = |a: f64, b: f64, c: f64, d: f64, e: f64, f: f64| {
            Affine::from_mat3_translation(
                DMat3::from_cols(DVec3::new(a, b, 0.0), DVec3::new(c, d, 0.0), DVec3::Z),
                Vec3::new(e, f, 0.0),
            )
        };
        let t = match (name, args.as_slice()) {
            ("matrix", &[a, b, c, d, e, f]) => m2(a, b, c, d, e, f),
            ("translate", &[x]) => m2(1.0, 0.0, 0.0, 1.0, x, 0.0),
            ("translate", &[x, y]) => m2(1.0, 0.0, 0.0, 1.0, x, y),
            ("scale", &[k]) => m2(k, 0.0, 0.0, k, 0.0, 0.0),
            ("scale", &[x, y]) => m2(x, 0.0, 0.0, y, 0.0, 0.0),
            ("rotate", &[a]) | ("rotate", &[a, _, _]) => {
                let (sn, cs) = libm::sincos(a.to_radians());
                let r = m2(cs, sn, -sn, cs, 0.0, 0.0);
                if let &[_, cx, cy] = args.as_slice() {
                    m2(1.0, 0.0, 0.0, 1.0, -cx, -cy)
                        .then(&r)
                        .then(&m2(1.0, 0.0, 0.0, 1.0, cx, cy))
                } else {
                    r
                }
            }
            ("skewX", &[a]) => m2(1.0, 0.0, libm::tan(a.to_radians()), 1.0, 0.0, 0.0),
            ("skewY", &[a]) => m2(1.0, libm::tan(a.to_radians()), 0.0, 1.0, 0.0, 0.0),
            _ => return None,
        };
        // `transform="A B"` maps a point by B first, then A.
        total = t.then(&total);
    }
    Some(total)
}

struct Walker<'d, 'i> {
    doc: &'d Document<'i>,
    ids: BTreeMap<&'d str, NodeId>,
    limits: Limits,
    curves: Vec<WireCurve>,
    items: usize,
    /// The `use` elements being expanded, outermost first.
    uses: Vec<NodeId>,
    report: ImportReport,
}

impl Walker<'_, '_> {
    fn children(&mut self, n: Node<'_, '_>, ctm: &Affine, depth: usize) -> Result<(), ReadError> {
        for c in n.children().filter(|c| c.is_element()) {
            self.element(c, ctm, depth + 1)?;
        }
        Ok(())
    }

    fn num(&mut self, n: Node<'_, '_>, name: &str) -> f64 {
        match n.attribute(name) {
            None => 0.0,
            Some(v) => length_px(v).unwrap_or_else(|| {
                self.report.warn(format!(
                    "{}: {name}={v:?} is not a length; taken as 0",
                    tag(n)
                ));
                0.0
            }),
        }
    }

    fn push(&mut self, c: Option<WireCurve>, m: &Affine) -> Result<(), ReadError> {
        match c.and_then(|c| transform(&c, m)) {
            Some(c) => {
                if self.curves.len() >= self.limits.max_curves {
                    return Err(ReadError::new(format!(
                        "more than {} curves",
                        self.limits.max_curves
                    )));
                }
                self.curves.push(c);
            }
            None => self.report.dropped += 1,
        }
        Ok(())
    }

    fn push_all(&mut self, b: Builder, m: &Affine) -> Result<(), ReadError> {
        self.report.dropped += b.dropped;
        if b.overflow {
            return Err(ReadError::new(format!(
                "a path has more than {} segments",
                self.limits.max_points
            )));
        }
        for c in b.curves {
            self.push(Some(c), m)?;
        }
        Ok(())
    }

    fn element(&mut self, n: Node<'_, '_>, parent: &Affine, depth: usize) -> Result<(), ReadError> {
        self.items += 1;
        if self.items > self.limits.max_items {
            return Err(ReadError::new(format!(
                "more than {} elements after use expansion",
                self.limits.max_items
            )));
        }
        if !svg_ns(n) {
            return Ok(());
        }
        let name = n.tag_name().name();
        if depth > MAX_NESTING {
            self.report.warn(format!(
                "{}: nested deeper than {MAX_NESTING}; skipped",
                tag(n)
            ));
            return Ok(());
        }
        let ctm = match n.attribute("transform") {
            None => *parent,
            Some(t) => match parse_transform(t) {
                Some(local) => local.then(parent),
                None => {
                    self.report
                        .warn(format!("{}: transform {t:?} is not valid; ignored", tag(n)));
                    *parent
                }
            },
        };
        let max_pts = self.limits.max_points;
        match name {
            "g" | "a" | "switch" => self.children(n, &ctm, depth)?,
            "svg" => {
                if n.attribute("viewBox").is_some() {
                    self.report.warn("viewBox of a nested svg ignored");
                }
                let at = Vec3::new(self.num(n, "x"), self.num(n, "y"), 0.0);
                let m = Affine::translation(at).then(&ctm);
                self.children(n, &m, depth)?;
            }
            "path" => {
                let mut b = Builder::new(max_pts);
                // Empty path data is valid and draws nothing.
                if let Err(e) = parse_path(n.attribute("d").unwrap_or(""), &mut b) {
                    self.report.warn(format!(
                        "{}: path data stops at byte {}: {}",
                        tag(n),
                        e.at,
                        e.msg
                    ));
                }
                self.push_all(b, &ctm)?;
            }
            "rect" => {
                let (x, y) = (self.num(n, "x"), self.num(n, "y"));
                let (w, h) = (self.num(n, "width"), self.num(n, "height"));
                if w > 0.0 && h > 0.0 {
                    let rx = n.attribute("rx").map(|_| self.num(n, "rx"));
                    let ry = n.attribute("ry").map(|_| self.num(n, "ry"));
                    let (rx, ry) = match (rx, ry) {
                        (Some(a), Some(b)) => (a, b),
                        (Some(a), None) | (None, Some(a)) => (a, a),
                        (None, None) => (0.0, 0.0),
                    };
                    let (rx, ry) = (rx.abs().min(w / 2.0), ry.abs().min(h / 2.0));
                    let mut b = Builder::new(max_pts);
                    if rx > 0.0 && ry > 0.0 {
                        rect(&mut b, x, y, w, h, rx, ry);
                    } else {
                        rect(&mut b, x, y, w, h, 0.0, 0.0);
                    }
                    self.push_all(b, &ctm)?;
                }
            }
            "circle" => {
                let r = self.num(n, "r");
                if r > 0.0 {
                    let c = ellipse(self.num(n, "cx"), self.num(n, "cy"), r, r);
                    self.push(c, &ctm)?;
                }
            }
            "ellipse" => {
                let (rx, ry) = (self.num(n, "rx"), self.num(n, "ry"));
                if rx > 0.0 && ry > 0.0 {
                    let c = ellipse(self.num(n, "cx"), self.num(n, "cy"), rx, ry);
                    self.push(c, &ctm)?;
                }
            }
            "line" => {
                let a = Point3::new(self.num(n, "x1"), self.num(n, "y1"), 0.0);
                let b = Point3::new(self.num(n, "x2"), self.num(n, "y2"), 0.0);
                if a != b {
                    self.push(line(a, b), &ctm)?;
                }
            }
            "polyline" | "polygon" => {
                let pts = n.attribute("points").unwrap_or("");
                let mut sc = Scanner::new(pts);
                sc.skip_ws();
                let mut b = Builder::new(max_pts);
                let mut count = 0;
                while let (Some(x), Some(y)) = (sc.number(), sc.number()) {
                    if count == 0 {
                        b.move_to([x, y]);
                    } else {
                        b.line_to([x, y]);
                    }
                    count += 1;
                }
                if !sc.at_end() {
                    self.report
                        .warn(format!("{}: points stop at byte {}", tag(n), sc.i));
                }
                if name == "polygon" && count > 1 {
                    b.close();
                }
                self.push_all(b, &ctm)?;
            }
            "use" => self.use_element(n, &ctm, depth)?,
            k if IGNORED.contains(&k) => self.report.ignore(k),
            k if SILENT.contains(&k) => {}
            k => self.report.unsupported(k),
        }
        Ok(())
    }

    /// `use`: the referenced element (or a `symbol`'s children) drawn at
    /// `x`, `y`.
    fn use_element(
        &mut self,
        n: Node<'_, '_>,
        ctm: &Affine,
        depth: usize,
    ) -> Result<(), ReadError> {
        let href = n
            .attribute((XLINK_NS, "href"))
            .or_else(|| n.attribute("href"))
            .unwrap_or("");
        let Some(target) = href
            .strip_prefix('#')
            .and_then(|id| self.ids.get(id))
            .and_then(|&id| self.doc.get_node(id))
        else {
            self.report
                .warn(format!("{}: reference {href:?} not found", tag(n)));
            return Ok(());
        };
        if self.uses.contains(&n.id()) || n.ancestors().any(|a| a.id() == target.id()) {
            self.report
                .warn(format!("{}: {href} refers to itself; skipped", tag(n)));
            return Ok(());
        }
        if self.uses.len() >= self.limits.max_depth {
            self.report.warn(format!(
                "{}: use nested deeper than {}; skipped",
                tag(n),
                self.limits.max_depth
            ));
            return Ok(());
        }
        let at = Vec3::new(self.num(n, "x"), self.num(n, "y"), 0.0);
        let m = Affine::translation(at).then(ctm);
        self.uses.push(n.id());
        let r = if target.tag_name().name() == "symbol" {
            if target.attribute("viewBox").is_some() {
                self.report.warn("viewBox of a symbol ignored");
            }
            let m = match target.attribute("transform").and_then(parse_transform) {
                Some(t) => t.then(&m),
                None => m,
            };
            self.children(target, &m, depth)
        } else {
            self.element(target, &m, depth + 1)
        };
        self.uses.pop();
        r
    }
}

/// `<name>` with its line, for messages.
fn tag(n: Node<'_, '_>) -> String {
    let pos = n.document().text_pos_at(n.range().start);
    format!("line {}: <{}>", pos.row, n.tag_name().name())
}
