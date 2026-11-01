// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bit-exact pins of region results (D01 §6, CH-01).
//!
//! Offsets run inside `cavalier_contours`, which the workspace patches to use
//! `libm` for trigonometry; booleans run in `fluut-cam`. Every platform must
//! give the same bits. When a pin fails on one platform only, a platform maths
//! function has crept in. When an intended change to the algorithms moves
//! the bits, update the pins and say why in the commit.

use fluut_cam::Region2;
use fluut_cam::spike::shapes;

/// FNV-1a over the bits of every vertex, in loop order.
fn vertex_hash(r: &Region2) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for l in &r.loops {
        for v in &l.verts {
            for x in [v.p.x(), v.p.y(), v.bulge] {
                for byte in x.to_bits().to_le_bytes() {
                    h ^= u64::from(byte);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
    }
    h
}

/// `(area bits, vertex count, vertex hash)`.
fn pin(r: &Region2) -> (u64, usize, u64) {
    let n = r.loops.iter().map(|l| l.verts.len()).sum();
    (r.area().to_bits(), n, vertex_hash(r))
}

fn check(name: &str, r: &Region2, want: (u64, usize, u64)) {
    let got = pin(r);
    assert_eq!(
        got,
        want,
        "{name}: got ({:#018x}, {}, {:#018x}), area {}",
        got.0,
        got.1,
        got.2,
        r.area()
    );
}

#[test]
fn offsets_are_bit_exact() {
    let cases = [
        (
            "plate -5",
            shapes::plate().offset(-5.0),
            (0x40ada24886bc0518, 6, 0x534b9d157c158d0c),
        ),
        (
            "plate -16 (splits)",
            shapes::plate().offset(-16.0),
            (0x40804e511edb7d86, 10, 0x4735996954884d4e),
        ),
        (
            "rooms +2.5",
            shapes::rooms().offset(2.5),
            (0x40b0c64518b3aa88, 20, 0x546e4dd79a68cd65),
        ),
        (
            "rooms -3",
            shapes::rooms().offset(-3.0),
            (0x40a357738a82669a, 16, 0xc08092561ea18d65),
        ),
        (
            "slotted disc -3",
            shapes::slotted_disc().offset(-3.0),
            (0x40acaab2870eefe0, 8, 0xeb3504991f28ba9c),
        ),
        (
            "slotted disc -3 +1.7",
            shapes::slotted_disc().offset(-3.0).offset(1.7),
            (0x40b06a11f30f3f50, 10, 0x63b4cc582d4075f2),
        ),
    ];
    for (name, r, want) in &cases {
        check(name, r, *want);
    }
}

#[test]
fn booleans_are_bit_exact() {
    let disc = Region2::circle(62.0, 30.0, 6.0);
    let lens = Region2::circle(0.0, 0.0, 5.0).intersect(&Region2::circle(3.3, 1.1, 4.0));
    let cases = [
        (
            "plate minus disc",
            shapes::plate().subtract(&disc),
            (0x40b5e073b0c35a73, 8, 0x3c38dab2e4497f92),
        ),
        ("lens", lens, (0x40403f65663bd082, 4, 0xcbce110eaee342e0)),
        (
            "rooms union offset disc",
            shapes::rooms().union(&shapes::slotted_disc().offset(-3.0)),
            (0x40b8b7d0db410d74, 19, 0xfc8fb40c3d3dace1),
        ),
        (
            "slotted disc",
            shapes::slotted_disc(),
            (0x40b21397a56640a5, 6, 0x6219e1dff0419ed6),
        ),
    ];
    for (name, r, want) in &cases {
        check(name, r, *want);
    }
}

/// Arc helpers of `cavalier_contours` that reach `acos` and `sin_cos`.
#[test]
fn arc_helpers_are_bit_exact() {
    use cavalier_contours::polyline::{PlineCreation, PlineSource, PlineSourceMut, Polyline};
    let mut pl = Polyline::<f64>::with_capacity(3, true);
    pl.add(10.0, 0.0, 0.4);
    pl.add(-3.0, 7.0, -0.7);
    pl.add(-5.0, -4.0, 0.0);
    let lines = pl.arcs_to_approx_lines(0.01).unwrap();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in lines.iter_vertexes() {
        for x in [v.x, v.y] {
            for byte in x.to_bits().to_le_bytes() {
                h ^= u64::from(byte);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    let (_, p) = pl.find_point_at_path_length(17.0).unwrap();
    let got = (lines.vertex_count(), h, p.x.to_bits(), p.y.to_bits());
    assert_eq!(
        got,
        (
            41,
            0x9daf_338a_2976_76b4,
            0xc003_4ba7_e57d_7254,
            0x401a_6dfb_ff4d_ba5c
        ),
        "{got:#x?}"
    );
}

/// Node and edge counts and a hash of node points, radii and edge
/// parameters of the medial axis (CH-17).
#[test]
fn medial_axes_are_bit_exact() {
    use fluut_cam::region::medial::medial_axis;
    let want = [
        ("A plate", 11, 11, 0xa537_bc9a_830c_486b),
        ("B rooms", 18, 17, 0x38f8_c9a5_ae6c_8ba7),
        ("C slotted disc", 7, 6, 0x815c_ca3f_e24b_f348),
    ];
    for ((name, r), want) in shapes::all().into_iter().zip(want) {
        let m = medial_axis(&r).unwrap();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |x: f64| {
            for byte in x.to_bits().to_le_bytes() {
                h ^= u64::from(byte);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for n in &m.nodes {
            eat(n.p.x());
            eat(n.p.y());
            eat(n.radius);
        }
        for e in &m.edges {
            eat(e.u0);
            eat(e.u1);
        }
        assert_eq!((name, m.nodes.len(), m.edges.len(), h), want);
    }
}
