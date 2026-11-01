// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::params::{Num, PValue};
use std::sync::Arc;

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
    // D15b examples of the types this build has (D15 §6 test).
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
            "engrave",
            "chamfer_2d",
            "bore",
            "drill",
            "adaptive_3d",
            "pocket_3d",
            "contour",
            "horizontal",
            "spiral",
            "scallop",
            "steep_and_shallow",
            "rotary_rough",
            "rotary_along_x",
            "rotary_around_a",
            "rotary_spiral",
            "manual_nc"
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
    assert_eq!(s.work_offset.as_deref(), Some("G55"));
    assert_eq!(
        s.orientation,
        Some(Orientation::ThreeAxis {
            tool_axis: AxisDir::NegY.into(),
            x_axis: AxisDir::PosX.into()
        })
    );
    assert_eq!(p.setups[1].work_offset, None);
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
    p.setups.push(Arc::new(s));
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
    let l = read(&format!("format = \"fluut.project/1.0\"\n{body}")).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    // The writer writes the current format, whatever was read.
    assert!(write(&l.project).starts_with("format = \"fluut.project/1\"\n"));
    let l = read(&format!("format = \"fluut.project/1.3\"\n{body}")).unwrap();
    assert!(l.warnings[0].contains("newer"), "{:?}", l.warnings);
    assert!(
        l.warnings[0].contains("will not be saved"),
        "{:?}",
        l.warnings
    );
    assert!(read(&format!("format = \"fluut.project/1.x\"\n{body}")).is_err());
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
            "`setup[0].stock.radius` is missing",
        ),
        (
            "[[setup]]\nid = \"s\"\nstock = { kind = \"slab\" }",
            "is not one of block, cylinder, body, from_previous",
        ),
        (
            "[[setup]]\nid = \"s\"\nstock = { kind = \"block\", margin = [1, 1, 1], size = [9, 9, 9] }",
            "not both",
        ),
        (
            "[[setup]]\nid = \"s\"\norientation = { tool_axis = \"+Z\" }",
            "`setup[0].orientation.x_axis` is missing",
        ),
        (
            "[[setup]]\nid = \"s\"\norientation = { tool_axis = { body = \"b\" }, x_axis = \"+X\" }",
            "expected an axis",
        ),
        (
            "[[setup]]\nid = \"s\"\nindex_angle = { mode = \"sideways\", value = 1 }",
            "is not one of absolute, relative",
        ),
        (
            "[[setup]]\nid = \"s\"\nzero = { x = \"middle\" }",
            "is not stock_left, stock_right or a number",
        ),
        (
            "[[setup]]\nid = \"s\"\ngeometry = { mirror = [true, false] }",
            "expected [x, y, z] of true or false",
        ),
        (
            "[[setup]]\nid = \"s\"\nregion = { floor_z = \"low\" }",
            "`setup[0].region.floor_z`: expected a number",
        ),
        (
            "[[setup]]\nid = \"s\"\nclearance = { kind = \"cylinder\" }",
            "`setup[0].clearance.radius` is missing",
        ),
        (
            "[[setup]]\nid = \"s\"\n[[setup.toolpath]]\nid = \"t\"\ntype = \"lathe_turn\"\ntool = \"a#b\"",
            "not supported by this build",
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
        "{base}colour = 1\n[[setup]]\nid = \"s\"\nspeed = 2\nstock = {{ kind = \"block\", fill = 1 }}\n"
    ))
    .unwrap();
    assert_eq!(
        l.warnings,
        [
            "unknown key `colour` ignored",
            "unknown key `setup[0].speed` ignored",
            "unknown key `setup[0].stock.fill` ignored"
        ]
    );
    // Ignored keys are not written back (D06 §7.2).
    let text = write(&l.project);
    for k in ["colour", "speed", "fill"] {
        assert!(!text.contains(k), "{text}");
    }
    assert!(read(&text).unwrap().warnings.is_empty());
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

