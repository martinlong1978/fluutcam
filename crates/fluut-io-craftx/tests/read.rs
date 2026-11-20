// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading `.craftx` files: the fixtures of the test part, and every
//! refusal (spec 02 §2.6.4).

#[path = "support/craftx.rs"]
mod craftx;
#[path = "../../fluut-cam/tests/support/step.rs"]
mod step;

use craftx::{Craftx, rev1, zip};
use fluut_io_craftx::zip::Limits;
use fluut_io_craftx::{Exchange, Finding, Options, Severity, read};

fn ok(c: &Craftx) -> Exchange {
    read(&c.bytes(), &Options::default()).unwrap_or_else(|e| panic!("{e:?}"))
}

fn refused(bytes: &[u8]) -> Finding {
    refused_with(bytes, &Options::default())
}

fn refused_with(bytes: &[u8], o: &Options) -> Finding {
    let e = read(bytes, o).expect_err("the file should be refused");
    let errs: Vec<_> = e.iter().filter(|f| f.severity == Severity::Error).collect();
    assert_eq!(errs.len(), 1, "{e:?}");
    errs[0].clone()
}

fn codes(x: &Exchange) -> Vec<&str> {
    x.warnings.iter().map(|f| f.code).collect()
}

fn face_names(x: &Exchange) -> Vec<Option<String>> {
    x.bodies[0]
        .step
        .body
        .names
        .faces
        .iter()
        .map(|n| n.as_ref().map(|n| n.name.clone()))
        .collect()
}

#[test]
fn revision_1_reads_with_names_params_and_meta() {
    let x = ok(&rev1());
    assert!(x.warnings.is_empty(), "{:?}", x.warnings);
    assert_eq!(x.source_id, "3f6c0d4e-design");
    assert_eq!(x.revision, "1");
    assert_eq!(x.source.app.as_deref(), Some("SolidCraft"));
    assert_eq!(x.units.as_deref(), Some("mm"));
    assert_eq!(x.bodies.len(), 1);
    assert_eq!(x.bodies[0].index, 0);
    let n = face_names(&x);
    assert_eq!(n.len(), 11);
    assert_eq!(n[0].as_deref(), Some("F1:side:l1"));
    assert_eq!(n[10].as_deref(), Some("F2:floor"));
    let names = &x.bodies[0].step.body.names;
    assert!(names.face(10).unwrap().exact);
    // Edges between two named faces are named from them.
    let e = names.edges_named("F1:side:l1|F1:side:l2");
    assert_eq!(e.len(), 1);
    let en = names.edge(e[0]).unwrap();
    assert!(en.derived);
    assert_eq!(
        en.between,
        Some(("F1:side:l1".to_owned(), "F1:side:l2".to_owned()))
    );
    assert_eq!(x.params.len(), 1);
    assert_eq!(
        (x.params[0].name.as_str(), x.params[0].value.as_str()),
        ("wall", "3.0")
    );
    assert_eq!(x.meta.material.as_deref(), Some("MDF"));
    assert_eq!(x.meta.part_number.as_deref(), Some("FC-1"));
}

#[test]
fn every_fixture_reads() {
    for (c, faces, name10) in [
        (craftx::hole(), 13, "F2:side:l3"),
        (craftx::fillet(), 12, "F2:side:l4"),
        (craftx::split(), 16, "F2:floor#[F6:side:l2]"),
        (craftx::merged(), 11, "F2:floor"),
        (craftx::inexact(), 11, "F2:floor"),
    ] {
        let x = ok(&c);
        assert!(
            x.warnings.is_empty(),
            "rev {}: {:?}",
            c.revision,
            x.warnings
        );
        let n = face_names(&x);
        assert_eq!(n.len(), faces, "rev {}", c.revision);
        assert!(n.iter().all(Option::is_some), "rev {}", c.revision);
        assert_eq!(n[10].as_deref(), Some(name10), "rev {}", c.revision);
    }
    let x = ok(&craftx::hole());
    assert_eq!(face_names(&x)[12].as_deref(), Some("F2:floor"));
    let x = ok(&craftx::split());
    let names = &x.bodies[0].step.body.names;
    assert_eq!(names.pieces_of("F2:floor"), [10, 15]);
    let x = ok(&craftx::merged());
    let names = &x.bodies[0].step.body.names;
    assert_eq!(names.faces_aliased("F2:floor#[F6:side:l4]"), [10]);
    let x = ok(&craftx::inexact());
    assert!(!x.bodies[0].step.body.names.face(10).unwrap().exact);
}

