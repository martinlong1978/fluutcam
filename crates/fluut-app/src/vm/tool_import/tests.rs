// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_model::tools::ToolLibrary;
use std::time::{Duration, Instant};

pub(crate) fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-post-import/tests/fixtures/fusion-sample.json")
}

pub(crate) fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "fluut-app-toolimport-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn codes(d: &Done) -> Vec<&str> {
    d.diagnostics.iter().map(|x| x.code.as_str()).collect()
}

#[test]
fn the_sample_imports_into_a_loadable_library() {
    let d = temp("ok");
    let out = d.join("shop.toml");
    let r = run(&fixture(), &out, false);
    assert!(r.ok(), "{:?}", r.diagnostics);
    assert_eq!(r.library_id, "fusion/fusion-sample");
    let rep = r.report.as_ref().unwrap();
    assert_eq!(
        (rep.tools_read, rep.tools_written, rep.tools_skipped),
        (9, 7, 2)
    );
    assert!(codes(&r).contains(&"import.tool.unsupported_type"));
    let (lib, w) = ToolLibrary::parse(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert!(w.is_empty());
    assert_eq!(lib.tools.len(), 7);
}

#[test]
fn the_result_text_has_the_counts_and_the_dropped_keys() {
    let d = temp("text");
    let r = run(&fixture(), &d.join("a.toml"), false);
    let counts = r.counts();
    assert!(counts.iter().any(|l| l.contains("fusion/fusion-sample")));
    assert!(counts.contains(&"Tools read: 9".to_owned()), "{counts:?}");
    assert!(counts.contains(&"Tools written: 7".to_owned()));
    assert!(counts.contains(&"Tools skipped: 2".to_owned()));
    assert!(counts.iter().any(|l| l.starts_with("Source keys not used")));
    assert!(!r.dropped().is_empty());
    assert!(r.headline().starts_with("Wrote 7 of 9 tools to "));
    assert!(r.messages().iter().all(|(error, _, _)| !error));
}

#[test]
fn an_existing_file_is_kept_unless_the_dialog_confirmed_replacing_it() {
    let d = temp("exists");
    let out = d.join("lib.toml");
    std::fs::write(&out, "keep").unwrap();
    let r = run(&fixture(), &out, false);
    assert!(!r.ok());
    assert_eq!(codes(&r), ["import.output.exists"]);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep");
    assert!(run(&fixture(), &out, true).ok());
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("fluut.tools/1")
    );
}

#[test]
fn the_library_never_replaces_its_own_source() {
    let d = temp("same");
    let src = d.join("tools.json");
    std::fs::copy(fixture(), &src).unwrap();
    let r = run(&src, &src, true);
    assert_eq!(codes(&r), ["import.output.is_input"]);
    assert_eq!(
        std::fs::read(&src).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

#[test]
fn vtdb_and_other_formats_are_unsupported_and_write_nothing() {
    let d = temp("formats");
    for name in ["db.vtdb", "DB.VTDB", "tools.txt", "tools"] {
        let src = d.join(name);
        std::fs::write(&src, "x").unwrap();
        let out = d.join(format!("{name}.toml"));
        let r = run(&src, &out, false);
        assert_eq!(codes(&r), ["import.format.unsupported"], "{name}");
        assert!(!out.exists());
    }
    let e = format_of(Path::new("a.vtdb")).unwrap_err();
    assert!(e.message.contains(".vtdb"));
    assert_eq!(format_of(Path::new("a.JSON")), Ok(Format::FusionJson));
}

#[test]
fn bad_input_is_reported_and_writes_nothing() {
    let d = temp("bad");
    let src = d.join("broken.json");
    std::fs::write(&src, "{ not json").unwrap();
    let out = d.join("broken.toml");
    let r = run(&src, &out, false);
    assert!(!r.ok() && r.messages().iter().all(|(error, _, _)| *error));
    assert!(!out.exists());
    assert_eq!(r.headline(), "Nothing was imported.");
    assert!(r.counts().is_empty());
    // Missing file.
    let r = run(&d.join("absent.json"), &out, false);
    assert_eq!(codes(&r), ["import.file.unreadable"]);
    assert!(!out.exists());
}

#[test]
fn an_unwritable_target_is_reported() {
    let d = temp("unwritable");
    let r = run(&fixture(), &d.join("no/such/folder/a.toml"), false);
    assert_eq!(codes(&r), ["import.output.unwritable"]);
}

#[test]
fn names_follow_the_input_and_the_project() {
    assert_eq!(default_name(Path::new("/x/My Tools.json")), "My Tools.toml");
    assert_eq!(with_extension(PathBuf::from("a")), PathBuf::from("a.toml"));
    assert_eq!(
        with_extension(PathBuf::from("a.lib")),
        PathBuf::from("a.lib")
    );
    let d = temp("start");
    let project = d.join("p.fluut");
    assert_eq!(start_dir(Some(&project)), Some(d.clone()));
    std::fs::create_dir(d.join("tools")).unwrap();
    assert_eq!(start_dir(Some(&project)), Some(d.join("tools")));
    assert_eq!(start_dir(None), None);
    assert!(loads_with_project(&project, &d.join("tools/x.toml")));
    assert!(!loads_with_project(&project, &d.join("tools/x.json")));
    assert!(!loads_with_project(&project, &d.join("x.toml")));
    assert!(!loads_with_project(&project, &d.join("tools/sub/x.toml")));
}

fn finish(t: &mut ToolImport) {
    let end = Instant::now() + Duration::from_secs(20);
    while t.is_running() {
        assert!(Instant::now() < end, "import did not finish");
        t.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_phases_run_in_order_on_a_worker() {
    let d = temp("phases");
    let mut t = ToolImport::default();
    assert!(t.is_free() && t.done().is_none());
    assert_eq!(
        t.source_picked(fixture()).as_deref(),
        Some("fusion-sample.toml")
    );
    assert!(t.awaiting_target() && !t.is_free());
    t.target_picked(d.join("lib"), false, || {});
    assert!(t.is_running() || t.done().is_some());
    finish(&mut t);
    let done = t.done().unwrap();
    assert!(done.ok());
    // The extension was added.
    assert_eq!(done.target, d.join("lib.toml"));
    assert!(d.join("lib.toml").is_file());
    t.close();
    assert!(t.is_free() && t.done().is_none());
}

#[test]
fn cancelling_the_save_dialog_ends_the_import() {
    let mut t = ToolImport::default();
    t.source_picked(fixture());
    t.cancel();
    assert!(matches!(t.phase, Phase::Idle));
    // Cancel does not close a result.
    t.source_picked(PathBuf::from("x.vtdb"));
    t.cancel();
    assert!(t.done().is_some());
}

#[test]
fn a_vtdb_choice_goes_straight_to_the_error() {
    let mut t = ToolImport::default();
    assert_eq!(t.source_picked(PathBuf::from("shop.vtdb")), None);
    let done = t.done().unwrap();
    assert!(!done.ok());
    assert_eq!(done.diagnostics[0].code, "import.format.unsupported");
}
