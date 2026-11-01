// SPDX-License-Identifier: MIT OR Apache-2.0

//! Edits and undo (D06 §6, §8): random edit sequences undo to the original
//! document and redo to the final one; each edit's checks.

use fluut_model::document::{COALESCE_MS, Document};
use fluut_model::edit::{Edit, apply, setup_field_text};
use fluut_model::params::ToolpathTypeId;
use fluut_model::project::{Project, Toolpath, read, write};
use proptest::prelude::*;
use std::sync::Arc;

const BASE: &str = r#"format = "fluut.project/1"
name   = "Undo"
units  = "mm"

[[geometry]]
id   = "g1"
path = "part.step"
kind = "step"

  [[geometry.body]]
  id   = "b-1"
  name = "Part"

[[setup]]
id       = "s1"
name     = "Top"
kind     = "three_axis"
material = "MDF"
stock    = { kind = "block", margin = [5, 5, 1] }

  [[setup.toolpath]]
  id   = "t1"
  name = "Pocket"
  type = "pocket_2d"
  tool = "shop#flat-6"

    [setup.toolpath.geometry]
    drive_faces = [{ face = "f-1.10" }]

    [setup.toolpath.passes]
    stepover = 2.4

  [[setup.toolpath]]
  id   = "t2"
  name = "Outline"
  type = "contour_2d"
  tool = "shop#flat-6"

[[setup]]
id       = "s2"
name     = "Again"
based_on = "s1"
stock    = { kind = "from_previous" }

  [[setup.toolpath]]
  id   = "t3"
  name = "Finish"
  type = "parallel"
  tool = "shop#ball-6"
"#;

fn base() -> Project {
    read(BASE).unwrap().project
}

/// An edit chosen by numbers, turned into a real edit against the current
/// document (ids by index).
#[derive(Clone, Debug)]
enum Intent {
    Name(u8),
    SetupField(u8, u8, u8),
    Param(u8, u8),
    Item(u8, u8),
    AddToolpath(u8, u8),
    RemoveToolpath(u8),
    MoveToolpath(u8, u8, u8),
    AddSetup(u8),
    MoveSetup(u8, u8),
    RemoveSetup(u8),
    Duplicate(u8, u8),
    Group(u8, u8),
    Undo,
    Redo,
}

fn intent() -> impl Strategy<Value = Intent> {
    let n = || any::<u8>();
    prop_oneof![
        n().prop_map(Intent::Name),
        (n(), n(), n()).prop_map(|(a, b, c)| Intent::SetupField(a, b, c)),
        (n(), n()).prop_map(|(a, b)| Intent::Param(a, b)),
        (n(), n()).prop_map(|(a, b)| Intent::Item(a, b)),
        (n(), n()).prop_map(|(a, b)| Intent::AddToolpath(a, b)),
        n().prop_map(Intent::RemoveToolpath),
        (n(), n(), n()).prop_map(|(a, b, c)| Intent::MoveToolpath(a, b, c)),
        n().prop_map(Intent::AddSetup),
        (n(), n()).prop_map(|(a, b)| Intent::MoveSetup(a, b)),
        n().prop_map(Intent::RemoveSetup),
        (n(), n()).prop_map(|(a, b)| Intent::Duplicate(a, b)),
        (n(), n()).prop_map(|(a, b)| Intent::Group(a, b)),
        Just(Intent::Undo),
        Just(Intent::Redo),
    ]
}

const SETUP_VALUES: [(&str, &[Option<&str>]); 9] = [
    ("name", &[Some("\"Renamed\""), None]),
    ("material", &[Some("\"Oak\""), None]),
    ("work_offset", &[Some("\"G55\""), None]),
    (
        "stock",
        &[
            Some("{ kind = \"block\", margin = [1, 2, 3.5] }"),
            Some("{ kind = \"from_previous\" }"),
            None,
        ],
    ),
    ("zero", &[Some("{ method = \"stock_top_centre\" }"), None]),
    ("region", &[Some("{ floor_z = -3.5 }"), None]),
    ("based_on", &[Some("\"s1\""), Some("\"s2\""), None]),
    ("kind", &[Some("\"three_axis\""), Some("\"indexed\""), None]),
    (
        "clearance",
        &[Some("{ kind = \"plane\", offset = 7 }"), None],
    ),
];

