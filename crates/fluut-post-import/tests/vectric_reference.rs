// SPDX-License-Identifier: MIT OR Apache-2.0

//! Converted Vectric posts on the reference toolpath suite (D10 §8, §9).
//!
//! The fixtures in `tests/fixtures/vectric/` are hand-written for these
//! tests. Each converted post must load, post the suite, match its golden
//! file (`FLUUT_BLESS=1` rewrites them), and move the tool through the same
//! points as a FluutCAM built-in post of the same controller family: every
//! endpoint the built-in writes appears, in order, in the converted post's
//! output (the converted post may add points, for example where it writes
//! arcs outside XY as lines), and the same tools and spindle speeds are
//! used.
//!
//! `FLUUT_VECTRIC_PAIRS_DIR` names a local folder of `<post>.pp` files, each
//! with a G-code file of the same stem made by Vectric software from that
//! post. The G-code cannot be compared byte for byte, because FluutCAM's
//! reference suite is not the job Vectric posted; the test compares the
//! number formats instead (decimals, sign, separator, zero padding per
//! address letter) and the line ending. Skipped when the variable is unset.

use fluut_model::{DefRef, MemStore};
use fluut_post::builtin::builtin_store;
use fluut_post::reference::{SuiteOptions, reference_job};
use fluut_post::{CompiledPost, Post, Provenance, post, resolve_post};
use fluut_post_import::vectric::{self, Converted, Options};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn convert(name: &str) -> Converted {
    let bytes = std::fs::read(dir().join("fixtures/vectric").join(format!("{name}.pp"))).unwrap();
    vectric::import(
        &bytes,
        &Options {
            post_id: format!("test/{name}"),
            name: None,
        },
    )
    .unwrap_or_else(|d| panic!("{name}: {d:?}"))
}

fn compiled(store: &MemStore, id: &str) -> (CompiledPost, Provenance) {
    let r = resolve_post(store, &DefRef::parse(id).unwrap(), None).unwrap();
    let cp = CompiledPost::compile(Post::load(&r).unwrap()).unwrap();
    (cp, Provenance::of(&r, id))
}

/// The reference suite posted with the converted post, and the suite
/// options used (so the built-in gets the same toolpaths).
fn post_converted(c: &Converted, id: &str) -> (String, SuiteOptions) {
    let mut s = MemStore::new();
    s.insert_toml(&c.post).unwrap();
    let (cp, prov) = compiled(&s, id);
    let opts = SuiteOptions::for_post(&cp);
    let out = post(&reference_job(opts, prov), &cp).unwrap();
    assert_eq!(out.files.len(), 1);
    (out.files[0].clone(), opts)
}

/// The suite posted with a built-in post that takes the same segments as
/// the converted post: canned cycles expanded (Vectric posts have no cycle
/// blocks) and the converted post's `[arcs]` and `[helix]` settings, so both
/// split and linearize alike.
fn post_builtin(base: &str, c: &Converted, opts: SuiteOptions) -> String {
    let t: toml::Table = c.post.parse().unwrap();
    let mut leaf = toml::Table::new();
    for k in ["arcs", "helix"] {
        leaf.insert(k.into(), t[k].clone());
    }
    let mut s = builtin_store();
    s.insert_toml(&format!(
        "id = 'test/builtin'\nversion = '1.0'\nextends = '{base}@1'\n{}[cycles]\nenabled = false",
        toml::to_string(&leaf).unwrap()
    ))
    .unwrap();
    let (cp, prov) = compiled(&s, "test/builtin");
    post(&reference_job(opts, prov), &cp)
        .unwrap()
        .files
        .concat()
}

/// Golden files are stored with LF line ends (git normalizes text files);
/// line endings are checked by the tests themselves.
fn golden(name: &str, ext: &str, text: &str) {
    let text = &text.replace("\r\n", "\n");
    let path = dir().join("golden/vectric").join(format!("{name}.{ext}"));
    if std::env::var_os("FLUUT_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing: run with FLUUT_BLESS=1", path.display()));
    if *text != want {
        let line = text
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "{name}.{ext} differs at line {}:\n  got:  {:?}\n  want: {:?}",
            line + 1,
            text.lines().nth(line),
            want.lines().nth(line)
        );
    }
}

/// How to read numbers back.
#[derive(Clone, Copy)]
struct Read {
    comma: bool,
    inch: bool,
    /// Sign of the rotary word relative to FluutCAM's A.
    a_sign: f64,
}

const MM: Read = Read {
    comma: false,
    inch: false,
    a_sign: 1.0,
};

