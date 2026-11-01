// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tool libraries (spec 02 §2.4).
//!
//! A library may have a `version` (`major.minor`, default `1.0`) and may
//! `extends` another library: its tools replace the parent's tools with
//! the same id and add the rest (D06 §4). References take a version as
//! other definitions do: `shop#flat-6`, `shop@2#flat-6`.

use crate::defs::{DefRef, MAX_DEPTH, Version, VersionReq};
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
    /// `major.minor`; absent means `1.0`.
    pub version: Option<String>,
    /// Parent library reference, e.g. `fluut/router-bits@1`.
    pub extends: Option<String>,
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
    Drill,
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
    /// V and taper tools: half the included angle, degrees (D07 §1).
    pub half_angle_deg: Option<f64>,
    /// V and taper tools: diameter of the tip (0 = sharp).
    pub tip_diameter: Option<f64>,
    /// Drill tools: included point angle, degrees; 118 when absent.
    pub point_angle_deg: Option<f64>,
    /// Taps: thread pitch, mm.
    pub pitch: Option<f64>,
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
    /// Radial width of cut for adaptive types (D15b §9, decision 1).
    pub optimal_load: Option<f64>,
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
            "optimal_load" => self.optimal_load,
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
            ToolShape::Flat | ToolShape::V | ToolShape::Drill => 0.0,
            ToolShape::Bull | ToolShape::Taper => self.corner_radius.unwrap_or(0.0),
        }
    }

    /// Half-angle α in radians of a V or taper tool with `half_angle_deg`
    /// above 0; `None` for other tools (D15b §1.9).
    pub fn v_angle(&self) -> Option<f64> {
        match (self.shape, self.half_angle_deg) {
            (ToolShape::V | ToolShape::Taper, Some(a)) if a > 0.0 && a < 90.0 => {
                Some(a.to_radians())
            }
            _ => None,
        }
    }

    /// Tip radius Rt of a V or taper tool.
    pub fn tip_radius(&self) -> f64 {
        0.5 * self.tip_diameter.unwrap_or(0.0).max(0.0)
    }

    /// Height of the cone of a V tool: (R − Rt) / tan α (D15b §2.4).
    pub fn cone_height(&self) -> Option<f64> {
        let a = self.v_angle()?;
        Some((self.radius() - self.tip_radius()) / libm::tan(a))
    }

    /// Point angle of a drill in degrees (default 118, D15b §9 decision 3).
    pub fn point_angle(&self) -> f64 {
        self.point_angle_deg.unwrap_or(118.0)
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
        if let Some(v) = &lib.version {
            Version::parse(v).map_err(|e| e.to_string())?;
        }
        if let Some(e) = &lib.extends {
            DefRef::parse(e).map_err(|e| e.to_string())?;
        }
        Ok((lib, warnings))
    }

    /// The library version (`1.0` when not given).
    pub fn version(&self) -> Version {
        self.version
            .as_deref()
            .and_then(|v| Version::parse(v).ok())
            .unwrap_or(Version { major: 1, minor: 0 })
    }

    pub fn tool(&self, id: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.id == id)
    }
}

/// `library#tool` or `library@version#tool` (spec 02 §2.1).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ToolRef {
    pub library: String,
    pub tool: String,
    pub version: VersionReq,
}

impl ToolRef {
    pub fn parse(s: &str) -> Option<ToolRef> {
        let (l, t) = s.split_once('#')?;
        let lib = DefRef::parse(l).ok()?;
        (!t.is_empty()).then(|| ToolRef {
            library: lib.id,
            tool: t.to_owned(),
            version: lib.version,
        })
    }
}

impl fmt::Display for ToolRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lib = DefRef {
            id: self.library.clone(),
            version: self.version,
        };
        write!(f, "{lib}#{}", self.tool)
    }
}

/// Every loaded tool library, by id and version.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToolStore {
    libs: BTreeMap<(String, Version), ToolLibrary>,
}

impl ToolStore {
    pub fn new() -> ToolStore {
        ToolStore::default()
    }

    /// Adds a library; a later library with the same id and version
    /// replaces it.
    pub fn insert(&mut self, lib: ToolLibrary) {
        self.libs.insert((lib.id.clone(), lib.version()), lib);
    }

    /// The highest version of library `id` that `req` accepts.
    pub fn library(&self, id: &str, req: VersionReq) -> Option<&ToolLibrary> {
        self.libs
            .range((id.to_owned(), Version { major: 0, minor: 0 })..)
            .take_while(|((l, _), _)| l == id)
            .filter(|((_, v), _)| req.matches(*v))
            .last()
            .map(|(_, lib)| lib)
    }

