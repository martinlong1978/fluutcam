// SPDX-License-Identifier: MIT OR Apache-2.0

//! Unit tests on hand-written posts. The text below is written for these
//! tests from the documented `.pp` syntax; it is not taken from any
//! vendor's post processor.

use super::*;
use fluut_model::Severity;
use fluut_post::format::format_number;
use fluut_post::post::WordFmt;

fn opts() -> Options {
    Options {
        post_id: "test/pp".into(),
        name: None,
    }
}

/// A small post; `extra` goes after the settings.
fn pp(extra: &str) -> String {
    format!(
        "+ test post\nPOST_NAME = \"Test (mm)\"\nFILE_EXTENSION = \"tap\"\nUNITS = \"MM\"\n\
         LINE_ENDING = \"[10]\"\n{extra}\n\
         VAR XPOS = [X|C|X|1.3]\nVAR YPOS = [Y|C|Y|1.3]\nVAR ZPOS = [Z|C|Z|1.3]\n\
         VAR FEED = [F|C|F|1.0]\n\
         begin HEADER\n\"G21 G90\"\n\
         begin RAPID_MOVE\n\"G0 [X] [Y] [Z]\"\n\
         begin FEED_MOVE\n\"G1 [X] [Y] [Z] [F]\"\n\
         begin FOOTER\n\"M30\"\n"
    )
}

fn ok(src: &str) -> Converted {
    match import(src.as_bytes(), &opts()) {
        Ok(c) => c,
        Err(d) => panic!("refused: {d:?}"),
    }
}

fn refused(src: &str) -> Vec<String> {
    match import(src.as_bytes(), &opts()) {
        Ok(c) => panic!("converted:\n{}", c.post),
        Err(d) => {
            assert!(d.iter().all(|x| x.severity == Severity::Error));
            d.into_iter().map(|x| x.code).collect()
        }
    }
}

