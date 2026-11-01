// SPDX-License-Identifier: MIT OR Apache-2.0

//! `manual_nc` (D15b §7, D09 §6.1): one block in the program as a single
//! pass with a `Marker` segment. No tool, no geometry, no motion.

use crate::toolpath::{CamError, Generated, Role, StrategyInput, ToolpathType, tag};
use fluut_model::Severity;
use fluut_model::params::{PValue, ToolpathTypeId};
use fluut_model::project::{SetupKind, Toolpath};
use fluut_model::validate::{ValidateCx, validate};
use fluut_path::{MarkerKind, Pass, PassKind, Segment, ToolpathResult};

#[derive(Debug)]
pub struct ManualNc;

impl ToolpathType for ManualNc {
    fn id(&self) -> ToolpathTypeId {
        ToolpathTypeId::ManualNc
    }

    fn role(&self) -> Role {
        Role::Utility
    }

    fn allowed_setups(&self) -> &'static [SetupKind] {
        &[SetupKind::ThreeAxis, SetupKind::Rotary, SetupKind::Indexed]
    }

    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError> {
        manual_nc(input.toolpath, input.setup_kind)
    }
}

/// The result of a `manual_nc` toolpath; it needs no tool, so the session
/// calls this directly.
pub fn manual_nc(tp: &Toolpath, setup_kind: SetupKind) -> Result<Generated, CamError> {
    let cx = ValidateCx {
        setup_kind,
        ..ValidateCx::new(None)
    };
    let diags = validate(tp, &cx);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(CamError::Invalid(diags));
    }
    let text = match tp.params.get("passes.text") {
        Some(PValue::Enum(t)) => Some(t.clone()),
        _ => None,
    };
    let kind = match tp.params.get("passes.kind") {
        Some(PValue::Enum(s)) => s.as_str(),
        _ => "optional_stop",
    };
    let dwell = tp
        .params
        .get("passes.dwell_s")
        .and_then(PValue::num)
        .unwrap_or(1.0);
    let mut segments = Vec::new();
    // Text on a stop is a comment before it (D15b §7).
    let marker = |k| Segment::Marker { kind: k };
    match kind {
        "stop" | "optional_stop" => {
            if let Some(t) = text.filter(|t| !t.is_empty()) {
                segments.push(marker(MarkerKind::Comment(t)));
            }
            segments.push(marker(if kind == "stop" {
                MarkerKind::Stop
            } else {
                MarkerKind::OptionalStop
            }));
        }
        "comment" => segments.push(marker(MarkerKind::Comment(text.unwrap_or_default()))),
        "pass_through" => segments.push(marker(MarkerKind::PassThrough(text.unwrap_or_default()))),
        _ => segments.push(marker(MarkerKind::Dwell(dwell))),
    }
    let result =
        ToolpathResult::new(vec![Pass::new(PassKind::Cut, segments)]).map_err(CamError::Path)?;
    Ok(Generated {
        result,
        diagnostics: tag(diags, tp),
    })
}
