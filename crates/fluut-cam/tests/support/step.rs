// SPDX-License-Identifier: MIT OR Apache-2.0

//! A small STEP writer for test parts: prismatic solids with planar and
//! cylindrical faces (spec 02 §2.6). Shared by the `fluut-cam` and
//! `fluut-cli` tests; the CLI test projects are generated with it.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write;

/// A profile edge from one point to the next.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Line,
    /// Arc about `centre`, counter-clockwise seen from +Z when `ccw`.
    Arc {
        centre: [f64; 2],
        ccw: bool,
    },
    /// Arc about `centre`, counter-clockwise about the unit `axis`.
    Arc3 {
        centre: [f64; 3],
        axis: [f64; 3],
    },
}

/// A closed profile: edge `i` runs from `pts[i]` to `pts[i + 1]`.
#[derive(Clone, Debug)]
pub struct Profile {
    pub pts: Vec<[f64; 2]>,
    pub kinds: Vec<Kind>,
}

impl Profile {
    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
        Profile {
            pts: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            kinds: vec![Kind::Line; 4],
        }
    }

    /// Counter-clockwise rectangle with corners rounded to radius `r`.
    pub fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Profile {
        let arc = |cx, cy| Kind::Arc {
            centre: [cx, cy],
            ccw: true,
        };
        Profile {
            pts: vec![
                [x0 + r, y0],
                [x1 - r, y0],
                [x1, y0 + r],
                [x1, y1 - r],
                [x1 - r, y1],
                [x0 + r, y1],
                [x0, y1 - r],
                [x0, y0 + r],
            ],
            kinds: vec![
                Kind::Line,
                arc(x1 - r, y0 + r),
                Kind::Line,
                arc(x1 - r, y1 - r),
                Kind::Line,
                arc(x0 + r, y1 - r),
                Kind::Line,
                arc(x0 + r, y0 + r),
            ],
        }
    }
}

pub struct Writer {
    text: String,
    next: u64,
    shift: [f64; 3],
    verts: BTreeMap<[u64; 3], u64>,
    /// Edges by end points and, for planar arcs, centre and sense: two
    /// arcs between the same points (the halves of a circle) differ.
    edges: BTreeMap<EdgeKey, u64>,
    faces: Vec<u64>,
}

/// Start, end and arc part ([`arc_key`]) of an edge.
type EdgeKey = ([u64; 3], [u64; 3], [u64; 3]);

fn key(p: [f64; 3]) -> [u64; 3] {
    p.map(|x| (x + 0.0).to_bits())
}

/// The arc part of an edge key; `reversed` for the same arc walked back.
fn arc_key(kind: Kind, reversed: bool) -> [u64; 3] {
    match kind {
        Kind::Arc { centre, ccw } => [
            (centre[0] + 0.0).to_bits(),
            (centre[1] + 0.0).to_bits(),
            u64::from(ccw != reversed) + 1,
        ],
        Kind::Line | Kind::Arc3 { .. } => [0; 3],
    }
}

impl Writer {
    /// Every point is moved by `shift` (for the translation test).
    pub fn new(shift: [f64; 3]) -> Writer {
        Writer {
            text: String::new(),
            next: 0,
            shift,
            verts: BTreeMap::new(),
            edges: BTreeMap::new(),
            faces: Vec::new(),
        }
    }

    fn add(&mut self, body: &str) -> u64 {
        self.next += 1;
        let _ = writeln!(self.text, "#{}={body};", self.next);
        self.next
    }

    fn point(&mut self, p: [f64; 3]) -> u64 {
        let s = self.shift;
        self.add(&format!(
            "CARTESIAN_POINT('',({:?},{:?},{:?}))",
            p[0] + s[0],
            p[1] + s[1],
            p[2] + s[2]
        ))
    }

    fn dir(&mut self, d: [f64; 3]) -> u64 {
        self.add(&format!("DIRECTION('',({:?},{:?},{:?}))", d[0], d[1], d[2]))
    }

