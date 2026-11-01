// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut run`: headless scripts (spec 09 §9.2, D12 §4).

use std::path::{Path, PathBuf};
use std::process::Command;

fn here(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(p)
}

fn fluut(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fluut"))
        .args(args)
        .output()
        .unwrap()
}

/// The `block` reference project copied into a fresh folder.
fn block(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("tools")).unwrap();
    for f in ["block.fluut", "block.step", "tools/shop.toml"] {
        std::fs::copy(here(&format!("projects/block/{f}")), d.join(f)).unwrap();
    }
    d
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn run_edits_saves_and_posts() {
    let d = block("run_job");
    let out = d.parent().unwrap().join("run_job_out.ngc");
    let _ = std::fs::remove_file(&out);
    std::fs::write(
        d.join("job.rhai"),
        r#"
        let p = fluut.project;
        fluut.log(`args: ${fluut.args}`);
        let t2 = p.toolpath("t2");
        t2.enabled = false;
        p.save();
        let r = p.post("all", fluut.args[0], #{ post: "fluut/linuxcnc" });
        print(r.files.len);
        "#,
    )
    .unwrap();
    let o = fluut(&[
        "run",
        d.join("job.rhai").to_str().unwrap(),
        d.join("block.fluut").to_str().unwrap(),
        "--date",
        "2026-11-01",
        "--time",
        "12:00:00",
        "--",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", text(&o.stderr));
    let stdout = text(&o.stdout);
    assert!(
        stdout.contains("args: [") && stdout.ends_with("1\n"),
        "{stdout}"
    );
    let g = std::fs::read_to_string(&out).unwrap();
    assert!(g.contains("(Pocket)") && g.contains("(Cut out)"), "{g}");
    assert!(!g.contains("(Pocket floor raster)"), "{g}");
    let saved = std::fs::read_to_string(d.join("block.fluut")).unwrap();
    assert!(saved.contains("enabled = false"), "{saved}");
}

#[test]
fn exit_codes() {
    let d = block("run_exit");
    let run = |src: &str, extra: &[&str]| {
        std::fs::write(d.join("s.rhai"), src).unwrap();
        let mut a = vec![
            "run".to_owned(),
            d.join("s.rhai").to_str().unwrap().to_owned(),
        ];
        a.extend(extra.iter().map(|s| s.to_string()));
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        let o = fluut(&a);
        (o.status.code(), text(&o.stderr))
    };
    assert_eq!(run("fluut.exit(7)", &[]).0, Some(7));
    assert_eq!(run("print(1)", &[]).0, Some(0));
    let (c, e) = run("let x = 1;\nthrow \"boom\";", &[]);
    assert_eq!(c, Some(1));
    assert!(e.contains("s.rhai:2") && e.contains("boom"), "{e}");
    let (c, e) = run("loop {", &[]);
    assert_eq!(c, Some(1));
    assert!(e.contains("s.rhai:1"), "{e}");
    // A path neither in the project folder nor an argument is denied.
    let (c, e) = run(r#"fluut.read_csv("/etc/hostname")"#, &[]);
    assert_eq!(c, Some(1));
    assert!(e.contains("permission denied"), "{e}");
    // Opening a project named as an argument.
    let p = d.join("block.fluut");
    let (c, e) = run(
        "let p = fluut.open(fluut.args[0]); if p.name != \"Block\" { fluut.exit(9) }",
        &["--", p.to_str().unwrap()],
    );
    assert_eq!(c, Some(0), "{e}");
    // Bad arguments.
    assert_eq!(fluut(&["run"]).status.code(), Some(2));
    assert_eq!(fluut(&["run", "nope.rhai"]).status.code(), Some(1));
    assert_eq!(
        fluut(&["run", d.join("s.rhai").to_str().unwrap(), "--bogus"])
            .status
            .code(),
        Some(2)
    );
}
