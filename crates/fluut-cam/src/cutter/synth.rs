// SPDX-License-Identifier: MIT OR Apache-2.0

//! Synthetic B-rep bodies for cutter-query tests and measurements: single
//! surface patches bounded by iso-parameter edges, boxes, and the tiled
//! 5 000-face models of the S2 spike.

use fluut_geom::{
    BSplineCurve3, BSplineSurface, Curve3, Frame, Interval, Point3, Surface, UnitVec3, UvBox, Vec3,
};
use fluut_topo::{BrepBody, Coedge, Edge, Face, Loop, Shell, Vertex};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Builds a body face by face. Vertices closer than 1e-9 mm are shared.
#[derive(Debug, Default)]
pub struct Builder {
    body: BrepBody,
    at: BTreeMap<[i64; 3], u32>,
}

impl Builder {
    pub fn new(name: &str) -> Builder {
        Builder {
            body: BrepBody {
                name: name.into(),
                tol_model: 1e-6,
                ..Default::default()
            },
            at: BTreeMap::new(),
        }
    }

    pub fn vertex(&mut self, p: Point3) -> u32 {
        let key = [p.x(), p.y(), p.z()].map(|c| (c * 1e9).round() as i64);
        let vs = &mut self.body.vertices;
        *self.at.entry(key).or_insert_with(|| {
            vs.push(Vertex { p });
            vs.len() as u32 - 1
        })
    }

    /// An edge along `curve` over `range`; `None` makes a degenerate edge at
    /// point `p0`.
    pub fn edge(&mut self, curve: Option<Curve3>, range: Interval, p0: Point3) -> u32 {
        let (a, b) = match &curve {
            Some(c) => (c.eval(range.lo), c.eval(range.hi)),
            None => (p0, p0),
        };
        let (v0, v1) = (self.vertex(a), self.vertex(b));
        let curve = curve.map(|c| {
            self.body.curves.push(c);
            self.body.curves.len() as u32 - 1
        });
        self.body.edges.push(Edge {
            curve,
            range,
            v0,
            v1,
        });
        self.body.edges.len() as u32 - 1
    }

    /// A face with loops of `(edge, forward)`; the first loop is outer.
    pub fn face(&mut self, surface: Surface, same_sense: bool, loops: &[Vec<(u32, bool)>]) -> u32 {
        self.body.surfaces.push(surface);
        let mut ls = Vec::new();
        for (i, l) in loops.iter().enumerate() {
            let mut cs = Vec::new();
            for &(edge, forward) in l {
                self.body.coedges.push(Coedge { edge, forward });
                cs.push(self.body.coedges.len() as u32 - 1);
            }
            self.body.loops.push(Loop {
                outer: i == 0,
                coedges: cs,
            });
            ls.push(self.body.loops.len() as u32 - 1);
        }
        self.body.faces.push(Face {
            surface: self.body.surfaces.len() as u32 - 1,
            same_sense,
            loops: ls,
        });
        self.body.faces.len() as u32 - 1
    }

    /// A face of an analytic surface over the parameter rectangle `uv`,
    /// bounded by its four iso-parameter edges (counter-clockwise in uv).
    pub fn patch(&mut self, s: Surface, uv: UvBox) -> u32 {
        let (u0, u1, v0, v1) = (uv.u.lo, uv.u.hi, uv.v.lo, uv.v.hi);
        let mut e = |iso: Option<(Curve3, Interval)>, at: Point3| match iso {
            Some((c, r)) => self.edge(Some(c), r, at),
            None => self.edge(None, Interval { lo: 0.0, hi: 0.0 }, at),
        };
        let bottom = e(iso_v(&s, v0, uv.u), s.eval(u0, v0));
        let right = e(iso_u(&s, u1, uv.v), s.eval(u1, v0));
        let top = e(iso_v(&s, v1, uv.u), s.eval(u0, v1));
        let left = e(iso_u(&s, u0, uv.v), s.eval(u0, v0));
        self.face(
            s,
            true,
            &[vec![
                (bottom, true),
                (right, true),
                (top, false),
                (left, false),
            ]],
        )
    }