    fn placement(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u64 {
        let (o, z, x) = (self.point(o), self.dir(z), self.dir(x));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }

    fn vertex(&mut self, p: [f64; 3]) -> u64 {
        if let Some(&v) = self.verts.get(&key(p)) {
            return v;
        }
        let pt = self.point(p);
        let v = self.add(&format!("VERTEX_POINT('',#{pt})"));
        self.verts.insert(key(p), v);
        v
    }

    /// An oriented edge from `a` to `b`; an edge already made from `b` to
    /// `a` is reused reversed.
    fn oriented(&mut self, a: [f64; 3], b: [f64; 3], kind: Kind) -> u64 {
        if let Some(&e) = self.edges.get(&(key(b), key(a), arc_key(kind, true))) {
            return self.add(&format!("ORIENTED_EDGE('',*,*,#{e},.F.)"));
        }
        let (va, vb) = (self.vertex(a), self.vertex(b));
        let e = match kind {
            Kind::Line => {
                let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let p = self.point(a);
                let dd = self.dir(d.map(|x| x / len));
                let v = self.add(&format!("VECTOR('',#{dd},{len:?})"));
                let l = self.add(&format!("LINE('',#{p},#{v})"));
                self.add(&format!("EDGE_CURVE('',#{va},#{vb},#{l},.T.)"))
            }
            Kind::Arc { centre, ccw } => {
                let c = [centre[0], centre[1], a[2]];
                let r = ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2)).sqrt();
                let x = [(a[0] - c[0]) / r, (a[1] - c[1]) / r, 0.0];
                let z = if ccw {
                    [0.0, 0.0, 1.0]
                } else {
                    [0.0, 0.0, -1.0]
                };
                let ax = self.placement(c, z, x);
                let ci = self.add(&format!("CIRCLE('',#{ax},{r:?})"));
                self.add(&format!("EDGE_CURVE('',#{va},#{vb},#{ci},.T.)"))
            }
            Kind::Arc3 { centre: c, axis } => {
                let d = [a[0] - c[0], a[1] - c[1], a[2] - c[2]];
                let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let ax = self.placement(c, axis, d.map(|x| x / r));
                let ci = self.add(&format!("CIRCLE('',#{ax},{r:?})"));
                self.add(&format!("EDGE_CURVE('',#{va},#{vb},#{ci},.T.)"))
            }
        };
        self.edges.insert((key(a), key(b), arc_key(kind, false)), e);
        self.add(&format!("ORIENTED_EDGE('',*,*,#{e},.T.)"))
    }

    fn bound(&mut self, edges: &[([f64; 3], [f64; 3], Kind)], outer: bool) -> u64 {
        let oes: Vec<String> = edges
            .iter()
            .map(|&(a, b, k)| format!("#{}", self.oriented(a, b, k)))
            .collect();
        let lp = self.add(&format!("EDGE_LOOP('',({}))", oes.join(",")));
        let kw = if outer {
            "FACE_OUTER_BOUND"
        } else {
            "FACE_BOUND"
        };
        self.add(&format!("{kw}('',#{lp},.T.)"))
    }

    fn face(&mut self, bounds: &[u64], surface: u64, same: bool) {
        let list: Vec<String> = bounds.iter().map(|b| format!("#{b}")).collect();
        let f = self.add(&format!(
            "ADVANCED_FACE('',({}),#{surface},.{}.)",
            list.join(","),
            if same { "T" } else { "F" }
        ));
        self.faces.push(f);
    }

    /// Side faces of `prof` from `z0` to `z1`; `outward` for a boss, inward
    /// for a pocket wall.
    pub fn walls(&mut self, prof: &Profile, z0: f64, z1: f64, outward: bool) {
        let n = prof.pts.len();
        for i in 0..n {
            let (p, q) = (prof.pts[i], prof.pts[(i + 1) % n]);
            let (p0, q0, q1, p1) = (
                [p[0], p[1], z0],
                [q[0], q[1], z0],
                [q[0], q[1], z1],
                [p[0], p[1], z1],
            );
            let k = prof.kinds[i];
            let rev = match k {
                Kind::Line | Kind::Arc3 { .. } => Kind::Line,
                Kind::Arc { centre, ccw } => Kind::Arc { centre, ccw: !ccw },
            };
            let b = self.bound(
                &[
                    (p0, q0, k),
                    (q0, q1, Kind::Line),
                    (q1, p1, rev),
                    (p1, p0, Kind::Line),
                ],
                true,
            );
            let surface = match k {
                Kind::Line => {
                    let d = [q[0] - p[0], q[1] - p[1]];
                    let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    let right = [d[1] / l, -d[0] / l, 0.0];
                    let nrm = if outward { right } else { right.map(|x| -x) };
                    let pl = self.placement(p0, nrm, [d[0] / l, d[1] / l, 0.0]);
                    self.add(&format!("PLANE('',#{pl})"))
                }
                Kind::Arc3 { .. } => panic!("walls take planar profiles"),
                Kind::Arc { centre, .. } => {
                    let r = ((p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2)).sqrt();
                    let ax = self.placement(
                        [centre[0], centre[1], z0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0],
                    );
                    self.add(&format!("CYLINDRICAL_SURFACE('',#{ax},{r:?})"))
                }
            };
            let same = match k {
                Kind::Line | Kind::Arc3 { .. } => true,
                Kind::Arc { ccw, .. } => ccw == outward,
            };
            self.face(&[b], surface, same);
        }
    }

    /// A horizontal face at `z` bounded by `outer` and `holes`, facing up or
    /// down.
    pub fn cap(&mut self, outer: &Profile, holes: &[Profile], z: f64, up: bool) {
        let edges = |p: &Profile| -> Vec<([f64; 3], [f64; 3], Kind)> {
            let n = p.pts.len();
            (0..n)
                .map(|i| {
                    let (a, b) = (p.pts[i], p.pts[(i + 1) % n]);
                    ([a[0], a[1], z], [b[0], b[1], z], p.kinds[i])
                })
                .collect()
        };
        let mut bounds = vec![self.bound(&edges(outer), true)];
        for h in holes {
            bounds.push(self.bound(&edges(h), false));
        }
        let o = [outer.pts[0][0], outer.pts[0][1], z];
        let nrm = if up {
            [0.0, 0.0, 1.0]
        } else {
            [0.0, 0.0, -1.0]
        };
        let pl = self.placement(o, nrm, [1.0, 0.0, 0.0]);
        let s = self.add(&format!("PLANE('',#{pl})"));
        self.face(&bounds, s, true);
    }

    /// A planar face with normal `n` bounded by `edges` (counter-clockwise
    /// seen from the side `n` points to).
    pub fn plane_face(&mut self, edges: &[([f64; 3], [f64; 3], Kind)], n: [f64; 3], x: [f64; 3]) {
        let b = self.bound(edges, true);
        let pl = self.placement(edges[0].0, n, x);
        let s = self.add(&format!("PLANE('',#{pl})"));
        self.face(&[b], s, true);
    }

    /// A cylindrical face of radius `r` about the line through `o` along
    /// `axis`, facing away from the axis, bounded by `edges`.
    pub fn cylinder_face(
        &mut self,
        edges: &[([f64; 3], [f64; 3], Kind)],
        o: [f64; 3],
        axis: [f64; 3],
        x: [f64; 3],
        r: f64,
    ) {
        let b = self.bound(edges, true);
        let ax = self.placement(o, axis, x);
        let s = self.add(&format!("CYLINDRICAL_SURFACE('',#{ax},{r:?})"));
        self.face(&[b], s, true);
    }

    /// A spherical face of radius `r` about `centre`, facing away from it,
    /// bounded by `edges` (counter-clockwise seen from outside).
    pub fn sphere_face(&mut self, edges: &[([f64; 3], [f64; 3], Kind)], centre: [f64; 3], r: f64) {
        let b = self.bound(edges, true);
        let ax = self.placement(centre, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let s = self.add(&format!("SPHERICAL_SURFACE('',#{ax},{r:?})"));
        self.face(&[b], s, true);
    }

    /// The file: one solid of every face added.
    pub fn finish(mut self, name: &str) -> String {
        let list: Vec<String> = self.faces.iter().map(|f| format!("#{f}")).collect();
        let shell = self.add(&format!("CLOSED_SHELL('',({}))", list.join(",")));
        let body = self.add(&format!("MANIFOLD_SOLID_BREP('{name}',#{shell})"));
        format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('FluutCAM test part'),'2;1');\n\
             FILE_NAME('{name}.step','',(''),(''),'','','');\n\
             FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n{}\
             #9001=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{body}),#9002);\n\
             #9002=(GEOMETRIC_REPRESENTATION_CONTEXT(3) \
             GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#9005)) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#9003,#9004)) REPRESENTATION_CONTEXT('',''));\n\
             #9003=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
             #9004=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
             #9005=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#9003,'','');\n\
             ENDSEC;\nEND-ISO-10303-21;\n",
            self.text
        )
    }
}