const PARAMS: [(&str, Option<&str>); 6] = [
    ("passes.stepover", Some("3.5")),
    ("passes.step_down", Some("2")),
    ("tool_params.feed.cut", Some("1500")),
    ("tool_params.feed", Some("{ cut = 900, plunge = 300 }")),
    ("heights.top", Some("{ ref = \"stock_top\", offset = -1 }")),
    ("passes.stepover", None),
];

fn toolpaths(p: &Project) -> Vec<&Toolpath> {
    p.setups.iter().flat_map(|s| s.toolpaths.iter()).collect()
}

fn pick<T>(v: &[T], i: u8) -> Option<&T> {
    (!v.is_empty()).then(|| &v[i as usize % v.len()])
}

struct Gen {
    next: u32,
}

impl Gen {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("n{}", self.next)
    }

    fn edit(&mut self, p: &Project, i: &Intent) -> Option<Edit> {
        let tps = toolpaths(p);
        Some(match *i {
            Intent::Name(n) => Edit::SetName(format!("P{n}")),
            Intent::SetupField(s, k, v) => {
                let s = pick(&p.setups, s)?;
                let (key, vals) = pick(&SETUP_VALUES, k)?;
                Edit::set_setup_field(&s.id, key, *pick(vals, v)?)
            }
            Intent::Param(t, k) => {
                let t = pick(&tps, t)?;
                let (key, v) = pick(&PARAMS, k)?;
                Edit::set_toolpath_field(&t.id, key, *v)
            }
            Intent::Item(t, k) => {
                let t = pick(&tps, t)?;
                let (key, v) = *pick(
                    &[
                        ("name", Some("\"Named\"")),
                        ("enabled", Some("false")),
                        ("enabled", None),
                        ("tool", Some("\"shop@1#flat-3\"")),
                        ("template", Some("\"user/t@1\"")),
                    ],
                    k,
                )?;
                Edit::set_toolpath_field(&t.id, key, v)
            }
            Intent::AddToolpath(s, at) => {
                let s = pick(&p.setups, s)?;
                let at = at as usize % (s.toolpaths.len() + 1);
                let t = Toolpath::new(&self.id(), "New", ToolpathTypeId::Contour2d, "shop#flat-6");
                Edit::add_toolpath(&s.id, Some(at), &t)
            }
            Intent::RemoveToolpath(t) => Edit::RemoveToolpath(pick(&tps, t)?.id.clone()),
            Intent::MoveToolpath(t, s, to) => {
                let t = pick(&tps, t)?;
                let s = pick(&p.setups, s)?;
                Edit::MoveToolpath {
                    id: t.id.clone(),
                    setup: s.id.clone(),
                    to: to as u32 % 3,
                }
            }
            Intent::AddSetup(at) => {
                let at = at as usize % (p.setups.len() + 1);
                let mut s = fluut_model::project::Setup::new(&self.id(), "Added");
                s.toolpaths.push(Toolpath::new(
                    &self.id(),
                    "In it",
                    ToolpathTypeId::Pocket2d,
                    "shop#x",
                ));
                Edit::add_setup(Some(at), &s)
            }
            Intent::MoveSetup(s, to) => Edit::MoveSetup {
                id: pick(&p.setups, s)?.id.clone(),
                to: to as u32 % (p.setups.len() as u32),
            },
            Intent::RemoveSetup(s) => Edit::RemoveSetup(pick(&p.setups, s)?.id.clone()),
            Intent::Duplicate(t, k) => Edit::DuplicateAs {
                toolpath: pick(&tps, t)?.id.clone(),
                new_id: self.id(),
                new_type: pick(&["contour_2d", "pocket_2d", "parallel"], k)?.to_string(),
            },
            Intent::Group(a, b) => Edit::Group {
                label: "Two".into(),
                edits: vec![
                    self.edit(p, &Intent::Name(a))?,
                    self.edit(p, &Intent::Param(a, b))?,
                ],
            },
            Intent::Undo | Intent::Redo => return None,
        })
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn undo_all_restores_and_redo_all_replays(intents in prop::collection::vec(intent(), 1..40)) {
        let original = base();
        let mut d = Document::new(original.clone());
        let mut g = Gen { next: 0 };
        for i in &intents {
            match i {
                Intent::Undo => { d.undo(); }
                Intent::Redo => { d.redo(); }
                _ => {
                    let Some(e) = g.edit(d.project(), i) else { continue };
                    let before = d.project().clone();
                    match d.apply(e.clone()) {
                        Ok(_) => {
                            // Every state the editor can reach is a file
                            // that reads back the same.
                            let text = write(d.project());
                            let back = read(&text).unwrap();
                            prop_assert!(back.warnings.is_empty(), "{:?}", back.warnings);
                            prop_assert_eq!(&back.project, d.project());
                        }
                        Err(_) => prop_assert_eq!(d.project(), &before),
                    }
                }
            }
        }
        let last = d.project().clone();
        let n = d.undo_len();
        while d.undo().is_some() {}
        prop_assert_eq!(d.project(), &original);
        for _ in 0..n {
            prop_assert!(d.redo().is_some());
        }
        prop_assert_eq!(d.project(), &last);
    }
}