#[test]
fn stored_entries_and_newer_minor_versions_read() {
    let mut c = rev1();
    c.deflate = false;
    c.format = "craft-exchange/1.7".into();
    let x = ok(&c);
    assert_eq!(face_names(&x)[10].as_deref(), Some("F2:floor"));
}

#[test]
fn an_unknown_major_version_is_refused() {
    let mut c = rev1();
    c.format = "craft-exchange/2".into();
    assert_eq!(refused(&c.bytes()).code, "import.craftx.format");
    c.format = "something-else/1".into();
    assert_eq!(refused(&c.bytes()).code, "import.craftx.format");
}

#[test]
fn a_required_part_it_cannot_read_is_refused() {
    let mut c = rev1();
    c.requires.push("brep.solidcore-record/1".into());
    let f = refused(&c.bytes());
    assert_eq!(f.code, "import.craftx.requires");
    assert!(
        f.message.contains("brep.solidcore-record/1"),
        "{}",
        f.message
    );
    c.requires = vec!["faces.names/2".into()];
    assert_eq!(refused(&c.bytes()).code, "import.craftx.requires");
}

#[test]
fn unknown_and_unlisted_parts_are_warned_and_skipped() {
    let mut c = rev1();
    c.extra.push(("notes.txt".into(), b"hello".to_vec()));
    c.extra.push(("mesh/0.bin".into(), vec![0; 16]));
    let mut entries = c.entries();
    entries.push(("stray.bin".into(), b"x".to_vec()));
    let x = read(&zip(&entries, true), &Options::default()).unwrap();
    assert_eq!(
        codes(&x),
        ["import.craftx.part.unlisted", "import.craftx.part.unknown"]
    );
}

#[test]
fn a_tampered_part_is_refused() {
    let c = rev1();
    let mut entries = c.entries();
    let faces = entries.iter_mut().find(|(n, _)| n == "faces.json").unwrap();
    faces.1 = String::from_utf8(faces.1.clone())
        .unwrap()
        .replace("F2:floor", "F2:flour")
        .into_bytes();
    let f = refused(&zip(&entries, true));
    assert_eq!(f.code, "import.craftx.part.hash");
    assert!(f.message.contains("faces.json"), "{}", f.message);
}

#[test]
fn a_zip_slip_entry_is_refused() {
    for bad in [
        "../evil.step",
        "bodies/../../evil",
        "/etc/x",
        "C:/x",
        "a\\b",
    ] {
        let mut entries = rev1().entries();
        entries.push((bad.to_owned(), b"x".to_vec()));
        assert_eq!(
            refused(&zip(&entries, false)).code,
            "import.craftx.zip.name",
            "{bad}"
        );
    }
}

#[test]
fn oversized_archives_are_refused() {
    let entries = rev1().entries();
    let small = Options {
        limits: Limits {
            max_unpacked: 1000,
            ..Limits::default()
        },
    };
    assert_eq!(
        refused_with(&zip(&entries, true), &small).code,
        "import.craftx.zip.size"
    );
    let few = Options {
        limits: Limits {
            max_entries: 3,
            ..Limits::default()
        },
    };
    assert_eq!(
        refused_with(&zip(&entries, true), &few).code,
        "import.craftx.zip.entries"
    );
    // A 4 MiB entry of zeros deflates about 1000:1.
    let mut bomb = rev1().entries();
    bomb.push(("bomb.bin".into(), vec![0; 4 << 20]));
    assert_eq!(refused(&zip(&bomb, true)).code, "import.craftx.zip.ratio");
}

