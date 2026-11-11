// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fusion tool-library import with hand-written fixtures (design D16 §8).

use fluut_model::tools::{ToolLibrary, ToolShape};
use fluut_post_import::{ImportOptions, ImportResult, fusion, limits};

fn opts() -> ImportOptions {
    ImportOptions {
        library_id: "fusion/sample".into(),
        library_name: "Sample".into(),
    }
}

fn run(json: &str) -> ImportResult {
    fusion::import(json.as_bytes(), &opts()).unwrap()
}

fn codes(r: &ImportResult) -> Vec<&str> {
    r.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn one(tool: &str) -> ImportResult {
    run(&format!("{{\"version\": 1, \"data\": [{tool}]}}"))
}

#[test]
fn sample_file_round_trips_through_the_native_parser() {
    let src = include_str!("fixtures/fusion-sample.json");
    let r = run(src);
    assert_eq!(r.report.tools_read, 9);
    assert_eq!(r.report.tools_written, 7);
    assert_eq!(r.report.tools_skipped, 2);
    let (lib, w) = ToolLibrary::parse(&r.library).unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(lib.id, "fusion/sample");
    assert_eq!(lib.tools.len(), 7);

    let flat = lib.tool("t1-6-mm-flat").unwrap();
    assert_eq!(
        (flat.shape, flat.number, flat.diameter),
        (ToolShape::Flat, 1, 6.0)
    );
    assert_eq!(flat.overall_length, Some(60.0));
    assert_eq!(flat.flute_length, Some(22.0));
    let h = flat.holder.as_ref().unwrap();
    assert_eq!((h.diameter, h.length), (30.0, 40.0));
    assert_eq!(flat.presets.len(), 2);
    let mdf = flat.preset(Some("MDF")).unwrap();
    assert_eq!(
        (mdf.rpm, mdf.feed, mdf.plunge, mdf.stepover, mdf.step_down),
        (
            Some(18000.0),
            Some(2000.0),
            Some(600.0),
            Some(2.4),
            Some(3.0)
        )
    );

    // Inches become millimetres, feeds too.
    let ball = lib.tool("t2-1-4-in-ball").unwrap();
    assert_eq!(ball.shape, ToolShape::Ball);
    assert_eq!(ball.diameter, 6.35);
    assert_eq!(ball.overall_length, Some(63.5));
    assert_eq!(ball.preset(Some("Plastic")).unwrap().feed, Some(1016.0));

    let bull = lib.tool("t3-bull-10").unwrap();
    assert_eq!((bull.shape, bull.corner_radius()), (ToolShape::Bull, 1.0));
    let v = lib.tool("t4-60-degree-v").unwrap();
    assert_eq!(
        (v.shape, v.half_angle_deg, v.tip_diameter),
        (ToolShape::V, Some(30.0), Some(0.2))
    );
    let d = lib.tool("t5-5-mm-drill").unwrap();
    assert_eq!((d.shape, d.point_angle()), (ToolShape::Drill, 135.0));
    let tap = lib.tool("t6-m6-tap").unwrap();
    assert_eq!(tap.pitch, Some(1.0));
}

#[test]
fn diagnostics_of_the_sample_file() {
    let r = run(include_str!("fixtures/fusion-sample.json"));
    let c = codes(&r);
    assert!(c.contains(&"import.tool.unsupported_type"));
    assert!(c.contains(&"import.tool.bad_value")); // no diameter
    assert!(c.contains(&"import.tool.number_duplicate")); // number 1 twice
    assert!(c.contains(&"import.tool.length_missing"));
    assert!(c.contains(&"import.fields.dropped"));
    assert!(!c.contains(&"import.file.version"));
    assert!(r.report.dropped_keys.contains_key("vendor"));
    assert!(r.report.dropped_keys.contains_key("geometry.NOF"));
    assert!(
        r.report
            .dropped_keys
            .contains_key("start-values.presets.f_z")
    );
    assert!(r.diagnostics.iter().all(|d| !d.is_error()));
}

#[test]
fn duplicate_names_get_distinct_ids() {
    let t = "{\"type\":\"flat end mill\",\"description\":\"x\",\"geometry\":{\"DC\":3,\"OAL\":40}}";
    let r = one(&format!("{t},{t},{t}"));
    let (lib, _) = ToolLibrary::parse(&r.library).unwrap();
    let ids: Vec<_> = lib.tools.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["x", "x-2", "x-3"]);
    assert_eq!(
        codes(&r)
            .iter()
            .filter(|c| **c == "import.tool.id_duplicate")
            .count(),
        2
    );
}