    /// A B-spline face over the surface's whole domain; `boundary` holds the
    /// iso curves at v = lo, u = hi, v = hi, u = lo, each in increasing
    /// parameter.
    pub fn bspline_patch(&mut self, s: Surface, boundary: [Curve3; 4]) -> u32 {
        let [b, r, t, l] = boundary.map(|c| {
            let range = c.natural_range().unwrap_or(Interval { lo: 0.0, hi: 1.0 });
            let p = c.eval(range.lo);
            self.edge(Some(c), range, p)
        });
        self.face(
            s,
            true,
            &[vec![(b, true), (r, true), (t, false), (l, false)]],
        )
    }

    /// Adds one open shell holding every face.
    pub fn build(mut self) -> BrepBody {
        let n = self.body.faces.len() as u32;
        self.body.shells.push(Shell {
            faces: (0..n).collect(),
            closed: false,
        });
        self.body
    }
}

/// A line from `a` to `b` parametrised by length.
pub fn line(a: Point3, b: Point3) -> Option<(Curve3, Interval)> {
    let d = b - a;
    let dir = UnitVec3::new(d)?;
    Some((
        Curve3::line(a, dir),
        Interval {
            lo: 0.0,
            hi: d.length(),
        },
    ))
}

fn radial(f: &Frame, u: f64) -> Vec3 {
    let (s, c) = libm::sincos(u);
    f.x.get() * c + f.y().get() * s
}

/// Circle in the plane of `f` (axis `f.z`) centred at `o` with parameter
/// equal to the surface's u.
fn circle_u(f: &Frame, o: Point3, r: f64, range: Interval) -> Option<(Curve3, Interval)> {
    let frame = Frame {
        origin: o,
        z: f.z,
        x: f.x,
    };
    Some((Curve3::circle(frame, r)?, range))
}

/// The curve `S(u, ·)` over `range`, with its parameter range.
pub fn iso_u(s: &Surface, u: f64, range: Interval) -> Option<(Curve3, Interval)> {
    match s {
        Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Cone { .. } => {
            line(s.eval(u, range.lo), s.eval(u, range.hi))
        }
        Surface::Sphere { frame, r } => {
            let e = radial(frame, u);
            let f = Frame::new(frame.origin, e.cross(frame.z.get()), e)?;
            Some((Curve3::circle(f, *r)?, range))
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let e = radial(frame, u);
            let f = Frame::new(frame.origin + e * *major, e.cross(frame.z.get()), e)?;
            Some((Curve3::circle(f, *minor)?, range))
        }
        _ => None,
    }
}

/// The curve `S(·, v)` over `range`, with its parameter range.
pub fn iso_v(s: &Surface, v: f64, range: Interval) -> Option<(Curve3, Interval)> {
    match s {
        Surface::Plane { .. } => line(s.eval(range.lo, v), s.eval(range.hi, v)),
        Surface::Cylinder { frame, r } => {
            circle_u(frame, frame.origin + frame.z.get() * v, *r, range)
        }
        Surface::Cone {
            frame,
            r,
            semi_angle,
        } => circle_u(
            frame,
            frame.origin + frame.z.get() * v,
            r + v * libm::tan(*semi_angle),
            range,
        ),
        Surface::Sphere { frame, r } => {
            let (sv, cv) = libm::sincos(v);
            circle_u(
                frame,
                frame.origin + frame.z.get() * (r * sv),
                r * cv,
                range,
            )
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            let (sv, cv) = libm::sincos(v);
            circle_u(
                frame,
                frame.origin + frame.z.get() * (minor * sv),
                major + minor * cv,
                range,
            )
        }
        _ => None,
    }
}

/// Clamped uniform knots for `n` poles of degree `p` on [0, 1].
fn knots(n: usize, p: usize) -> Vec<f64> {
    let inner = n - p;
    let mut k = vec![0.0; p + 1];
    k.extend((1..inner).map(|i| i as f64 / inner as f64));
    k.extend(vec![1.0; p + 1]);
    k
}

