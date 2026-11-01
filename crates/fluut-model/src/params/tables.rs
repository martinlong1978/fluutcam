// SPDX-License-Identifier: MIT OR Apache-2.0

//! Field tables of D15 §2–§5. Each type lists the common fields it uses, in
//! the canonical order the writer uses (it matches the D15 examples).

use super::meta::{Class, ConstVal, DefaultRule, FieldKind, FieldMeta, Group, ToolpathTypeId};

const fn f(
    key: &'static str,
    group: Group,
    kind: FieldKind,
    default: DefaultRule,
    class: Class,
    expr: bool,
) -> FieldMeta {
    FieldMeta {
        key,
        group,
        kind,
        default,
        class,
        expr,
    }
}

const fn num(v: f64) -> DefaultRule {
    DefaultRule::Const(ConstVal::Num(v))
}
const fn text(v: &'static str) -> DefaultRule {
    DefaultRule::Const(ConstVal::Str(v))
}
const fn flag(v: bool) -> DefaultRule {
    DefaultRule::Const(ConstVal::Bool(v))
}
const fn height(r: &'static str, offset: f64) -> DefaultRule {
    DefaultRule::Const(ConstVal::Height(r, offset))
}

use Class::{P, R};
use DefaultRule::{Computed, FromTool, None as NoDefault};
use FieldKind::*;
use Group::*;

/// Height references a user may choose (D15 §2.4).
pub const HEIGHT_REFS: &[&str] = &[
    "setup_clearance",
    "stock_top",
    "stock_bottom",
    "model_top",
    "model_bottom",
    "work_zero",
    "selection",
];

// ── Tool group (§2.2): all class P ──────────────────────────────────

const FEED: &[FieldMeta] = &[
    f("cut", ToolParams, Feed, FromTool("feed"), P, true),
    f("plunge", ToolParams, Feed, FromTool("plunge"), P, true),
    f("ramp", ToolParams, Feed, Computed("feed.cut"), P, true),
    f("lead_in", ToolParams, Feed, Computed("feed.cut"), P, true),
    f(
        "lead_out",
        ToolParams,
        Feed,
        Computed("feed.lead_in"),
        P,
        true,
    ),
    f(
        "transition",
        ToolParams,
        Feed,
        Computed("feed.cut"),
        P,
        true,
    ),
];

const SPINDLE_RPM: FieldMeta = f("spindle_rpm", ToolParams, Speed, FromTool("rpm"), P, true);
const FEEDS: FieldMeta = f("feed", ToolParams, Table(FEED), NoDefault, P, false);
const COOLANT: FieldMeta = f(
    "coolant",
    ToolParams,
    Enum(&["off", "flood", "mist"]),
    text("off"),
    P,
    false,
);

// ── Geometry group (§2.3): all class R ──────────────────────────────

const DRIVE_FACES: FieldMeta = f("drive_faces", Geometry, Selection, NoDefault, R, false);
const DRIVE_CURVES: FieldMeta = f("drive_curves", Geometry, Selection, NoDefault, R, false);
const CHECK_FACES: FieldMeta = f(
    "check_faces",
    Geometry,
    Selection,
    Computed("faces not in drive_faces or excluded_faces"),
    R,
    false,
);
const EXCLUDED_FACES: FieldMeta = f("excluded_faces", Geometry, Selection, NoDefault, R, false);
const BOUNDARY_FIELDS: &[FieldMeta] = &[
    f("curves", Geometry, Selection, NoDefault, R, false),
    f(
        "side",
        Geometry,
        Enum(&["inside", "outside"]),
        text("inside"),
        R,
        false,
    ),
    f(
        "tool_containment",
        Geometry,
        Enum(&["inside", "centre", "outside"]),
        text("inside"),
        R,
        false,
    ),
];
const BOUNDARY: FieldMeta = f(
    "boundary",
    Geometry,
    Table(BOUNDARY_FIELDS),
    NoDefault,
    R,
    false,
);
const REST_FROM: FieldMeta = f(
    "rest_from",
    Geometry,
    Enum(&["off", "previous"]),
    text("off"),
    R,
    false,
);

// ── Heights group (§2.4): all class R ───────────────────────────────

const CLEARANCE: FieldMeta = f(
    "clearance",
    Heights,
    Height,
    height("setup_clearance", 0.0),
    R,
    false,
);
const RETRACT_H: FieldMeta = f(
    "retract",
    Heights,
    Height,
    height("stock_top", 5.0),
    R,
    false,
);
const FEED_H: FieldMeta = f("feed", Heights, Height, height("top", 2.0), R, false);
const fn top(r: &'static str) -> FieldMeta {
    f("top", Heights, Height, height(r, 0.0), R, false)
}
const fn bottom(r: &'static str) -> FieldMeta {
    f("bottom", Heights, Height, height(r, 0.0), R, false)
}

