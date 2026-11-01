// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical inputs of the result and post hashes (D01 §4, D15 §1.8).
//!
//! The `ToolpathResult` node hashes its R inputs only: R fields, the tool
//! geometry, the R values of the tool preset and the Setup. P fields (feeds,
//! speed, names) go into the `PostOutput` node, so editing them keeps the
//! result.

use crate::params::Class;
use crate::project::{Project, Toolpath, write};
use crate::setup::ResolvedSetup;
use crate::tools::Tool;
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// `sha256:<hex>` of `text`.
pub fn sha256_hex(text: &[u8]) -> String {
    let mut s = String::from("sha256:");
    for b in Sha256::digest(text) {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn opt(v: Option<f64>) -> String {
    v.map_or("-".into(), |x| format!("{x:?}"))
}

/// The tool fields that change a toolpath result: geometry and the R
/// values of the preset for `material`.
pub fn tool_r_text(tool: Option<&Tool>, material: Option<&str>) -> String {
    let Some(t) = tool else {
        return "tool = missing\n".into();
    };
    let p = t.preset(material);
    format!(
        "tool.shape = {:?}\ntool.diameter = {:?}\ntool.corner_radius = {:?}\n\
         tool.flute_length = {}\ntool.centre_cutting = {}\n\
         preset.stepover = {}\npreset.step_down = {}\n",
        t.shape,
        t.diameter,
        t.corner_radius(),
        opt(t.flute_length),
        t.centre_cutting,
        opt(p.and_then(|p| p.stepover)),
        opt(p.and_then(|p| p.step_down)),
    )
}

/// The tool fields used only at post time.
pub fn tool_p_text(tool: Option<&Tool>, material: Option<&str>) -> String {
    let Some(t) = tool else {
        return "tool = missing\n".into();
    };
    let p = t.preset(material);
    format!(
        "tool.number = {}\ntool.name = {:?}\npreset.rpm = {}\npreset.feed = {}\npreset.plunge = {}\n",
        t.number,
        t.name,
        opt(p.and_then(|p| p.rpm)),
        opt(p.and_then(|p| p.feed)),
        opt(p.and_then(|p| p.plunge)),
    )
}

/// R inputs of a toolpath result, apart from the Setup and geometry.
pub fn toolpath_r_text(tp: &Toolpath, tool: Option<&Tool>, material: Option<&str>) -> String {
    format!(
        "type = {}\ntool = {}\n{}{}",
        tp.kind(),
        tp.tool_ref(),
        tool_r_text(tool, material),
        tp.params.class_text(Class::R)
    )
}

/// P inputs of a toolpath's post output.
pub fn toolpath_p_text(tp: &Toolpath, tool: Option<&Tool>, material: Option<&str>) -> String {
    format!(
        "name = {:?}\nenabled = {}\n{}{}",
        tp.name,
        tp.is_enabled(),
        tool_p_text(tool, material),
        tp.params.class_text(Class::P)
    )
}

/// Every resolved Setup field (all class R), without its toolpaths. The
/// resolved form makes a parent edit change every child's hash.
pub fn setup_text(s: &ResolvedSetup) -> String {
    let mut s = s.to_setup();
    // The name is display only (class N).
    s.name.clear();
    let mut p = Project::new("");
    p.setups.push(std::sync::Arc::new(s));
    write(&p)
}
