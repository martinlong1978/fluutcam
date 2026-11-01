// SPDX-License-Identifier: MIT OR Apache-2.0

//! Templates (D06 §4.4): resolution, expressions, application.

use fluut_model::defs::DefRef;
use fluut_model::document::Document;
use fluut_model::library::{DefKind, LayerKind, Library};
use fluut_model::params::{Num, PValue};
use fluut_model::project::{Project, Setup, SetupKind, write};
use fluut_model::template::Target;
use fluut_model::tools::{ToolLibrary, ToolStore};
use std::sync::Arc;

const TOOLS: &str = r#"format = "fluut.tools/1"
id = "shop"
[[tool]]
id = "flat-6"
shape = "flat"
diameter = 6
flute_length = 20
[[tool]]
id = "flat-10"
shape = "flat"
diameter = 10
"#;

/// The D06 §4.4 form, with an M3 type.
const BASE: &str = r#"format  = "fluut.template/1"
id      = "fluut/pocket-default"
version = "1.0"
kind    = "toolpath"
[toolpath]
type = "pocket_2d"
name = "Pocket"
[toolpath.passes]
stepover  = "0.4*tool.diameter"
step_down = 3
[toolpath.linking]
ramp = { kind = "helix", angle_deg = 3, radius = "tool.radius - 0.5" }
"#;

const MDF: &str = r#"format  = "fluut.template/1"
id      = "user/mdf-pocket"
version = "1.0"
kind    = "toolpath"
extends = "fluut/pocket-default@1"
[toolpath.passes]
step_down      = "tool.flute_length / 4"
stock_to_leave = 0.2
"#;

const SETUP: &str = r#"format  = "fluut.template/1"
id      = "user/sheet"
version = "1.0"
kind    = "setup"
[setup]
kind     = "three_axis"
material = "MDF"
stock    = { kind = "block", margin = [5, 5, 0] }
[[toolpath]]
template = "user/mdf-pocket@1"
name     = "Pockets"
[[toolpath]]
type = "contour_2d"
name = "Cut out"
tool = "shop#flat-10"
[toolpath.passes]
step_down = "tool.diameter / 2"
"#;

fn setup() -> (Library, ToolStore) {
    let mut l = Library::new();
    l.add(DefKind::Template, LayerKind::BuiltIn, "base", BASE)
        .unwrap();
    l.add(DefKind::Template, LayerKind::User, "mdf", MDF)
        .unwrap();
    l.add(DefKind::Template, LayerKind::User, "sheet", SETUP)
        .unwrap();
    let mut tools = ToolStore::new();
    tools.insert(ToolLibrary::parse(TOOLS).unwrap().0);
    (l, tools)
}

fn r(s: &str) -> DefRef {
    DefRef::parse(s).unwrap()
}

fn project() -> Project {
    let mut p = Project::new("T");
    p.setups.push(Arc::new(Setup::new("s1", "Top")));
    p
}

fn num(p: &Project, tp: &str, path: &str) -> f64 {
    p.toolpath(tp)
        .unwrap()
        .1
        .params
        .get(path)
        .unwrap()
        .num()
        .unwrap()
}

#[test]
fn toolpath_template_with_extends_and_expressions() {
    let (l, tools) = setup();
    let t = l.template(&r("user/mdf-pocket@1")).unwrap();
    assert_eq!(t.layer, "user/mdf-pocket@1.0");
    let target = Target::Toolpath {
        setup: "s1".into(),
        at: None,
        id: "t1".into(),
        name: None,
        tool: "shop#flat-6".into(),
    };
    let e = l
        .apply_template(
            &r("user/mdf-pocket@1"),
            &target,
            &tools,
            &mut || unreachable!(),
        )
        .unwrap();
    let mut d = Document::new(project());
    d.apply(e).unwrap();
    let p = d.project();
    let (_, tp) = p.toolpath("t1").unwrap();
    assert_eq!(tp.name, "Pocket");
    assert_eq!(tp.template.as_deref(), Some("user/mdf-pocket@1.0"));
    assert_eq!(num(p, "t1", "passes.stepover"), 0.4 * 6.0);
    // The child overrides the parent's step-down, evaluated for this tool.
    assert_eq!(num(p, "t1", "passes.step_down"), 5.0);
    assert_eq!(num(p, "t1", "passes.stock_to_leave"), 0.2);
    assert_eq!(num(p, "t1", "linking.ramp.radius"), 2.5);
    // The file holds numbers only (D15 §1.6).
    let text = write(p);
    assert!(!text.contains("tool."), "{text}");
    assert!(
        text.contains("template = \"user/mdf-pocket@1.0\""),
        "{text}"
    );
    // Another tool, other numbers.
    let target = Target::Toolpath {
        setup: "s1".into(),
        at: Some(0),
        id: "t2".into(),
        name: Some("Big".into()),
        tool: "shop#flat-10".into(),
    };
    d.apply(
        l.apply_template(
            &r("fluut/pocket-default"),
            &target,
            &tools,
            &mut || unreachable!(),
        )
        .unwrap(),
    )
    .unwrap();
    let p = d.project();
    assert_eq!(p.setups[0].toolpaths[0].id, "t2");
    assert_eq!(num(p, "t2", "passes.stepover"), 4.0);
    assert_eq!(
        p.toolpath("t2").unwrap().1.params.get("passes.step_down"),
        Some(&PValue::Num(Num::int(3)))
    );
}

