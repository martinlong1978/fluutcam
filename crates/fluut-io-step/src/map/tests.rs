// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::parse::parse;
use fluut_topo::check;
use std::fmt::Write;

/// Builds DATA-section text with sequential entity ids.
#[derive(Default)]
struct Step {
    text: String,
    next: u64,
}

impl Step {
    fn add(&mut self, body: &str) -> u64 {
        self.next += 1;
        let _ = writeln!(self.text, "#{}={body};", self.next);
        self.next
    }

    fn point(&mut self, p: [f64; 3]) -> u64 {
        self.add(&format!(
            "CARTESIAN_POINT('',({:?},{:?},{:?}))",
            p[0], p[1], p[2]
        ))
    }

    fn dir(&mut self, d: [f64; 3]) -> u64 {
        self.add(&format!("DIRECTION('',({:?},{:?},{:?}))", d[0], d[1], d[2]))
    }

    fn placement(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u64 {
        let (o, z, x) = (self.point(o), self.dir(z), self.dir(x));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }

    fn file(&self, unit: &str) -> String {
        format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;\n{}\
             #9001=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{}),#9002);\n\
             #9002=(GEOMETRIC_REPRESENTATION_CONTEXT(3) \
             GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#9005)) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#9003,#9004)) REPRESENTATION_CONTEXT('',''));\n\
             #9003={unit};\n\
             #9004=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
             #9005=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#9003,'','');\n\
             ENDSEC;END-ISO-10303-21;",
            self.text, self.next
        )
    }
}

const MM: &str = "(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))";

