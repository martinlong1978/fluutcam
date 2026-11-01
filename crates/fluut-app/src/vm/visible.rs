// SPDX-License-Identifier: MIT OR Apache-2.0

//! `visible_if` (D15 §6): which fields the inspector shows, from the
//! "Depends on" columns of D15 §2.6, §3.2 and §4.1.
//!
//! A hidden field keeps its value; it is just not shown. The rules live
//! here until `FieldMeta` carries them (after M4; see the M5 plan).

use fluut_model::ToolpathTypeId;

#[derive(Copy, Clone, Debug)]
enum Cond {
    Is(&'static str, &'static str),
    IsNot(&'static str, &'static str),
    Positive(&'static str),
}

/// `(type, field path, condition)`; `None` applies to every type.
const RULES: &[(Option<ToolpathTypeId>, &str, Cond)] = {
    use Cond::*;
    use ToolpathTypeId::*;
    &[
        // Linking (§2.6)
        (
            None,
            "linking.safety_distance",
            Is("linking.retract", "minimum"),
        ),
        (
            None,
            "linking.transitions",
            IsNot("linking.retract", "full"),
        ),
        (
            None,
            "linking.lead_in.radius",
            Is("linking.lead_in.kind", "arc"),
        ),
        (
            None,
            "linking.lead_in.length",
            IsNot("linking.lead_in.kind", "none"),
        ),
        (
            None,
            "linking.lead_out.radius",
            Is("linking.lead_out.kind", "arc"),
        ),
        (
            None,
            "linking.lead_out.length",
            IsNot("linking.lead_out.kind", "none"),
        ),
        (
            None,
            "linking.ramp.angle_deg",
            IsNot("linking.ramp.kind", "plunge"),
        ),
        (
            None,
            "linking.ramp.radius",
            Is("linking.ramp.kind", "helix"),
        ),
        // contour_2d (§3.2)
        (
            Some(Contour2d),
            "passes.direction",
            IsNot("passes.side", "on"),
        ),
        (
            Some(Contour2d),
            "passes.finishing_stepover",
            Positive("passes.finishing_pass_count"),
        ),
        (
            Some(Contour2d),
            "passes.finish_every_level",
            Positive("passes.finishing_pass_count"),
        ),
        (
            Some(Contour2d),
            "passes.tabs.count",
            Is("passes.tabs.mode", "count"),
        ),
        (
            Some(Contour2d),
            "passes.tabs.distance",
            Is("passes.tabs.mode", "distance"),
        ),
        (
            Some(Contour2d),
            "passes.tabs.points",
            Is("passes.tabs.mode", "points"),
        ),
        (
            Some(Contour2d),
            "passes.tabs.width",
            IsNot("passes.tabs.mode", "off"),
        ),
        (
            Some(Contour2d),
            "passes.tabs.height",
            IsNot("passes.tabs.mode", "off"),
        ),
        (
            Some(Contour2d),
            "passes.tabs.ramp_angle_deg",
            IsNot("passes.tabs.mode", "off"),
        ),
        // pocket_2d (§4.1)
        (
            Some(Pocket2d),
            "passes.offset_order",
            Is("passes.pattern", "offset"),
        ),
        (
            Some(Pocket2d),
            "passes.pass_angle_deg",
            Is("passes.pattern", "raster"),
        ),
    ]
};

/// Whether field `path` of a `kind` toolpath is shown. `value(path)` gives
/// another field's effective value as text (local value or constant
/// default), or `None` when it has none; an unknown value shows the field.
pub fn visible(kind: ToolpathTypeId, path: &str, value: impl Fn(&str) -> Option<String>) -> bool {
    RULES
        .iter()
        .filter(|(t, p, _)| *p == path && t.is_none_or(|t| t == kind))
        .all(|(_, _, c)| match *c {
            Cond::Is(f, v) => value(f).is_none_or(|x| x == v),
            Cond::IsNot(f, v) => value(f).is_none_or(|x| x != v),
            Cond::Positive(f) => value(f)
                .and_then(|x| x.parse::<f64>().ok())
                .is_none_or(|x| x > 0.0),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluut_model::Params;

    #[test]
    fn every_rule_names_real_fields() {
        for (t, path, c) in RULES {
            let kinds: Vec<ToolpathTypeId> = match t {
                Some(t) => vec![*t],
                None => ToolpathTypeId::ALL.to_vec(),
            };
            let dep = match *c {
                Cond::Is(f, _) | Cond::IsNot(f, _) | Cond::Positive(f) => f,
            };
            // Each rule applies to at least one type that has both fields.
            assert!(
                kinds
                    .iter()
                    .any(|k| Params::meta(*k, path).is_some() && Params::meta(*k, dep).is_some()),
                "{path} / {dep}"
            );
        }
    }

    #[test]
    fn pattern_switches_fields() {
        let raster = |p: &str| (p == "passes.pattern").then(|| "raster".to_owned());
        let k = ToolpathTypeId::Pocket2d;
        assert!(visible(k, "passes.pass_angle_deg", raster));
        assert!(!visible(k, "passes.offset_order", raster));
        // `parallel` has no such rule.
        assert!(visible(
            ToolpathTypeId::Parallel,
            "passes.pass_angle_deg",
            |_| Some("offset".into())
        ));
        assert!(visible(k, "passes.stepover", raster));
    }

    #[test]
    fn counts_and_unknown_values() {
        let k = ToolpathTypeId::Contour2d;
        assert!(!visible(k, "passes.finishing_stepover", |_| Some(
            "0".into()
        )));
        assert!(visible(k, "passes.finishing_stepover", |_| Some(
            "1".into()
        )));
        assert!(visible(k, "passes.finishing_stepover", |_| None));
    }
}
