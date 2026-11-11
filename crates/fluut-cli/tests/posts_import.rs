// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut posts import` (design D10 §6.1).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-post-import/tests/fixtures/vectric")
        .join(name)
}

fn fluut(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fluut"))
        .args(args)
        .output()
        .unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn imports_a_post_and_the_result_posts_a_project() {
    let d = tmp("posts-import-ok");
    let posts = d.join("posts");
    std::fs::create_dir_all(&posts).unwrap();
    let target = posts.join("router.post.toml");
    let o = fluut(&[
        "posts",
        "import",
        fixture("grbl-like.pp").to_str().unwrap(),
        "-o",
        target.to_str().unwrap(),
        "--id",
        "shop/router",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(
        out(&o).contains("converted `FluutCAM test, Grbl-like (mm)` as post shop/router"),
        "{}",
        out(&o)
    );
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.contains("id      = \"shop/router\""));
    assert!(text.contains("origin  = \"imported\""));
    // A project beside the posts folder posts with it.
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/projects/plate"),
        &d,
    );
    let nc = d.join("plate.gcode");
    let o = fluut(&[
        "post",
        d.join("plate.fluut").to_str().unwrap(),
        "--post",
        "shop/router",
        "-o",
        nc.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let g = std::fs::read_to_string(&nc).unwrap();
    assert!(g.contains("\nG21 G90 G17\n"), "{g}");
    assert!(g.contains("\nG1 X"), "{g}");
    assert!(g.ends_with("M30\n") && g.contains("\nM5\nG0 Z"), "{g}");
}

fn copy_dir(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let t = to.join(p.file_name().unwrap());
        if p.is_dir() {
            std::fs::create_dir_all(&t).unwrap();
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

#[test]
fn default_id_from_the_file_name_and_warnings_printed() {
    let d = tmp("posts-import-default-id");
    let target = d.join("x.post.toml");
    let o = fluut(&[
        "posts",
        "import",
        fixture("wrap-y.pp").to_str().unwrap(),
        "-o",
        target.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("as post vectric/wrap-y"), "{}", out(&o));
    assert!(
        err(&o).contains("warning import.pp.rotary_wrap"),
        "{}",
        err(&o)
    );
}

#[test]
fn post_base_is_read_beside_the_input() {
    let d = tmp("posts-import-base");
    std::fs::copy(fixture("grbl-like.pp"), d.join("base.pp")).unwrap();
    std::fs::write(
        d.join("child.pp"),
        "POST_NAME = \"Child\"\nPOST_BASE = \"base.pp\"\nbegin FOOTER\n\"M2\"\n",
    )
    .unwrap();
    let target = d.join("child.post.toml");
    let o = fluut(&[
        "posts",
        "import",
        d.join("child.pp").to_str().unwrap(),
        "-o",
        target.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.contains("\"M2\""));
    assert!(text.contains("\"G0 [X] [Y] [Z]\""));
}

#[test]
fn refusals_and_bad_arguments() {
    let d = tmp("posts-import-refuse");
    let target = d.join("out.post.toml");
    let t = target.to_str().unwrap();
    // A refused post: error, exit 1, nothing written.
    let bad = d.join("inverse.pp");
    std::fs::write(
        &bad,
        std::fs::read_to_string(fixture("grbl-like.pp"))
            .unwrap()
            .replace("UNITS = \"MM\"", "UNITS = \"MM\"\nINVERSE_TIME_MODE = YES"),
    )
    .unwrap();
    let o = fluut(&["posts", "import", bad.to_str().unwrap(), "-o", t]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("error import.pp.inverse_time"),
        "{}",
        err(&o)
    );
    assert!(!target.exists());
    // Not a .pp file.
    let o = fluut(&["posts", "import", "x.cps", "-o", t]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.format.unsupported"));
    // A bad id.
    let src = fixture("grbl-like.pp");
    let o = fluut(&[
        "posts",
        "import",
        src.to_str().unwrap(),
        "-o",
        t,
        "--id",
        "Bad Id",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.post.bad_id"));
    // No overwrite without --force.
    std::fs::write(&target, "keep").unwrap();
    let o = fluut(&["posts", "import", src.to_str().unwrap(), "-o", t]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.output.exists"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    let o = fluut(&["posts", "import", src.to_str().unwrap(), "-o", t, "--force"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    // Usage errors exit 2.
    for args in [
        &["posts"][..],
        &["posts", "export"][..],
        &["posts", "import", "a.pp"][..],
        &["posts", "import", "a.pp", "-o", t, "--name", "x"][..],
    ] {
        let o = fluut(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(err(&o).contains("usage:"));
    }
}