#[test]
fn unchanged_setups_stay_shared() {
    let p = base();
    let (q, changed) = apply(&p, &Edit::set_toolpath_field("t3", "name", Some("\"X\""))).unwrap();
    assert!(Arc::ptr_eq(&p.setups[0], &q.setups[0]));
    assert!(!Arc::ptr_eq(&p.setups[1], &q.setups[1]));
    assert_eq!(changed.into_iter().collect::<Vec<_>>(), ["t3"]);
}

fn err(e: Edit) -> String {
    apply(&base(), &e).unwrap_err().0
}

#[test]
fn edit_errors() {
    for (e, want) in [
        (Edit::SetUnits("feet".into()), "not mm or inch"),
        (
            Edit::set_setup_field("s9", "name", Some("\"x\"")),
            "`s9` is not",
        ),
        (
            Edit::set_setup_field("s1", "colour", Some("1")),
            "not a Setup field",
        ),
        (
            Edit::set_setup_field("s1", "id", Some("\"z\"")),
            "not a Setup field",
        ),
        (
            Edit::set_setup_field("s1", "stock", Some("{ kind = ")),
            "not a TOML value",
        ),
        (
            Edit::set_setup_field("s1", "stock", Some("{ kind = \"slab\" }")),
            "is not one of",
        ),
        (
            Edit::set_setup_field("s1", "stock", Some("{ kind = \"block\", fill = 1 }")),
            "unknown key",
        ),
        (
            Edit::set_setup_field("s1", "based_on", Some("\"s2\"")),
            "based_on",
        ),
        (
            Edit::set_setup_field("s2", "based_on", Some("\"s7\"")),
            "based_on",
        ),
        (
            Edit::set_toolpath_field("t1", "passes.nope", Some("1")),
            "not a field",
        ),
        (
            Edit::set_toolpath_field("t1", "passes.stepover", Some("\"x\"")),
            "expected a number",
        ),
        (
            Edit::set_toolpath_field("t1", "type", Some("\"parallel\"")),
            "fixed at creation",
        ),
        (
            Edit::set_toolpath_field("t9", "name", Some("\"x\"")),
            "`t9` is not",
        ),
        (Edit::RemoveSetup("s1".into()), "based_on"),
        (
            Edit::MoveSetup {
                id: "s2".into(),
                to: 0,
            },
            "based_on",
        ),
        (
            Edit::MoveSetup {
                id: "s2".into(),
                to: 5,
            },
            "past the end",
        ),
        (Edit::RemoveToolpath("t9".into()), "`t9` is not"),
        (Edit::RemoveGeometry("g9".into()), "`g9` is not"),
        (
            Edit::DuplicateAs {
                toolpath: "t1".into(),
                new_id: "t2".into(),
                new_type: "pocket_2d".into(),
            },
            "used twice",
        ),
        (
            Edit::DuplicateAs {
                toolpath: "t1".into(),
                new_id: "t9".into(),
                new_type: "drill".into(),
            },
            "not a toolpath type",
        ),
        (
            Edit::AddSetup {
                at: None,
                text: "[[setup]]\nid = \"t1\"\n".into(),
            },
            "used twice",
        ),
        (
            Edit::AddToolpath {
                setup: "s1".into(),
                at: None,
                text: "x = 1".into(),
            },
            "unknown key",
        ),
    ] {
        let m = err(e.clone());
        assert!(m.contains(want), "{e:?}: {m}");
    }
}

