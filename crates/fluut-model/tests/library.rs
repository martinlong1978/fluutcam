// SPDX-License-Identifier: MIT OR Apache-2.0

//! The definition library (D06 §4, §8): layers, versions, machines and
//! tool libraries.

use fluut_model::defs::{DefError, DefRef, DefWarning, LayerId};
use fluut_model::library::{DefKind, LayerKind, Library};
use fluut_model::machine::{BUILTIN_MACHINES, ChangerKind, Direction};

fn r(s: &str) -> DefRef {
    DefRef::parse(s).unwrap()
}

fn machine(id: &str, version: &str, extra: &str) -> String {
    format!("format = \"fluut.machine/1\"\nid = \"{id}\"\nversion = \"{version}\"\n{extra}\n")
}

fn builtin() -> Library {
    let mut l = Library::new();
    for (id, text) in BUILTIN_MACHINES {
        l.add(DefKind::Machine, LayerKind::BuiltIn, id, text)
            .unwrap();
    }
    l
}

#[test]
fn builtin_machine_loads() {
    let l = builtin();
    let m = l.machine(&r("fluut/generic-3axis-router@1")).unwrap();
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    assert_eq!(m.fields.name, "Generic 3-axis router");
    assert_eq!(m.fields.post.as_deref(), Some("fluut/gcode@1"));
    assert_eq!(m.fields.travel.z, Some([-150.0, 0.0]));
    assert_eq!(m.fields.tool_changer.kind, ChangerKind::Manual);
    assert!(!m.has_rotary());
    let lim = m.limits().unwrap();
    assert_eq!(lim.rpm, (6000.0, 24000.0));
    assert_eq!(lim.max_cut_feed, 10000.0);
}

#[test]
fn spec_machine_extends_and_overrides() {
    // Spec 02 §2.3.
    let mut l = builtin();
    let text = r#"format  = "fluut.machine/1"
id      = "user/my-router"
version = "2.0"
name    = "My router"
extends = "fluut/generic-3axis-router@1"
post    = "user/my-router@3"

[travel]
x = [0, 1220]
y = [0, 2440]
z = [-150, 0]

[spindle]
rpm   = [6000, 24000]
ranges = []

[feed]
max_cut   = 10000
max_rapid = 15000

[[axis.rotary]]
name        = "A"
parallel_to = "X"
centre      = [0, 600, -75]
continuous  = true
direction   = "right_hand"
clamp       = "M10"
unclamp     = "M11"
limits_deg  = []

[tool_changer]
kind   = "atc"
slots  = 8

[post_overrides]
"words.F.decimals" = 0
"#;
    l.add(DefKind::Machine, LayerKind::User, "my-router", text)
        .unwrap();
    let m = l.machine(&r("user/my-router@2")).unwrap();
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    assert_eq!(m.fields.travel.x, Some([0.0, 1220.0]));
    assert_eq!(m.fields.tool_changer.kind, ChangerKind::Atc);
    assert_eq!(m.fields.tool_changer.slots, Some(8));
    let a = &m.fields.axis.rotary[0];
    assert_eq!((a.name.as_str(), a.continuous), ("A", true));
    assert_eq!(a.direction, Direction::RightHand);
    assert_eq!(a.clamp.as_deref(), Some("M10"));
    assert_eq!(
        m.fields.post_overrides["words.F.decimals"].as_integer(),
        Some(0)
    );
    // Provenance: own keys from the user layer, the rest from the parent.
    let p = &m.resolved.provenance;
    assert_eq!(p["travel.x"], LayerId("user/my-router@2.0".into()));
    assert_eq!(p["name"], LayerId("user/my-router@2.0".into()));
    assert_eq!(
        m.resolved.layers,
        [
            LayerId("fluut/generic-3axis-router@1.0".into()),
            LayerId("user/my-router@2.0".into())
        ]
    );
    assert_eq!(m.layer().unwrap().0, "user/my-router@2.0");
}

#[test]
fn machine_errors_and_warnings() {
    let mut l = Library::new();
    l.add(
        DefKind::Machine,
        LayerKind::User,
        "a",
        &machine(
            "user/a",
            "1.0",
            "colour = \"red\"\n[feed]\nmax_cut = \"fast\"",
        ),
    )
    .unwrap();
    let e = l.machine(&r("user/a")).unwrap_err();
    assert!(matches!(e, DefError::Merge { .. }), "{e:?}");
    l.add(
        DefKind::Machine,
        LayerKind::User,
        "b",
        &machine("user/b", "1.0", "colour = \"red\""),
    )
    .unwrap();
    let m = l.machine(&r("user/b")).unwrap();
    assert_eq!(m.warnings, ["unknown key `colour` ignored"]);
    assert!(m.limits().is_none());
    // A post file is not a machine.
    let e = l
        .add(
            DefKind::Machine,
            LayerKind::User,
            "p",
            "format = \"fluut.post/1\"\nid = \"user/p\"\nversion = \"1.0\"",
        )
        .unwrap_err();
    assert_eq!(e.code, "def.format");
    let e = l
        .add(DefKind::Machine, LayerKind::User, "x", "id = \"user/x\"")
        .unwrap_err();
    assert_eq!(e.code, "def.load");
}

