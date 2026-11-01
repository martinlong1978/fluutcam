// SPDX-License-Identifier: MIT OR Apache-2.0

//! One run is one undo step (spec 09 §9.2), permission denials, and the
//! app's dialogs through the host.

mod support;

use fluut_model::project::write;
use fluut_script::{
    Console, ErrorKind, Level, Permissions, ScriptHost, UiRequest, UiResponse, run,
};
use std::sync::{Arc, Mutex};
use support::*;

const EDITS: &str = r#"
    let p = fluut.project;
    p.name = "Changed";
    let t = p.toolpath("t1");
    t.passes.stepover = 1.0;
    t.linking.ramp.angle_deg = 4;
    let s = p.add_setup(#{ name: "Second", based_on: "s1" });
    s.add_toolpath("contour_2d", #{ tool: "shop#flat-6" });
    p.toolpath("t3").remove();
"#;

#[test]
fn one_run_is_one_undo_step() {
    let (mut s, _, perm) = open("block", "undo_one");
    s.apply(fluut_model::edit::Edit::SetName("Before".into()))
        .unwrap();
    let before = write(&s.project);
    let (o, c) = on(EDITS, s, perm);
    ok(&o, &c);
    let undo = o.undo.unwrap();
    assert_eq!(undo.label, "Script test.rhai");
    assert!(undo.changed.contains("t1") && undo.changed.contains("t3"));
    let mut s = o.session.unwrap();
    let after = write(&s.project);
    assert_ne!(before, after);
    assert_eq!(s.undo().unwrap().label, "Script test.rhai");
    assert_eq!(write(&s.project), before);
    s.redo().unwrap();
    assert_eq!(write(&s.project), after);
    // The edit before the run is still its own step.
    s.undo().unwrap();
    assert_eq!(s.undo().unwrap().label, "Rename project");
}

#[test]
fn a_failed_run_keeps_its_edits_as_one_step() {
    let (s, _, perm) = open("block", "undo_failed");
    let before = write(&s.project);
    let (o, _) = on(&format!("{EDITS}\nthrow \"half way\";"), s, perm);
    assert_eq!(kind(&o), Some(ErrorKind::Runtime));
    assert!(o.undo.is_some());
    let mut s = o.session.unwrap();
    assert_ne!(write(&s.project), before);
    s.undo().unwrap();
    assert_eq!(write(&s.project), before);
    assert!(s.undo().is_none());
}

#[test]
fn a_run_without_edits_leaves_no_step() {
    let (s, _, perm) = open("block", "undo_none");
    let (o, c) = on(
        "print(fluut.project.name); try { fluut.project.setup(\"x\") } catch {}",
        s,
        perm,
    );
    ok(&o, &c);
    assert!(o.undo.is_none());
    assert!(!o.session.unwrap().undo().is_some());
}

#[test]
fn permission_denials() {
    let (s, dir, perm) = open("block", "denials");
    let outside = dir.parent().unwrap().join("outside.csv");
    std::fs::write(&outside, "a\n1\n").unwrap();
    let o_path = outside.to_string_lossy().into_owned();
    let script = format!(
        r#"
        let tries = [
            || fluut.read_csv({o_path:?}),
            || fluut.project.save_as("../elsewhere.fluut"),
            || fluut.project.post("all", "../out.ngc", #{{ post: "fluut/linuxcnc" }}),
            || fluut.project.model.import_file("../outside.step"),
            || fluut.open("/etc/passwd"),
        ];
        for f in tries {{
            try {{ f.call(); print("allowed") }} catch (e) {{ print(e) }}
        }}
        "#
    );
    let (o, c) = on(&script, s, perm);
    ok(&o, &c);
    let lines = c.lines();
    assert_eq!(lines.len(), 5);
    for (_, l) in &lines {
        assert!(l.contains("permission denied"), "{l}");
    }
    assert!(!dir.parent().unwrap().join("elsewhere.fluut").exists());
    assert!(!dir.parent().unwrap().join("out.ngc").exists());
    // The same file given as an allowed path is readable.
    let (s, _, mut perm) = open("block", "denials_allowed");
    perm.allowed_paths.push(outside);
    let (o, c) = on(&format!("print(fluut.read_csv({o_path:?}))"), s, perm);
    ok(&o, &c);
    assert_eq!(c.text(), "[#{\"a\": 1.0}]\n");
}

/// A host that answers dialogs from a list and records them.
#[derive(Default)]
struct App {
    asked: Mutex<Vec<UiRequest>>,
    answers: Mutex<Vec<Result<UiResponse, String>>>,
    console: Console,
}

impl ScriptHost for App {
    fn log(&self, level: Level, msg: &str) {
        self.console.log(level, msg);
    }
    fn ui_request(&self, req: UiRequest) -> Result<UiResponse, String> {
        self.asked.lock().unwrap().push(req);
        self.answers.lock().unwrap().remove(0)
    }
}

#[test]
fn app_dialogs() {
    let (s, dir, perm) = open("block", "dialogs");
    let picked = dir.parent().unwrap().join("picked.csv");
    std::fs::write(&picked, "n\n7\n").unwrap();
    let app = Arc::new(App::default());
    *app.answers.lock().unwrap() = vec![
        Ok(UiResponse::Done),
        Ok(UiResponse::Number(2.5)),
        Ok(UiResponse::Choice("b".into())),
        Ok(UiResponse::Faces(vec!["f-494.10".into()])),
        Ok(UiResponse::File(picked.clone())),
        Err("cancelled".into()),
    ];
    let perm = Permissions { ui: true, ..perm };
    let src = r#"
        let ui = fluut.ui;
        ui.message("hello");
        print(ui.ask_number("Depth", 3, "mm"));
        print(ui.ask_choice("Which", ["a", "b"]));
        print(ui.pick_faces("Pick"));
        let f = ui.pick_file("*.csv");
        print(fluut.read_csv(f));
        try { ui.message("again") } catch (e) { print(e) }
        try { fluut.open("x.fluut") } catch (e) { print("headless only") }
    "#;
    let o = run(src, Some(s), opts(perm, Default::default()), app.clone());
    ok(&o, &app.console);
    assert_eq!(
        app.console.text(),
        "2.5\nb\n[\"f-494.10\"]\n[#{\"n\": 7.0}]\nRuntime error: cancelled\nheadless only\n"
            .replace("Runtime error: cancelled", &first_error_line(&app.console))
    );
    let asked = app.asked.lock().unwrap();
    assert_eq!(asked.len(), 6);
    assert_eq!(
        asked[1],
        UiRequest::AskNumber {
            label: "Depth".into(),
            default: 3.0,
            unit: "mm".into()
        }
    );
}

fn first_error_line(c: &Console) -> String {
    c.lines()
        .into_iter()
        .map(|(_, l)| l)
        .find(|l| l.contains("cancelled"))
        .unwrap_or_default()
}