/// A 40 × 30 × 10 boss with 5 mm corner radii, its lower left corner at
/// the origin.
pub fn boss(shift: [f64; 3]) -> String {
    let mut w = Writer::new(shift);
    let p = Profile::rounded_rect(0.0, 0.0, 40.0, 30.0, 5.0);
    w.walls(&p, 0.0, 10.0, true);
    w.cap(&p, &[], 0.0, false);
    w.cap(&p, &[], 10.0, true);
    w.finish("boss")
}

/// An 80 × 60 × 20 block with a 60 × 40 pocket 8 mm deep (floor at Z 12)
/// and a 10 × 10 island in the middle.
pub fn pocket_block(shift: [f64; 3]) -> String {
    let mut w = Writer::new(shift);
    let outer = Profile::rect(0.0, 0.0, 80.0, 60.0);
    let pocket = Profile::rect(10.0, 10.0, 70.0, 50.0);
    let island = Profile::rect(35.0, 25.0, 45.0, 35.0);
    w.walls(&outer, 0.0, 20.0, true);
    w.cap(&outer, &[], 0.0, false);
    w.cap(&outer, std::slice::from_ref(&pocket), 20.0, true);
    w.walls(&pocket, 12.0, 20.0, false);
    w.cap(&pocket, std::slice::from_ref(&island), 12.0, true);
    w.walls(&island, 12.0, 20.0, true);
    w.cap(&island, &[], 20.0, true);
    w.finish("pocket_block")
}