/// The unit cube with some edges running against their loops, so that
/// ORIENTED_EDGE and EDGE_CURVE senses both matter.
fn cube_step(size: f64) -> Step {
    let mut s = Step::default();
    let corner = |i: u32| {
        [
            (i & 1) as f64 * size,
            ((i >> 1) & 1) as f64 * size,
            ((i >> 2) & 1) as f64 * size,
        ]
    };
    let mut verts = Vec::new();
    for i in 0..8 {
        let p = s.point(corner(i));
        verts.push(s.add(&format!("VERTEX_POINT('',#{p})")));
    }
    let faces: [([u32; 4], [f64; 3]); 6] = [
        ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
    ];
    let mut edges: BTreeMap<(u32, u32), (u64, u32)> = BTreeMap::new();
    let mut face_ids = Vec::new();
    for (cyc, n) in &faces {
        let mut oes = Vec::new();
        for j in 0..4 {
            let (a, b) = (cyc[j], cyc[(j + 1) % 4]);
            let key = (a.min(b), a.max(b));
            let (ec, start) = *edges.entry(key).or_insert_with(|| {
                // Edge curves alternate between running with and against
                // their line; the line always runs low → high vertex.
                let (lo, hi) = (corner(key.0), corner(key.1));
                let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                let p = s.point(lo);
                let dd = s.dir(d);
                let v = s.add(&format!("VECTOR('',#{dd},{size:?})"));
                let line = s.add(&format!("LINE('',#{p},#{v})"));
                let same = (key.0 + key.1) % 2 == 0;
                let (vs, ve) = if same { (key.0, key.1) } else { (key.1, key.0) };
                let id = s.add(&format!(
                    "EDGE_CURVE('',#{},#{},#{line},.{}.)",
                    verts[vs as usize],
                    verts[ve as usize],
                    if same { "T" } else { "F" }
                ));
                (id, vs)
            });
            let o = if start == a { "T" } else { "F" };
            oes.push(s.add(&format!("ORIENTED_EDGE('',*,*,#{ec},.{o}.)")));
        }
        let list = oes
            .iter()
            .map(|i| format!("#{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let lp = s.add(&format!("EDGE_LOOP('',({list}))"));
        let bound = s.add(&format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let o = corner(cyc[0]);
        let x = {
            let b = corner(cyc[1]);
            [b[0] - o[0], b[1] - o[1], b[2] - o[2]]
        };
        let pl = s.placement(o, *n, x);
        let plane = s.add(&format!("PLANE('',#{pl})"));
        face_ids.push(s.add(&format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
    }
    let list = face_ids
        .iter()
        .map(|i| format!("#{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let shell = s.add(&format!("CLOSED_SHELL('',({list}))"));
    s.add(&format!("MANIFOLD_SOLID_BREP('cube',#{shell})"));
    s
}

fn map_one(src: &str) -> MappedBody {
    let m = parse(src.as_bytes()).unwrap();
    let mut bodies = map_bodies(&m);
    assert_eq!(bodies.len(), 1);
    bodies.pop().unwrap().unwrap()
}

#[test]
fn cube_maps_and_is_valid() {
    let b = map_one(&cube_step(10.0).file(MM));
    assert_eq!(b.body.name, "cube");
    assert!(b.skipped.is_empty(), "{:?}", b.skipped);
    assert_eq!(
        (
            b.body.vertices.len(),
            b.body.edges.len(),
            b.body.faces.len()
        ),
        (8, 12, 6)
    );
    assert_eq!(check(&b.body, b.body.tol_model), vec![]);
    assert_eq!(b.units.uncertainty_mm, Some(1e-6));
}

#[test]
fn inch_cube_is_scaled() {
    let inch = "(CONVERSION_BASED_UNIT('INCH',#9006) LENGTH_UNIT() NAMED_UNIT(*));\n\
                #9006=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#9007);\n\
                #9007=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))";
    let b = map_one(&cube_step(1.0).file(inch));
    let far = b
        .body
        .vertices
        .iter()
        .map(|v| v.p.x() + v.p.y() + v.p.z())
        .fold(0.0, f64::max);
    assert!((far - 3.0 * 25.4).abs() < 1e-9, "{far}");
    assert_eq!(check(&b.body, 1e-6), vec![]);
}

#[test]
fn flipped_orientation_is_caught() {
    // Turning one ORIENTED_EDGE around breaks its loop and the manifold
    // rule, so the check sees the combined senses.
    let src = cube_step(10.0).file(MM);
    let at = src.find("ORIENTED_EDGE").unwrap();
    let end = at + src[at..].find(';').unwrap();
    let seg = &src[at..end];
    let new = if seg.contains(".T.") {
        seg.replace(".T.", ".F.")
    } else {
        seg.replace(".F.", ".T.")
    };
    let mut flipped = src.clone();
    flipped.replace_range(at..end, &new);
    let b = map_one(&flipped);
    assert!(!check(&b.body, 1e-6).is_empty());
}

const CTX2: &str = "(GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() \
                    REPRESENTATION_CONTEXT('2D SPACE',''))";

impl Step {
    /// A 2-D `LINE` through `p` along `d`, as the curve of a `PCURVE` on `surf`.
    fn pcurve_line(&mut self, surf: u64, p: [f64; 2], d: [f64; 2], mag: f64) -> u64 {
        let pt = self.add(&format!("CARTESIAN_POINT('',({:?},{:?}))", p[0], p[1]));
        let dir = self.add(&format!("DIRECTION('',({:?},{:?}))", d[0], d[1]));
        let v = self.add(&format!("VECTOR('',#{dir},{mag:?})"));
        let l = self.add(&format!("LINE('',#{pt},#{v})"));
        let ctx = self.add(CTX2);
        let rep = self.add(&format!("DEFINITIONAL_REPRESENTATION('',(#{l}),#{ctx})"));
        self.add(&format!("PCURVE('',#{surf},#{rep})"))
    }
}

/// A closed cylinder with a seam. With `pcurves = Some(swapped)` the side
/// face's edges carry p-curves, the seam's two in either order.
fn cylinder_step(pcurves: Option<bool>) -> Step {
    let mut s = Step::default();
    let (r, h) = (5.0, 10.0);
    let tau = std::f64::consts::TAU;
    let ax0 = s.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let ax1 = s.placement([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cyl = s.add(&format!("CYLINDRICAL_SURFACE('',#{ax0},{r:?})"));
    let pa = s.point([r, 0.0, 0.0]);
    let pb = s.point([r, 0.0, h]);
    let va = s.add(&format!("VERTEX_POINT('',#{pa})"));
    let vb = s.add(&format!("VERTEX_POINT('',#{pb})"));
    let mut c0 = s.add(&format!("CIRCLE('',#{ax0},{r:?})"));
    let mut c1 = s.add(&format!("CIRCLE('',#{ax1},{r:?})"));
    let sp = s.point([r, 0.0, 0.0]);
    let sd = s.dir([0.0, 0.0, 1.0]);
    let sv = s.add(&format!("VECTOR('',#{sd},1.)"));
    let mut sl = s.add(&format!("LINE('',#{sp},#{sv})"));
    if let Some(swapped) = pcurves {
        let p0 = s.pcurve_line(cyl, [0.0, 0.0], [1.0, 0.0], 1.0);
        c0 = s.add(&format!("SURFACE_CURVE('',#{c0},(#{p0}),.CURVE_3D.)"));
        let p1 = s.pcurve_line(cyl, [0.0, h], [1.0, 0.0], 1.0);
        c1 = s.add(&format!("SURFACE_CURVE('',#{c1},(#{p1}),.CURVE_3D.)"));
        let mut a = s.pcurve_line(cyl, [0.0, 0.0], [0.0, 1.0], 1.0);
        let mut b = s.pcurve_line(cyl, [tau, 0.0], [0.0, 1.0], 1.0);
        if swapped {
            std::mem::swap(&mut a, &mut b);
        }
        sl = s.add(&format!("SEAM_CURVE('',#{sl},(#{a},#{b}),.CURVE_3D.)"));
    }
    let e0 = s.add(&format!("EDGE_CURVE('',#{va},#{va},#{c0},.T.)"));
    let e1 = s.add(&format!("EDGE_CURVE('',#{vb},#{vb},#{c1},.T.)"));
    let e2 = s.add(&format!("EDGE_CURVE('',#{va},#{vb},#{sl},.T.)"));
    let oe = |s: &mut Step, e: u64, o: bool| {
        s.add(&format!(
            "ORIENTED_EDGE('',*,*,#{e},.{}.)",
            if o { "T" } else { "F" }
        ))
    };
    let side = [
        oe(&mut s, e0, true),
        oe(&mut s, e2, true),
        oe(&mut s, e1, false),
        oe(&mut s, e2, false),
    ];
    let bottom = oe(&mut s, e0, false);
    let top = oe(&mut s, e1, true);
    let loop_of = |s: &mut Step, oes: &[u64]| {
        let l = oes
            .iter()
            .map(|i| format!("#{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let lp = s.add(&format!("EDGE_LOOP('',({l}))"));
        s.add(&format!("FACE_OUTER_BOUND('',#{lp},.T.)"))
    };
    let side_b = loop_of(&mut s, &side);
    let bottom_b = loop_of(&mut s, &[bottom]);
    let top_b = loop_of(&mut s, &[top]);
    let down = s.placement([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let pl0 = s.add(&format!("PLANE('',#{down})"));
    let pl1 = s.add(&format!("PLANE('',#{ax1})"));
    let f0 = s.add(&format!("ADVANCED_FACE('',(#{side_b}),#{cyl},.T.)"));
    let f1 = s.add(&format!("ADVANCED_FACE('',(#{bottom_b}),#{pl0},.T.)"));
    let f2 = s.add(&format!("ADVANCED_FACE('',(#{top_b}),#{pl1},.T.)"));
    let sh = s.add(&format!("CLOSED_SHELL('',(#{f0},#{f1},#{f2}))"));
    s.add(&format!("MANIFOLD_SOLID_BREP('cyl',#{sh})"));
    s
}

/// Start and end (u, v) of coedge `c` in its direction of travel.
fn uv_ends(b: &BrepBody, c: u32) -> (fluut_geom::Point2, fluut_geom::Point2) {
    let ce = &b.coedges[c as usize];
    let r = b.edges[ce.edge as usize].range;
    let pc = b.pcurves[c as usize].as_ref().unwrap();
    let (p, q) = (pc.eval(r.lo), pc.eval(r.hi));
    if ce.forward { (p, q) } else { (q, p) }
}

/// The side face's loop closes in (u, v) and its seam coedges are one
/// period apart.
fn assert_side_loop(b: &BrepBody) {
    let side = &b.loops[b.faces[0].loops[0] as usize].coedges;
    for i in 0..4 {
        let (_, e) = uv_ends(b, side[i]);
        let (s, _) = uv_ends(b, side[(i + 1) % 4]);
        assert!(e.distance(s) < 1e-9, "joint {i}: {e:?} {s:?}");
    }
    let (a, _) = uv_ends(b, side[1]);
    let (c, _) = uv_ends(b, side[3]);
    assert!(((a.x() - c.x()).abs() - std::f64::consts::TAU).abs() < 1e-9);
}

#[test]
fn cylinder_with_seam() {
    let b = map_one(&cylinder_step(None).file(MM));
    assert_eq!(check(&b.body, 1e-6), vec![]);
    let circle = &b.body.edges[0];
    assert!((circle.range.length() - std::f64::consts::TAU).abs() < 1e-12);
    // No p-curves in the file: all projected, exactly.
    let st = &b.pcurves;
    assert_eq!(
        (st.coedges, st.given, st.exact, st.within_tol),
        (6, 0, 6, 6)
    );
    assert_side_loop(&b.body);
}

#[test]
fn file_pcurves_are_used_and_seams_follow_the_loop() {
    for swapped in [false, true] {
        let b = map_one(&cylinder_step(Some(swapped)).file(MM));
        let st = &b.pcurves;
        assert!(st.failures.is_empty(), "{:?}", st.failures);
        assert_eq!((st.given, st.exact, st.within_tol), (4, 2, 6), "{st:?}");
        assert_eq!(b.pcurves_rejected, 0);
        assert_side_loop(&b.body);
    }
}

/// A 10 mm square in the plane z = 0 whose edges carry p-curves, the first
/// with `pc0 = (point, direction, magnitude)`.
fn square_step(pc0: ([f64; 2], [f64; 2], f64)) -> Step {
    let mut s = Step::default();
    let ax = s.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let plane = s.add(&format!("PLANE('',#{ax})"));
    let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let vs: Vec<u64> = c
        .iter()
        .map(|p| {
            let pt = s.point([p[0], p[1], 0.0]);
            s.add(&format!("VERTEX_POINT('',#{pt})"))
        })
        .collect();
    let mut oes = Vec::new();
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        let d = [(b[0] - a[0]) / 10.0, (b[1] - a[1]) / 10.0];
        let pt = s.point([a[0], a[1], 0.0]);
        let dir = s.dir([d[0], d[1], 0.0]);
        let v = s.add(&format!("VECTOR('',#{dir},1.)"));
        let line = s.add(&format!("LINE('',#{pt},#{v})"));
        let pc = if i == 0 {
            s.pcurve_line(plane, pc0.0, pc0.1, pc0.2)
        } else {
            s.pcurve_line(plane, a, d, 1.0)
        };
        let sc = s.add(&format!("SURFACE_CURVE('',#{line},(#{pc}),.CURVE_3D.)"));
        let ec = s.add(&format!(
            "EDGE_CURVE('',#{},#{},#{sc},.T.)",
            vs[i],
            vs[(i + 1) % 4]
        ));
        oes.push(s.add(&format!("ORIENTED_EDGE('',*,*,#{ec},.T.)")));
    }
    let l = oes
        .iter()
        .map(|i| format!("#{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let lp = s.add(&format!("EDGE_LOOP('',({l}))"));
    let bound = s.add(&format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
    let f = s.add(&format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)"));
    let sh = s.add(&format!("OPEN_SHELL('',(#{f}))"));
    s.add(&format!("SHELL_BASED_SURFACE_MODEL('sq',(#{sh}))"));
    s
}

#[test]
fn file_pcurve_with_another_parameter_is_reparametrised() {
    // Magnitude 10: the p-curve's parameter is a tenth of the edge's.
    let b = map_one(&square_step(([0.0, 0.0], [1.0, 0.0], 10.0)).file(MM));
    assert_eq!((b.pcurves.given, b.pcurves.within_tol), (4, 4));
    assert_eq!(b.pcurves_rejected, 0);
    let pc = b.body.pcurves[0].as_ref().unwrap();
    assert!(pc.eval(7.0).distance(fluut_geom::Point2::new(7.0, 0.0)) < 1e-9);
}

#[test]
fn wrong_file_pcurve_is_replaced() {
    // The first p-curve is 1 mm off its edge.
    let b = map_one(&square_step(([0.0, 1.0], [1.0, 0.0], 1.0)).file(MM));
    assert_eq!(b.pcurves_rejected, 1);
    let st = &b.pcurves;
    assert_eq!((st.given, st.exact, st.within_tol), (3, 1, 4), "{st:?}");
    let pc = b.body.pcurves[0].as_ref().unwrap();
    assert!(pc.eval(3.0).distance(fluut_geom::Point2::new(3.0, 0.0)) < 1e-9);
}

#[test]
fn cube_pcurves_are_projected() {
    let b = map_one(&cube_step(10.0).file(MM));
    let st = &b.pcurves;
    assert_eq!((st.coedges, st.exact, st.within_tol), (24, 24, 24));
    assert!(st.max_dev < 1e-12);
}

#[test]
fn unsupported_surface_skips_face() {
    let src = cube_step(10.0).file(MM);
    let i = src.find("PLANE('',").unwrap();
    let mut s = src.clone();
    s.replace_range(i..i + 5, "WEIRD");
    let b = map_one(&s);
    assert_eq!(b.body.faces.len(), 5);
    assert_eq!(b.skipped.len(), 1);
    assert!(b.skipped[0].msg.contains("WEIRD"), "{:?}", b.skipped);
}

#[test]
fn closed_bspline_edge_spans_its_domain() {
    // A closed quadratic B-spline (a rounded square) as one edge of a
    // planar face, with one vertex where it closes.
    let mut s = Step::default();
    let ax = s.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let plane = s.add(&format!("PLANE('',#{ax})"));
    let pts = [
        [5.0, 0.0],
        [5.0, 5.0],
        [-5.0, 5.0],
        [-5.0, -5.0],
        [5.0, -5.0],
        [5.0, 0.0],
    ];
    let ids: Vec<String> = pts
        .iter()
        .map(|p| format!("#{}", s.point([p[0], p[1], 0.0])))
        .collect();
    let bs = s.add(&format!(
        "B_SPLINE_CURVE_WITH_KNOTS('',2,({}),.UNSPECIFIED.,.T.,.F.,(3,1,1,1,3),(0.,0.25,0.5,0.75,1.),.UNSPECIFIED.)",
        ids.join(",")
    ));
    let v = s.add(&format!("VERTEX_POINT('',{})", ids[0]));
    let ec = s.add(&format!("EDGE_CURVE('',#{v},#{v},#{bs},.T.)"));
    let oe = s.add(&format!("ORIENTED_EDGE('',*,*,#{ec},.T.)"));
    let lp = s.add(&format!("EDGE_LOOP('',(#{oe}))"));
    let bound = s.add(&format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
    let f = s.add(&format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)"));
    let sh = s.add(&format!("OPEN_SHELL('',(#{f}))"));
    s.add(&format!("SHELL_BASED_SURFACE_MODEL('loop',(#{sh}))"));
    let b = map_one(&s.file(MM));
    let r = b.body.edges[0].range;
    assert_eq!((r.lo, r.hi), (0.0, 1.0));
    assert_eq!(b.pcurves.within_tol, 1, "{:?}", b.pcurves);
}
