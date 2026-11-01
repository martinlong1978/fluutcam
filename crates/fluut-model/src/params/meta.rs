// SPDX-License-Identifier: MIT OR Apache-2.0

//! Field metadata (D06 §3.3, D15 §1, §6).

use std::fmt;

/// The five parameter groups of spec 03 §3.10.2, in file order.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Group {
    ToolParams,
    Geometry,
    Heights,
    Passes,
    Linking,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::ToolParams,
        Group::Geometry,
        Group::Heights,
        Group::Passes,
        Group::Linking,
    ];

    /// TOML key of the group table.
    pub fn key(self) -> &'static str {
        match self {
            Group::ToolParams => "tool_params",
            Group::Geometry => "geometry",
            Group::Heights => "heights",
            Group::Passes => "passes",
            Group::Linking => "linking",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_key(k: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.key() == k)
    }
}

/// Invalidation class (D15 §1.8).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Class {
    /// Changes the toolpath result.
    R,
    /// Changes the post output only.
    P,
    /// Changes nothing computed.
    N,
}

/// Value kind of a field (D15 §1.3).
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum FieldKind {
    /// mm.
    Length,
    /// mm/min.
    Feed,
    /// rev/min.
    Speed,
    /// Degrees in the file.
    Angle,
    Count,
    Bool,
    /// One of the listed snake_case strings.
    Enum(&'static [&'static str]),
    /// `{ ref, offset }` (D15 §2.4).
    Height,
    /// Array of `{ body | face | edge = id }` (D15 §2.3.1).
    Selection,
    /// `[x, y, z]` in the model frame, mm.
    Point,
    /// Array of points.
    Points,
    /// Inline table with its own fields.
    Table(&'static [FieldMeta]),
}

/// A constant default.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ConstVal {
    Num(f64),
    Bool(bool),
    Str(&'static str),
    /// `{ ref, offset }`.
    Height(&'static str, f64),
}

/// How a field gets its value when the key is absent (D15 §1.5).
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum DefaultRule {
    /// No value: the field is unset (e.g. `max_segment_length`).
    None,
    Const(ConstVal),
    /// The tool's cutting-data preset for the Setup's material.
    FromTool(&'static str),
    /// A formula over other resolved values; the text is for display.
    Computed(&'static str),
}

/// One field of a parameter group.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct FieldMeta {
    pub key: &'static str,
    pub group: Group,
    pub kind: FieldKind,
    pub default: DefaultRule,
    pub class: Class,
    /// The inspector accepts an expression (D15 §1.6).
    pub expr: bool,
}

impl FieldMeta {
    /// Fluent ids of the label and help (D15 §1.7).
    pub fn label_id(&self) -> String {
        format!("param-{}-label", self.key.replace('.', "-"))
    }

    pub fn help_id(&self) -> String {
        format!("param-{}-help", self.key.replace('.', "-"))
    }

    pub fn sub_fields(&self) -> &'static [FieldMeta] {
        match self.kind {
            FieldKind::Table(f) => f,
            _ => &[],
        }
    }
}

/// Toolpath types with parameter definitions (D15 §3–§5).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ToolpathTypeId {
    Contour2d,
    Pocket2d,
    Parallel,
    // 3-D finishing (D15b §5).
    Contour,
    Horizontal,
    Spiral,
    Scallop,
    SteepAndShallow,
}

impl ToolpathTypeId {
    pub const ALL: [ToolpathTypeId; 8] = [
        ToolpathTypeId::Contour2d,
        ToolpathTypeId::Pocket2d,
        ToolpathTypeId::Parallel,
        ToolpathTypeId::Contour,
        ToolpathTypeId::Horizontal,
        ToolpathTypeId::Spiral,
        ToolpathTypeId::Scallop,
        ToolpathTypeId::SteepAndShallow,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ToolpathTypeId::Contour2d => "contour_2d",
            ToolpathTypeId::Pocket2d => "pocket_2d",
            ToolpathTypeId::Parallel => "parallel",
            ToolpathTypeId::Contour => "contour",
            ToolpathTypeId::Horizontal => "horizontal",
            ToolpathTypeId::Spiral => "spiral",
            ToolpathTypeId::Scallop => "scallop",
            ToolpathTypeId::SteepAndShallow => "steep_and_shallow",
        }
    }

    pub fn parse(s: &str) -> Option<ToolpathTypeId> {
        ToolpathTypeId::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// Every field of the type, group by group, in canonical order.
    pub fn fields(self) -> &'static [FieldMeta] {
        match self {
            ToolpathTypeId::Contour2d => super::tables::CONTOUR_2D,
            ToolpathTypeId::Pocket2d => super::tables::POCKET_2D,
            ToolpathTypeId::Parallel => super::tables::PARALLEL,
            ToolpathTypeId::Contour => super::tables::CONTOUR,
            ToolpathTypeId::Horizontal => super::tables::HORIZONTAL,
            ToolpathTypeId::Spiral => super::tables::SPIRAL,
            ToolpathTypeId::Scallop => super::tables::SCALLOP,
            ToolpathTypeId::SteepAndShallow => super::tables::STEEP_AND_SHALLOW,
        }
    }

    /// 3-D types that follow the surface (D15 §5, D15b §5): stock to leave
    /// is a normal offset, `top` may equal `bottom`, and there are no
    /// ramps.
    pub fn follows_surface(self) -> bool {
        matches!(
            self,
            ToolpathTypeId::Parallel
                | ToolpathTypeId::Contour
                | ToolpathTypeId::Horizontal
                | ToolpathTypeId::Spiral
                | ToolpathTypeId::Scallop
                | ToolpathTypeId::SteepAndShallow
        )
    }

    /// The fields of one group.
    pub fn group_fields(self, g: Group) -> impl Iterator<Item = &'static FieldMeta> + Clone {
        self.fields().iter().filter(move |f| f.group == g)
    }
}

impl fmt::Display for ToolpathTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