#[test]
fn setup_template_creates_its_toolpaths_in_order() {
    let (l, tools) = setup();
    let mut n = 0;
    let e = l
        .apply_template(
            &r("user/sheet"),
            &Target::Setup {
                at: None,
                id: "s2".into(),
                name: Some("Sheet".into()),
                tool: "shop#flat-6".into(),
            },
            &tools,
            &mut || {
                n += 1;
                format!("t{n}")
            },
        )
        .unwrap();
    let mut d = Document::new(project());
    d.apply(e).unwrap();
    let s = d.project().setup("s2").unwrap();
    assert_eq!(s.name, "Sheet");
    assert_eq!(s.kind, Some(SetupKind::ThreeAxis));
    assert_eq!(s.material.as_deref(), Some("MDF"));
    let ids: Vec<_> = s
        .toolpaths
        .iter()
        .map(|t| (t.id.as_str(), t.name.as_str()))
        .collect();
    assert_eq!(ids, [("t1", "Pockets"), ("t2", "Cut out")]);
    assert_eq!(s.toolpaths[1].tool, "shop#flat-10");
    assert_eq!(num(d.project(), "t2", "passes.step_down"), 5.0);
    assert_eq!(num(d.project(), "t1", "passes.step_down"), 5.0);
    assert_eq!(s.toolpaths[1].template.as_deref(), Some("user/sheet@1.0"));
    // One undo step removes the Setup and its toolpaths.
    d.undo().unwrap();
    assert_eq!(d.project(), &project());
}

#[test]
fn later_template_edits_leave_toolpaths_alone() {
    let (mut l, tools) = setup();
    let target = Target::Toolpath {
        setup: "s1".into(),
        at: None,
        id: "t1".into(),
        name: None,
        tool: "shop#flat-6".into(),
    };
    let mut d = Document::new(project());
    d.apply(
        l.apply_template(
            &r("user/mdf-pocket"),
            &target,
            &tools,
            &mut || unreachable!(),
        )
        .unwrap(),
    )
    .unwrap();
    let before = d.project().clone();
    l.add(
        DefKind::Template,
        LayerKind::User,
        "mdf 1.1",
        &MDF.replace("version = \"1.0\"", "version = \"1.1\"")
            .replace("stock_to_leave = 0.2", "stock_to_leave = 0.5"),
    )
    .unwrap();
    assert_eq!(d.project(), &before);
    // Applying again uses the new version.
    let target = Target::Toolpath {
        setup: "s1".into(),
        at: None,
        id: "t2".into(),
        name: None,
        tool: "shop#flat-6".into(),
    };
    d.apply(
        l.apply_template(
            &r("user/mdf-pocket@1"),
            &target,
            &tools,
            &mut || unreachable!(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(num(d.project(), "t1", "passes.stock_to_leave"), 0.2);
    assert_eq!(num(d.project(), "t2", "passes.stock_to_leave"), 0.5);
}

#[test]
fn template_errors() {
    let (mut l, tools) = setup();
    let add = |l: &mut Library, id: &str, body: &str| {
        l.add(
            DefKind::Template,
            LayerKind::User,
            id,
            &format!("format = \"fluut.template/1\"\nid = \"{id}\"\nversion = \"1.0\"\n{body}"),
        )
        .unwrap();
    };
    add(
        &mut l,
        "user/retype",
        "kind = \"toolpath\"\nextends = \"fluut/pocket-default@1\"\n[toolpath]\ntype = \"contour_2d\"",
    );
    add(&mut l, "user/nokind", "[toolpath]\ntype = \"pocket_2d\"");
    add(
        &mut l,
        "user/badexpr",
        "kind = \"toolpath\"\n[toolpath]\ntype = \"pocket_2d\"\n[toolpath.passes]\nstepover = \"tool.diameter *\"",
    );
    add(
        &mut l,
        "user/unknown",
        "kind = \"toolpath\"\n[toolpath]\ntype = \"pocket_2d\"\n[toolpath.passes]\nzigzag = 1",
    );
    add(
        &mut l,
        "user/geom",
        "kind = \"setup\"\n[setup]\ngeometry = { bodies = [\"b-1\"] }",
    );
    let tp = Target::Toolpath {
        setup: "s1".into(),
        at: None,
        id: "t1".into(),
        name: None,
        tool: "shop#flat-6".into(),
    };
    let st = Target::Setup {
        at: None,
        id: "s9".into(),
        name: None,
        tool: "shop#flat-6".into(),
    };
    for (id, target, want) in [
        ("user/retype", &tp, "changes the toolpath type"),
        ("user/nokind", &tp, "`kind` is missing"),
        ("user/badexpr", &tp, "passes.stepover"),
        ("user/unknown", &tp, "zigzag"),
        ("user/geom", &st, "chosen when the template is applied"),
        ("user/sheet", &tp, "cannot be applied here"),
        ("user/none", &tp, "not found"),
    ] {
        let e = l
            .apply_template(&r(id), target, &tools, &mut || "x".into())
            .unwrap_err();
        assert!(e.to_string().contains(want), "{id}: {e}");
    }
    // A missing tool leaves its names without values.
    let tp = Target::Toolpath {
        setup: "s1".into(),
        at: None,
        id: "t1".into(),
        name: None,
        tool: "shop#gone".into(),
    };
    let e = l
        .apply_template(&r("user/mdf-pocket"), &tp, &tools, &mut || "x".into())
        .unwrap_err();
    assert!(e.to_string().contains("has no value"), "{e}");
}
