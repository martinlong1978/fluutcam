// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tool libraries (spec 02 §2.4).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

/// Format id of a tool library file.
pub const TOOLS_FORMAT: &str = "fluut.tools/1";

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct ToolLibrary {
    pub format: String,
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "tool", default)]
    pub tools: Vec<Tool>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolShape {
    Flat,
    Ball,
    Bull,
    V,
    Taper,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct Holder {
    pub diameter: f64,
    pub length: f64,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct Tool {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// ATC / T-number.
    #[serde(default)]
    pub number: u32,
    pub shape: ToolShape,
    pub diameter: f64,
    pub corner_radius: Option<f64>,
    pub flute_length: Option<f64>,
    #[serde(default = "yes")]
    pub centre_cutting: bool,
    pub overall_length: Option<f64>,
    pub shank_diameter: Option<f64>,
    pub holder: Option<Holder>,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(rename = "preset", default)]
    pub presets: Vec<Preset>,
}

fn yes() -> bool {
    true
}

/// Cutting data for one material.
#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct Preset {
    pub material: String,
    pub rpm: Option<f64>,
    pub feed: Option<f64>,
    pub plunge: Option<f64>,
    pub stepover: Option<f64>,
    pub step_down: Option<f64>,
}

impl Preset {
    /// The preset value named in a `FromTool` default (D15 §1.5).
    pub fn get(&self, key: &str) -> Option<f64> {
        match key {
            "rpm" => self.rpm,
            "feed" => self.feed,
            "plunge" => self.plunge,
            "stepover" => self.stepover,
            "step_down" => self.step_down,
            _ => None,
        }
    }
}

impl Tool {
    pub fn radius(&self) -> f64 {
        0.5 * self.diameter
    }

    /// Corner radius r of the cutter profile: D/2 for ball tools, 0 for flat
    /// and V tools, the given value for bull tools.
    pub fn corner_radius(&self) -> f64 {
        match self.shape {
            ToolShape::Ball => self.radius(),
            ToolShape::Flat | ToolShape::V => 0.0,
            ToolShape::Bull | ToolShape::Taper => self.corner_radius.unwrap_or(0.0),
        }
    }

    /// The preset for `material`, if the Setup has one and the tool lists it.
    pub fn preset(&self, material: Option<&str>) -> Option<&Preset> {
        let m = material?;
        self.presets.iter().find(|p| p.material == m)
    }
}

impl ToolLibrary {
    /// Parses a library file. Unknown keys are warnings (D06 §7.2).
    pub fn parse(src: &str) -> Result<(ToolLibrary, Vec<String>), String> {
        let de = toml::Deserializer::parse(src).map_err(|e| e.to_string())?;
        let mut warnings = Vec::new();
        let lib: ToolLibrary = serde_ignored::deserialize(de, |p| {
            warnings.push(format!("unknown key `{p}` ignored"));
        })
        .map_err(|e| e.to_string())?;
        if lib.format != TOOLS_FORMAT {
            return Err(format!(
                "`format` is `{}`, expected `{TOOLS_FORMAT}`",
                lib.format
            ));
        }
        for t in &lib.tools {
            if !(t.diameter.is_finite()) {
                return Err(format!("tool `{}`: diameter is not a number", t.id));
            }
        }
        Ok((lib, warnings))
    }

    pub fn tool(&self, id: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.id == id)
    }
}

/// `library#tool` (spec 02 §2.1).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ToolRef {
    pub library: String,
    pub tool: String,
}

impl ToolRef {
    pub fn parse(s: &str) -> Option<ToolRef> {
        let (l, t) = s.split_once('#')?;
        (!l.is_empty() && !t.is_empty()).then(|| ToolRef {
            library: l.to_owned(),
            tool: t.to_owned(),
        })
    }
}

impl fmt::Display for ToolRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.library, self.tool)
    }
}

/// Every loaded tool library, by id.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToolStore {
    libs: BTreeMap<String, ToolLibrary>,
}

impl ToolStore {
    pub fn new() -> ToolStore {
        ToolStore::default()
    }

    /// Adds a library; a later library with the same id replaces it.
    pub fn insert(&mut self, lib: ToolLibrary) {
        self.libs.insert(lib.id.clone(), lib);
    }

    /// The tool named by `library#tool`.
    pub fn get(&self, r: &str) -> Option<&Tool> {
        let r = ToolRef::parse(r)?;
        self.libs.get(&r.library)?.tool(&r.tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: &str = r#"
format = "fluut.tools/1"
id     = "user/router-bits"
name   = "Router bits"

[[tool]]
id          = "flat-6"
name        = "6 mm 2-flute flat"
number      = 1
shape       = "flat"
diameter    = 6.0
corner_radius = 0.0
flute_length  = 22
centre_cutting = true
overall_length = 60
shank_diameter = 6.0
holder      = { diameter = 20, length = 40 }
names       = { de = "6 mm Schaftfräser" }

  [[tool.preset]]
  material   = "MDF"
  rpm        = 18000
  feed       = 2500
  plunge     = 800
  stepover   = 2.4
  step_down  = 3.0

[[tool]]
id = "ball-6"
shape = "ball"
diameter = 6
colour = "red"
"#;

    #[test]
    fn spec_example_loads() {
        let (lib, w) = ToolLibrary::parse(LIB).unwrap();
        assert_eq!(w, ["unknown key `tool.1.colour` ignored"]);
        let mut s = ToolStore::new();
        s.insert(lib);
        let t = s.get("user/router-bits#flat-6").unwrap();
        assert_eq!(t.number, 1);
        assert!(t.centre_cutting);
        assert_eq!(t.preset(Some("MDF")).unwrap().get("step_down"), Some(3.0));
        assert!(t.preset(Some("Oak")).is_none());
        assert!(t.preset(None).is_none());
        let b = s.get("user/router-bits#ball-6").unwrap();
        assert_eq!(b.corner_radius(), 3.0);
        assert!(b.centre_cutting);
        assert!(s.get("user/router-bits#nope").is_none());
        assert!(s.get("no-hash").is_none());
    }

    #[test]
    fn bad_format_rejected() {
        let e = ToolLibrary::parse("format = \"fluut.tools/2\"\nid = \"x\"").unwrap_err();
        assert!(e.contains("expected"), "{e}");
        assert!(ToolLibrary::parse("format = \"fluut.tools/1\"\nid = \"x\"\n[[tool]]\nid = \"a\"\nshape = \"star\"\ndiameter = 1").is_err());
    }
}