#[test]
fn bull_without_radius_becomes_flat_and_tapered_warns() {
    let r = one(
        "{\"type\":\"bull nose end mill\",\"geometry\":{\"DC\":8,\"OAL\":50}},\
         {\"type\":\"tapered mill\",\"geometry\":{\"DC\":6,\"TA\":5,\"RE\":0.5,\"OAL\":50}}",
    );
    let (lib, _) = ToolLibrary::parse(&r.library).unwrap();
    assert_eq!(lib.tools[0].shape, ToolShape::Flat);
    assert_eq!(lib.tools[1].shape, ToolShape::Taper);
    assert_eq!(lib.tools[1].half_angle_deg, Some(5.0));
    assert_eq!(
        codes(&r)
            .iter()
            .filter(|c| **c == "import.tool.approximated")
            .count(),
        2
    );
}

#[test]
fn bad_values_are_dropped_not_trusted() {
    let r = one(
        "{\"type\":\"flat end mill\",\"description\":\"a\",\"geometry\":{\"DC\":6,\"OAL\":-5,\"LCF\":\"long\",\"SFDM\":1e30},\
         \"post-process\":{\"number\":-3},\
         \"start-values\":{\"presets\":[{\"name\":\"m\",\"n\":-1,\"v_f\":1e300}]}}",
    );
    let (lib, _) = ToolLibrary::parse(&r.library).unwrap();
    let t = &lib.tools[0];
    assert_eq!(
        (t.overall_length, t.flute_length, t.shank_diameter, t.number),
        (None, None, None, 0)
    );
    assert_eq!(t.presets[0].rpm, None);
    assert_eq!(t.presets[0].feed, None);
    assert!(
        codes(&r)
            .iter()
            .filter(|c| **c == "import.tool.bad_value")
            .count()
            >= 5
    );
}

#[test]
fn names_cannot_break_the_toml() {
    let r = one(
        "{\"type\":\"flat end mill\",\"description\":\"q\\\"uote\\\\ \\n\\\"\\u0000 [[tool]]\\nid = \\\"evil\\\"\",\
         \"geometry\":{\"DC\":6,\"OAL\":50}}",
    );
    let (lib, _) = ToolLibrary::parse(&r.library).unwrap();
    assert_eq!(lib.tools.len(), 1);
    assert!(lib.tools[0].name.contains("[[tool]]"));
    assert_ne!(lib.tools[0].id, "evil");
}

#[test]
fn errors_for_bad_files() {
    let bad = |b: &[u8], code: &str| {
        let e = fusion::import(b, &opts()).unwrap_err();
        assert_eq!(e[0].code, code, "{e:?}");
        assert!(e[0].is_error());
    };
    bad(b"not json", "import.file.invalid");
    bad(b"{\"nodata\": 1}", "import.file.invalid");
    bad(b"42", "import.file.invalid");
    bad(&[0xff, 0xfe, 0x00], "import.file.invalid");
    let deep = "[".repeat(100_000);
    bad(deep.as_bytes(), "import.file.invalid");
    let many = format!("[{}]", vec!["{}"; limits::MAX_TOOLS + 1].join(","));
    bad(many.as_bytes(), "import.limit.tools");
    let big = vec![b' '; limits::MAX_FILE_BYTES as usize + 1];
    bad(&big, "import.file.too_large");
    let o = ImportOptions {
        library_id: "Bad Id".into(),
        ..opts()
    };
    assert_eq!(
        fusion::import(b"[]", &o).unwrap_err()[0].code,
        "import.library.bad_id"
    );
}

#[test]
fn bare_array_and_bom_are_accepted() {
    let mut b = b"\xEF\xBB\xBF".to_vec();
    b.extend_from_slice(b"[{\"type\":\"ball end mill\",\"geometry\":{\"DC\":2,\"OAL\":30}}]");
    let r = fusion::import(&b, &opts()).unwrap();
    assert_eq!(r.report.tools_written, 1);
    assert!(codes(&r).contains(&"import.file.version"));
}

#[test]
fn unknown_unit_skips_the_tool_and_missing_unit_assumes_mm() {
    let r = one(
        "{\"type\":\"flat end mill\",\"unit\":\"furlongs\",\"geometry\":{\"DC\":2}},\
         {\"type\":\"flat end mill\",\"geometry\":{\"DC\":2,\"OAL\":20}}",
    );
    assert_eq!((r.report.tools_written, r.report.tools_skipped), (1, 1));
}

#[test]
fn output_is_deterministic() {
    let src = include_str!("fixtures/fusion-sample.json");
    assert_eq!(run(src).library, run(src).library);
}
