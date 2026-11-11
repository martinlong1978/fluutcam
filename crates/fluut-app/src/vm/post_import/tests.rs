// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_model::DefRef;
use fluut_post::{CompiledPost, Post, resolve_post};
use std::time::{Duration, Instant};

/// The importer's own hand-written fixtures (never a vendor file).
pub(crate) fn fixture_named(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-post-import/tests/fixtures/vectric")
        .join(name)
}

pub(crate) fn fixture() -> PathBuf {
    fixture_named("grbl-like.pp")
}

pub(crate) fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "fluut-app-postimport-{}-{name}",
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
fn every_fixture_imports_into_a_post_that_loads_and_compiles() {
    let d = temp("ok");
    for name in [
        "grbl-like.pp",
        "linuxcnc-like.pp",
        "comma-inch.pp",
        "wrap-y.pp",
    ] {
        let out = d.join(name.replace(".pp", ".post.toml"));
        let r = run(&fixture_named(name), &out, false);
        assert!(r.ok(), "{name}: {:?}", r.diagnostics);
        let id = format!("vectric/{}", name.trim_end_matches(".pp"));
        assert_eq!(r.post_id, id);
        let mut store = fluut_model::MemStore::new();
        store
            .insert_toml(&std::fs::read_to_string(&out).unwrap())
            .unwrap();
        let resolved = resolve_post(&store, &DefRef::parse(&id).unwrap(), None).unwrap();
        let loaded = Post::load(&resolved).unwrap();
        CompiledPost::compile(loaded).unwrap();
    }
}

#[test]
fn the_result_text_has_the_id_the_counts_and_the_warnings() {
    let d = temp("text");
    let out = d.join("a.post.toml");
    let r = run(&fixture(), &out, false);
    assert!(r.ok());
    let rep = r.report.as_ref().unwrap();
    assert!(rep.blocks_mapped > 0 && rep.words_written > 0);
    let counts = r.counts();
    assert_eq!(counts[0], "Post id: vectric/grbl-like");
    assert!(
        counts[1].starts_with("Post name: FluutCAM test"),
        "{counts:?}"
    );
    assert_eq!(
        counts[2],
        format!(
            "Settings mapped: {} of {}",
            rep.settings_mapped, rep.settings_read
        )
    );
    assert_eq!(
        counts[3],
        format!(
            "Blocks mapped: {} of {}",
            rep.blocks_mapped, rep.blocks_read
        )
    );
    assert_eq!(counts[4], format!("Words written: {}", rep.words_written));
    // No base file: no line for it.
    assert!(counts.iter().all(|l| !l.starts_with("Base files")));
    let warnings = r.messages().len();
    assert_eq!(counts.last().unwrap(), &format!("Warnings: {warnings}"));
    assert!(r.messages().iter().all(|(error, _, _)| !error));
    assert!(
        r.messages()
            .iter()
            .all(|(_, c, _)| c.starts_with("import."))
    );
    assert!(r.headline().starts_with("Wrote post vectric/grbl-like to "));
}

#[test]
fn a_post_base_file_beside_the_input_is_read() {
    let d = temp("base");
    std::fs::copy(fixture(), d.join("base.pp")).unwrap();
    let child = d.join("child.pp");
    std::fs::write(
        &child,
        "POST_NAME = \"Child\"\nPOST_BASE = \"base.pp\"\nbegin FOOTER\n\"M2\"\n",
    )
    .unwrap();
    let r = run(&child, &d.join("child.post.toml"), false);
    assert!(r.ok(), "{:?}", r.diagnostics);
    assert_eq!(r.report.as_ref().unwrap().base_files, 1);
    assert!(r.counts().contains(&"Base files merged: 1".to_owned()));
    // Without the base beside it the import is refused and writes nothing.
    let lone = temp("base-missing");
    let child2 = lone.join("child.pp");
    std::fs::copy(&child, &child2).unwrap();
    let out = lone.join("child.post.toml");
    let r = run(&child2, &out, false);
    assert!(!r.ok());
    assert_eq!(codes(&r), ["import.pp.post_base"]);
    assert!(!out.exists());
}

#[test]
fn an_existing_file_is_kept_unless_the_dialog_confirmed_replacing_it() {
    let d = temp("exists");
    let out = d.join("p.post.toml");
    std::fs::write(&out, "keep").unwrap();
    let r = run(&fixture(), &out, false);
    assert!(!r.ok());
    assert_eq!(codes(&r), ["import.output.exists"]);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep");
    assert!(run(&fixture(), &out, true).ok());
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("fluut.post/1")
    );
}

