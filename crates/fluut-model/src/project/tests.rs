// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::params::{Num, PValue};

const HEADER: &str = "format = \"fluut.project/1\"
name   = \"D15 examples\"
units  = \"mm\"

[[setup]]
id   = \"s1\"
name = \"Top\"
kind = \"three_axis\"
";

/// The ```toml blocks of D15, by the heading they follow.
fn d15_examples() -> Vec<(String, String)> {
    doc_examples("D15-toolpath-parameters.md")
}

/// The ```toml blocks of a design document, by the heading they follow.
fn doc_examples(file: &str) -> Vec<(String, String)> {
    let path = format!("{}/../../docs/design/{file}", env!("CARGO_MANIFEST_DIR"));
    let doc = std::fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut block: Option<String> = None;
    for line in doc.lines() {
        if let Some(b) = &mut block {
            if line == "```" {
                out.push((heading.clone(), block.take().unwrap()));
            } else {
                b.push_str(line);
                b.push('\n');
            }
        } else if line.starts_with('#') {
            heading = line.to_owned();
        } else if line == "```toml" {
            block = Some(String::new());
        }
    }
    out
}

/// Removes `# …` comments outside strings and trailing spaces.
fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let mut in_str = false;
        let mut end = line.len();
        for (i, c) in line.char_indices() {
            match c {
                '"' => in_str = !in_str,
                '#' if !in_str => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        out.push_str(line[..end].trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn every_d15_example_round_trips() {
    let examples: Vec<_> = d15_examples()
        .into_iter()
        .filter(|(_, b)| b.contains("[[setup.toolpath]]"))
        .collect();
    let headings: Vec<&str> = examples.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(
        headings,
        ["### 3.4 Example", "### 4.3 Example", "### 5.3 Example"]
    );
    for (h, block) in examples {
        let src = format!("{HEADER}\n{}", strip_comments(&block));
        let loaded = read(&src).unwrap_or_else(|e| panic!("{h}: {e}"));
        assert!(loaded.warnings.is_empty(), "{h}: {:?}", loaded.warnings);
        assert_eq!(write(&loaded.project), src, "{h}");
    }
}

/// The type of an example toolpath, if this build has it.
fn example_type(block: &str) -> Option<ToolpathTypeId> {
    let line = block.lines().find(|l| l.trim_start().starts_with("type"))?;
    let v = line.split('"').nth(1)?;
    ToolpathTypeId::parse(v)
}

#[test]
fn every_d15b_example_round_trips() {
    // D15b §2–§3 examples of the types this build has (D15 §6 test).
    let mut seen = Vec::new();
    for (h, block) in doc_examples("D15b-toolpath-parameters-2.md") {
        let Some(kind) = example_type(&block) else {
            continue;
        };
        seen.push(kind.as_str());
        let src = format!("{HEADER}\n{}", strip_comments(&block));
        let loaded = read(&src).unwrap_or_else(|e| panic!("{h}: {e}"));
        assert!(loaded.warnings.is_empty(), "{h}: {:?}", loaded.warnings);
        assert_eq!(write(&loaded.project), src, "{h}");
    }
    assert_eq!(
        seen,
        [
            "face",
            "adaptive_2d",
            "trace",
            "chamfer_2d",
            "bore",
            "drill"
        ]
    );
}

const FULL: &str = r#"format = "fluut.project/1"
name   = "Bracket"
units  = "inch"

[[geometry]]
id   = "g1"
path = "parts/bracket.step"
kind = "step"
hash = "sha256:00ff"

  [[geometry.body]]
  id   = "b-17"
  name = "Bracket"

  [[geometry.body]]
  id = "b-90"

[[setup]]
id          = "s1"
name        = "Top"
kind        = "three_axis"
machine     = "user/my-router@2"
material    = "MDF"
work_offset = "G55"
geometry    = { bodies = ["b-17"] }
orientation = { tool_axis = "-Y", x_axis = "+X" }
stock       = { kind = "block", margin = [2, 2, 1.5] }
zero        = { method = "stock_top_corner", corner = "back_right" }
clearance   = { kind = "plane", offset = 15 }

  [[setup.toolpath]]
  id       = "t1"
  name     = "Pocket"
  type     = "pocket_2d"
  tool     = "user/router-bits#flat-6"
  enabled  = false
  template = "user/mdf-pocket@1"

    [setup.toolpath.passes]
    pattern = "raster"

[[setup]]
id   = "s2"
name = "Bottom"
kind = "three_axis"
zero = { method = "point", at = [0, 0.5, -12.25] }
"#;

#[test]
fn full_project_round_trips() {
    let l = read(FULL).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    let p = &l.project;
    assert_eq!(p.units, DisplayUnit::Inch);
    assert_eq!(p.geometry[0].bodies.len(), 2);
    let s = &p.setups[0];
    assert_eq!(s.work_offset(), "G55");
    assert_eq!(s.orientation.unwrap().tool_axis, AxisDir::NegY);
    assert_eq!(p.setups[1].work_offset(), "G54");
    let (_, t) = p.toolpath("t1").unwrap();
    assert!(!t.is_enabled());
    assert_eq!(write(p), FULL);
}

#[test]
fn edits_write_canonically() {
    let mut p = Project::new("New");
    let mut s = Setup::new("s1", "Top");
    let mut t = Toolpath::new("t1", "Profile", ToolpathTypeId::Contour2d, "lib#flat-6");
    t.params
        .set("passes.step_down", Some(PValue::Num(Num::float(3.0))))
        .unwrap();
    t.params
        .set(
            "tool_params.feed.cut",
            Some(PValue::Num(Num::float(1500.0))),
        )
        .unwrap();
    s.toolpaths.push(t);
    p.setups.push(s);
    let text = write(&p);
    assert!(text.ends_with(
        "    [setup.toolpath.tool_params]\n    feed = { cut = 1500.0 }\n\n    [setup.toolpath.passes]\n    step_down = 3.0\n"
    ), "{text}");
    assert_eq!(read(&text).unwrap().project, p);
}

#[test]
fn format_versions() {
    let body = "name = \"x\"\n";
    assert!(
        read(&format!("format = \"fluut.project/1\"\n{body}"))
            .unwrap()
            .warnings
            .is_empty()
    );
    let l = read(&format!("format = \"fluut.project/1.3\"\n{body}")).unwrap();
    assert!(l.warnings[0].contains("newer"), "{:?}", l.warnings);
    let e = read(&format!("format = \"fluut.project/2\"\n{body}")).unwrap_err();
    assert!(e.0.contains("not supported"), "{e}");
    assert!(read(&format!("format = \"fluut.post/1\"\n{body}")).is_err());
    assert!(read(body).is_err());
}

#[test]
fn load_errors_and_warnings() {
    let base = "format = \"fluut.project/1\"\n";
    for (src, want) in [
        (
            "[[setup]]\nid = \"s\"\nkind = \"five_axis\"",
            "is not one of",
        ),
        (
            "[[setup]]\nid = \"s\"\nstock = { kind = \"cylinder\" }",
            "not supported",
        ),
        (
            "[[setup]]\nid = \"s\"\n[[setup.toolpath]]\nid = \"t\"\ntype = \"adaptive_3d\"\ntool = \"a#b\"",
            "not supported by this build",
        ),
        (
            "[[setup]]\nid = \"s\"\n[[setup.toolpath]]\nid = \"t\"\ntype = \"pocket_2d\"",
            "`setup[0].toolpath[0].tool` is missing",
        ),
        ("[[setup]]\nid = \"s\"\n[[setup]]\nid = \"s\"", "used twice"),
        (
            "[[setup]]\nid = \"s\"\nzero = { method = \"point\" }",
            "expected [x, y, z]",
        ),
    ] {
        let e = read(&format!("{base}{src}")).unwrap_err();
        assert!(e.0.contains(want), "{src}: {e}");
    }
    let l = read(&format!(
        "{base}colour = 1\n[[setup]]\nid = \"s\"\nbased_on = \"s0\"\n[[view]]\n"
    ))
    .unwrap();
    assert_eq!(
        l.warnings,
        [
            "unknown key `colour` ignored",
            "saved views are not supported yet and were ignored",
            "unknown key `setup[0].based_on` ignored"
        ]
    );
}

#[test]
fn inch_entry_round_trip() {
    // D15 §6: entering a length in inches and reading it back gives the
    // same value to 1e-12.
    for v in [0.25, 1.0 / 3.0, 12.7, 1e-4, 39.37] {
        let mm = length_entry(&format!("{v}"), DisplayUnit::Inch).unwrap();
        assert!((mm / 25.4 - v).abs() <= 1e-12 * v.max(1.0));
        let mm2 = length_entry(&format!("{v}in"), DisplayUnit::Mm).unwrap();
        assert_eq!(mm, mm2);
    }
    assert_eq!(length_entry("6mm", DisplayUnit::Inch), Some(6.0));
    assert_eq!(length_entry("6", DisplayUnit::Mm), Some(6.0));
    assert_eq!(length_entry("six", DisplayUnit::Mm), None);
}