#[test]
fn a_lying_entry_is_refused() {
    let mut bytes = rev1().bytes();
    // Flip a byte inside the first entry's data (the manifest).
    bytes[50] ^= 0x55;
    let f = refused(&bytes);
    assert!(
        ["import.craftx.zip.crc", "import.craftx.zip.invalid"].contains(&f.code),
        "{f:?}"
    );
}

#[test]
fn files_that_are_not_exchange_files_are_refused() {
    assert_eq!(refused(b"ISO-10303-21;").code, "import.craftx.zip.invalid");
    let no_manifest = zip(&[("bodies/0.step".into(), b"x".to_vec())], true);
    assert_eq!(refused(&no_manifest).code, "import.craftx.manifest.missing");
    let bad_json = zip(&[("manifest.json".into(), b"{".to_vec())], true);
    assert_eq!(refused(&bad_json).code, "import.craftx.manifest.invalid");
    let mut c = rev1();
    c.source_id = String::new();
    assert_eq!(refused(&c.bytes()).code, "import.craftx.manifest.invalid");
    let mut c = rev1();
    c.step = "not step".into();
    assert_eq!(refused(&c.bytes()).code, "import.craftx.body.step");
    // No STEP body at all.
    let c = rev1();
    let parts: Vec<_> = c
        .parts()
        .into_iter()
        .filter(|(n, _)| n != "bodies/0.step")
        .collect();
    let mut entries = vec![("manifest.json".to_owned(), c.manifest(&parts).into_bytes())];
    entries.extend(parts);
    assert_eq!(
        refused(&zip(&entries, true)).code,
        "import.craftx.body.none"
    );
}

#[test]
fn every_truncation_is_refused_without_panic() {
    let bytes = rev1().bytes();
    for n in (0..bytes.len()).step_by(97) {
        assert!(read(&bytes[..n], &Options::default()).is_err(), "{n}");
    }
}

#[test]
fn a_point_off_its_face_marks_the_name_inexact() {
    let mut c = rev1();
    // Face 10 is the floor at Z 12; this point is on the top.
    c.faces[10].point = Some([5.0, 5.0, 20.0]);
    let x = ok(&c);
    assert_eq!(codes(&x), ["import.craftx.faces.point"]);
    let n = x.bodies[0].step.body.names.face(10).unwrap();
    assert_eq!(n.name, "F2:floor");
    assert!(!n.exact);
    // A wrong surface kind does the same.
    let mut c = rev1();
    c.faces[4].surface = Some("cylinder".into());
    c.faces[3].surface = Some("plane".into());
    let x = ok(&c);
    assert_eq!(codes(&x), ["import.craftx.faces.surface"]);
    assert!(!x.bodies[0].step.body.names.face(4).unwrap().exact);
    assert!(x.bodies[0].step.body.names.face(3).unwrap().exact);
}

/// Writes the STEP names of `names` into the `ADVANCED_FACE`s of `text`,
/// in file order.
fn name_step_faces(text: &str, names: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut k = 0;
    while let Some(i) = rest.find("ADVANCED_FACE('',") {
        out.push_str(&rest[..i]);
        out.push_str(&format!(
            "ADVANCED_FACE('{}',",
            names.get(k).copied().unwrap_or("")
        ));
        k += 1;
        rest = &rest[i + "ADVANCED_FACE('',".len()..];
    }
    out.push_str(rest);
    out
}

