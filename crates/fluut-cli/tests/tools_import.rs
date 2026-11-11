// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut tools import` (design D16 §7).

use fluut_model::tools::ToolLibrary;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-post-import/tests/fixtures/fusion-sample.json")
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

#[test]
fn imports_the_sample_and_the_result_loads() {
    let d = tmp("tools-import-ok");
    let out = d.join("shop.tools.toml");
    let o = fluut(&[
        "tools",
        "import",
        fixture().to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--id",
        "shop/fusion",
        "--name",
        "Shop",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(err(&o).contains("import.tool.unsupported_type"));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("7 of 9 tools imported"), "{stdout}");
    let (lib, w) = ToolLibrary::parse(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert!(w.is_empty());
    assert_eq!(
        (lib.id.as_str(), lib.name.as_str()),
        ("shop/fusion", "Shop")
    );
    assert_eq!(lib.tools.len(), 7);
}

#[test]
fn refuses_to_overwrite_without_force() {
    let d = tmp("tools-import-force");
    let out = d.join("lib.toml");
    std::fs::write(&out, "keep").unwrap();
    let fx = fixture();
    let args = [
        "tools",
        "import",
        fx.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ];
    let o = fluut(&args);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.output.exists"));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep");
    let mut forced = args.to_vec();
    forced.push("--force");
    assert_eq!(fluut(&forced).status.code(), Some(0));
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("fluut.tools/1")
    );
}

#[test]
fn bad_input_and_bad_arguments() {
    let d = tmp("tools-import-bad");
    let out = d.join("lib.toml");
    let bad = d.join("bad.json");
    std::fs::write(&bad, "{ nope").unwrap();
    let o = fluut(&[
        "tools",
        "import",
        bad.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.file.invalid"));
    assert!(!out.exists());

    let vt = d.join("tools.vtdb");
    std::fs::write(&vt, "x").unwrap();
    let o = fluut(&[
        "tools",
        "import",
        vt.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.format.unsupported"));

    let o = fluut(&["tools", "import", fixture().to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    let o = fluut(&["tools", "export"]);
    assert_eq!(o.status.code(), Some(2));
    let o = fluut(&[
        "tools",
        "import",
        fixture().to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--id",
        "Bad Id",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.library.bad_id"));
    let o = fluut(&[
        "tools",
        "import",
        d.join("missing.json").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("import.file.unreadable"));
}