#[test]
fn the_post_never_replaces_its_own_source() {
    let d = temp("same");
    let src = d.join("post.pp");
    std::fs::copy(fixture(), &src).unwrap();
    let r = run(&src, &src, true);
    assert_eq!(codes(&r), ["import.output.is_input"]);
    assert_eq!(
        std::fs::read(&src).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

#[test]
fn cps_and_other_formats_are_unsupported_and_write_nothing() {
    let d = temp("formats");
    for name in ["post.cps", "POST.CPS", "post.txt", "post"] {
        let src = d.join(name);
        std::fs::write(&src, "x").unwrap();
        let out = d.join(format!("{name}.post.toml"));
        let r = run(&src, &out, false);
        assert_eq!(codes(&r), ["import.format.unsupported"], "{name}");
        assert!(!out.exists());
    }
    let e = check_format(Path::new("a.cps")).unwrap_err();
    assert!(e.message.starts_with("Autodesk .cps posts are programs"));
    assert!(check_format(Path::new("a.PP")).is_ok());
}

#[test]
fn bad_input_is_reported_and_writes_nothing() {
    let d = temp("bad");
    let src = d.join("broken.pp");
    std::fs::write(&src, "just some text\n").unwrap();
    let out = d.join("broken.post.toml");
    let r = run(&src, &out, false);
    assert!(!r.ok() && r.messages().iter().all(|(error, _, _)| *error));
    assert_eq!(codes(&r), ["import.file.invalid"]);
    assert!(!out.exists());
    assert_eq!(r.headline(), "Nothing was imported.");
    assert!(r.counts().is_empty());
    // Missing file.
    let missing = run(&d.join("absent.pp"), &out, false);
    assert_eq!(codes(&missing), ["import.file.unreadable"]);
    assert!(!out.exists());
    // Too large: refused without being read whole.
    let big = d.join("big.pp");
    std::fs::write(&big, "+".repeat(limits::MAX_POST_BYTES as usize + 1)).unwrap();
    assert_eq!(codes(&run(&big, &out, false)), ["import.limit.size"]);
    assert!(!out.exists());
}

#[test]
fn an_unwritable_target_is_reported() {
    let d = temp("unwritable");
    let r = run(&fixture(), &d.join("no/such/folder/a.post.toml"), false);
    assert_eq!(codes(&r), ["import.output.unwritable"]);
}

#[test]
fn names_follow_the_input_and_the_project() {
    assert_eq!(
        default_name(Path::new("/x/My Post.pp")),
        "My Post.post.toml"
    );
    assert_eq!(
        with_extension(PathBuf::from("a")),
        PathBuf::from("a.post.toml")
    );
    assert_eq!(
        with_extension(PathBuf::from("a.toml")),
        PathBuf::from("a.toml")
    );
    let d = temp("start");
    let project = d.join("p.fluut");
    assert_eq!(start_dir(Some(&project)), Some(d.clone()));
    std::fs::create_dir(d.join("posts")).unwrap();
    assert_eq!(start_dir(Some(&project)), Some(d.join("posts")));
    assert_eq!(start_dir(None), None);
    assert!(loads_with_project(&project, &d.join("posts/x.post.toml")));
    assert!(!loads_with_project(&project, &d.join("posts/x.toml")));
    assert!(!loads_with_project(&project, &d.join("x.post.toml")));
    assert!(!loads_with_project(
        &project,
        &d.join("posts/sub/x.post.toml")
    ));
}

fn finish(t: &mut PostImport) {
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
    let mut t = PostImport::default();
    assert!(t.is_free() && t.done().is_none());
    assert_eq!(
        t.source_picked(fixture()).as_deref(),
        Some("grbl-like.post.toml")
    );
    assert!(t.awaiting_target() && !t.is_free());
    t.target_picked(d.join("mine"), false, || {});
    assert!(t.is_running() || t.done().is_some());
    finish(&mut t);
    let done = t.done().unwrap();
    assert!(done.ok());
    // The extension was added.
    assert_eq!(done.target, d.join("mine.post.toml"));
    assert!(d.join("mine.post.toml").is_file());
    assert!(!done.used);
    t.mark_used();
    assert!(t.done().unwrap().used);
    t.close();
    assert!(t.is_free() && t.done().is_none());
}

#[test]
fn cancelling_the_save_dialog_ends_the_import() {
    let mut t = PostImport::default();
    t.source_picked(fixture());
    t.cancel();
    assert!(matches!(t.phase, Phase::Idle));
    // Cancel does not close a result.
    t.source_picked(PathBuf::from("x.cps"));
    t.cancel();
    assert!(t.done().is_some());
}

#[test]
fn a_cps_choice_goes_straight_to_the_error() {
    let mut t = PostImport::default();
    assert_eq!(t.source_picked(PathBuf::from("shop.cps")), None);
    let done = t.done().unwrap();
    assert!(!done.ok());
    assert_eq!(done.diagnostics[0].code, "import.format.unsupported");
}
