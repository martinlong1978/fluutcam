// SPDX-License-Identifier: MIT OR Apache-2.0

//! The object API (D12b) on the reference projects, and the spec 09 §9.5
//! example.

mod support;

use fluut_model::params::PValue;
use fluut_model::project::write;
use fluut_script::{Console, ErrorKind, RunOptions, run};
use std::sync::Arc;
use support::*;

/// Spec 09 §9.5, verbatim.
const SPEC_EXAMPLE: &str = r#"//! fluut >= 1.0
// Drill every vertical hole of 6 mm or smaller in the top setup.
let p = fluut.project;
let top = p.setups[0];

let holes = [];
for body in p.model.bodies {
    for f in body.faces {
        if f.kind == "cylinder" && f.params.radius <= 3.0 && f.params.axis_is_vertical {
            holes.push(f.id);
        }
    }
}

top.add_toolpath("drill", #{
    tool: "user/drills#d6",
    geometry: #{ drive_faces: holes },
    passes: #{ cycle: "peck", peck_depth: 2.0 },
});
fluut.log(`Added drilling for ${holes.len} holes`);
"#;

#[test]
fn spec_example() {
    let (s, _, perm) = open("holes", "spec_example");
    // This build is 0.1: the header refuses it with a clear message.
    let c = Arc::new(Console::new());
    let o = run(
        SPEC_EXAMPLE,
        Some(s),
        opts(perm.clone(), small()),
        c.clone(),
    );
    let e = o.result.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Version);
    assert!(e.message.contains("1.0"), "{e}");
    // As FluutCAM 1.0 it runs.
    let o = run(
        SPEC_EXAMPLE,
        o.session,
        RunOptions {
            version: "1.0.0".into(),
            ..opts(perm, Default::default())
        },
        c.clone(),
    );
    ok(&o, &c);
    let s = o.session.unwrap();
    let setup = &s.project.setups[0];
    let t = setup.toolpaths.last().unwrap();
    assert_eq!(t.kind().as_str(), "drill");
    assert_eq!(t.tool, "user/drills#d6");
    assert_eq!(
        t.params.get("passes.peck_depth").and_then(PValue::num),
        Some(2.0)
    );
    let Some(PValue::Selection(sel)) = t.params.get("geometry.drive_faces") else {
        panic!("no selection");
    };
    assert!(!sel.is_empty());
    assert!(
        c.text()
            .contains(&format!("Added drilling for {} holes", sel.len())),
        "{}",
        c.text()
    );
    assert_eq!(o.undo.unwrap().label, "Script test.rhai");
}

