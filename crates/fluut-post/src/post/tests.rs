// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::resolve_post;
use fluut_model::{DefRef, MemStore};

const MIN: &str = r#"
id = "t"
version = "1.0"
[arcs]
enabled = false
[blocks.rapid]
lines = ["G0 [X][Y][Z]"]
[blocks.feed]
lines = ["G1 [X][Y][Z] F[F]"]
"#;

fn load(extra: &str) -> Result<LoadedPost, PostError> {
    let mut s = MemStore::new();
    s.insert_toml(MIN).unwrap();
    s.insert_toml(&format!(
        "id = 'c'\nversion = '1.0'\nextends = 't'\n{extra}"
    ))
    .unwrap();
    Post::load(&resolve_post(&s, &DefRef::parse("c").unwrap(), None).unwrap())
}

fn errors(extra: &str) -> Vec<String> {
    match load(extra) {
        Err(PostError::Invalid(e)) => e,
        other => panic!("expected validation errors, got {other:?}"),
    }
}

#[test]
fn defaults_applied() {
    let p = load("[words.X]\nprefix = 'X'").unwrap().post;
    assert_eq!(p.output.extension, "nc");
    assert_eq!(p.output.line_ending, LineEnding::Lf);
    assert_eq!(p.arcs.radius_tolerance, 0.002);
    assert_eq!(p.arcs.planes.len(), 3);
    let x = &p.words["X"];
    assert_eq!((x.decimals, x.scale, x.leading_zero), (3, 1.0, true));
    assert_eq!(x.when, When::Always);
    assert_eq!(p.blocks["feed"].lines[0].line, "G1 [X][Y][Z] F[F]");
    assert!(p.blocks["feed"].lines[0].layer.is_some());
}

#[test]
fn enums_parse() {
    let p = load(
        "[output]\nline_ending='crlf'\nunits='inch'\n[feed]\nrotary_method='inverse_time'\n\
         [words.F]\nwhen='changed'\nsign='always'",
    )
    .unwrap()
    .post;
    assert_eq!(p.output.line_ending.as_str(), "\r\n");
    assert_eq!(p.output.units, Units::Inch);
    assert_eq!(p.feed.rotary_method, RotaryMethod::InverseTime);
    assert_eq!(p.words["F"].when, When::Changed);
    assert_eq!(p.words["F"].sign, Sign::Always);
    assert!(matches!(
        load("[output]\nline_ending='lfcr'"),
        Err(PostError::Read(_))
    ));
}

#[test]
fn unknown_key_warns_with_path() {
    let w = load("[words.X]\ndecimal = 4").unwrap().warnings;
    assert_eq!(w, ["unknown key `words.X.decimal`"]);
}

#[test]
fn validation_errors() {
    let has = |extra: &str, needle: &str| {
        let e = errors(extra);
        assert!(e.iter().any(|m| m.contains(needle)), "{needle}: {e:?}");
    };
    has("[blocks.bogus]\nlines=[]", "unknown block `bogus`");
    has("[arcs]\nenabled=true", "`arc_cw`");
    has("[helix]\nenabled=true", "`helix_ccw`");
    has("[rotary]\nenabled=true", "`rotary`");
    has("[cycles]\nenabled=true", "`cycle_point`");
    has("[arcs]\ncentre_mode='r'", "max_sweep_deg <= 180");
    has("[arcs]\nmax_sweep_deg=0", "(0, 360]");
    has("[arcs]\nmin_radius=10.0\nmax_radius=1.0", "min_radius");
    has("[output]\ndecimal_char=''", "decimal_char");
    has("[line_numbering]\nincrement=0", "increment");
    has("[words.X]\ndecimals=12", "words.X.decimals");
}

#[test]
fn first_and_comment_blocks_known() {
    assert!(
        load("[blocks.first_feed]\nlines=['G1']\n[blocks.comment]\nlines=['([TEXT])']").is_ok()
    );
    assert!(!errors("[blocks.first_header]\nlines=[]").is_empty());
}

#[test]
fn missing_motion_blocks() {
    let mut s = MemStore::new();
    s.insert_toml("id='e'\nversion='1.0'\n[arcs]\nenabled=false")
        .unwrap();
    let r = resolve_post(&s, &DefRef::parse("e").unwrap(), None).unwrap();
    match Post::load(&r) {
        Err(PostError::Invalid(e)) => assert_eq!(e.len(), 2),
        other => panic!("{other:?}"),
    }
}
