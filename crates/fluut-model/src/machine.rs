// SPDX-License-Identifier: MIT OR Apache-2.0

//! Machine definitions (spec 02 §2.3): layered like posts, then read into
//! a typed [`Machine`].

use crate::defs::{DefError, DefRef, DefStore, LayerId, NoModes, Resolved, resolve};
use crate::validate::MachineLimits;
use serde::Deserialize;
use toml::Table;

/// Format id of a machine file.
pub const MACHINE_FORMAT: &str = "fluut.machine/1";

/// `(id, file text)` of every built-in machine.
pub const BUILTIN_MACHINES: [(&str, &str); 2] = [
    (
        "fluut/generic-3axis-router",
        include_str!("../machines/fluut/generic-3axis-router.machine.toml"),
    ),
    (
        "fluut/generic-4axis-router",
        include_str!("../machines/fluut/generic-4axis-router.machine.toml"),
    ),
];

#[derive(Clone, PartialEq, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Travel {
    pub x: Option<[f64; 2]>,
    pub y: Option<[f64; 2]>,
    pub z: Option<[f64; 2]>,
}

#[derive(Clone, PartialEq, Debug, Deserialize, Default)]
pub struct Spindle {
    pub rpm: Option<[f64; 2]>,
    /// Gear ranges, each `[min, max]`.
    #[serde(default)]
    pub ranges: Vec<[f64; 2]>,
}

#[derive(Clone, PartialEq, Debug, Deserialize, Default)]
pub struct FeedLimits {
    /// mm/min.
    pub max_cut: Option<f64>,
    pub max_rapid: Option<f64>,
}

/// Positive sense of a rotary axis (spec 03 §3.9.3).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    RightHand,
    LeftHand,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct RotaryAxis {
    pub name: String,
    /// The linear axis it turns about: `X` or `Y`.
    pub parallel_to: String,
    pub centre: [f64; 3],
    #[serde(default)]
    pub continuous: bool,
    #[serde(default)]
    pub direction: Direction,
    pub clamp: Option<String>,
    pub unclamp: Option<String>,
    /// `[min, max]` in degrees; empty when unlimited.
    #[serde(default)]
    pub limits_deg: Vec<f64>,
}

#[derive(Clone, PartialEq, Debug, Deserialize, Default)]
pub struct Axes {
    #[serde(default)]
    pub rotary: Vec<RotaryAxis>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChangerKind {
    None,
    #[default]
    Manual,
    Atc,
}

impl ChangerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangerKind::None => "none",
            ChangerKind::Manual => "manual",
            ChangerKind::Atc => "atc",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Deserialize, Default)]
pub struct ToolChanger {
    #[serde(default)]
    pub kind: ChangerKind,
    pub slots: Option<u32>,
}

/// The fields of a resolved machine definition.
#[derive(Clone, PartialEq, Debug, Deserialize, Default)]
pub struct MachineFields {
    #[serde(default)]
    pub name: String,
    /// Post reference, e.g. `user/my-router@3`.
    pub post: Option<String>,
    #[serde(default)]
    pub travel: Travel,
    #[serde(default)]
    pub spindle: Spindle,
    #[serde(default)]
    pub feed: FeedLimits,
    #[serde(default)]
    pub axis: Axes,
    #[serde(default)]
    pub tool_changer: ToolChanger,
    /// Dotted post paths and values (spec 05 §5.2.3).
    #[serde(default)]
    pub post_overrides: Table,
}

/// A resolved machine with its layers.
#[derive(Clone, PartialEq, Debug)]
pub struct Machine {
    pub id: String,
    pub fields: MachineFields,
    pub resolved: Resolved,
    /// Unknown keys and mixin conflicts.
    pub warnings: Vec<String>,
}

impl Machine {
    /// Resolves `r` through `store` and reads the result.
    pub fn resolve(store: &dyn DefStore, r: &DefRef) -> Result<Machine, DefError> {
        let resolved = resolve(store, r, &NoModes)?;
        let mut warnings: Vec<String> =
            resolved.warnings.iter().map(|w| format!("{w:?}")).collect();
        let fields: MachineFields =
            serde_ignored::deserialize(toml::Value::Table(resolved.table.clone()), |p| {
                warnings.push(format!("unknown key `{p}` ignored"))
            })
            .map_err(|e| DefError::Merge {
                layer: resolved
                    .layers
                    .last()
                    .cloned()
                    .unwrap_or(LayerId(r.id.clone())),
                path: String::new(),
                msg: e.to_string(),
            })?;
        Ok(Machine {
            id: r.id.clone(),
            fields,
            resolved,
            warnings,
        })
    }

    /// The layer that supplied this machine's own keys, `id@major.minor`.
    pub fn layer(&self) -> Option<&LayerId> {
        self.resolved.layers.last()
    }

    pub fn has_rotary(&self) -> bool {
        !self.fields.axis.rotary.is_empty()
    }

    /// Limits for the machine validation rules, when the file gives both
    /// the spindle range and the cutting feed.
    pub fn limits(&self) -> Option<MachineLimits> {
        let [lo, hi] = self.fields.spindle.rpm?;
        Some(MachineLimits {
            rpm: (lo, hi),
            max_cut_feed: self.fields.feed.max_cut?,
        })
    }
}