#[test]
fn project_members() {
    let (s, dir, perm) = open("block", "project_members");
    let (o, c) = on(
        r#"
        let p = fluut.project;
        print(p.name); print(p.units); print(type_of(p.path));
        print(p.setups.len); print(p.toolpaths.len);
        print(p.setup("Top").id); print(p.setup("s1").name);
        print(p.toolpath("Cut out").id); print(p.toolpath("t1").name);
        try { p.setup("nope") } catch (e) { print(e) }
        p.name = "Renamed"; p.units = "inch";
        print(p.name); print(p.units);
        let s2 = p.add_setup(#{ name: "Second", based_on: "s1", stock: #{ kind: "from_previous" } });
        print(s2.id); print(p.setups.len);
        let s3 = p.add_setup(#{});
        print(s3.name);
        try { p.add_setup(#{ colour: "red" }) } catch (e) { print(e) }
        try { p.units = "furlong" } catch (e) { print(e) }
        print(fluut.has_project);
        print(fluut.library.posts.contains("fluut/linuxcnc@1.0"));
        print(fluut.library.machines.len > 0);
        print(fluut.library.tool("shop#flat-6").diameter);
        print(fluut.library.tools("shop").len > 0);
        print(fluut.library.tool("shop#nope"));
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let lines: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(
        &lines[..9],
        [
            "Block", "mm", "string", "1", "3", "s1", "Top", "t3", "Pocket"
        ]
    );
    assert!(
        lines[9].contains("no Setup has the id or name `nope`"),
        "{}",
        lines[9]
    );
    assert_eq!(&lines[10..14], ["Renamed", "inch", "s2", "2"]);
    assert_eq!(lines[14], "Setup 3");
    assert!(lines[15].contains("colour"), "{}", lines[15]);
    assert!(lines[16].contains("furlong"), "{}", lines[16]);
    assert_eq!(&lines[17..], ["true", "true", "true", "6.0", "true", ""]);
    let s = o.session.unwrap();
    assert_eq!(s.project.setups.len(), 3);
    assert_eq!(s.project.setups[1].based_on.as_deref(), Some("s1"));
    let _ = dir;
}

#[test]
fn setup_members_resolve_and_inherit() {
    let (s, _, perm) = open("block", "setup_members");
    let (o, c) = on(
        r#"
        let p = fluut.project;
        let top = p.setups[0];
        print(top.kind); print(top.material); print(top.stock); print(top.zero);
        print(top.is_inherited("material")); print(top.origin("zero")); print(top.origin("clearance"));
        let child = p.add_setup(#{ name: "Child", based_on: "s1" });
        print(child.material); print(child.is_inherited("material")); print(child.origin("material"));
        child.material = "Oak";
        print(child.material); print(child.origin("material"));
        child.reset("material");
        print(child.material);
        top.stock = #{ kind: "block", margin: ["0.25in", "5mm", 0] };
        print(top.stock);
        top.material = ();
        print(top.material);
        try { top.colour = 1 } catch (e) { print("no colour") }
        try { top.stock = #{ kind: "blob" } } catch (e) { print(e) }
        try { top.is_inherited("name") } catch (e) { print("not inheritable") }
        print(top.toolpaths.len); print(top.toolpath("Pocket").id);
        child.remove();
        print(p.setups.len);
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(l[0], "three_axis");
    assert_eq!(l[1], "MDF");
    assert!(
        l[2].contains("\"block\"") && l[2].contains("margin"),
        "{}",
        l[2]
    );
    assert!(l[3].contains("stock_top_corner"), "{}", l[3]);
    assert_eq!(&l[4..7], ["false", "local", "default"]);
    assert_eq!(&l[7..10], ["MDF", "true", "s1"]);
    assert_eq!(&l[10..12], ["Oak", "local"]);
    assert_eq!(l[12], "MDF");
    assert!(l[13].contains("6.35") && l[13].contains("5.0"), "{}", l[13]);
    assert_eq!(l[14], "");
    assert_eq!(l[15], "no colour");
    assert!(l[16].contains("blob"), "{}", l[16]);
    assert_eq!(&l[17..], ["not inheritable", "3", "t1", "1"]);
}

#[test]
fn toolpath_members() {
    let (s, _, perm) = open("block", "toolpath_members");
    let (o, c) = on(
        r#"
        let t = fluut.project.toolpath("t1");
        print(t.id); print(t.type); print(t.name); print(t.tool); print(t.enabled);
        print(t.template); print(t.setup.id);
        print(t.passes.stepover); print(t.passes["step_down"]); print(t.passes.pattern);
        print(t.linking.ramp);
        print(t.passes.keys());
        print(t.passes.fields().contains("stepover"));
        print(t.passes.to_map());
        t.passes.stepover = "0.1in";
        print(t.passes.stepover);
        t.linking.ramp.angle_deg = 5;
        print(t.linking.ramp);
        t.passes.stepover = ();
        print(t.passes.stepover);
        try { t.passes.stepover_x = 1 } catch (e) { print(e) }
        try { t.passes.stepover = "wide" } catch (e) { print("bad value") }
        try { t.type = "drill" } catch (e) { print("read-only") }
        t.name = "P"; t.enabled = false; t.tool = "shop#ball-6";
        print(t.name); print(t.enabled); print(t.tool);
        t.passes = #{ step_down: 3.0 };
        print(t.passes.to_map());
        let n = fluut.project.setups[0].add_toolpath("contour_2d", #{
            name: "Outline",
            tool: "shop#flat-6",
            geometry: #{ drive_faces: ["f-494.0", fluut.project.model.body("b-494").face("f-494.1")] },
            heights: #{ bottom: #{ ref: "stock_bottom", offset: -0.2 } },
            passes: #{ step_down: "5mm" },
        });
        print(n.id); print(n.geometry.drive_faces); print(n.passes.step_down);
        try { fluut.project.setups[0].add_toolpath("contour_2d", #{ passes: #{ nope: 1 } }) } catch (e) { print(e) }
        try { fluut.project.setups[0].add_toolpath("whirl", #{}) } catch (e) { print(e) }
        try { fluut.project.setups[0].add_toolpath("drill", #{ colour: 1 }) } catch (e) { print(e) }
        n.remove();
        print(fluut.project.toolpaths.len);
        try { n.name } catch (e) { print(e) }
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(
        &l[..7],
        ["t1", "pocket_2d", "Pocket", "shop#flat-6", "true", "", "s1"]
    );
    assert_eq!(&l[7..10], ["2.4", "4.0", ""]);
    assert!(
        l[10].contains("helix") && l[10].contains("angle_deg"),
        "{}",
        l[10]
    );
    assert_eq!(
        l[11],
        r#"["stepover", "step_down", "stock_to_leave", "stock_to_leave_axial"]"#
    );
    assert_eq!(l[12], "true");
    assert!(l[13].contains("\"stepover\": 2.4"), "{}", l[13]);
    assert_eq!(l[14], format!("{}", 0.1 * 25.4));
    assert!(l[15].contains("\"angle_deg\": 5"), "{}", l[15]);
    assert_eq!(l[16], "");
    assert!(
        l[17].contains("passes.stepover_x") && l[17].contains("pocket_2d"),
        "{}",
        l[17]
    );
    assert_eq!(&l[18..20], ["bad value", "read-only"]);
    assert_eq!(&l[20..23], ["P", "false", "shop#ball-6"]);
    assert_eq!(l[23], r#"#{"step_down": 3.0}"#);
    assert_eq!(l[24], "t4");
    assert_eq!(l[25], r#"[#{"face": "f-494.0"}, #{"face": "f-494.1"}]"#);
    assert_eq!(l[26], "5.0");
    assert!(l[27].contains("passes.nope"), "{}", l[27]);
    assert!(l[28].contains("whirl"), "{}", l[28]);
    assert!(l[29].contains("colour"), "{}", l[29]);
    assert_eq!(l[30], "3");
    assert!(l[31].contains("no longer in the project"), "{}", l[31]);
}

#[test]
fn values_round_trip_through_scripts() {
    // Reading every Setup and toolpath key and writing it back changes
    // nothing: file keys and script properties are one to one.
    for name in ["block", "boss", "holes", "plate", "plate2d", "ridge"] {
        let (s, _, perm) = open(name, &format!("round_trip_{name}"));
        let before = write(&s.project);
        let (o, c) = on(
            r#"
            for s in fluut.project.setups {
                s.name = s.name;
                for t in s.toolpaths {
                    t.name = t.name;
                    t.tool = t.tool;
                    for g in [t.tool_params, t.geometry, t.heights, t.passes, t.linking] {
                        for k in g.keys() { g[k] = g[k]; }
                    }
                }
            }
            "#,
            s,
            perm,
        );
        ok(&o, &c);
        assert_eq!(write(&o.session.unwrap().project), before, "{name}");
    }
}

#[test]
fn model_bodies_and_faces() {
    let (s, _, perm) = open("holes", "model_members");
    let (o, c) = on(
        r#"
        let m = fluut.project.model;
        print(m.sources);
        let b = m.bodies[0];
        print(b.id); print(b.name); print(b.source); print(b.kind);
        print(m.body("holes").id);
        print(b.bounds.min.len);
        let cyl = b.faces.filter(|f| f.kind == "cylinder");
        print(cyl.len > 0);
        let f = cyl[0];
        print(f.body); print(f.params.keys());
        print(f.bounds.max[2] >= f.bounds.min[2]);
        print(b.face(f.id).id == f.id);
        try { b.face("f-1.999") } catch (e) { print("no face") }
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert!(
        l[0].contains("holes.step") && l[0].contains("\"step\""),
        "{}",
        l[0]
    );
    assert_eq!(
        &l[1..8],
        ["b-612", "holes", "g1", "solid", "b-612", "3", "true"]
    );
    assert_eq!(l[8], "b-612");
    assert_eq!(
        l[9],
        r#"["axis", "axis_is_vertical", "diameter", "inside", "origin", "radius"]"#
    );
    assert_eq!(&l[10..], ["true", "true", "no face"]);
}

#[test]
fn import_reimport_and_save() {
    let (s, dir, perm) = open("block", "import");
    std::fs::copy(dir.join("block.step"), dir.join("again.step")).unwrap();
    let (o, c) = on(
        r#"
        let m = fluut.project.model;
        let b = m.import_file("again.step");
        print(b.len); print(m.sources.len); print(m.sources[1].path);
        let r = m.reimport("g1");
        print(r.matched > 0); print(r.unmatched);
        try { m.import_file("nope.step") } catch (e) { print("missing") }
        try { m.import_file("x.obj") } catch (e) { print(e) }
        try { m.import_file("../../../../etc/hosts.stl", "mm") } catch (e) { print(e) }
        fluut.project.save_as("sub/copy.fluut");
        print(fluut.project.path.ends_with("copy.fluut"));
        print(m.sources[0].path);
        fluut.project.save();
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(&l[..3], ["1", "2", "again.step"]);
    assert_eq!(&l[3..6], ["true", "[]", "missing"]);
    assert!(l[6].contains("not a STEP"), "{}", l[6]);
    assert!(l[7].contains("permission denied"), "{}", l[7]);
    assert_eq!(&l[8..], ["true", "../block.step"]);
    let saved = std::fs::read_to_string(dir.join("sub/copy.fluut")).unwrap();
    assert!(saved.contains("path = \"../block.step\""), "{saved}");
    assert!(saved.contains("path = \"../again.step\""), "{saved}");
    // The copy opens, with its geometry.
    let s = fluut_core::Session::open(&dir.join("sub/copy.fluut"), &[]).unwrap();
    assert!(s.warnings.iter().all(|w| !w.is_error()), "{:?}", s.warnings);
}

#[test]
fn new_project_open_and_csv() {
    let d = tmp("open_csv");
    let block = reference("block", "open_csv_ref");
    std::fs::write(d.join("holes.csv"), "x,y,name\n1,2.5,\"a, b\"\n3,,c\n").unwrap();
    let perm = fluut_script::Permissions {
        project_dir: Some(d.clone()),
        allowed_paths: vec![block.clone()],
        ..Default::default()
    };
    let c = Arc::new(Console::new());
    let src = format!(
        r#"
        print(fluut.has_project);
        let rows = fluut.read_csv("holes.csv");
        print(rows);
        let p = fluut.new_project("inch");
        print(p.units); print(p.setups.len); print(p.path);
        try {{ p.model.import_file("x.step") }} catch (e) {{ print("save first") }}
        let q = fluut.open({:?});
        print(q.name);
        try {{ p.name }} catch (e) {{ print("closed") }}
        try {{ fluut.read_csv("/etc/passwd") }} catch (e) {{ print("denied") }}
        try {{ fluut.open("/etc/passwd") }} catch (e) {{ print("denied") }}
        "#,
        block.join("block.fluut").to_string_lossy()
    );
    let o = run(&src, None, opts(perm, Default::default()), c.clone());
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(l[0], "false");
    assert_eq!(
        l[1],
        r#"[#{"name": "a, b", "x": 1.0, "y": 2.5}, #{"name": "c", "x": 3.0, "y": ()}]"#
    );
    assert_eq!(
        &l[2..],
        [
            "inch",
            "0",
            "",
            "save first",
            "Block",
            "closed",
            "denied",
            "denied"
        ]
    );
    assert_eq!(o.session.unwrap().project.name, "Block");
}

#[test]
fn templates() {
    let (s, dir, perm) = open("block", "templates");
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(
        dir.join("templates/rough.template.toml"),
        r#"format  = "fluut.template/1"
id      = "project/rough"
version = "1.0"
kind    = "toolpath"

[toolpath]
type = "pocket_2d"

[toolpath.passes]
stepover = "0.4*tool.diameter"
"#,
    )
    .unwrap();
    let s = fluut_core::Session::open(&s.path.clone().unwrap(), &[]).unwrap();
    let (o, c) = on(
        r#"
        print(fluut.library.templates);
        let t = fluut.project.setups[0].apply_template("project/rough", #{ tool: "shop#flat-6", name: "R" });
        print(t.name); print(t.type); print(t.passes.stepover); print(t.template);
        try { fluut.project.apply_template("project/rough") } catch (e) { print("not a setup template") }
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(l[0], r#"["project/rough@1.0"]"#);
    assert_eq!(&l[1..3], ["R", "pocket_2d"]);
    assert!(
        (l[3].parse::<f64>().unwrap() - 2.4).abs() < 1e-9,
        "{}",
        l[3]
    );
    assert!(l[4].starts_with("project/rough"), "{}", l[4]);
    assert_eq!(l[5], "not a setup template");
}