fn codes(c: &Converted) -> Vec<&str> {
    c.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn table(c: &Converted) -> toml::Table {
    c.post.parse().unwrap()
}

/// Formats `v` the way the converted post's word `key` will.
fn word_text(c: &Converted, key: &str, v: f64) -> String {
    let t = table(c);
    let w: WordFmt = t["words"][key].clone().try_into().unwrap();
    let dc = t["output"]["decimal_char"].as_str().unwrap().to_owned();
    format!("{}{}", w.prefix, format_number(v, &w, &dc))
}

#[test]
fn minimal_post_converts_cleanly() {
    let c = ok(&pp(""));
    assert!(c.diagnostics.is_empty(), "{:?}", c.diagnostics);
    assert_eq!(c.name, "Test (mm)");
    let t = table(&c);
    assert_eq!(t["origin"].as_str(), Some("imported"));
    assert_eq!(t["output"]["extension"].as_str(), Some("tap"));
    assert_eq!(t["output"]["line_ending"].as_str(), Some("lf"));
    assert_eq!(t["output"]["units"].as_str(), Some("mm"));
    assert_eq!(t["arcs"]["enabled"].as_bool(), Some(false));
    assert_eq!(
        t["blocks"]["feed"]["lines"][0].as_str(),
        Some("G1 [X] [Y] [Z] [F]")
    );
    assert_eq!(c.report.blocks_read, 4);
    assert_eq!(c.report.blocks_mapped, 4);
    assert_eq!(c.report.words_written, 4);
}

// ----- value formats (CH-21): one test per form seen in real posts.

#[test]
fn vf_width_one_and_decimals() {
    // Width 1 is never wider than a number: decimals only, zeros kept.
    let f = ValueFormat::parse("1.3").unwrap();
    assert_eq!(
        (f.plus, f.zero_pad, f.width, f.decimals),
        (false, false, 1, 3)
    );
    assert_eq!((f.pad_width(), f.space_padded()), (0, false));
    let c = ok(&pp("VAR TOOL_DIA = [TDIA|A|D|1.3]"));
    assert_eq!(word_text(&c, "TDIA", 6.0), "D6.000");
    assert_eq!(word_text(&c, "TDIA", -0.25), "D-0.250");
    assert_eq!(word_text(&c, "TDIA", 0.0004), "D0.000");
}

#[test]
fn vf_no_decimals() {
    let c = ok(&pp("VAR SPINDLE = [S|A|S|1.0]"));
    assert_eq!(word_text(&c, "S", 18000.4), "S18000");
    assert_eq!(word_text(&c, "S", 2499.5), "S2500");
}

#[test]
fn vf_plus_sign() {
    let f = ValueFormat::parse("+1.3").unwrap();
    assert!(f.plus && !f.space_padded());
    let c = ok(&pp("VAR TOOL_DIA = [TDIA|A|D|+1.3]"));
    assert_eq!(word_text(&c, "TDIA", 2.5), "D+2.500");
    assert_eq!(word_text(&c, "TDIA", -2.5), "D-2.500");
}

#[test]
fn vf_comma_separator_with_plus() {
    let f = ValueFormat::parse("+1,4").unwrap();
    assert_eq!((f.separator, f.decimals, f.plus), (',', 4, true));
    let src = pp("").replace("|1.3]", "|+1,4]");
    let c = ok(&src);
    assert!(c.diagnostics.is_empty(), "{:?}", c.diagnostics);
    assert_eq!(word_text(&c, "X", 12.5), "X+12,5000");
}

#[test]
fn vf_comma_separator_without_sign() {
    let src = pp("").replace("|1.3]", "|1,3]");
    let c = ok(&src);
    assert_eq!(word_text(&c, "Y", -0.5), "Y-0,500");
}

#[test]
fn vf_mixed_separators_are_reported() {
    let src = pp("").replace("[X|C|X|1.3]", "[X|C|X|1,3]");
    let c = ok(&src);
    assert!(codes(&c).contains(&"import.pp.decimal_char"));
}

#[test]
fn vf_zero_padded_integer() {
    // A width starting with 0 pads with zeros to that many characters.
    let f = ValueFormat::parse("04.0").unwrap();
    assert_eq!((f.zero_pad, f.width, f.pad_width()), (true, 4, 4));
    let c = ok(&pp(
        "LINE_NUMBER_START = 10\nLINE_NUMBER_INCREMENT = 5\nLINE_NUMBER_MAXIMUM = 9990\n\
         VAR LN = [N|A|N|04.0]\nVAR TOOL = [T|A|T|02.0]",
    ));
    assert_eq!(word_text(&c, "N", 10.0), "N0010");
    assert_eq!(word_text(&c, "T", 3.0), "T03");
    assert_eq!(word_text(&c, "T", 12.0), "T12");
}

#[test]
fn vf_zero_padded_with_decimals_counts_the_point_and_decimals() {
    let f = ValueFormat::parse("06.2").unwrap();
    assert_eq!(f.pad_width(), 3);
    let f = ValueFormat::parse("+07.2").unwrap();
    assert_eq!(f.pad_width(), 3);
}

#[test]
fn vf_wide_width_pads_with_spaces_and_is_reported() {
    let f = ValueFormat::parse("9.5").unwrap();
    assert!(f.space_padded());
    let c = ok(&pp("VAR TOOL_DIA = [TDIA|A|D|9.5]"));
    assert!(codes(&c).contains(&"import.pp.var_format"));
    assert_eq!(word_text(&c, "TDIA", 1.0), "D1.00000");
    // A width that a number always fills has no effect.
    assert!(!ValueFormat::parse("2.6").unwrap().space_padded());
}

#[test]
fn vf_without_width() {
    let f = ValueFormat::parse(".1").unwrap();
    assert_eq!((f.width, f.decimals), (0, 1));
    let c = ok(&pp("").replace("[F|C|F|1.0]", "[F|C|F|.1]"));
    assert_eq!(word_text(&c, "F", 1500.0), "F1500.0");
}

#[test]
fn vf_rejects_garbage() {
    for bad in ["", "3", "x.3", "1.", "1.x", "1.12", "+", "99999999999.1"] {
        assert!(ValueFormat::parse(bad).is_err(), "{bad}");
    }
    let c = ok(&pp("VAR TOOL_DIA = [TDIA|A|D|abc]"));
    assert!(codes(&c).contains(&"import.pp.var_format"));
}

#[test]
fn multiplier_becomes_scale_and_output_mode_changed() {
    let c = ok(&pp("").replace("[F|C|F|1.0]", "[F|C|F|1.2|0.0166667]"));
    let t = table(&c);
    let f = &t["words"]["F"];
    assert_eq!(f["when"].as_str(), Some("changed"));
    assert!((f["scale"].as_float().unwrap() - 0.0166667).abs() < 1e-12);
    assert_eq!(word_text(&c, "F", 600.0), "F10.00");
}

#[test]
fn unknown_output_mode_is_always() {
    let c = ok(&pp("").replace("[X|C|X|1.3]", "[X|@|X|1.3]"));
    assert!(codes(&c).contains(&"import.pp.var_format"));
    assert_eq!(table(&c)["words"]["X"]["when"].as_str(), Some("always"));
}

// ----- settings

#[test]
fn units_line_endings_and_names() {
    let src = pp("")
        .replace("\"MM\"", "\"inches\"")
        .replace("\"[10]\"", "\"[13][10]\"");
    let t = table(&ok(&src));
    assert_eq!(t["output"]["units"].as_str(), Some("inch"));
    assert_eq!(t["output"]["line_ending"].as_str(), Some("crlf"));
    let t = table(&ok(&pp("").replace("LINE_ENDING = \"[10]\"\n", "")));
    assert_eq!(t["output"]["line_ending"].as_str(), Some("crlf"));
    let c = ok(&pp("").replace("\"[10]\"", "\"[32][10]\""));
    assert!(codes(&c).contains(&"import.pp.line_ending"));
}

#[test]
fn substitute_maps_pairs_for_user_text() {
    let t = table(&ok(&pp("SUBSTITUTE = \"(<)>\"")));
    let s = &t["output"]["substitute_values"];
    assert_eq!(s["("].as_str(), Some("<"));
    assert_eq!(s[")"].as_str(), Some(">"));
}

#[test]
fn line_number_settings() {
    let t = table(&ok(&pp(
        "LINE_NUMBER_START = 0\nLINE_NUMBER_INCREMENT = 5\nLINE_NUMBER_MAXIMUM = 500",
    )));
    let ln = &t["line_numbering"];
    assert_eq!(ln["enabled"].as_bool(), Some(false));
    assert_eq!(
        (
            ln["start"].as_integer(),
            ln["increment"].as_integer(),
            ln["maximum"].as_integer()
        ),
        (Some(0), Some(5), Some(500))
    );
}

#[test]
fn ignored_and_unknown_settings_are_reported() {
    let c = ok(&pp(
        "DIRECT_OUTPUT = \"Some|Thing\"\nTAPE_SPLITTING = 100 10 \"%s\" 0 \"NO\"\nMY_OWN_KEY = 3",
    ));
    let k = codes(&c);
    assert!(k.contains(&"import.pp.setting_ignored"));
    assert!(k.contains(&"import.pp.setting_unknown"));
}

#[test]
fn arc_and_length_limits_in_mm() {
    let src = pp("MIN_ARC_RADIUS = 0.01\nMAX_ARC_RADIUS = 100\nMAXIMUM_LENGTH = 2")
        .replace("\"MM\"", "\"INCHES\"");
    let t = table(&ok(&src));
    assert!((t["arcs"]["min_radius"].as_float().unwrap() - 0.254).abs() < 1e-9);
    assert!((t["arcs"]["max_radius"].as_float().unwrap() - 2540.0).abs() < 1e-9);
    assert!((t["output"]["max_segment_length"].as_float().unwrap() - 50.8).abs() < 1e-9);
}

// ----- refusals

#[test]
fn refusals() {
    assert_eq!(
        refused(&pp("").replace("UNITS = \"MM\"\n", "")),
        ["import.pp.units"]
    );
    assert_eq!(
        refused(&pp("").replace("\"MM\"", "\"FURLONGS\"")),
        ["import.pp.units"]
    );
    assert_eq!(
        refused(&pp("INVERSE_TIME_MODE = YES")),
        ["import.pp.inverse_time"]
    );
    assert_eq!(
        refused(&pp("SPINDLE_SPEED_RANGE = 1 10 1000 10000")),
        ["import.pp.spindle_range"]
    );
    assert_eq!(
        refused(&pp("ROTARY_WRAP_X = \"Q\"")),
        ["import.pp.rotary_wrap"]
    );
    assert_eq!(
        refused(&format!("{}SCRIPT\nx = 1\nENDSCRIPT\n", pp(""))),
        ["import.pp.script"]
    );
    assert_eq!(
        refused(&pp("").replace("\"G1 [X] [Y] [Z] [F]\"", "")),
        ["import.pp.block_missing"]
    );
    assert_eq!(refused("just some text\n"), ["import.file.invalid"]);
    assert_eq!(refused("POST_NAME = \"x\"\0\n"), ["import.file.invalid"]);
    assert_eq!(
        refused(&pp("POST_BASE = \"base.pp\"")),
        ["import.pp.post_base"]
    );
}

#[test]
fn size_limit() {
    let big = "+".repeat(limits::MAX_POST_BYTES as usize + 1);
    assert_eq!(refused(&big), ["import.limit.size"]);
    let many = "+\n".repeat(limits::MAX_POST_LINES + 1);
    assert_eq!(refused(&many), ["import.limit.lines"]);
}

#[test]
fn not_utf8_is_read_as_latin1() {
    let mut b = pp("").replace("Test (mm)", "Fr?se").into_bytes();
    let q = b.iter().position(|&x| x == b'?').unwrap();
    b[q] = 0xE4;
    let c = import(&b, &opts()).unwrap();
    assert_eq!(c.name, "Fr\u{e4}se");
    assert!(codes(&c).contains(&"import.file.encoding"));
}

// ----- blocks and placeholders

fn lines_of(c: &Converted, block: &str) -> Vec<String> {
    table(c)["blocks"][block]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| match l {
            toml::Value::String(s) => s.clone(),
            toml::Value::Table(t) => format!(
                "{} ? {}",
                t["when"].as_str().unwrap(),
                t["line"].as_str().unwrap()
            ),
            _ => panic!("a line is a string or a table"),
        })
        .collect()
}