/// Radius of the [`ridge`] top.
pub const RIDGE_RADIUS: f64 = 40.0;

/// Z of the [`ridge`] cylinder axis: the top meets the 10 mm walls at
/// y = 0 and y = 40.
pub fn ridge_axis_z() -> f64 {
    10.0 - (RIDGE_RADIUS * RIDGE_RADIUS - 400.0).sqrt()
}

/// A 60 × 40 block, 10 mm high at its long sides, whose top is a convex
/// cylinder of radius 40 along X: a ridge 5.36 mm high at y = 20. Faces:
/// bottom, two sides, two ends, and the cylinder.
pub fn ridge(shift: [f64; 3]) -> String {
    let mut w = Writer::new(shift);
    let zc = ridge_axis_z();
    let (a, b, c, d) = (
        [0.0, 0.0, 10.0],
        [60.0, 0.0, 10.0],
        [60.0, 40.0, 10.0],
        [0.0, 40.0, 10.0],
    );
    let arc = |x: f64| Kind::Arc3 {
        centre: [x, 20.0, zc],
        axis: [1.0, 0.0, 0.0],
    };
    w.cap(&Profile::rect(0.0, 0.0, 60.0, 40.0), &[], 0.0, false);
    let l = Kind::Line;
    w.plane_face(
        &[
            ([0.0, 0.0, 0.0], [60.0, 0.0, 0.0], l),
            ([60.0, 0.0, 0.0], b, l),
            (b, a, l),
            (a, [0.0, 0.0, 0.0], l),
        ],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
    );
    w.plane_face(
        &[
            ([60.0, 40.0, 0.0], [0.0, 40.0, 0.0], l),
            ([0.0, 40.0, 0.0], d, l),
            (d, c, l),
            (c, [60.0, 40.0, 0.0], l),
        ],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
    );
    w.plane_face(
        &[
            ([60.0, 0.0, 0.0], [60.0, 40.0, 0.0], l),
            ([60.0, 40.0, 0.0], c, l),
            (c, b, arc(60.0)),
            (b, [60.0, 0.0, 0.0], l),
        ],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    );
    w.plane_face(
        &[
            ([0.0, 40.0, 0.0], [0.0, 0.0, 0.0], l),
            ([0.0, 0.0, 0.0], a, l),
            (
                a,
                d,
                Kind::Arc3 {
                    centre: [0.0, 20.0, zc],
                    axis: [-1.0, 0.0, 0.0],
                },
            ),
            (d, [0.0, 40.0, 0.0], l),
        ],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
    );
    w.cylinder_face(
        &[(a, b, l), (b, c, arc(60.0)), (c, d, l), (d, a, arc(0.0))],
        [0.0, 20.0, zc],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        RIDGE_RADIUS,
    );
    w.finish("ridge")
}

/// Centre (XY) and radius of the [`dome`] boss.
pub const DOME_CENTRE: [f64; 2] = [40.0, 30.0];
pub const DOME_RADIUS: f64 = 12.0;
/// Z of the top of the [`dome`] boss's cylinder (the hemisphere's centre).
pub const DOME_SHOULDER: f64 = 16.0;

/// An 80 × 60 × 10 block with a round boss of radius 12 at (40, 30): a
/// vertical cylinder from Z 10 to 16 under a hemisphere, top at Z 28.
/// Faces: bottom, four sides, the top with a hole, two half cylinders and
/// the hemisphere.
pub fn dome(shift: [f64; 3]) -> String {
    let mut w = Writer::new(shift);
    let outer = Profile::rect(0.0, 0.0, 80.0, 60.0);
    let [cx, cy] = DOME_CENTRE;
    let r = DOME_RADIUS;
    let arc = Kind::Arc {
        centre: [cx, cy],
        ccw: true,
    };
    let circle = Profile {
        pts: vec![[cx - r, cy], [cx + r, cy]],
        kinds: vec![arc; 2],
    };
    w.walls(&outer, 0.0, 10.0, true);
    w.cap(&outer, &[], 0.0, false);
    w.cap(&outer, std::slice::from_ref(&circle), 10.0, true);
    w.walls(&circle, 10.0, DOME_SHOULDER, true);
    let (a, b) = ([cx - r, cy, DOME_SHOULDER], [cx + r, cy, DOME_SHOULDER]);
    w.sphere_face(&[(a, b, arc), (b, a, arc)], [cx, cy, DOME_SHOULDER], r);
    w.finish("dome")
}