    /// The tool named by `library#tool` or `library@version#tool`. A tool
    /// not in the library is looked up in the library it `extends`.
    pub fn get(&self, r: &str) -> Option<&Tool> {
        let r = ToolRef::parse(r)?;
        let mut lib = self.library(&r.library, r.version)?;
        for _ in 0..MAX_DEPTH {
            if let Some(t) = lib.tool(&r.tool) {
                return Some(t);
            }
            let parent = DefRef::parse(lib.extends.as_deref()?).ok()?;
            lib = self.library(&parent.id, parent.version)?;
        }
        None
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
    fn drill_v_and_adaptive_keys() {
        // Spec 02 §2.4 additions of D15b §9 (decisions 1, 3, 4), and the V
        // tool angle and tip that engrave and chamfer_2d need.
        let src = "format = \"fluut.tools/1\"\nid = \"l\"\n\
                   [[tool]]\nid = \"d5\"\nshape = \"drill\"\ndiameter = 5\n\
                   [[tool]]\nid = \"tap\"\nshape = \"drill\"\ndiameter = 6\npitch = 1.0\n\
                   point_angle_deg = 135\n\
                   [[tool]]\nid = \"v90\"\nshape = \"v\"\ndiameter = 12\nhalf_angle_deg = 45\n\
                   tip_diameter = 0.2\n[[tool.preset]]\nmaterial = \"MDF\"\noptimal_load = 0.6\n\
                   [[tool]]\nid = \"v0\"\nshape = \"v\"\ndiameter = 12\n";
        let (lib, w) = ToolLibrary::parse(src).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let d = lib.tool("d5").unwrap();
        assert_eq!(d.shape, ToolShape::Drill);
        assert_eq!(d.point_angle(), 118.0);
        assert_eq!(d.corner_radius(), 0.0);
        assert!(d.v_angle().is_none() && d.pitch.is_none());
        let t = lib.tool("tap").unwrap();
        assert_eq!((t.point_angle(), t.pitch), (135.0, Some(1.0)));
        let v = lib.tool("v90").unwrap();
        assert!((v.v_angle().unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-15);
        assert_eq!(v.tip_radius(), 0.1);
        assert!((v.cone_height().unwrap() - 5.9).abs() < 1e-12);
        assert_eq!(
            v.preset(Some("MDF")).unwrap().get("optimal_load"),
            Some(0.6)
        );
        assert!(lib.tool("v0").unwrap().v_angle().is_none());
    }

    #[test]
    fn bad_format_rejected() {
        let e = ToolLibrary::parse("format = \"fluut.tools/2\"\nid = \"x\"").unwrap_err();
        assert!(e.contains("expected"), "{e}");
        assert!(ToolLibrary::parse("format = \"fluut.tools/1\"\nid = \"x\"\n[[tool]]\nid = \"a\"\nshape = \"star\"\ndiameter = 1").is_err());
    }
    fn lib(src: &str) -> ToolLibrary {
        ToolLibrary::parse(src).unwrap().0
    }

    #[test]
    fn versions_and_extends() {
        let mut s = ToolStore::new();
        s.insert(lib(
            "format = \"fluut.tools/1\"\nid = \"base\"\nversion = \"1.0\"\n\
             [[tool]]\nid = \"a\"\nshape = \"flat\"\ndiameter = 3\n\
             [[tool]]\nid = \"b\"\nshape = \"flat\"\ndiameter = 4",
        ));
        s.insert(lib(
            "format = \"fluut.tools/1\"\nid = \"base\"\nversion = \"2.1\"\n\
             [[tool]]\nid = \"a\"\nshape = \"flat\"\ndiameter = 30",
        ));
        s.insert(lib(
            "format = \"fluut.tools/1\"\nid = \"shop\"\nextends = \"base@1\"\n\
             [[tool]]\nid = \"b\"\nshape = \"ball\"\ndiameter = 5",
        ));
        assert_eq!(s.get("base#a").unwrap().diameter, 30.0);
        assert_eq!(s.get("base@1#a").unwrap().diameter, 3.0);
        assert_eq!(s.get("base@2.1#a").unwrap().diameter, 30.0);
        assert!(s.get("base@2.0#a").is_none());
        assert!(s.get("base@3#a").is_none());
        // The child replaces `b` and inherits `a` from base 1.x.
        assert_eq!(s.get("shop#b").unwrap().diameter, 5.0);
        assert_eq!(s.get("shop#a").unwrap().diameter, 3.0);
        assert_eq!(s.get("shop@1.0#a").unwrap().diameter, 3.0);
        assert!(s.get("shop#c").is_none());
        assert_eq!(ToolRef::parse("shop@2#a").unwrap().to_string(), "shop@2#a");
        assert!(ToolRef::parse("shop@x#a").is_none());
        // A cycle ends the lookup.
        s.insert(lib(
            "format = \"fluut.tools/1\"\nid = \"loop\"\nextends = \"loop\"",
        ));
        assert!(s.get("loop#a").is_none());
        assert!(
            ToolLibrary::parse("format = \"fluut.tools/1\"\nid = \"x\"\nversion = \"1\"").is_err()
        );
    }
}