// ── Passes group, common fields (§2.5): all class R ─────────────────

const MACHINING_TOLERANCE: FieldMeta = f("machining_tolerance", Passes, Length, num(0.01), R, true);
const MAX_SEGMENT_LENGTH: FieldMeta = f("max_segment_length", Passes, Length, NoDefault, R, true);
const OUTPUT_ARCS: FieldMeta = f("output_arcs", Passes, Bool, flag(true), R, false);
const PASS_LAYOUT: FieldMeta = f(
    "pass_layout",
    Passes,
    Enum(&["even", "fixed_from_edge"]),
    text("even"),
    R,
    false,
);
const STOCK_TO_LEAVE: FieldMeta = f("stock_to_leave", Passes, Length, num(0.0), R, true);
const STOCK_TO_LEAVE_AXIAL: FieldMeta = f(
    "stock_to_leave_axial",
    Passes,
    Length,
    Computed("stock_to_leave"),
    R,
    true,
);
const CLIMB_CONVENTIONAL: &[&str] = &["climb", "conventional"];

// ── Linking group (§2.6): all class R ───────────────────────────────

const RETRACT: FieldMeta = f(
    "retract",
    Linking,
    Enum(&["full", "minimum", "stay_down"]),
    text("minimum"),
    R,
    false,
);
const SAFETY_DISTANCE: FieldMeta = f("safety_distance", Linking, Length, num(1.0), R, true);
const TRANSITIONS: FieldMeta = f(
    "transitions",
    Linking,
    Enum(&["direct", "smooth"]),
    text("direct"),
    R,
    false,
);
const LEAD_KINDS: &[&str] = &["none", "arc", "linear"];
const LEAD_ARC: &[FieldMeta] = &[
    f("kind", Linking, Enum(LEAD_KINDS), text("arc"), R, false),
    f(
        "radius",
        Linking,
        Length,
        Computed("0.5 * tool.diameter"),
        R,
        true,
    ),
    f(
        "length",
        Linking,
        Length,
        Computed("0 (arc) or tool.radius (linear)"),
        R,
        true,
    ),
];
const LEAD_NONE: &[FieldMeta] = &[
    f("kind", Linking, Enum(LEAD_KINDS), text("none"), R, false),
    LEAD_ARC[1],
    LEAD_ARC[2],
];
const fn lead_in(fields: &'static [FieldMeta]) -> FieldMeta {
    f("lead_in", Linking, Table(fields), NoDefault, R, false)
}
/// `lead_out` absent copies `lead_in` (D15 §2.6).
const fn lead_out(fields: &'static [FieldMeta]) -> FieldMeta {
    f(
        "lead_out",
        Linking,
        Table(fields),
        Computed("lead_in"),
        R,
        false,
    )
}
const fn ramp_fields(kinds: &'static [&'static str], default: &'static str) -> [FieldMeta; 3] {
    [
        f("kind", Linking, Enum(kinds), text(default), R, false),
        f("angle_deg", Linking, Angle, num(3.0), R, true),
        f(
            "radius",
            Linking,
            Length,
            Computed("0.5 * tool.diameter"),
            R,
            true,
        ),
    ]
}
const CONTOUR_RAMP: &[FieldMeta] = &ramp_fields(&["plunge", "profile"], "plunge");
const POCKET_RAMP: &[FieldMeta] = &ramp_fields(&["plunge", "zigzag", "helix", "profile"], "helix");
const ORDER: FieldMeta = f(
    "order",
    Linking,
    Enum(&["by_level", "by_area"]),
    text("by_level"),
    R,
    false,
);

// ── contour_2d (§3) ──────────────────────────────────────────────────

const TABS: &[FieldMeta] = &[
    f(
        "mode",
        Passes,
        Enum(&["off", "count", "distance", "points"]),
        text("off"),
        R,
        false,
    ),
    f("count", Passes, Count, num(4.0), R, false),
    f("distance", Passes, Length, num(100.0), R, true),
    f("points", Passes, Points, NoDefault, R, false),
    f("width", Passes, Length, Computed("tool.diameter"), R, true),
    f("height", Passes, Length, num(1.0), R, true),
    f("ramp_angle_deg", Passes, Angle, num(45.0), R, true),
];

