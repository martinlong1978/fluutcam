// SPDX-License-Identifier: MIT OR Apache-2.0

//! Holes of `drill` and `bore` (D09 §2.8, §3.1, D15b §1.5–§1.6): from
//! cylindrical drive faces, full drive circles, or every cylinder whose
//! diameter passes the `hole_diameter` filter.

use crate::chain::chain_pieces;
use crate::drive::{curve_pieces, faces};
use crate::geometry::{BodyGeom, SetupGeometry, face_z_range};
use fluut_geom::{Point2, Surface};
use fluut_model::Diagnostic;
use fluut_model::resolve::GeometryGroup;
use fluut_topo::BrepBody;

/// Below this, a direction component counts as zero.
const ANGLE: f64 = 1e-9;

/// One cylinder (or a full drive circle) in W.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Cyl {
    pub centre: Point2,
    pub radius: f64,
    /// Z range of the face; for a circle, its Z.
    pub lo: f64,
    pub hi: f64,
    /// The material is inside the cylinder (a boss), not outside (a hole).
    pub boss: bool,
    /// The tip of a coaxial cone face whose top meets the bottom of the
    /// cylinder (a drilled hole bottom).
    pub cone_tip: Option<f64>,
    /// From a drive circle: the heights set its depth.
    pub circle: bool,
}

/// Cylinders of the selection, in selection order, with the half faces of
/// one cylinder merged. `kind` names the type in diagnostic codes. Faces
/// whose axis is not parallel to Z give `cam.<kind>.axis` (in `skipped`
/// for `drill`, an error for `bore`); curves that are not full circles give
/// `cam.<kind>.not_circle`.
#[derive(Debug, Default)]
pub struct Found {
    pub cyls: Vec<Cyl>,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

pub fn find(
    g: &SetupGeometry,
    geo: &GeometryGroup,
    kind: &str,
    tol: f64,
    axis_is_error: bool,
) -> Result<Found, Diagnostic> {
    let mut out = Found::default();
    let axis = |id: &str| {
        let d = Diagnostic::warning(
            &format!("cam.{kind}.axis"),
            format!("the cylinder `{id}` is not parallel to the tool axis"),
        );
        if axis_is_error {
            Diagnostic::error(&d.code, d.message)
        } else {
            d
        }
    };
    let push = |c: Cyl, v: &mut Vec<Cyl>| {
        let same = v.iter_mut().find(|o| {
            !o.circle
                && !c.circle
                && o.boss == c.boss
                && o.centre.distance(c.centre) <= tol
                && (o.radius - c.radius).abs() <= tol
                && o.lo <= c.hi + tol
                && c.lo <= o.hi + tol
        });
        match same {
            Some(o) => {
                o.lo = o.lo.min(c.lo);
                o.hi = o.hi.max(c.hi);
                o.cone_tip = o.cone_tip.or(c.cone_tip);
            }
            None => v.push(c),
        }
    };
    let chord = tol / 4.0;
    if geo.drive_faces.is_empty() && geo.drive_curves.is_empty() {
        if let Some((lo, hi)) = geo.hole_diameter {
            for b in &g.bodies {
                let BodyGeom::Brep(body) = &b.geom else {
                    continue;
                };
                for f in 0..body.faces.len() as u32 {
                    let Some(c) = cylinder(body, f, chord) else {
                        continue;
                    };
                    let Some(c) = c else {
                        continue; // not parallel: not a hole of this Setup
                    };
                    let d = 2.0 * c.radius;
                    if !c.boss
                        && lo.is_none_or(|m| d >= m - 1e-12)
                        && hi.is_none_or(|m| d <= m + 1e-12)
                    {
                        push(c, &mut out.cyls);
                    }
                }
            }
        }
        return Ok(out);
    }
    for (body, f, id) in faces(
        g,
        &geo.drive_faces,
        &geo.excluded_faces,
        "geometry.drive_faces",
    )? {
        match cylinder(body, f, chord) {
            None => {
                return Err(Diagnostic::error(
                    "param.geometry.drive_faces.kind",
                    format!("face `{id}` is not cylindrical"),
                )
                .at("geometry.drive_faces"));
            }
            Some(None) => {
                let d = axis(&id);
                if axis_is_error {
                    out.errors.push(d);
                } else {
                    out.warnings.push(d);
                }
            }
            Some(Some(c)) => push(c, &mut out.cyls),
        }
    }
    let pieces = curve_pieces(g, &geo.drive_curves, chord, "geometry.drive_curves")?;
    for ch in chain_pieces(pieces, g.tol_model().max(1e-5)) {
        let arcs: Vec<_> = ch.path.segs.iter().map(|s| s.arc()).collect();
        let first = arcs.first().copied().flatten();
        let circle = ch.path.closed
            && first.is_some_and(|a0| {
                arcs.iter().all(|a| {
                    a.is_some_and(|a| {
                        a.centre.distance(a0.centre) <= tol && (a.radius - a0.radius).abs() <= tol
                    })
                })
            });
        match (circle, first) {
            (true, Some(a)) => out.cyls.push(Cyl {
                centre: a.centre,
                radius: a.radius,
                lo: ch.z.lo,
                hi: ch.z.hi,
                boss: false,
                cone_tip: None,
                circle: true,
            }),
            _ => out.errors.push(Diagnostic::error(
                &format!("cam.{kind}.not_circle"),
                "a drive curve is not a full circle",
            )),
        }
    }
    Ok(out)
}

/// The cylinder of face `f`: `None` if the face is not cylindrical,
/// `Some(None)` if its axis is not parallel to Z.
fn cylinder(b: &BrepBody, f: u32, chord: f64) -> Option<Option<Cyl>> {
    let face = &b.faces[f as usize];
    let Surface::Cylinder { frame, r } = &b.surfaces[face.surface as usize] else {
        return None;
    };
    let z = frame.z.get();
    if z.x().abs() > ANGLE || z.y().abs() > ANGLE {
        return Some(None);
    }
    let zr = face_z_range(b, f, chord);
    let centre = Point2::new(frame.origin.x(), frame.origin.y());
    // The surface normal points away from the axis; a face whose normal
    // points to the axis has its material outside: a hole.
    let boss = face.same_sense;
    let cone_tip = if boss {
        None
    } else {
        cone_below(b, centre, zr.lo, chord)
    };
    Some(Some(Cyl {
        centre,
        radius: *r,
        lo: zr.lo,
        hi: zr.hi,
        boss,
        cone_tip,
        circle: false,
    }))
}

/// The apex Z of a cone face on the axis through `c` whose top is at `z`.
fn cone_below(b: &BrepBody, c: Point2, z: f64, chord: f64) -> Option<f64> {
    let tol = b.tol_model.max(1e-6) * 10.0;
    for (i, face) in b.faces.iter().enumerate() {
        let Surface::Cone {
            frame,
            r,
            semi_angle,
        } = &b.surfaces[face.surface as usize]
        else {
            continue;
        };
        let ax = frame.z.get();
        if ax.x().abs() > ANGLE || ax.y().abs() > ANGLE {
            continue;
        }
        let o = frame.origin;
        if Point2::new(o.x(), o.y()).distance(c) > tol {
            continue;
        }
        let zr = face_z_range(b, i as u32, chord);
        if (zr.hi - z).abs() > tol {
            continue;
        }
        let t = libm::tan(*semi_angle);
        if t.abs() < ANGLE {
            continue;
        }
        // Apex at v = −r / tan α along the frame axis.
        return Some(o.z() - r / t * ax.z());
    }
    None
}

/// A hole for `drill`: a coaxial stack of cylinders (D15b §3.1).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Stack {
    pub centre: Point2,
    pub top: f64,
    /// Bottom of the deepest cylinder at least the tool diameter wide.
    pub bottom: f64,
    pub diameter: f64,
    pub cone_tip: Option<f64>,
    pub circle: bool,
}