/// Code text of a line: comments (`( … )`, `;` to the end) removed.
fn code(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ';' if depth == 0 => break,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Address words of a code line: letter and number text.
fn words(line: &str) -> Vec<(char, String)> {
    let b: Vec<char> = code(line).chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i].to_ascii_uppercase();
        if c.is_ascii_alphabetic() {
            let mut j = i + 1;
            let mut num = String::new();
            while j < b.len() && (b[j].is_ascii_digit() || matches!(b[j], '.' | ',' | '+' | '-')) {
                num.push(b[j]);
                j += 1;
            }
            if !num.is_empty() {
                out.push((c, num));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn number(s: &str, r: Read) -> f64 {
    let s = if r.comma {
        s.replace(',', ".")
    } else {
        s.to_owned()
    };
    s.parse()
        .unwrap_or_else(|_| panic!("`{s}` is not a number"))
}

/// Tool positions after each motion line, in mm: X, Y, Z, A.
fn endpoints(text: &str, r: Read) -> Vec<[f64; 4]> {
    let mut pos = [f64::NAN; 4];
    let mut out: Vec<[f64; 4]> = Vec::new();
    for line in text.lines() {
        let w = words(line);
        let mut moved = false;
        let mut motion = false;
        for (c, n) in &w {
            let unit = if r.inch { 25.4 } else { 1.0 };
            match c {
                'G' => motion |= matches!(number(n, r) as i64, 0..=3),
                'X' => (pos[0], moved) = (number(n, r) * unit, true),
                'Y' => (pos[1], moved) = (number(n, r) * unit, true),
                'Z' => (pos[2], moved) = (number(n, r) * unit, true),
                'A' => (pos[3], moved) = (number(n, r) * r.a_sign, true),
                _ => {}
            }
        }
        let skip = w
            .iter()
            .any(|(c, n)| *c == 'G' && matches!(number(n, r) as i64, 4 | 80..=89));
        if moved
            && !skip
            && (motion || !w.iter().any(|(c, _)| *c == 'G'))
            && !out.last().is_some_and(|l| close(l, &pos, 0.0))
        {
            out.push(pos);
        }
    }
    out
}

fn close(a: &[f64; 4], b: &[f64; 4], tol: f64) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x.is_nan() && y.is_nan()) || (x - y).abs() <= tol)
}

/// Every point of `want` appears in `got`, in order.
fn assert_subsequence(name: &str, want: &[[f64; 4]], got: &[[f64; 4]], tol: f64) {
    let mut it = got.iter();
    for (i, w) in want.iter().enumerate() {
        assert!(
            it.any(|g| close(g, w, tol)),
            "{name}: built-in point {i} {w:?} not found in order in the converted output"
        );
    }
}

/// Values of one address letter, from lines that are not comments.
fn values(text: &str, letter: char, r: Read) -> BTreeSet<i64> {
    text.lines()
        .flat_map(words)
        .filter(|(c, _)| *c == letter)
        .map(|(_, n)| number(&n, r).round() as i64)
        .collect()
}

/// Tool numbers named anywhere, comments included (a manual change names
/// the tool in a comment).
fn tools(text: &str) -> BTreeSet<u32> {
    let b = text.as_bytes();
    let mut out = BTreeSet::new();
    for i in 0..b.len() {
        if b[i] == b'T' && (i == 0 || !b[i - 1].is_ascii_alphabetic()) {
            let n: String = text[i + 1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(n) = n.parse() {
                out.insert(n);
            }
        }
    }
    out
}

fn check_family(name: &str, base: &str, r: Read, tol: f64) -> String {
    let c = convert(name);
    let (text, opts) = post_converted(&c, &format!("test/{name}"));
    golden(name, "post.toml", &c.post);
    golden(name, "nc", &text);
    let builtin = post_builtin(base, &c, opts);
    assert_subsequence(name, &endpoints(&builtin, MM), &endpoints(&text, r), tol);
    assert_eq!(tools(&text), tools(&builtin), "{name}: tools");
    assert_eq!(
        values(&text, 'S', r),
        values(&builtin, 'S', MM),
        "{name}: spindle speeds"
    );
    text
}

#[test]
fn grbl_like_moves_like_fluut_grbl() {
    let c = convert("grbl-like");
    assert!(c.diagnostics.is_empty(), "{:?}", c.diagnostics);
    let text = check_family("grbl-like", "fluut/grbl", MM, 0.0011);
    assert!(text.ends_with("M5\nG0 Z25.000\nM30\n"), "{text}");
    // The first tool is set up by the header, the two later changes (to
    // the drill and back) by the tool change block.
    assert_eq!(text.matches("M0 (Load T").count(), 2);
    assert!(text.contains("(Job for 6 mm flat)"));
}

#[test]
fn linuxcnc_like_moves_like_fluut_linuxcnc() {
    let text = check_family("linuxcnc-like", "fluut/linuxcnc", MM, 0.0006);
    assert!(text.contains("\r\n"));
    let numbered: Vec<&str> = text.lines().filter(|l| l.starts_with('N')).collect();
    assert!(numbered[0].starts_with("N100 "), "{}", numbered[0]);
    assert!(numbered[1].starts_with("N105 "), "{}", numbered[1]);
    // SUBSTITUTE keeps parentheses in names out of the comment.
    assert!(text.contains("(Post: FluutCAM test, LinuxCNC-like [mm])"));
}

#[test]
fn comma_inch_moves_like_fluut_gcode() {
    let r = Read {
        comma: true,
        inch: true,
        a_sign: 1.0,
    };
    let text = check_family("comma-inch", "fluut/gcode", r, 0.0035);
    let first: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with('N'))
        .take(6)
        .collect();
    assert_eq!(first[0], "N0001;6 mm flat");
    assert_eq!(first[2], "N0003T01 M6");
    // The first rapid moves in XY, then down.
    assert_eq!(first[4], "N0005G0 X+0,0000 Y+0,0000");
    assert!(first[5].starts_with("N0006G0 Z+"), "{first:?}");
    assert!(text.contains("G2 X") && text.contains(" R0,"));
    let code: String = text.lines().filter(|l| !l.starts_with(';')).collect();
    assert!(!code.contains('.'), "a decimal point in a comma post");
}

