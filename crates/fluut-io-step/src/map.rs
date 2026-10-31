// SPDX-License-Identifier: MIT OR Apache-2.0

//! Geometry and topology mapping (design D04 §5) for the S1 spike: every
//! body root becomes a `fluut_topo::BrepBody` in mm and radians. Faces whose
//! geometry is unsupported or broken are skipped and counted; assemblies
//! and placements (D04 §4) are not handled yet.

use crate::parse::{Param, RawEntity, RawModel};
use crate::schema::{BODY_KINDS, Units, part, parts, units};
use fluut_geom::{
    BSplineCurve3, BSplineSurface, Curve3, Frame, Interval, Point3, Surface, Tol, UnitVec3, Vec3,
};
use fluut_topo::{BrepBody, Coedge, Edge, Face, Loop, Shell, Vertex};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Why an entity could not be mapped.
#[derive(Debug, Clone, PartialEq)]
pub struct MapError {
    pub id: u64,
    pub msg: String,
}

type R<T> = Result<T, MapError>;

fn fail<T>(id: u64, msg: impl Into<String>) -> R<T> {
    Err(MapError {
        id,
        msg: msg.into(),
    })
}

#[derive(Debug, Clone)]
pub struct MappedBody {
    pub id: u64,
    pub kind: String,
    pub body: BrepBody,
    pub units: Units,
    /// Faces left out, with the reason.
    pub skipped: Vec<MapError>,
}

/// Maps every body root in the file.
pub fn map_bodies(m: &RawModel) -> Vec<R<MappedBody>> {
    let ctx = body_contexts(m);
    let mut out = Vec::new();
    for (&id, &slot) in &m.index {
        let e = &m.entities[slot as usize];
        let Some(kind) = parts(m, e)
            .into_iter()
            .map(|(n, _)| n)
            .find(|n| BODY_KINDS.contains(n))
        else {
            continue;
        };
        let u = ctx.get(&id).map_or_else(Units::default, |&c| units(m, c));
        let mut mp = Mapper::new(m, u);
        out.push(mp.body(id, kind).map(|()| MappedBody {
            id,
            kind: kind.to_owned(),
            body: mp.b,
            units: u,
            skipped: mp.skipped,
        }));
    }
    out
}

/// Body root → representation context, from every representation that
/// lists the body among its items.
fn body_contexts(m: &RawModel) -> BTreeMap<u64, u64> {
    let mut out = BTreeMap::new();
    for e in &m.entities {
        for (name, p) in parts(m, e) {
            if !name.ends_with("REPRESENTATION") {
                continue;
            }
            if let [_, Param::List(items), Param::Ref(ctx)] = p {
                for it in items {
                    if let Param::Ref(i) = it {
                        out.insert(*i, *ctx);
                    }
                }
            }
        }
    }
    out
}

fn real(id: u64, p: Option<&Param>) -> R<f64> {
    match p {
        Some(Param::Real(x)) => Ok(*x),
        Some(Param::Int(i)) => Ok(*i as f64),
        Some(Param::Typed(_, x)) => real(id, Some(x)),
        _ => fail(id, "expected a number"),
    }
}

fn int(id: u64, p: Option<&Param>) -> R<i64> {
    match p {
        Some(Param::Int(i)) => Ok(*i),
        _ => fail(id, "expected an integer"),
    }
}

fn reference(id: u64, p: Option<&Param>) -> R<u64> {
    match p {
        Some(Param::Ref(r)) => Ok(*r),
        _ => fail(id, "expected a reference"),
    }
}

fn list(id: u64, p: Option<&Param>) -> R<&[Param]> {
    match p {
        Some(Param::List(l)) => Ok(l),
        _ => fail(id, "expected a list"),
    }
}

fn boolean(id: u64, p: Option<&Param>) -> R<bool> {
    match p {
        Some(Param::Enum(e)) if &**e == "T" => Ok(true),
        Some(Param::Enum(e)) if &**e == "F" => Ok(false),
        _ => fail(id, "expected .T. or .F."),
    }
}

fn reals(id: u64, p: Option<&Param>) -> R<Vec<f64>> {
    list(id, p)?.iter().map(|x| real(id, Some(x))).collect()
}