/// A bicubic B-spline surface with `n × n` poles over the square at
/// `origin` of side `size`, pole heights from `height(i, j)`, with its four
/// boundary curves.
pub fn bspline_square(
    origin: Point3,
    size: f64,
    n: usize,
    height: impl Fn(usize, usize) -> f64,
) -> (Surface, [Curve3; 4]) {
    let p = 3;
    let pole = |i: usize, j: usize| {
        let f = size / (n - 1) as f64;
        origin + Vec3::new(i as f64 * f, j as f64 * f, height(i, j))
    };
    let mut poles = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            poles.push(pole(i, j));
        }
    }
    let k = knots(n, p);
    let s = BSplineSurface::new(3, 3, k.clone(), k.clone(), poles, n, n, None)
        .expect("valid B-spline surface");
    let curve = |pts: Vec<Point3>| {
        Curve3::BSpline(Arc::new(
            BSplineCurve3::new(3, k.clone(), pts, None).expect("valid B-spline curve"),
        ))
    };
    // Pole (i, j): i along u, j along v.
    let bottom = curve((0..n).map(|i| pole(i, 0)).collect());
    let right = curve((0..n).map(|j| pole(n - 1, j)).collect());
    let top = curve((0..n).map(|i| pole(i, n - 1)).collect());
    let left = curve((0..n).map(|j| pole(0, j)).collect());
    (Surface::BSpline(Arc::new(s)), [bottom, right, top, left])
}