#[test]
fn tool_change_and_new_segment_skip_the_first_toolpath() {
    let c = ok(&format!(
        "{}begin TOOLCHANGE\n\"M5\"\n\"T[T] M6\"\nbegin NEW_SEGMENT\n\"[S] M3\"\n",
        pp("VAR SPINDLE = [S|A|S|1.0]")
    ));
    assert_eq!(
        lines_of(&c, "tool_change"),
        ["!state.first_tool ? M5", "!state.first_tool ? T[T] M6"]
    );
    assert_eq!(
        lines_of(&c, "toolpath_start"),
        ["!state.first_tool ? [S] M3"]
    );
}

#[test]
fn arcs_helices_and_first_blocks() {
    let c = ok(&format!(
        "{}VAR I = [I|A|I|1.3]\nVAR J = [J|A|J|1.3]\n\
         begin FIRST_CW_ARC_MOVE\n\"G2 [X] [Y] [I] [J] [F]\"\nbegin CW_ARC_MOVE\n\"G2 [X] [Y] [I] [J]\"\n\
         begin CCW_ARC_MOVE\n\"G3 [X] [Y] [I] [J]\"\n\
         begin cw_helical_arc_plunge_move\n\"G2 [X] [Y] [Z] [I] [J]\"\n\
         begin Ccw_Helical_Arc_Plunge_Move\n\"G3 [X] [Y] [Z] [I] [J]\"\n\
         begin PLUNGE_MOVE\n\"G1 [Z] [F]\"\nbegin FIRST_FEED_MOVE\n\"G1 [X] [Y] [Z] [F]\"\n",
        pp("")
    ));
    let t = table(&c);
    assert_eq!(t["arcs"]["enabled"].as_bool(), Some(true));
    assert_eq!(t["arcs"]["centre_mode"].as_str(), Some("ijk_incremental"));
    assert_eq!(t["arcs"]["planes"][0].as_str(), Some("XY"));
    assert_eq!(t["helix"]["enabled"].as_bool(), Some(true));
    assert_eq!(lines_of(&c, "helix_cw"), ["G2 [X] [Y] [Z] [I] [J]"]);
    assert_eq!(lines_of(&c, "first_arc_cw"), ["G2 [X] [Y] [I] [J] [F]"]);
    assert_eq!(lines_of(&c, "plunge"), ["G1 [Z] [F]"]);
    assert!(codes(&c).contains(&"import.pp.block_fallback"));
}