pub(crate) const CONTOUR_2D: &[FieldMeta] = &[
    SPINDLE_RPM,
    FEEDS,
    COOLANT,
    DRIVE_FACES,
    DRIVE_CURVES,
    EXCLUDED_FACES,
    CLEARANCE,
    RETRACT_H,
    FEED_H,
    top("stock_top"),
    bottom("selection"),
    MACHINING_TOLERANCE,
    MAX_SEGMENT_LENGTH,
    OUTPUT_ARCS,
    f(
        "side",
        Passes,
        Enum(&["inside", "outside", "left", "right", "on"]),
        Computed("outside (closed chains) or right (open chains)"),
        R,
        false,
    ),
    f(
        "direction",
        Passes,
        Enum(CLIMB_CONVENTIONAL),
        text("climb"),
        R,
        false,
    ),
    f(
        "compensation",
        Passes,
        Enum(&["computer", "control", "wear"]),
        text("computer"),
        R,
        false,
    ),
    f("step_down", Passes, Length, FromTool("step_down"), R, true),
    f("depth_count", Passes, Count, NoDefault, R, false),
    f("finishing_pass_count", Passes, Count, num(0.0), R, false),
    f(
        "finishing_stepover",
        Passes,
        Length,
        Computed("0.1 * tool.diameter"),
        R,
        true,
    ),
    f("finish_every_level", Passes, Bool, flag(false), R, false),
    f("spring_pass_count", Passes, Count, num(0.0), R, false),
    STOCK_TO_LEAVE,
    STOCK_TO_LEAVE_AXIAL,
    f("start_point", Passes, Point, NoDefault, R, false),
    f("tabs", Passes, Table(TABS), NoDefault, R, false),
    RETRACT,
    SAFETY_DISTANCE,
    TRANSITIONS,
    lead_in(LEAD_ARC),
    lead_out(LEAD_ARC),
    f("ramp", Linking, Table(CONTOUR_RAMP), NoDefault, R, false),
    ORDER,
];

// ── pocket_2d (§4) ───────────────────────────────────────────────────

pub(crate) const POCKET_2D: &[FieldMeta] = &[
    SPINDLE_RPM,
    FEEDS,
    COOLANT,
    DRIVE_FACES,
    DRIVE_CURVES,
    EXCLUDED_FACES,
    BOUNDARY,
    REST_FROM,
    CLEARANCE,
    RETRACT_H,
    FEED_H,
    top("stock_top"),
    bottom("selection"),
    MACHINING_TOLERANCE,
    MAX_SEGMENT_LENGTH,
    OUTPUT_ARCS,
    f(
        "pattern",
        Passes,
        Enum(&["offset", "raster"]),
        text("offset"),
        R,
        false,
    ),
    f("stepover", Passes, Length, FromTool("stepover"), R, true),
    f("step_down", Passes, Length, FromTool("step_down"), R, true),
    f(
        "direction",
        Passes,
        Enum(CLIMB_CONVENTIONAL),
        text("climb"),
        R,
        false,
    ),
    f(
        "offset_order",
        Passes,
        Enum(&["inside_out", "outside_in"]),
        text("inside_out"),
        R,
        false,
    ),
    f("pass_angle_deg", Passes, Angle, num(0.0), R, true),
    PASS_LAYOUT,
    STOCK_TO_LEAVE,
    STOCK_TO_LEAVE_AXIAL,
    RETRACT,
    SAFETY_DISTANCE,
    TRANSITIONS,
    lead_in(LEAD_NONE),
    lead_out(LEAD_NONE),
    f("ramp", Linking, Table(POCKET_RAMP), NoDefault, R, false),
    ORDER,
];

// ── parallel (§5) ────────────────────────────────────────────────────

pub(crate) const PARALLEL: &[FieldMeta] = &[
    SPINDLE_RPM,
    FEEDS,
    COOLANT,
    f(
        "drive_faces",
        Geometry,
        Selection,
        Computed("all faces of the Setup's bodies"),
        R,
        false,
    ),
    CHECK_FACES,
    EXCLUDED_FACES,
    BOUNDARY,
    CLEARANCE,
    RETRACT_H,
    FEED_H,
    top("model_top"),
    bottom("model_bottom"),
    MACHINING_TOLERANCE,
    MAX_SEGMENT_LENGTH,
    OUTPUT_ARCS,
    f("stepover", Passes, Length, FromTool("stepover"), R, true),
    f("scallop_height", Passes, Length, NoDefault, R, true),
    f("pass_angle_deg", Passes, Angle, num(0.0), R, true),
    f(
        "direction",
        Passes,
        Enum(&["zigzag", "climb", "conventional"]),
        text("zigzag"),
        R,
        false,
    ),
    PASS_LAYOUT,
    f("min_slope_deg", Passes, Angle, num(0.0), R, true),
    f("max_slope_deg", Passes, Angle, num(90.0), R, true),
    STOCK_TO_LEAVE,
    RETRACT,
    SAFETY_DISTANCE,
    TRANSITIONS,
    lead_in(LEAD_NONE),
    lead_out(LEAD_NONE),
];