#[test]
fn cycles_missing_parents_and_mixins() {
    let mut l = Library::new();
    for (id, extra) in [
        ("user/a", "extends = \"user/b\""),
        ("user/b", "extends = \"user/a\""),
        ("user/orphan", "extends = \"user/gone@2\""),
        ("user/m1", "[feed]\nmax_cut = 1"),
        ("user/m2", "[feed]\nmax_cut = 2"),
        ("user/mixed", "mixins = [\"user/m1\", \"user/m2\"]"),
    ] {
        l.add(
            DefKind::Machine,
            LayerKind::User,
            id,
            &machine(id, "1.0", extra),
        )
        .unwrap();
    }
    assert!(matches!(
        l.machine(&r("user/a")).unwrap_err(),
        DefError::Cycle(_)
    ));
    assert_eq!(
        l.machine(&r("user/orphan")).unwrap_err(),
        DefError::NotFound("user/gone@2".into())
    );
    let m = l.machine(&r("user/mixed")).unwrap();
    assert_eq!(m.fields.feed.max_cut, Some(2.0));
    assert_eq!(
        m.resolved.warnings,
        [DefWarning::MixinConflict {
            path: "feed.max_cut".into(),
            earlier: LayerId("user/m1@1.0".into()),
            later: LayerId("user/m2@1.0".into()),
        }]
    );
    assert_eq!(m.resolved.provenance["feed.max_cut"].0, "user/m2@1.0");
}

#[test]
fn versions_and_layers() {
    let mut l = Library::new();
    for (layer, v, name) in [
        (LayerKind::User, "3.1", "user 3.1"),
        (LayerKind::User, "3.2", "user 3.2"),
        (LayerKind::User, "4.0", "user 4.0"),
        (LayerKind::Project, "3.2", "project 3.2"),
    ] {
        l.add(
            DefKind::Machine,
            layer,
            name,
            &machine("user/mill", v, &format!("name = \"{name}\"")),
        )
        .unwrap();
    }
    let name = |s: &str| l.machine(&r(s)).unwrap().fields.name;
    assert_eq!(name("user/mill"), "user 4.0");
    // The project layer wins a tie.
    assert_eq!(name("user/mill@3"), "project 3.2");
    assert_eq!(name("user/mill@3.2"), "project 3.2");
    assert_eq!(name("user/mill@3.1"), "user 3.1");
    assert!(l.machine(&r("user/mill@5")).is_err());
    let e = l.find(DefKind::Machine, &r("user/mill@3")).unwrap();
    assert_eq!(e.layer, LayerKind::Project);
    assert_eq!(e.source, "project 3.2");
    // Kinds are separate: a post may share a machine's id.
    assert!(l.find(DefKind::Post, &r("user/mill")).is_none());
    // The standalone store resolves the same way.
    let s = l.mem_store(DefKind::Machine);
    use fluut_model::defs::DefStore;
    let d = s.lookup(&r("user/mill@3")).unwrap();
    assert_eq!(d.table["name"].as_str(), Some("project 3.2"));
}

#[test]
fn fluut_namespace_is_built_in_only() {
    let mut l = Library::new();
    for layer in [LayerKind::User, LayerKind::Project] {
        let e = l
            .add(
                DefKind::Machine,
                layer,
                "x",
                &machine("fluut/generic-3axis-router", "9.0", ""),
            )
            .unwrap_err();
        assert_eq!(e.code, "def.namespace.reserved");
    }
    let lib =
        fluut_model::tools::ToolLibrary::parse("format = \"fluut.tools/1\"\nid = \"fluut/bits\"")
            .unwrap()
            .0;
    assert_eq!(
        l.add_tools(LayerKind::User, lib).unwrap_err().code,
        "def.namespace.reserved"
    );
}

#[test]
fn directories_load_in_layers() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("library-dirs");
    let _ = std::fs::remove_dir_all(&root);
    let (user, proj) = (root.join("user"), root.join("project"));
    for d in [&user, &proj] {
        for sub in ["machines", "tools", "posts"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
    }
    std::fs::write(
        user.join("machines/mill.machine.toml"),
        machine("user/mill", "1.0", "name = \"user\""),
    )
    .unwrap();
    std::fs::write(
        proj.join("machines/mill.machine.toml"),
        machine("user/mill", "1.0", "name = \"project\""),
    )
    .unwrap();
    std::fs::write(proj.join("machines/notes.txt"), "not a machine").unwrap();
    std::fs::write(proj.join("machines/bad.machine.toml"), "id = ").unwrap();
    let tools = |d: f64| {
        format!(
            "format = \"fluut.tools/1\"\nid = \"shop\"\n[[tool]]\nid = \"t\"\nshape = \"flat\"\ndiameter = {d:?}\n"
        )
    };
    std::fs::write(user.join("tools/shop.toml"), tools(3.0)).unwrap();
    std::fs::write(proj.join("tools/shop.toml"), tools(6.0)).unwrap();
    let mut l = Library::new();
    assert!(l.load_dir(&user, LayerKind::User).is_empty());
    let d = l.load_dir(&proj, LayerKind::Project);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "def.load");
    assert_eq!(l.machine(&r("user/mill")).unwrap().fields.name, "project");
    assert_eq!(l.tools().get("shop#t").unwrap().diameter, 6.0);
    // A missing directory is not an error.
    assert!(l.load_dir(&root.join("none"), LayerKind::User).is_empty());
}