#[test]
fn radius_arcs_use_r_mode() {
    let c = ok(&format!(
        "{}VAR RADIUS = [Radius|A|R|1.3]\n\
         begin CW_ARC_MOVE\n\"G2 [X] [Y] [RADIUS]\"\nbegin CCW_ARC_MOVE\n\"G3 [X] [Y] [Radius]\"\n",
        pp("")
    ));
    let t = table(&c);
    assert_eq!(t["arcs"]["centre_mode"].as_str(), Some("r"));
    assert_eq!(t["arcs"]["max_sweep_deg"].as_integer(), Some(90));
    assert_eq!(t["words"]["R"]["placeholder"].as_str(), Some("Radius"));
    assert_eq!(lines_of(&c, "arc_cw"), ["G2 [X] [Y] [Radius]"]);
}

#[test]
fn arcs_without_a_centre_are_linearized() {
    let c = ok(&format!(
        "{}begin CW_ARC_MOVE\n\"ARC [X] [Y] [ArcMidX]\"\nbegin CCW_ARC_MOVE\n\"ARC [X] [Y] [ArcMidX]\"\n",
        pp("")
    ));
    assert_eq!(table(&c)["arcs"]["enabled"].as_bool(), Some(false));
    assert!(codes(&c).contains(&"import.pp.arcs_linearized"));
}