fn refs(id: u64, p: Option<&Param>) -> R<Vec<u64>> {
    list(id, p)?
        .iter()
        .map(|x| reference(id, Some(x)))
        .collect()
}

/// Knot vector from distinct knots and multiplicities.
fn expand_knots(id: u64, mults: &[i64], knots: &[f64]) -> R<Vec<f64>> {
    if mults.len() != knots.len() || mults.iter().any(|m| !(1..=64).contains(m)) {
        return fail(id, "bad knot multiplicities");
    }
    Ok(knots
        .iter()
        .zip(mults)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m as usize))
        .collect())
}

struct Mapper<'a> {
    m: &'a RawModel,
    len: f64,
    ang: f64,
    tol: Tol,
    b: BrepBody,
    curves: BTreeMap<u64, u32>,
    surfaces: BTreeMap<u64, u32>,
    vertices: BTreeMap<u64, u32>,
    edges: BTreeMap<u64, u32>,
    skipped: Vec<MapError>,
    depth: u32,
}

impl<'a> Mapper<'a> {
    fn new(m: &'a RawModel, u: Units) -> Self {
        let tol_model = u.uncertainty_mm.unwrap_or(1e-6).clamp(1e-7, 1e-2);
        Mapper {
            m,
            len: u.length_mm,
            ang: u.angle_rad,
            tol: Tol::for_machining(tol_model, 0.01),
            b: BrepBody {
                tol_model,
                ..Default::default()
            },
            curves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            vertices: BTreeMap::new(),
            edges: BTreeMap::new(),
            skipped: Vec::new(),
            depth: 0,
        }
    }