#[test]
fn wrapped_post_turns_like_fluut_linuxcnc_4x() {
    let c = convert("wrap-y");
    let r = Read { a_sign: -1.0, ..MM };
    let (text, opts) = post_converted(&c, "test/wrap-y");
    assert!(opts.rotary);
    golden("wrap-y", "post.toml", &c.post);
    golden("wrap-y", "nc", &text);
    let builtin = post_builtin("fluut/linuxcnc-4x", &c, opts);
    assert_subsequence(
        "wrap-y",
        &endpoints(&builtin, MM),
        &endpoints(&text, r),
        0.0011,
    );
}

// ----- Vectric-made output pairs (local only)

/// Number shapes per address letter: decimals, explicit `+`, decimal
/// separator, zero padding of the integer part.
fn shapes(text: &str) -> BTreeMap<char, BTreeSet<(usize, bool, char, bool)>> {
    let mut out: BTreeMap<char, BTreeSet<_>> = BTreeMap::new();
    for l in text.lines() {
        for (c, n) in words(l) {
            let plus = n.starts_with('+');
            let body = n.trim_start_matches(['+', '-']);
            let (int, sep, dec) = match body.find(['.', ',']) {
                Some(i) => (&body[..i], body.as_bytes()[i] as char, body.len() - i - 1),
                None => (body, '.', 0),
            };
            let padded = int.len() > 1 && int.starts_with('0');
            out.entry(c).or_default().insert((dec, plus, sep, padded));
        }
    }
    out
}

#[test]
fn vectric_output_pairs() {
    let Some(dir) = std::env::var_os("FLUUT_VECTRIC_PAIRS_DIR") else {
        eprintln!("FLUUT_VECTRIC_PAIRS_DIR not set; pair test skipped");
        return;
    };
    let dir = PathBuf::from(dir);
    let mut checked = 0;
    let mut problems = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for pp in entries
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pp")))
    {
        let stem = pp.file_stem().unwrap();
        let Some(expected) = entries.iter().find(|p| {
            p.file_stem() == Some(stem)
                && !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pp"))
        }) else {
            continue;
        };
        let bytes = std::fs::read(pp).unwrap();
        let folder = pp.parent().unwrap().to_owned();
        let mut base = |n: &str| std::fs::read(folder.join(n)).ok();
        let id = "pairs/post".to_owned();
        let c = match vectric::import_with_base(
            &bytes,
            &Options {
                post_id: id.clone(),
                name: None,
            },
            &mut base,
        ) {
            Ok(c) => c,
            Err(d) => {
                problems.push(format!("{}: refused: {d:?}", pp.display()));
                continue;
            }
        };
        let (ours, _) = post_converted(&c, &id);
        let want = String::from_utf8_lossy(&std::fs::read(expected).unwrap()).into_owned();
        checked += 1;
        if want.contains("\r\n") != ours.contains("\r\n") {
            problems.push(format!("{}: line ending differs", pp.display()));
        }
        let (a, b) = (shapes(&ours), shapes(&want));
        for (letter, s) in &a {
            if let Some(w) = b.get(letter)
                && s != w
            {
                problems.push(format!(
                    "{}: {letter} numbers: ours {s:?}, Vectric's {w:?} (decimals, +, separator, zero padded)",
                    pp.display()
                ));
            }
        }
    }
    eprintln!("Vectric output pairs checked: {checked}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