#[test]
fn reset_to_inherited_and_duplicate() {
    let p = base();
    let (q, _) = apply(&p, &Edit::set_setup_field("s1", "material", None)).unwrap();
    assert_eq!(q.setups[0].material, None);
    assert_eq!(
        setup_field_text(&p.setups[0], "stock").as_deref(),
        Some("{ kind = \"block\", margin = [5, 5, 1] }")
    );
    let (q, ch) = apply(
        &p,
        &Edit::DuplicateAs {
            toolpath: "t1".into(),
            new_id: "t9".into(),
            new_type: "contour_2d".into(),
        },
    )
    .unwrap();
    let (_, t) = q.toolpath("t9").unwrap();
    assert_eq!(t.kind(), ToolpathTypeId::Contour2d);
    assert_eq!(q.setups[0].toolpaths[1].id, "t9");
    // Shared fields come along; `drive_faces` is common to both types.
    assert!(t.params.is_set("geometry.drive_faces"));
    assert!(ch.contains("t9"));
}

#[test]
fn remap_rewrites_every_reference() {
    let p = read(&BASE.replace(
        "stock    = { kind = \"block\", margin = [5, 5, 1] }",
        "geometry = { bodies = [\"b-1\"] }\norientation = { tool_axis = { face = \"f-1.10\" }, x_axis = \"+X\" }\nstock    = { kind = \"body\", body = \"b-1\" }",
    ))
    .unwrap()
    .project;
    let e = Edit::RemapIds {
        map: vec![
            ("b-1".into(), "b-7".into()),
            ("f-1.10".into(), "f-7.3".into()),
        ],
    };
    let (q, ch) = apply(&p, &e).unwrap();
    let text = write(&q);
    // The source's body list is updated by `SetGeometry`, not here.
    assert_eq!(text.matches("\"b-1\"").count(), 1, "{text}");
    assert_eq!(text.matches("\"b-7\"").count(), 2, "{text}");
    assert!(!text.contains("f-1.10"), "{text}");
    assert_eq!(text.matches("f-7.3").count(), 2, "{text}");
    assert!(ch.contains("s1") && ch.contains("t1") && !ch.contains("t2"));
    assert!(Arc::ptr_eq(&p.setups[1], &q.setups[1]));
}

#[test]
fn field_edits_coalesce() {
    let mut d = Document::new(base());
    for (i, v) in ["1", "2", "3"].iter().enumerate() {
        d.apply_at(
            Edit::set_toolpath_field("t1", "passes.stepover", Some(v)),
            i as u64 * 500,
        )
        .unwrap();
    }
    assert_eq!(d.undo_len(), 1);
    // Too late, or another key: a new step.
    d.apply_at(
        Edit::set_toolpath_field("t1", "passes.stepover", Some("4")),
        1000 + COALESCE_MS + 1,
    )
    .unwrap();
    d.apply_at(
        Edit::set_toolpath_field("t1", "passes.step_down", Some("4")),
        2300,
    )
    .unwrap();
    // Plain apply never coalesces.
    d.apply(Edit::set_toolpath_field(
        "t1",
        "passes.step_down",
        Some("5"),
    ))
    .unwrap();
    assert_eq!(d.undo_len(), 4);
    for _ in 0..4 {
        d.undo().unwrap();
    }
    assert_eq!(d.project(), &base());
    assert_eq!(d.undo_labels().count(), 0);
}

#[test]
fn the_cap_drops_the_oldest_steps() {
    let mut d = Document::new(base());
    d.set_cap(10_000);
    for i in 0..50 {
        d.apply(Edit::set_setup_field(
            "s1",
            "name",
            Some(&format!("\"N{i}\"")),
        ))
        .unwrap();
    }
    assert!(d.undo_len() < 50 && d.undo_len() >= 1, "{}", d.undo_len());
    while d.undo().is_some() {}
    // The oldest steps are gone: undo stops short of the original.
    assert_ne!(d.project(), &base());
    // A new edit clears redo.
    d.apply(Edit::SetName("x".into())).unwrap();
    assert!(!d.can_redo());
}
