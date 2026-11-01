// SPDX-License-Identifier: MIT OR Apache-2.0

//! `engrave` (D09 §2.6, D15b §2.4): V-carving along the medial axis of
//! closed shapes. The exact medial axis of lines and arcs (CH-17) is not
//! built yet, so this type checks its parameters and drive chains and
//! then reports `cam.type.unavailable`.

use super::contour_2d::selection_z;
use crate::chain::chain_pieces;
use crate::drive::curve_pieces;
use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, prepare, tag};
use fluut_model::Diagnostic;
use fluut_model::params::ToolpathTypeId;
use fluut_model::resolve::resolve;

#[derive(Debug)]
pub struct Engrave;

impl ToolpathType for Engrave {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::Engrave
    }

    fn role(&self) -> Role {
        Role::Finish
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        let tp = input.toolpath;
        let fail = |d: Vec<Diagnostic>| CamError::Invalid(tag(d, tp));
        let r0 = resolve(&tp.params, input.tool, input.material);
        let tol = r0.passes.machining_tolerance.max(1e-4);
        let g = input.geometry;
        let chains = chain_pieces(
            curve_pieces(
                g,
                &r0.geometry.drive_curves,
                tol / 4.0,
                "geometry.drive_curves",
            )
            .map_err(|d| fail(vec![d]))?,
            g.tol_model().max(1e-5),
        );
        let prep = prepare(input, selection_z(&chains)).map_err(|e| match e {
            CamError::Invalid(d) => fail(d),
            e => e,
        })?;
        let mut diags = prep.warnings;
        let open: Vec<Diagnostic> = chains
            .iter()
            .filter(|c| !c.path.closed)
            .map(|_| {
                Diagnostic::error(
                    "cam.pocket_2d.region.open",
                    "a drive chain is not closed and cannot bound a shape",
                )
            })
            .collect();
        if !open.is_empty() {
            diags.extend(open);
            return Err(fail(diags));
        }
        Err(CamError::Unavailable(
            "engrave needs the exact medial axis of lines and arcs (CH-17), which this build \
             does not have"
                .into(),
        ))
    }
}