// ── face (D15b §2.1) ─────────────────────────────────────────────────

const ZIGZAG_CLIMB_CONVENTIONAL: &[&str] = &["zigzag", "climb", "conventional"];

pub(crate) const FACE: &[FieldMeta] = &[
    SPINDLE_RPM,
    FEEDS,
    COOLANT,
    DRIVE_FACES,
    BOUNDARY,
    CLEARANCE,
    RETRACT_H,
    FEED_H,
    top("stock_top"),
    bottom("model_top"),
    MACHINING_TOLERANCE,
    MAX_SEGMENT_LENGTH,
    OUTPUT_ARCS,
    f("stepover", Passes, Length, FromTool("stepover"), R, true),
    f(
        "direction",
        Passes,
        Enum(ZIGZAG_CLIMB_CONVENTIONAL),
        text("zigzag"),
        R,
        false,
    ),
    f("pass_angle_deg", Passes, Angle, num(0.0), R, true),
    f("step_down", Passes, Length, FromTool("step_down"), R, true),
    f("final_step_down", Passes, Length, NoDefault, R, true),
    f(
        "overlap",
        Passes,
        Length,
        Computed("0.6 * tool.diameter"),
        R,
        true,
    ),
    PASS_LAYOUT,
    f("stock_to_leave_axial", Passes, Length, num(0.0), R, true),
    f(
        "retract",
        Linking,
        Enum(&["full", "minimum", "stay_down"]),
        text("stay_down"),
        R,
        false,
    ),
    SAFETY_DISTANCE,
    TRANSITIONS,
    lead_in(LEAD_NONE),
    lead_out(LEAD_NONE),
];

// ── trace (D15b §2.3) ────────────────────────────────────────────────

pub(crate) const TRACE: &[FieldMeta] = &[
    SPINDLE_RPM,
    FEEDS,
    COOLANT,
    DRIVE_CURVES,
    CLEARANCE,
    RETRACT_H,
    FEED_H,
    top("stock_top"),
    bottom("selection"),
    MACHINING_TOLERANCE,
    MAX_SEGMENT_LENGTH,
    OUTPUT_ARCS,
    f(
        "depth_mode",
        Passes,
        Enum(&["heights", "curve"]),
        text("heights"),
        R,
        false,
    ),
    f("curve_depth", Passes, Length, num(0.0), R, true),
    f("side_offset", Passes, Length, num(0.0), R, true),
    f("step_down", Passes, Length, FromTool("step_down"), R, true),
    f("depth_count", Passes, Count, NoDefault, R, false),
    f(
        "direction",
        Passes,
        Enum(&["any", "selection", "reversed"]),
        text("any"),
        R,
        false,
    ),
    RETRACT,
    SAFETY_DISTANCE,
    TRANSITIONS,
    lead_in(LEAD_NONE),
    lead_out(LEAD_NONE),
    f("ramp", Linking, Table(CONTOUR_RAMP), NoDefault, R, false),
    ORDER,
];

// ── chamfer_2d (D15b §2.5) ───────────────────────────────────────────

pub(crate) const CHAMFER_2D: &[FieldMeta] = &[
    SPINDLE_RPM,
    FEEDS,
    COOLANT,
    DRIVE_FACES,
    DRIVE_CURVES,
    CLEARANCE,
    RETRACT_H,
    FEED_H,
    top("selection"),
    MACHINING_TOLERANCE,
    MAX_SEGMENT_LENGTH,
    OUTPUT_ARCS,
    f("chamfer_width", Passes, Length, num(1.0), R, true),
    f("tip_offset", Passes, Length, num(0.5), R, true),
    f(
        "side",
        Passes,
        Enum(&["inside", "outside", "left", "right"]),
        Computed("outside (closed chains) or right (open chains)"),
        R,
        false,
    ),
    f(
        "direction",
        Passes,
        Enum(CLIMB_CONVENTIONAL),
        text("climb"),
        R,
        false,
    ),
    RETRACT,
    SAFETY_DISTANCE,
    TRANSITIONS,
    lead_in(LEAD_ARC),
    lead_out(LEAD_ARC),
    f("ramp", Linking, Table(CONTOUR_RAMP), NoDefault, R, false),
];

/// Whether `key` is a field of `group` for some type. Used to tell an
/// undeclared common field (an error, spec 03 §3.10.3) from an unknown key.
pub fn known_in_any_type(group: Group, key: &str) -> bool {
    ToolpathTypeId::ALL
        .iter()
        .any(|t| t.group_fields(group).any(|m| m.key == key))
}