#[test]
fn step_face_names_are_preferred_and_a_disagreement_is_warned() {
    let mut c = rev1();
    let table: Vec<String> = c.faces.iter().map(|f| f.name.clone()).collect();
    let mut names: Vec<&str> = table.iter().map(String::as_str).collect();
    // The STEP file names face 6 differently from the table, and face 10
    // carries its name although the table's index is off by one.
    names[6] = "F2:wall:a";
    c.step = name_step_faces(&c.step, &names);
    c.faces[10].index = 9;
    c.faces[9].index = 10;
    let x = ok(&c);
    assert_eq!(codes(&x), ["import.craftx.faces.name_mismatch"]);
    let n = face_names(&x);
    assert_eq!(n[6].as_deref(), Some("F2:wall:a"));
    assert_eq!(n[9].as_deref(), Some("F2:side:l4"));
    assert_eq!(n[10].as_deref(), Some("F2:floor"));
    // The point still checks: entry 10 now says index 9 but names the floor.
    assert!(x.bodies[0].step.body.names.face(10).unwrap().exact);
}

#[test]
fn a_face_table_that_cannot_be_read_refuses_only_when_required() {
    let mut c = rev1();
    c.faces[0].name = String::new();
    let x = ok(&c);
    assert_eq!(codes(&x), ["import.craftx.faces.index"]);
    // A face table index past the faces of the body.
    let mut c = rev1();
    c.faces[0].index = 99;
    let x = ok(&c);
    assert_eq!(codes(&x), ["import.craftx.faces.index"]);
    // A repeated name names nothing.
    let mut c = rev1();
    c.faces[1].name = "F1:side:l1".into();
    let x = ok(&c);
    assert_eq!(codes(&x), ["import.craftx.faces.repeated"]);
    assert_eq!(face_names(&x)[0], None);
}

/// `c`'s archive with part `name` replaced by `data`, hashed correctly.
fn with_part(c: &Craftx, name: &str, data: &[u8]) -> Vec<u8> {
    let parts: Vec<_> = c
        .parts()
        .into_iter()
        .map(|(n, d)| {
            if n == name {
                (n, data.to_vec())
            } else {
                (n, d)
            }
        })
        .collect();
    let mut entries = vec![("manifest.json".to_owned(), c.manifest(&parts).into_bytes())];
    entries.extend(parts);
    zip(&entries, true)
}

#[test]
fn a_broken_face_table_refuses_when_required_and_warns_otherwise() {
    let mut c = rev1();
    let f = refused(&with_part(&c, "faces.json", b"{\"bodies\": 3}"));
    assert_eq!(f.code, "import.craftx.faces.invalid");
    c.requires = vec!["step/ap214".into()];
    let x = read(
        &with_part(&c, "faces.json", b"not json"),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(codes(&x), ["import.craftx.faces.invalid"]);
    assert!(x.bodies[0].step.body.names.is_empty());
    // Broken parameters or metadata only warn.
    let x = read(&with_part(&c, "params.json", b"[]"), &Options::default()).unwrap();
    assert_eq!(codes(&x), ["import.craftx.params.invalid"]);
    let x = read(&with_part(&c, "meta.json", b"{"), &Options::default()).unwrap();
    assert_eq!(codes(&x), ["import.craftx.meta.invalid"]);
}

/// Writes the six test revisions as files, for the user acceptance tests
/// (`docs/wiki/UAT-Import.md`):
///
/// ```text
/// FLUUT_CRAFTX_OUT=<folder> cargo test -p fluut-io-craftx --test read write_uat_fixtures -- --ignored
/// ```
#[test]
#[ignore = "writes files for the UAT; run by hand"]
fn write_uat_fixtures() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("FLUUT_CRAFTX_OUT").expect("set FLUUT_CRAFTX_OUT to a folder"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    for (name, c) in [
        ("1-block", craftx::rev1()),
        ("2-hole", craftx::hole()),
        ("3-fillet", craftx::fillet()),
        ("4-split", craftx::split()),
        ("5-merged", craftx::merged()),
        ("6-inexact", craftx::inexact()),
    ] {
        std::fs::write(dir.join(format!("{name}.craftx")), c.bytes()).unwrap();
    }
}
