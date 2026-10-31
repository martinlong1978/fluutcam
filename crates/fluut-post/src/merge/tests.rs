// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use fluut_model::MemStore;

const PARENT: &str = r#"
id = "fluut/fanuc"
version = "1.0"
[blocks.header]
lines = [
  { id = "pct",   line = "%" },
  { id = "units", line = "G21" },
  { id = "safe",  line = "G17 G40 G49 G80 G90" },
]
[blocks.footer]
lines = ["M30", "%"]
"#;

fn resolve_child(child: &str) -> Result<Resolved, DefError> {
    let mut s = MemStore::new();
    s.insert_toml(PARENT).unwrap();
    s.insert_toml(child).unwrap();
    resolve_post(&s, &DefRef::parse("c").unwrap(), None)
}

fn child(block: &str) -> String {
    format!("id = 'c'\nversion = '1.0'\nextends = 'fluut/fanuc@1'\n{block}")
}

/// `(line, layer)` for each line of `blocks.<name>`.
fn lines(r: &Resolved, name: &str) -> Vec<(String, String)> {
    r.table["blocks"][name]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["line"].as_str().unwrap().to_owned(),
                l["layer"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn texts(r: &Resolved, name: &str) -> Vec<String> {
    lines(r, name).into_iter().map(|(l, _)| l).collect()
}

fn merge_err(block: &str) -> String {
    match resolve_child(&child(block)) {
        Err(DefError::Merge { msg, .. }) => msg,
        other => panic!("expected a merge error, got {other:?}"),
    }
}

#[test]
fn parent_lines_record_their_layer() {
    let r = resolve_child(&child("")).unwrap();
    assert_eq!(
        lines(&r, "footer"),
        [
            ("M30".to_owned(), "fluut/fanuc@1.0".to_owned()),
            ("%".to_owned(), "fluut/fanuc@1.0".to_owned())
        ]
    );
}

#[test]
fn replace_is_default() {
    let r = resolve_child(&child("[blocks.footer]\nlines = ['M2']")).unwrap();
    assert_eq!(lines(&r, "footer"), [("M2".to_owned(), "c@1.0".to_owned())]);
    let r = resolve_child(&child("[blocks.footer]\nmode='replace'\nlines = ['M2']")).unwrap();
    assert_eq!(texts(&r, "footer"), ["M2"]);
}

#[test]
fn append_prepend_remove() {
    let r = resolve_child(&child("[blocks.footer]\nmode='append'\nlines=['X']")).unwrap();
    assert_eq!(texts(&r, "footer"), ["M30", "%", "X"]);
    let r = resolve_child(&child("[blocks.footer]\nmode='prepend'\nlines=['X']")).unwrap();
    assert_eq!(texts(&r, "footer"), ["X", "M30", "%"]);
    let r = resolve_child(&child("[blocks.footer]\nmode='remove'")).unwrap();
    assert!(texts(&r, "footer").is_empty());
}

#[test]
fn append_to_missing_block() {
    let r = resolve_child(&child("[blocks.comment]\nmode='append'\nlines=['(x)']")).unwrap();
    assert_eq!(texts(&r, "comment"), ["(x)"]);
}

#[test]
fn patch_all_operations() {
    let r = resolve_child(&child(
        r#"[blocks.header]
mode = "patch"
replace_line = { safe = "G17 G40 G49 G80 G90 G94" }
remove_line = ["pct"]
before = { units = ["(mm)"] }
after = { units = ["G54", { id = "x", line = "G64", when = "true" }] }
"#,
    ))
    .unwrap();
    assert_eq!(
        texts(&r, "header"),
        ["(mm)", "G21", "G54", "G64", "G17 G40 G49 G80 G90 G94"]
    );
    let l = lines(&r, "header");
    assert_eq!(l[1].1, "fluut/fanuc@1.0");
    assert_eq!(l[4].1, "c@1.0");
    let x = &r.table["blocks"]["header"]["lines"][3];
    assert_eq!(x["id"].as_str(), Some("x"));
    assert_eq!(x["when"].as_str(), Some("true"));
}

#[test]
fn patch_errors() {
    assert!(merge_err("[blocks.header]\nmode='patch'\nreplace_line={nope='x'}").contains("nope"));
    assert!(merge_err("[blocks.header]\nmode='patch'\nremove_line=['nope']").contains("nope"));
    assert!(merge_err("[blocks.header]\nmode='patch'\nafter={nope=['x']}").contains("nope"));
    // An id removed earlier in the same patch is gone.
    assert!(
        merge_err("[blocks.header]\nmode='patch'\nremove_line=['pct']\nbefore={pct=['x']}")
            .contains("pct")
    );
    assert!(merge_err("[blocks.header]\nmode='patch'\nlines=['x']").contains("lines"));
}

#[test]
fn duplicate_ids_rejected() {
    assert!(
        merge_err("[blocks.header]\nmode='append'\nlines=[{id='pct', line='x'}]")
            .contains("duplicate")
    );
}

#[test]
fn bad_blocks_rejected() {
    assert!(merge_err("[blocks.footer]\nmode='merge'\nlines=[]").contains("unknown mode"));
    assert!(merge_err("[blocks.footer]\nlines=[1]").contains("string or table"));
    assert!(merge_err("[blocks.footer]\nlines=[{text='x'}]").contains("unknown line key"));
    assert!(merge_err("[blocks.footer]\nlines=[{id='a'}]").contains("without `line`"));
    assert!(merge_err("[blocks.footer]\nlines=['x']\nextra=1").contains("extra"));
    assert!(merge_err("[blocks]\nmode='append'").contains("only allowed"));
}

/// Strips recorded layers so tables from different layer stacks compare.
fn without_layers(t: &Table) -> Table {
    let mut t = t.clone();
    if let Some(Value::Table(blocks)) = t.get_mut("blocks") {
        for (_, b) in blocks.iter_mut() {
            for l in b["lines"].as_array_mut().unwrap() {
                l.as_table_mut().unwrap().remove("layer");
            }
        }
    }
    t
}

#[test]
fn flatten_round_trip() {
    let r = resolve_child(&child(
        "[words.X]\ndecimals=4\n[blocks.header]\nmode='patch'\nafter={units=['G54']}",
    ))
    .unwrap();
    let flat = flatten(&r, "user/flat", "1.0");
    assert!(!flat.contains("extends"));
    assert!(!flat.contains("mode"));
    assert!(!flat.contains("layer"));
    let mut s = MemStore::new();
    s.insert_toml(&flat).unwrap();
    let again = resolve_post(&s, &DefRef::parse("user/flat").unwrap(), None).unwrap();
    assert!(flat.contains(POST_FORMAT));
    assert_eq!(without_layers(&again.table), without_layers(&r.table));
}

#[test]
fn machine_overrides_apply_last() {
    let mut s = MemStore::new();
    s.insert_toml(PARENT).unwrap();
    s.insert_toml(&child("[words.F]\ndecimals=1")).unwrap();
    let ov: Table = "'words.F.decimals' = 0".parse().unwrap();
    let layer = LayerId("machine:user/router@2.0".into());
    let r = resolve_post(&s, &DefRef::parse("c").unwrap(), Some((&ov, &layer))).unwrap();
    assert_eq!(r.table["words"]["F"]["decimals"].as_integer(), Some(0));
    assert_eq!(r.layers.last(), Some(&layer));
}