/// An axis-aligned box as six planar faces.
pub fn cuboid(b: &mut Builder, lo: Point3, hi: Point3) {
    let c = |i: usize| {
        Point3::new(
            if i & 1 == 0 { lo.x() } else { hi.x() },
            if i & 2 == 0 { lo.y() } else { hi.y() },
            if i & 4 == 0 { lo.z() } else { hi.z() },
        )
    };
    // Outward-CCW vertex cycles.
    let faces: [[usize; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut edges: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    for f in faces {
        let pts: Vec<Point3> = f.iter().map(|&i| c(i)).collect();
        let n = (pts[1] - pts[0]).cross(pts[2] - pts[1]);
        let frame = Frame::new(pts[0], n, pts[1] - pts[0]).expect("box face frame");
        let mut lp = Vec::new();
        for k in 0..4 {
            let (a, d) = (f[k], f[(k + 1) % 4]);
            let key = (a.min(d), a.max(d));
            let e = match edges.get(&key) {
                Some(&e) => e,
                None => {
                    let (curve, range) = line(c(key.0), c(key.1)).expect("box edge");
                    let e = b.edge(Some(curve), range, c(key.0));
                    edges.insert(key, e);
                    e
                }
            };
            lp.push((e, a == key.0));
        }
        b.face(Surface::plane(frame), true, &[lp]);
    }
}

/// Small deterministic generator for the synthetic models.
#[derive(Debug, Clone)]
pub struct Lcg(pub u64);

impl Lcg {
    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn iv(lo: f64, hi: f64) -> Interval {
    Interval { lo, hi }
}

fn frame(o: Point3, z: Vec3, x: Vec3) -> Frame {
    Frame::new(o, z, x).expect("frame")
}

/// Adds the eight faces of one 10 × 10 mm tile at `(x0, y0)`.
///
/// `freeform`: every face a B-spline bump. Otherwise a mix like the corpus
/// (mostly analytic): planes (flat and tilted), cylinders (horizontal and
/// vertical axis), a cone, a sphere, a torus and one B-spline bump.
pub fn tile(b: &mut Builder, x0: f64, y0: f64, freeform: bool, rng: &mut Lcg) {
    // Eight cells of 5 × 2.5 mm.
    for k in 0..8 {
        let cx = x0 + 5.0 * (k % 2) as f64;
        let cy = y0 + 2.5 * (k / 2) as f64;
        let o = Point3::new(cx, cy, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let h = 2.0 * rng.uniform();
        if freeform || k == 7 {
            let seed = rng.0;
            let (s, bd) = bspline_square(o + z * h, 2.4, 5, |i, j| {
                let mut r = Lcg(seed ^ ((i * 7 + j) as u64 * 0x9e37_79b9));
                2.0 * r.uniform() - 1.0
            });
            b.bspline_patch(s, bd);
            continue;
        }
        let (s, uv) = match k {
            0 => (
                Some(Surface::plane(frame(o + z * h, z, x))),
                UvBox::new(iv(0.0, 4.5), iv(0.0, 2.2)),
            ),
            1 => {
                let n = Vec3::new(0.3 * rng.uniform(), 0.5, 1.0);
                (
                    Some(Surface::plane(frame(o + z * h, n, x))),
                    UvBox::new(iv(0.2, 4.3), iv(0.1, 2.0)),
                )
            }
            2 => (
                Surface::cylinder(frame(o + y * 1.2 + z * h, x, y), 1.0),
                UvBox::new(iv(0.3, 2.8), iv(0.2, 4.6)),
            ),
            3 => (
                Surface::cylinder(frame(o + x * 2.5 + y * 1.2, z, x), 1.0),
                UvBox::new(iv(0.5, 3.5), iv(0.0, h + 1.0)),
            ),
            4 => (
                Surface::cone(
                    frame(o + x * 2.5 + y * 1.2 + z * (h + 1.5), -z, x),
                    0.5,
                    0.4,
                ),
                UvBox::new(iv(0.0, 5.5), iv(0.0, 1.5)),
            ),
            5 => (
                Surface::sphere(frame(o + x * 2.5 + y * 1.2 + z * h, z, x), 1.1),
                UvBox::new(iv(0.3, 6.0), iv(-0.6, 1.2)),
            ),
            _ => (
                Surface::torus(frame(o + x * 2.5 + y * 1.2 + z * h, z, x), 1.4, 0.5),
                UvBox::new(iv(0.0, 4.0), iv(-0.5, 2.5)),
            ),
        };
        b.patch(s.expect("tile surface"), uv);
    }
}

/// `n × n` tiles: `8 n²` faces (n = 25 gives 5 000).
pub fn tiled(n: usize, freeform: bool) -> BrepBody {
    let mut b = Builder::new(if freeform {
        "tiles-freeform"
    } else {
        "tiles-mixed"
    });
    let mut rng = Lcg(0x5eed);
    for i in 0..n {
        for j in 0..n {
            tile(&mut b, 10.0 * i as f64, 10.0 * j as f64, freeform, &mut rng);
        }
    }
    b.build()
}

/// Height of the smooth sculpted surface of [`sculpted`] at `(x, y)`: slopes
/// up to about 1, ripples of a few millimetres over 10 to 60 mm.
pub fn sculpt_height(x: f64, y: f64) -> f64 {
    6.0 * libm::sin(0.09 * x)
        + 4.0 * libm::cos(0.07 * y + 0.5)
        + 2.0 * libm::sin(0.21 * x + 0.15 * y)
        + 0.6 * libm::sin(0.5 * x) * libm::cos(0.45 * y)
}

/// A smooth free-form surface of `n × n` bicubic B-spline faces of side
/// `size` mm, `poles × poles` poles each, whose pole heights follow
/// [`sculpt_height`] (so neighbours meet along shared boundary curves). It
/// stands for a sculpted model whose faces are larger than a ripple, unlike
/// the rough [`tiled`] model.
pub fn sculpted(n: usize, size: f64, poles: usize) -> BrepBody {
    let mut b = Builder::new("sculpted");
    let step = size / (poles - 1) as f64;
    for i in 0..n {
        for j in 0..n {
            let (ox, oy) = (size * i as f64, size * j as f64);
            let (s, bd) = bspline_square(Point3::new(ox, oy, 0.0), size, poles, |a, c| {
                sculpt_height(ox + a as f64 * step, oy + c as f64 * step)
            });
            b.bspline_patch(s, bd);
        }
    }
    b.build()
}