/// Groups hole cylinders into coaxial stacks, in order of first
/// appearance. Bosses are left out.
pub fn stacks(cyls: &[Cyl], tool_diameter: f64, tol: f64) -> Vec<Stack> {
    let mut groups: Vec<Vec<Cyl>> = Vec::new();
    for c in cyls.iter().filter(|c| !c.boss) {
        match groups
            .iter_mut()
            .find(|g| !c.circle && !g[0].circle && g[0].centre.distance(c.centre) <= tol)
        {
            Some(g) => g.push(*c),
            None => groups.push(vec![*c]),
        }
    }
    groups
        .into_iter()
        .map(|g| {
            let top = g.iter().map(|c| c.hi).fold(f64::NEG_INFINITY, f64::max);
            let wide = g
                .iter()
                .filter(|c| 2.0 * c.radius >= tool_diameter - tol)
                .min_by(|a, b| a.lo.total_cmp(&b.lo));
            let deepest = g
                .iter()
                .min_by(|a, b| a.lo.total_cmp(&b.lo))
                .copied()
                .unwrap_or(g[0]);
            let c = wide.copied().unwrap_or(deepest);
            Stack {
                centre: g[0].centre,
                top,
                bottom: c.lo,
                diameter: 2.0 * c.radius,
                cone_tip: c.cone_tip,
                circle: c.circle,
            }
        })
        .collect()
}

/// Visiting order: nearest neighbour from the point nearest `from`, then
/// 2-opt (D08 §8.1).
pub fn shortest_order(pts: &[Point2], from: Point2) -> Vec<usize> {
    let n = pts.len();
    if n == 0 {
        return Vec::new();
    }
    let mut left: Vec<usize> = (0..n).collect();
    let mut order = Vec::with_capacity(n);
    let mut cur = from;
    while !left.is_empty() {
        let (k, _) = left
            .iter()
            .enumerate()
            .min_by(|a, b| {
                pts[*a.1]
                    .distance(cur)
                    .total_cmp(&pts[*b.1].distance(cur))
                    .then(a.1.cmp(b.1))
            })
            .unwrap_or((0, &0));
        let i = left.remove(k);
        cur = pts[i];
        order.push(i);
    }
    // 2-opt on the open path from `from`.
    let d = |a: Point2, b: Point2| a.distance(b);
    let at = |o: &[usize], k: usize| if k == 0 { from } else { pts[o[k - 1]] };
    let mut improved = true;
    let mut rounds = 0;
    while improved && rounds < 100 {
        improved = false;
        rounds += 1;
        for i in 1..n {
            for j in i + 1..=n {
                // Reverse order[i-1 .. j-1]: edges (i-1, i) and (j-1, j).
                let a = at(&order, i - 1);
                let b = pts[order[i - 1]];
                let c = pts[order[j - 1]];
                let before = d(a, b) + if j < n { d(c, pts[order[j]]) } else { 0.0 };
                let after = d(a, c) + if j < n { d(b, pts[order[j]]) } else { 0.0 };
                if after < before - 1e-9 {
                    order[i - 1..j].reverse();
                    improved = true;
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_opt_untangles_a_crossing() {
        let pts = [
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(10.0, 0.0),
            Point2::new(0.0, 10.0),
        ];
        let o = shortest_order(&pts, Point2::new(-1.0, 0.0));
        let len: f64 = std::iter::once(Point2::new(-1.0, 0.0))
            .chain(o.iter().map(|&i| pts[i]))
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum();
        assert!((len - 31.0).abs() < 1e-9, "{o:?} {len}");
        assert_eq!(o[0], 0);
    }
}