#[test]
fn initial_rapid_guards_with_state_moved() {
    let c = ok(&format!(
        "{}begin INITIAL_RAPID_MOVE\n\"G0 [X] [Y]\"\n\"G0 [Z]\"\n",
        pp("")
    ));
    assert_eq!(
        lines_of(&c, "first_rapid"),
        [
            "!state.moved ? G0 [X] [Y]",
            "!state.moved ? G0 [Z]",
            "state.moved ? G0 [X] [Y] [Z]"
        ]
    );
}

#[test]
fn character_codes_and_literal_brackets() {
    let c = ok(&format!(
        "{}begin NEW_SEGMENT\n\"[91]Job[93] [34]x[34] [ [126]\"\n",
        pp("")
    ));
    assert_eq!(
        lines_of(&c, "toolpath_start"),
        ["!state.first_tool ? [LBRACKET]Job] \"x\" [LBRACKET] ~"]
    );
}

#[test]
fn unsupported_placeholders() {
    let c = ok(&format!(
        "{}VAR POWER = [P|C|S|1.0]\n\
         begin NEW_SEGMENT\n\"( [MATERIAL_THING] )\"\n\"(Path [TOOLPATH_NAME] [NOTES_X])\"\n\
         \"G1 [X] [Y] [P]\"\n\"G0 Z[ZLENGTH]\"\n\"Q[ZLENGTH]\"\n\"([POST_NAME])\"\n",
        pp("")
    ));
    assert_eq!(
        lines_of(&c, "toolpath_start"),
        [
            "!state.first_tool ? (Path [TOOLPATH_NAME] )",
            "!state.first_tool ? G1 [X] [Y]",
            "!state.first_tool ? G0",
            "!state.first_tool ? (Test (mm))"
        ]
    );
    let k = codes(&c);
    assert!(k.contains(&"import.pp.placeholder"));
    assert!(k.contains(&"import.pp.line_dropped"));
}

#[test]
fn placeholder_names_ignore_case() {
    let c = ok(&format!(
        "{}begin NEW_SEGMENT\n\"[toolname] [s]\"\n",
        pp("")
    ));
    assert_eq!(
        lines_of(&c, "toolpath_start"),
        ["!state.first_tool ? [TOOLNAME] [S]"]
    );
}

#[test]
fn line_numbers_from_placeholder() {
    let c = ok(&pp("LINE_NUMBER_START = 10").replace("\"G21 G90\"", "\"[N] G21 G90\""));
    assert!(codes(&c).contains(&"import.pp.var_missing"));
    assert_eq!(table(&c)["words"]["N"]["prefix"].as_str(), Some("N"));
}

