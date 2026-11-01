// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `boundary` of the Geometry group (D15 §2.3.2): closed chains that
//! limit a tool-centre region, offset by the tool containment.

use crate::chain::chain_pieces;
use crate::drive::{curve_pieces, nest};
use crate::region::Region2;
use crate::toolpath::StrategyInput;
use fluut_model::Diagnostic;
use fluut_model::resolve::{Boundary, BoundarySide, Containment, ResolvedParams};

/// The boundary chains as a region, or `None` without a boundary. Open
/// chains are `param.geometry.boundary.open`.
pub fn boundary_region(
    input: &StrategyInput,
    p: &ResolvedParams,
    tol: f64,
) -> Result<Option<Region2>, Vec<Diagnostic>> {
    let Some(b) = &p.geometry.boundary else {
        return Ok(None);
    };
    let g = input.geometry;
    let bc = chain_pieces(
        curve_pieces(g, &b.curves, tol / 4.0, "geometry.boundary.curves").map_err(|d| vec![d])?,
        g.tol_model().max(1e-5),
    );
    if bc.iter().any(|c| !c.path.closed) {
        return Err(vec![
            Diagnostic::error(
                "param.geometry.boundary.open",
                "a boundary chain is not closed",
            )
            .at("geometry.boundary.curves"),
        ]);
    }
    Ok(Some(nest(bc.iter().map(|c| c.ccw_loop()).collect())))
}

/// The tool-centre region `centre` limited by the boundary region `b`,
/// grown by −r, 0 or +r for the containment (as `pocket_2d`).
pub fn apply_boundary(centre: &Region2, b: &Region2, bp: &Boundary, r: f64) -> Region2 {
    let grow = match bp.containment {
        Containment::Inside => -r,
        Containment::Centre => 0.0,
        Containment::Outside => r,
    };
    match bp.side {
        BoundarySide::Inside => centre.intersect(&b.offset(grow)),
        BoundarySide::Outside => centre.subtract(&b.offset(-grow)),
    }
}