    fn entity(&self, id: u64) -> R<&'a RawEntity> {
        self.m.get(id).ok_or(MapError {
            id,
            msg: "dangling reference".into(),
        })
    }

    /// The single part of a simple entity, with its name.
    fn simple(&self, id: u64) -> R<(&'a str, &'a [Param])> {
        let m = self.m;
        match self.entity(id)? {
            RawEntity::Simple { kind, params, .. } => Ok((m.keywords.name(*kind), params)),
            RawEntity::Complex { .. } => fail(id, "unexpected complex entity"),
        }
    }

    fn expect(&self, id: u64, want: &str) -> R<&'a [Param]> {
        let (n, p) = self.simple(id)?;
        if n == want {
            Ok(p)
        } else {
            fail(id, format!("expected {want}, found {n}"))
        }
    }

    fn point(&self, id: u64) -> R<Point3> {
        let p = self.expect(id, "CARTESIAN_POINT")?;
        let c = reals(id, p.get(1))?;
        let s = self.len;
        match c[..] {
            [x, y, z] => Ok(Point3::new(x * s, y * s, z * s)),
            [x, y] => Ok(Point3::new(x * s, y * s, 0.0)),
            _ => fail(id, "point needs 2 or 3 coordinates"),
        }
    }

    fn direction(&self, id: u64) -> R<Vec3> {
        let p = self.expect(id, "DIRECTION")?;
        match reals(id, p.get(1))?[..] {
            [x, y, z] => Ok(Vec3::new(x, y, z)),
            [x, y] => Ok(Vec3::new(x, y, 0.0)),
            _ => fail(id, "direction needs 2 or 3 components"),
        }
    }

    fn unit(&self, id: u64, v: Vec3) -> R<UnitVec3> {
        UnitVec3::new(v).ok_or(MapError {
            id,
            msg: "zero direction".into(),
        })
    }

    fn frame(&self, id: u64) -> R<Frame> {
        let p = self.expect(id, "AXIS2_PLACEMENT_3D")?;
        let o = self.point(reference(id, p.get(1))?)?;
        let z = match p.get(2) {
            Some(Param::Ref(r)) => self.direction(*r)?,
            _ => Vec3::new(0.0, 0.0, 1.0),
        };
        let x = match p.get(3) {
            Some(Param::Ref(r)) => self.direction(*r)?,
            _ => Vec3::new(1.0, 0.0, 0.0),
        };
        Frame::new(o, z, x)
            .or_else(|| {
                // Reference direction parallel to the axis: pick any.
                // Directions need not be unit length (some exporters scale
                // them by the unit conversion), so compare the unit axis.
                let zu = z.length();
                let alt = if zu > 0.0 && (z.x() / zu).abs() < 0.9 {
                    Vec3::new(1.0, 0.0, 0.0)
                } else {
                    Vec3::new(0.0, 1.0, 0.0)
                };
                Frame::new(o, z, alt)
            })
            .ok_or(MapError {
                id,
                msg: "degenerate placement".into(),
            })
    }

    fn curve(&mut self, id: u64) -> R<u32> {
        if let Some(&c) = self.curves.get(&id) {
            return Ok(c);
        }
        self.depth += 1;
        if self.depth > 16 {
            self.depth -= 1;
            return fail(id, "curve nesting too deep");
        }
        let c = self.curve_geom(id);
        self.depth -= 1;
        let c = c?;
        self.b.curves.push(c);
        let i = self.b.curves.len() as u32 - 1;
        self.curves.insert(id, i);
        Ok(i)
    }

    fn curve_geom(&mut self, id: u64) -> R<Curve3> {
        let e = self.entity(id)?;
        if let RawEntity::Complex { .. } = e {
            return self.bspline_curve(id, e);
        }
        let (name, p) = self.simple(id)?;
        let s = self.len;
        let bad = |what: &str| MapError {
            id,
            msg: format!("invalid {what}"),
        };
        match name {
            "LINE" => {
                let o = self.point(reference(id, p.get(1))?)?;
                let v = self.expect(reference(id, p.get(2))?, "VECTOR")?;
                let d = self.direction(reference(id, v.get(1))?)?;
                Ok(Curve3::line(o, self.unit(id, d)?))
            }
            "CIRCLE" => {
                let f = self.frame(reference(id, p.get(1))?)?;
                Curve3::circle(f, real(id, p.get(2))? * s).ok_or_else(|| bad("circle"))
            }
            "ELLIPSE" => {
                let f = self.frame(reference(id, p.get(1))?)?;
                let (a, b) = (real(id, p.get(2))? * s, real(id, p.get(3))? * s);
                Curve3::ellipse(f, a, b).ok_or_else(|| bad("ellipse"))
            }
            "B_SPLINE_CURVE_WITH_KNOTS" => self.bspline_curve(id, e),
            "SURFACE_CURVE" | "SEAM_CURVE" => {
                let c = self.curve(reference(id, p.get(1))?)?;
                Ok(self.b.curves[c as usize].clone())
            }
            n => fail(id, format!("unsupported curve {n}")),
        }
    }

    fn bspline_curve(&self, id: u64, e: &RawEntity) -> R<Curve3> {
        let m = self.m;
        // Simple form: (name, degree, poles, form, closed, self_int, mults, knots, spec).
        // Complex form: B_SPLINE_CURVE(degree, poles, …), _WITH_KNOTS(mults, knots, …).
        let (deg, poles, mults, knots) = match e {
            RawEntity::Simple { params: p, .. } => (p.get(1), p.get(2), p.get(6), p.get(7)),
            RawEntity::Complex { .. } => {
                let b = part(m, e, "B_SPLINE_CURVE").ok_or(MapError {
                    id,
                    msg: "unsupported complex curve".into(),
                })?;
                let k = part(m, e, "B_SPLINE_CURVE_WITH_KNOTS").ok_or(MapError {
                    id,
                    msg: "B-spline curve without knots".into(),
                })?;
                (b.first(), b.get(1), k.first(), k.get(1))
            }
        };
        let weights = match part(m, e, "RATIONAL_B_SPLINE_CURVE") {
            Some(w) => Some(reals(id, w.first())?),
            None => None,
        };
        let deg = int(id, deg)?;
        let poles: Vec<Point3> = refs(id, poles)?
            .into_iter()
            .map(|r| self.point(r))
            .collect::<R<_>>()?;
        let mults: Vec<i64> = list(id, mults)?
            .iter()
            .map(|x| int(id, Some(x)))
            .collect::<R<_>>()?;
        let knots = expand_knots(id, &mults, &reals(id, knots)?)?;
        let deg = u8::try_from(deg).map_err(|_| MapError {
            id,
            msg: "bad degree".into(),
        })?;
        BSplineCurve3::new(deg, knots, poles, weights)
            .map(|c| Curve3::BSpline(Arc::new(c)))
            .map_err(|err| MapError {
                id,
                msg: format!("B-spline curve: {err:?}"),
            })
    }

    fn surface(&mut self, id: u64) -> R<u32> {
        if let Some(&s) = self.surfaces.get(&id) {
            return Ok(s);
        }
        let s = self.surface_geom(id)?;
        self.b.surfaces.push(s);
        let i = self.b.surfaces.len() as u32 - 1;
        self.surfaces.insert(id, i);
        Ok(i)
    }

    fn surface_geom(&mut self, id: u64) -> R<Surface> {
        let e = self.entity(id)?;
        if let RawEntity::Complex { .. } = e {
            return self.bspline_surface(id, e);
        }
        let (name, p) = self.simple(id)?;
        let s = self.len;
        let bad = |what: &str| MapError {
            id,
            msg: format!("invalid {what}"),
        };
        match name {
            "PLANE" => Ok(Surface::plane(self.frame(reference(id, p.get(1))?)?)),
            "CYLINDRICAL_SURFACE" => {
                let f = self.frame(reference(id, p.get(1))?)?;
                Surface::cylinder(f, real(id, p.get(2))? * s).ok_or_else(|| bad("cylinder"))
            }
            "CONICAL_SURFACE" => {
                let f = self.frame(reference(id, p.get(1))?)?;
                let (r, a) = (real(id, p.get(2))? * s, real(id, p.get(3))? * self.ang);
                Surface::cone(f, r, a).ok_or_else(|| bad("cone"))
            }
            "SPHERICAL_SURFACE" => {
                let f = self.frame(reference(id, p.get(1))?)?;
                Surface::sphere(f, real(id, p.get(2))? * s).ok_or_else(|| bad("sphere"))
            }
            "TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE" => {
                let f = self.frame(reference(id, p.get(1))?)?;
                let (a, b) = (real(id, p.get(2))? * s, real(id, p.get(3))? * s);
                Surface::torus(f, a, b).ok_or_else(|| bad("torus"))
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                let c = self.curve(reference(id, p.get(1))?)?;
                let v = self.expect(reference(id, p.get(2))?, "VECTOR")?;
                let d = self.direction(reference(id, v.get(1))?)?;
                Ok(Surface::extrusion(
                    self.b.curves[c as usize].clone(),
                    self.unit(id, d)?,
                ))
            }
            "SURFACE_OF_REVOLUTION" => {
                let c = self.curve(reference(id, p.get(1))?)?;
                let ax = self.expect(reference(id, p.get(2))?, "AXIS1_PLACEMENT")?;
                let o = self.point(reference(id, ax.get(1))?)?;
                let d = match ax.get(2) {
                    Some(Param::Ref(r)) => self.direction(*r)?,
                    _ => Vec3::new(0.0, 0.0, 1.0),
                };
                Ok(Surface::revolution(
                    self.b.curves[c as usize].clone(),
                    o,
                    self.unit(id, d)?,
                ))
            }
            "B_SPLINE_SURFACE_WITH_KNOTS" => self.bspline_surface(id, e),
            n => fail(id, format!("unsupported surface {n}")),
        }
    }

    fn bspline_surface(&self, id: u64, e: &RawEntity) -> R<Surface> {
        let m = self.m;
        // Simple: (name, du, dv, poles, form, uc, vc, si, umults, vmults, uknots, vknots, spec).
        let (du, dv, poles, um, vm, uk, vk) = match e {
            RawEntity::Simple { params: p, .. } => (
                p.get(1),
                p.get(2),
                p.get(3),
                p.get(8),
                p.get(9),
                p.get(10),
                p.get(11),
            ),
            RawEntity::Complex { .. } => {
                let b = part(m, e, "B_SPLINE_SURFACE").ok_or(MapError {
                    id,
                    msg: "unsupported complex surface".into(),
                })?;
                let k = part(m, e, "B_SPLINE_SURFACE_WITH_KNOTS").ok_or(MapError {
                    id,
                    msg: "B-spline surface without knots".into(),
                })?;
                (
                    b.first(),
                    b.get(1),
                    b.get(2),
                    k.first(),
                    k.get(1),
                    k.get(2),
                    k.get(3),
                )
            }
        };
        let rows = list(id, poles)?;
        let nu = rows.len();
        let mut pts = Vec::new();
        let mut nv = 0;
        for r in rows {
            let row = refs(id, Some(r))?;
            if nv != 0 && row.len() != nv {
                return fail(id, "ragged pole grid");
            }
            nv = row.len();
            for p in row {
                pts.push(self.point(p)?);
            }
        }
        let weights = match part(m, e, "RATIONAL_B_SPLINE_SURFACE") {
            Some(w) => {
                let mut out = Vec::new();
                for r in list(id, w.first())? {
                    out.extend(reals(id, Some(r))?);
                }
                Some(out)
            }
            None => None,
        };
        let ints = |p: Option<&Param>| -> R<Vec<i64>> {
            list(id, p)?.iter().map(|x| int(id, Some(x))).collect()
        };
        let ku = expand_knots(id, &ints(um)?, &reals(id, uk)?)?;
        let kv = expand_knots(id, &ints(vm)?, &reals(id, vk)?)?;
        let deg = |p| {
            u8::try_from(int(id, p)?).map_err(|_| MapError {
                id,
                msg: "bad degree".into(),
            })
        };
        BSplineSurface::new(deg(du)?, deg(dv)?, ku, kv, pts, nu, nv, weights)
            .map(|s| Surface::BSpline(Arc::new(s)))
            .map_err(|err| MapError {
                id,
                msg: format!("B-spline surface: {err:?}"),
            })
    }

    fn vertex(&mut self, id: u64) -> R<u32> {
        if let Some(&v) = self.vertices.get(&id) {
            return Ok(v);
        }
        let p = self.expect(id, "VERTEX_POINT")?;
        let pt = self.point(reference(id, p.get(1))?)?;
        self.b.vertices.push(Vertex { p: pt });
        let i = self.b.vertices.len() as u32 - 1;
        self.vertices.insert(id, i);
        Ok(i)
    }

    fn edge(&mut self, id: u64) -> R<u32> {
        if let Some(&e) = self.edges.get(&id) {
            return Ok(e);
        }
        let p = self.expect(id, "EDGE_CURVE")?;
        let vs = self.vertex(reference(id, p.get(1))?)?;
        let ve = self.vertex(reference(id, p.get(2))?)?;
        let c = self.curve(reference(id, p.get(3))?)?;
        let same = boolean(id, p.get(4))?;
        let (v0, v1) = if same { (vs, ve) } else { (ve, vs) };
        let curve = &self.b.curves[c as usize];
        let (p0, p1) = (
            self.b.vertices[v0 as usize].p,
            self.b.vertices[v1 as usize].p,
        );
        let range = match curve.natural_range() {
            Some(nat) => {
                let (t0, _) = curve.closest(p0, nat, &self.tol);
                let (mut t1, _) = curve.closest(p1, nat, &self.tol);
                let periodic = matches!(curve, Curve3::Circle { .. } | Curve3::Ellipse { .. });
                if periodic && (v0 == v1 || t1 <= t0) {
                    t1 += std::f64::consts::TAU;
                }
                Interval::new(t0.min(t1), t0.max(t1))
            }
            None => {
                let big = Interval { lo: -1e9, hi: 1e9 };
                let (t0, _) = curve.closest(p0, big, &self.tol);
                let (t1, _) = curve.closest(p1, big, &self.tol);
                Interval::new(t0.min(t1), t0.max(t1))
            }
        }
        .ok_or(MapError {
            id,
            msg: "bad edge range".into(),
        })?;
        self.b.edges.push(Edge {
            curve: Some(c),
            range,
            v0,
            v1,
        });
        let i = self.b.edges.len() as u32 - 1;
        self.edges.insert(id, i);
        Ok(i)
    }

    /// A face bound as a loop; `orientation` false reverses it.
    fn bound(&mut self, id: u64) -> R<Loop> {
        let (name, p) = self.simple(id)?;
        let outer = match name {
            "FACE_OUTER_BOUND" => true,
            "FACE_BOUND" => false,
            n => return fail(id, format!("unsupported bound {n}")),
        };
        let lp = reference(id, p.get(1))?;
        let orient = boolean(id, p.get(2))?;
        let (lname, lpar) = self.simple(lp)?;
        let mut coedges = Vec::new();
        match lname {
            "EDGE_LOOP" => {
                for oe in refs(lp, lpar.get(1))? {
                    let op = self.expect(oe, "ORIENTED_EDGE")?;
                    let ec = reference(oe, op.get(3))?;
                    let edge = self.edge(ec)?;
                    let o = boolean(oe, op.get(4))?;
                    let same = boolean(ec, self.expect(ec, "EDGE_CURVE")?.get(4))?;
                    coedges.push(Coedge {
                        edge,
                        forward: o == same,
                    });
                }
            }
            "VERTEX_LOOP" => {}
            n => return fail(lp, format!("unsupported loop {n}")),
        }
        if !orient {
            coedges.reverse();
            for c in &mut coedges {
                c.forward = !c.forward;
            }
        }
        let mut ids = Vec::with_capacity(coedges.len());
        for c in coedges {
            self.b.coedges.push(c);
            ids.push(self.b.coedges.len() as u32 - 1);
        }
        Ok(Loop {
            outer,
            coedges: ids,
        })
    }

    /// Diagonal of the bounding box of a loop's edges.
    fn loop_size(&self, l: &Loop) -> f64 {
        let mut b = fluut_geom::Aabb3::empty();
        for &c in &l.coedges {
            for p in self.b.edge_points(self.b.coedges[c as usize].edge, 1.0) {
                b = b.with_point(p);
            }
        }
        if b.is_empty() {
            0.0
        } else {
            b.max.distance(b.min)
        }
    }

    fn face(&mut self, id: u64) -> R<u32> {
        let (name, p) = self.simple(id)?;
        if name != "ADVANCED_FACE" && name != "FACE_SURFACE" {
            return fail(id, format!("unsupported face {name}"));
        }
        let mut loops = Vec::new();
        for b in refs(id, p.get(1))? {
            loops.push(self.bound(b)?);
        }
        // AP203 allows faces whose bounds are all FACE_BOUND. One of them
        // is then the outer one: the only one, else the widest. (Spike rule;
        // the trim cache of D03 §3.1 decides this in the parameter plane.)
        if !loops.iter().any(|l| l.outer) {
            let widest = (0..loops.len()).max_by(|&i, &j| {
                self.loop_size(&loops[i])
                    .total_cmp(&self.loop_size(&loops[j]))
            });
            if let Some(i) = widest {
                loops[i].outer = true;
            }
        }
        let surface = self.surface(reference(id, p.get(2))?)?;
        let same_sense = boolean(id, p.get(3))?;
        let mut lids = Vec::new();
        for l in loops {
            self.b.loops.push(l);
            lids.push(self.b.loops.len() as u32 - 1);
        }
        self.b.faces.push(Face {
            surface,
            same_sense,
            loops: lids,
        });
        Ok(self.b.faces.len() as u32 - 1)
    }

    fn shell(&mut self, id: u64) -> R<()> {
        let (name, p) = self.simple(id)?;
        let (closed, faces) = match name {
            "CLOSED_SHELL" => (true, refs(id, p.get(1))?),
            "OPEN_SHELL" => (false, refs(id, p.get(1))?),
            "ORIENTED_CLOSED_SHELL" => return self.shell(reference(id, p.get(2))?),
            n => return fail(id, format!("unsupported shell {n}")),
        };
        let mut fs = Vec::new();
        for f in faces {
            match self.face(f) {
                Ok(i) => fs.push(i),
                Err(e) => self.skipped.push(e),
            }
        }
        self.b.shells.push(Shell { faces: fs, closed });
        Ok(())
    }

    fn body(&mut self, id: u64, kind: &str) -> R<()> {
        let e = self.entity(id)?;
        let p = part(self.m, e, kind).unwrap_or_default();
        self.b.name = match p.first() {
            Some(Param::Str(s)) => s.to_string(),
            _ => String::new(),
        };
        match kind {
            "MANIFOLD_SOLID_BREP" => self.shell(reference(id, p.get(1))?),
            "BREP_WITH_VOIDS" => {
                self.shell(reference(id, p.get(1))?)?;
                for v in refs(id, p.get(2))? {
                    self.shell(v)?;
                }
                Ok(())
            }
            "SHELL_BASED_SURFACE_MODEL" => {
                for s in refs(id, p.get(1))? {
                    self.shell(s)?;
                }
                Ok(())
            }
            n => fail(id, format!("unsupported body {n}")),
        }
    }
}

#[cfg(test)]
mod tests;