/// Collapses runs of blank lines and drops blank lines at the end.
fn squeeze_blank(s: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in s.lines() {
        if line.is_empty() {
            blank = true;
            continue;
        }
        if blank && !out.is_empty() {
            out.push('\n');
        }
        blank = false;
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The spec 02 §2.1 project: the first ```toml block of the section.
fn spec_project() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/spec/02-data-formats.md"
    );
    let doc = std::fs::read_to_string(path).unwrap();
    let at = doc.find("## 2.1 Project file").unwrap();
    let start = doc[at..].find("```toml\n").unwrap() + at + 8;
    let end = doc[start..].find("```").unwrap() + start;
    doc[start..end].to_owned()
}

/// Drops the toolpaths whose type this build does not have (the rotary
/// types come with M6), with their group tables.
fn drop_unknown_toolpaths(src: &str) -> String {
    let mut out = String::new();
    let mut chunk = String::new();
    let flush = |chunk: &mut String, out: &mut String| {
        let unknown = chunk.starts_with("  [[setup.toolpath]]")
            && !["contour_2d", "pocket_2d", "parallel"]
                .iter()
                .any(|t| chunk.contains(&format!("type = \"{t}\"")));
        if !unknown {
            out.push_str(chunk);
        }
        chunk.clear();
    };
    for line in src.lines() {
        if line == "  [[setup.toolpath]]" || line.starts_with("[[") {
            flush(&mut chunk, &mut out);
        }
        chunk.push_str(line);
        chunk.push('\n');
    }
    flush(&mut chunk, &mut out);
    out
}

#[test]
fn spec_project_round_trips() {
    // D06 §8: the spec example, comments removed.
    let src = squeeze_blank(&drop_unknown_toolpaths(&strip_comments(&spec_project())));
    assert!(src.contains("based_on    = \"s1\""), "{src}");
    let l = read(&src).unwrap_or_else(|e| panic!("{e}\n{src}"));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    let p = &l.project;
    assert_eq!(p.setups.len(), 3);
    let s1 = &p.setups[0];
    assert_eq!(s1.kind, Some(SetupKind::Rotary));
    assert_eq!(
        s1.orientation,
        Some(Orientation::Rotary {
            rotary_axis: AxisPick::Face("f-12".into()),
            a_zero_toward: Some(AxisDir::PosZ.into())
        })
    );
    assert_eq!(
        s1.stock,
        Some(StockSpec::Cylinder {
            radius: Num::int(26),
            length: Num::int(120)
        })
    );
    assert_eq!(
        s1.zero,
        Some(ZeroSpec::OnAxis {
            x: AxisX::StockLeft
        })
    );
    let g = s1.geometry.as_ref().unwrap();
    assert_eq!(g.mirror, Some([false; 3]));
    let s3 = &p.setups[2];
    assert_eq!(s3.based_on.as_deref(), Some("s2"));
    assert_eq!(s3.kind, None);
    assert_eq!(s3.stock, Some(StockSpec::FromPrevious));
    assert_eq!(
        s3.index_angle,
        Some(IndexAngle {
            mode: AngleMode::Relative,
            value: Num::int(90)
        })
    );
    let (_, t3) = p.toolpath("t3").unwrap();
    assert_eq!(t3.tool, "");
    assert_eq!(p.views.len(), 1);
    assert_eq!(write(p), src);
}

#[test]
fn every_reference_project_round_trips() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../fluut-cli/tests/projects");
    let mut n = 0;
    let mut dirs: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    dirs.sort_by_key(|e| e.path());
    for d in dirs {
        let mut files: Vec<_> = std::fs::read_dir(d.path()).unwrap().flatten().collect();
        files.sort_by_key(|e| e.path());
        for f in files {
            let path = f.path();
            if path.extension().is_none_or(|x| x != "fluut") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let l = read(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(
                l.warnings.is_empty(),
                "{}: {:?}",
                path.display(),
                l.warnings
            );
            assert_eq!(write(&l.project), src, "{}", path.display());
            n += 1;
        }
    }
    assert!(n >= 4, "{n} reference projects");
}

const FORMS: &str = r#"format = "fluut.project/1"
name   = "Forms"
units  = "mm"

[[setup]]
id          = "s1"
name        = "Block"
kind        = "three_axis"
geometry    = { bodies = ["b-1", "b-2"], scale = [1, 1, 0.5], mirror = [true, false, false] }
orientation = { tool_axis = { face = "f-3" }, x_axis = { edge = "e-1.4" } }
stock       = { kind = "block", size = [100, 80, 25.5] }
zero        = { method = "stock_top_centre" }
region      = { floor_z = -20 }
clearance   = { kind = "plane" }

[[setup]]
id        = "s2"
name      = "Casting"
based_on  = "s1"
stock     = { kind = "body", body = "b-2" }
zero      = { method = "stock_bottom_corner", corner = "front_right" }
clearance = { kind = "plane", offset = 12.5 }

[[setup]]
id          = "s3"
name        = "Turn"
kind        = "rotary"
orientation = { rotary_axis = "+X" }
stock       = { kind = "cylinder", radius = 20, length = 90.5 }
zero        = { x = -3.5 }
clearance   = { kind = "cylinder", radius = 30 }

[[setup]]
id          = "s4"
name        = "Side"
based_on    = "s3"
kind        = "indexed"
index_angle = { mode = "absolute", value = 45.5 }
stock       = { kind = "from_previous" }
zero        = { x = "stock_right" }

[[setup]]
id    = "s5"
name  = "Plain"
stock = { kind = "block" }

[[view]]
eye    = [0, -100, 50]
name   = "Front"
target = { x = 0, y = 0.5 }
"#;

#[test]
fn every_setup_form_round_trips() {
    let l = read(FORMS).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    let p = &l.project;
    assert_eq!(
        p.setups[0].stock,
        Some(StockSpec::Block {
            margin: None,
            size: Some([Num::int(100), Num::int(80), Num::float(25.5)])
        })
    );
    assert_eq!(
        p.setups[0].clearance.as_ref().unwrap().plane_offset(),
        Some(10.0)
    );
    assert_eq!(
        p.setups[1].clearance.as_ref().unwrap().plane_offset(),
        Some(12.5)
    );
    assert_eq!(
        p.setups[2].zero,
        Some(ZeroSpec::OnAxis {
            x: AxisX::At(Num::float(-3.5))
        })
    );
    assert_eq!(p.setups[4].kind, None);
    assert_eq!(write(p), FORMS);
}

#[test]
fn setups_are_shared_between_clones() {
    let p = read(FORMS).unwrap().project;
    let mut q = p.clone();
    q.setup_mut("s2").unwrap().name = "Renamed".into();
    assert!(Arc::ptr_eq(&p.setups[0], &q.setups[0]));
    assert!(!Arc::ptr_eq(&p.setups[1], &q.setups[1]));
    assert_eq!(p.setups[1].name, "Casting");
}
