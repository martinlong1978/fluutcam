// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::post::Line;
use crate::resolve_post;
use fluut_model::{DefRef, MemStore};

fn words() -> BTreeMap<String, String> {
    [("X", "X"), ("Y", "Y"), ("F", "F"), ("TN", "T")]
        .into_iter()
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

fn lit(s: &str) -> Tok {
    Tok::Lit(s.into())
}

#[test]
fn tokenises_words_and_placeholders() {
    let t = tokenize("G1 [X][Y] F[F]", &words()).unwrap();
    assert_eq!(
        t,
        [
            lit("G1 "),
            Tok::Word("X".into()),
            Tok::Word("Y".into()),
            lit(" F"),
            Tok::Word("F".into()),
        ]
    );
    // A word renamed by `placeholder` answers to its placeholder name.
    assert_eq!(
        tokenize("T[TN]", &words()).unwrap()[1],
        Tok::Word("T".into())
    );
    assert_eq!(
        tokenize("([TOOLNAME])", &words()).unwrap(),
        [lit("("), Tok::Placeholder("TOOLNAME".into()), lit(")")]
    );
}

#[test]
fn other_brackets_are_literal() {
    assert_eq!(
        tokenize("(a [b c] [ ] [x)", &words()).unwrap(),
        [lit("(a [b c] [ ] [x)")]
    );
    assert_eq!(tokenize("", &words()).unwrap(), []);
}

#[test]
fn unknown_placeholder_error() {
    assert_eq!(
        tokenize("G1 [Q9]", &words()).unwrap_err(),
        "unknown placeholder `[Q9]`"
    );
}

#[test]
fn block_errors_name_block_and_line() {
    let b = Block {
        lines: vec![
            Line {
                line: "M5".into(),
                ..Line::default()
            },
            Line {
                line: "T[NOPE]".into(),
                ..Line::default()
            },
        ],
    };
    let e = compile_block("tool_change", &b, &words(), &Schema::post()).unwrap_err();
    assert_eq!(
        e.to_string(),
        "blocks.tool_change line 2: unknown placeholder `[NOPE]`"
    );
    let b = Block {
        lines: vec![Line {
            line: "M0".into(),
            when: Some("tool.number".into()),
            ..Line::default()
        }],
    };
    let e = compile_block("tool_change", &b, &words(), &Schema::post()).unwrap_err();
    assert!(e.msg.contains("not a boolean"), "{e}");
}

#[test]
fn compiled_post_collects_errors_and_warnings() {
    let mut s = MemStore::new();
    s.insert_toml(
        r#"id = "p"
version = "1.0"
[arcs]
enabled = false
[words.X]
prefix = "X"
[blocks.rapid]
lines = ["G0 [X]", { when = "1 / 0 == 0", line = "(x)" }]
[blocks.feed]
lines = ["G1 [X] [BAD]"]
[blocks.footer]
lines = ["[ALSO_BAD]"]
"#,
    )
    .unwrap();
    let r = resolve_post(&s, &DefRef::parse("p").unwrap(), None).unwrap();
    let errs = CompiledPost::compile(crate::Post::load(&r).unwrap()).unwrap_err();
    assert_eq!(errs.len(), 2);

    let mut s = MemStore::new();
    s.insert_toml(
        "id='p'\nversion='1.0'\n[arcs]\nenabled=false\n[blocks.rapid]\nlines=[{when='1 / 0 == 0', line='G0'}]\n[blocks.feed]\nlines=['G1']",
    )
    .unwrap();
    let r = resolve_post(&s, &DefRef::parse("p").unwrap(), None).unwrap();
    let c = CompiledPost::compile(crate::Post::load(&r).unwrap()).unwrap();
    assert_eq!(c.warnings.len(), 1);
    assert!(c.warnings[0].starts_with("blocks.rapid line 1:"));
}
