// SPDX-License-Identifier: MIT OR Apache-2.0

//! What to post: the emitter's input (spec 05 §5.6). `fluut-core` fills it
//! from the project; tests build it directly.

use fluut_model::Resolved;
use fluut_path::{FeedRef, ToolpathResult};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, PartialEq, Debug, Default)]
pub struct PostJob {
    pub project_name: String,
    /// Shown by `[FILE_NOTES]`.
    pub notes: String,
    /// `[DATE]`, `[TIME]`: supplied by the caller so output is reproducible.
    pub date: String,
    pub time: String,
    pub provenance: Provenance,
    pub machine: MachineInfo,
    pub setups: Vec<SetupJob>,
}

/// Recorded in the output's first comment block (spec 05 §5.9).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Provenance {
    pub post_id: String,
    pub layers: Vec<String>,
    /// `sha256:<hex>` of the resolved post.
    pub hash: String,
}

impl Provenance {
    pub fn of(resolved: &Resolved, post_id: &str) -> Provenance {
        let text = toml::to_string(&resolved.table).expect("a TOML table always serialises");
        let hash: String = Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Provenance {
            post_id: post_id.to_owned(),
            layers: resolved.layers.iter().map(|l| l.0.clone()).collect(),
            hash: format!("sha256:{hash}"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct MachineInfo {
    pub name: String,
    /// `none`, `manual` or `atc`.
    pub tool_changer: String,
    pub has_rotary: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum SetupKind {
    #[default]
    ThreeAxis,
    Rotary,
    Indexed,
}

impl SetupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SetupKind::ThreeAxis => "three_axis",
            SetupKind::Rotary => "rotary",
            SetupKind::Indexed => "indexed",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct SetupJob {
    pub name: String,
    pub kind: SetupKind,
    pub work_offset: String,
    /// Index angles in degrees, for indexed Setups.
    pub index_a: Option<f64>,
    pub index_b: Option<f64>,
    pub toolpaths: Vec<ToolpathJob>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToolInfo {
    pub number: u32,
    pub name: String,
    pub diameter: f64,
    pub corner_radius: f64,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Coolant {
    #[default]
    Off,
    Flood,
    Mist,
}

impl Coolant {
    pub fn as_str(self) -> &'static str {
        match self {
            Coolant::Off => "off",
            Coolant::Flood => "flood",
            Coolant::Mist => "mist",
        }
    }
}

/// Feeds in mm/min for each symbolic feed (D08 §9).
#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Feeds {
    pub cut: f64,
    pub plunge: f64,
    pub ramp: f64,
    pub lead_in: f64,
    pub lead_out: f64,
    pub transition: f64,
}

impl Feeds {
    /// `None` for rapid moves.
    pub fn get(&self, f: FeedRef) -> Option<f64> {
        match f {
            FeedRef::Rapid => None,
            FeedRef::Cut => Some(self.cut),
            FeedRef::Plunge => Some(self.plunge),
            FeedRef::Ramp => Some(self.ramp),
            FeedRef::LeadIn => Some(self.lead_in),
            FeedRef::LeadOut => Some(self.lead_out),
            FeedRef::Transition => Some(self.transition),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ToolpathJob {
    pub name: String,
    /// Toolpath type id (spec 04 §4.13), e.g. `parallel`.
    pub kind: String,
    pub tool: ToolInfo,
    pub spindle_rpm: f64,
    pub coolant: Coolant,
    pub feeds: Feeds,
    pub safe_z: f64,
    pub result: Arc<ToolpathResult>,
}