#[test]
fn wrapped_output_adds_the_rotary_word() {
    let c = ok(&pp("ROTARY_WRAP_Y = \"-A\""));
    let t = table(&c);
    assert_eq!(t["rotary"]["enabled"].as_bool(), Some(true));
    assert_eq!(t["feed"]["rotary_method"].as_str(), Some("linear_only"));
    assert_eq!(t["words"]["A"]["prefix"].as_str(), Some("A"));
    assert_eq!(t["words"]["A"]["scale"].as_float(), Some(-1.0));
    assert_eq!(lines_of(&c, "rapid"), ["G0 [X] [Y] [A] [Z]"]);
    assert_eq!(lines_of(&c, "rotary"), ["G1 [X] [Y] [A] [Z] [F]"]);
    assert!(codes(&c).contains(&"import.pp.rotary_wrap"));
}

#[test]
fn comment_style_is_learned() {
    let c = ok(&pp("").replace("\"G21 G90\"", "\"; start\"\n\"G21 G90\""));
    assert_eq!(table(&c)["output"]["comment_start"].as_str(), Some(";"));
    let c = ok(&pp("")
        .replace("\"G21 G90\"", "\"PA;\"")
        .replace("\"G0 ", "\"PU")
        .replace("\"G1 ", "\"PD")
        .replace("\"M30\"", "\"SP0;\""));
    assert!(codes(&c).contains(&"import.pp.comment_syntax"));
}

#[test]
fn rapid_falls_back_to_feed_and_unknown_blocks_are_reported() {
    let src = pp("")
        .replace("begin RAPID_MOVE\n\"G0 [X] [Y] [Z]\"\n", "")
        .replace("begin FOOTER", "begin JET_TOOL_ON\n\"M3\"\nbegin FOOTER");
    let c = ok(&src);
    assert_eq!(lines_of(&c, "rapid"), ["G1 [X] [Y] [Z] [F]"]);
    let k = codes(&c);
    assert!(k.contains(&"import.pp.block_fallback"));
    assert!(k.contains(&"import.pp.block_unsupported"));
}

#[test]
fn unknown_lines_are_reported() {
    let c = ok(&format!("{}%\nthis is not a post line\n", pp("")));
    let n = codes(&c)
        .iter()
        .filter(|k| **k == "import.pp.unknown_line")
        .count();
    assert_eq!(n, 2);
}

#[test]
fn post_base_merges_child_over_base() {
    let base = pp("VAR TOOL = [T|A|T|1.0]");
    let child = "POST_NAME = \"Child\"\nPOST_BASE = \"base.pp\"\n\
                 VAR XPOS = [X|A|X|1.4]\nbegin FOOTER\n\"M2\"\n";
    let mut asked = Vec::new();
    let c = import_with_base(child.as_bytes(), &opts(), &mut |n| {
        asked.push(n.to_owned());
        Some(base.clone().into_bytes())
    })
    .unwrap();
    assert_eq!(asked, ["base.pp"]);
    assert_eq!(c.name, "Child");
    assert_eq!(c.report.base_files, 1);
    assert_eq!(lines_of(&c, "footer"), ["M2"]);
    assert_eq!(lines_of(&c, "rapid"), ["G0 [X] [Y] [Z]"]);
    assert_eq!(word_text(&c, "X", 1.0), "X1.0000");
    // A base that names itself is a cycle.
    let looped = format!("{}POST_BASE = \"base.pp\"\n", pp(""));
    let r = import_with_base(child.as_bytes(), &opts(), &mut |_| {
        Some(looped.clone().into_bytes())
    });
    assert_eq!(r.unwrap_err()[0].code, "import.pp.post_base");
    // Paths are not followed.
    let r = import_with_base(b"POST_BASE = \"../x.pp\"\n", &opts(), &mut |_| {
        panic!("not asked")
    });
    assert_eq!(r.unwrap_err()[0].code, "import.pp.post_base");
}

#[test]
fn output_is_escaped_toml() {
    let c = ok(&format!(
        "{}begin NEW_SEGMENT\n\"(a \\ b [34] c)\"\n",
        pp("").replace("Test (mm)", "Name with \\ and quote")
    ));
    let t = table(&c);
    assert_eq!(t["name"].as_str(), Some("Name with \\ and quote"));
    assert_eq!(
        lines_of(&c, "toolpath_start"),
        ["!state.first_tool ? (a \\ b \" c)"]
    );
}

#[test]
fn default_id_from_file_stem() {
    assert_eq!(
        Options::for_file_stem("My Router_mm").post_id,
        "vectric/my-router-mm"
    );
    assert_eq!(Options::for_file_stem("").post_id, "vectric/post");
}
