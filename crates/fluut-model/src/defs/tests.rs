// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

fn store(defs: &[&str]) -> MemStore {
    let mut s = MemStore::new();
    for d in defs {
        s.insert_toml(d).unwrap();
    }
    s
}

fn r(s: &str) -> DefRef {
    DefRef::parse(s).unwrap()
}

fn get<'a>(t: &'a Table, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut v = t.get(parts.next()?)?;
    for p in parts {
        v = v.as_table()?.get(p)?;
    }
    Some(v)
}

/// A merger that records it was called and returns the child's `lines`
/// appended to the parent's.
#[derive(Debug)]
struct Append;

impl ModeMerger for Append {
    fn merge_mode(
        &self,
        _path: &str,
        parent: Option<&Value>,
        child: &Table,
        _layer: &LayerId,
    ) -> Result<Value, DefError> {
        let mut lines = parent
            .and_then(|p| p.get("lines"))
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default();
        lines.extend(child["lines"].as_array().unwrap().iter().cloned());
        let mut t = Table::new();
        t.insert("lines".into(), Value::Array(lines));
        Ok(Value::Table(t))
    }
}

#[test]
fn parse_refs() {
    assert_eq!(r("fluut/gcode").version, VersionReq::Latest);
    assert_eq!(r("fluut/gcode@3").version, VersionReq::Major(3));
    assert_eq!(
        r("fluut/gcode@3.2").version,
        VersionReq::Exact(Version { major: 3, minor: 2 })
    );
    assert_eq!(r("fluut/gcode@3.2").to_string(), "fluut/gcode@3.2");
    for bad in [
        "", "@3", "a@", "a@x", "a@3.", "a@3.2.1", "a b", "a@+3", "a@3@4",
    ] {
        assert!(DefRef::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn version_requires_major_minor() {
    assert!(Version::parse("3").is_err());
    assert!(Version::parse("3.x").is_err());
    assert_eq!(
        Version::parse("10.04").unwrap(),
        Version {
            major: 10,
            minor: 4
        }
    );
}

#[test]
fn store_picks_highest_matching() {
    let s = store(&[
        "id='a'\nversion='1.0'\nx=10",
        "id='a'\nversion='1.3'\nx=13",
        "id='a'\nversion='2.1'\nx=21",
    ]);
    let x = |q| s.lookup(&r(q)).unwrap().table["x"].as_integer().unwrap();
    assert_eq!(x("a"), 21);
    assert_eq!(x("a@1"), 13);
    assert_eq!(x("a@1.0"), 10);
    assert!(matches!(s.lookup(&r("a@3")), Err(DefError::NotFound(_))));
}

#[test]
fn deep_merge_tables_scalars_arrays() {
    let s = store(&[
        "id='p'\nversion='1.0'\n[w.X]\ndecimals=3\nprefix='X'\n[o]\nlist=[1,2,3]",
        "id='c'\nversion='1.0'\nextends='p@1'\n[w.X]\ndecimals=4\n[o]\nlist=[9]",
    ]);
    let out = resolve(&s, &r("c"), &NoModes).unwrap();
    assert_eq!(
        get(&out.table, "w.X.decimals").unwrap().as_integer(),
        Some(4)
    );
    assert_eq!(get(&out.table, "w.X.prefix").unwrap().as_str(), Some("X"));
    assert_eq!(
        get(&out.table, "o.list").unwrap().as_array().unwrap().len(),
        1
    );
    for meta in META_KEYS {
        assert!(!out.table.contains_key(meta), "{meta}");
    }
}

#[test]
fn extends_chain_three_deep() {
    let s = store(&[
        "id='a'\nversion='1.0'\nx=1\ny=1\nz=1",
        "id='b'\nversion='1.0'\nextends='a'\ny=2\nz=2",
        "id='c'\nversion='1.0'\nextends='b'\nz=3",
    ]);
    let out = resolve(&s, &r("c"), &NoModes).unwrap();
    let ints: Vec<i64> = ["x", "y", "z"]
        .iter()
        .map(|k| out.table[*k].as_integer().unwrap())
        .collect();
    assert_eq!(ints, [1, 2, 3]);
    let ids: Vec<&str> = out.layers.iter().map(|l| l.0.as_str()).collect();
    assert_eq!(ids, ["a@1.0", "b@1.0", "c@1.0"]);
}

#[test]
fn mixins_apply_in_order_with_conflict_warning() {
    let s = store(&[
        "id='base'\nversion='1.0'\nx=0\ny=0",
        "id='m1'\nversion='1.0'\nx=1\nonly1=1",
        "id='m2'\nversion='1.0'\nx=2\nonly2=2",
        "id='leaf'\nversion='1.0'\nextends='base'\nmixins=['m1','m2']\ny=9",
    ]);
    let out = resolve(&s, &r("leaf"), &NoModes).unwrap();
    assert_eq!(out.table["x"].as_integer(), Some(2));
    assert_eq!(out.table["y"].as_integer(), Some(9));
    assert_eq!(out.table["only1"].as_integer(), Some(1));
    assert_eq!(
        out.warnings,
        vec![DefWarning::MixinConflict {
            path: "x".into(),
            earlier: LayerId("m1@1.0".into()),
            later: LayerId("m2@1.0".into()),
        }]
    );
}

#[test]
fn mixin_mode_tables_act_on_parent() {
    let s = store(&[
        "id='base'\nversion='1.0'\n[blocks.h]\nlines=['a']",
        "id='m'\nversion='1.0'\n[blocks.h]\nmode='append'\nlines=['b']",
        "id='leaf'\nversion='1.0'\nextends='base'\nmixins=['m']",
    ]);
    let out = resolve(&s, &r("leaf"), &Append).unwrap();
    let lines = get(&out.table, "blocks.h.lines")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(out.provenance["blocks.h"], LayerId("m@1.0".into()));
}

#[test]
fn missing_parent_error() {
    let s = store(&["id='c'\nversion='1.0'\nextends='nope@1'"]);
    assert_eq!(
        resolve(&s, &r("c"), &NoModes),
        Err(DefError::NotFound("nope@1".into()))
    );
    let s = store(&["id='c'\nversion='1.0'\nmixins=['gone']"]);
    assert!(matches!(
        resolve(&s, &r("c"), &NoModes),
        Err(DefError::NotFound(_))
    ));
}

#[test]
fn cycle_error() {
    let s = store(&[
        "id='a'\nversion='1.0'\nextends='b'",
        "id='b'\nversion='1.0'\nextends='a'",
    ]);
    assert_eq!(
        resolve(&s, &r("a"), &NoModes),
        Err(DefError::Cycle(vec!["a".into(), "b".into(), "a".into()]))
    );
}

#[test]
fn diamond_is_not_a_cycle() {
    let s = store(&[
        "id='core'\nversion='1.0'\nx=1",
        "id='m1'\nversion='1.0'\nextends='core'",
        "id='m2'\nversion='1.0'\nextends='core'",
        "id='leaf'\nversion='1.0'\nmixins=['m1','m2']",
    ]);
    assert!(resolve(&s, &r("leaf"), &NoModes).is_ok());
}

#[test]
fn depth_limit() {
    let mut defs = vec!["id='d0'\nversion='1.0'".to_owned()];
    for i in 1..=MAX_DEPTH {
        defs.push(format!("id='d{i}'\nversion='1.0'\nextends='d{}'", i - 1));
    }
    let refs: Vec<&str> = defs.iter().map(String::as_str).collect();
    let s = store(&refs);
    assert!(resolve(&s, &r(&format!("d{}", MAX_DEPTH - 1)), &NoModes).is_ok());
    assert!(matches!(
        resolve(&s, &r(&format!("d{MAX_DEPTH}")), &NoModes),
        Err(DefError::TooDeep(_))
    ));
}

#[test]
fn provenance_records_layer() {
    let s = store(&[
        "id='p'\nversion='2.1'\n[w.X]\ndecimals=3\nprefix='X'\n[w.Y]\nprefix='Y'",
        "id='c'\nversion='1.0'\nextends='p@2'\n[w.X]\ndecimals=4",
    ]);
    let out = resolve(&s, &r("c"), &NoModes).unwrap();
    assert_eq!(out.provenance["w.X.decimals"].0, "c@1.0");
    assert_eq!(out.provenance["w.X.prefix"].0, "p@2.1");
    assert_eq!(out.provenance["w.Y.prefix"].0, "p@2.1");
}

#[test]
fn table_replacing_scalar_forgets_provenance() {
    let s = store(&[
        "id='p'\nversion='1.0'\nx=1",
        "id='c'\nversion='1.0'\nextends='p'\n[x]\ny=2",
    ]);
    let out = resolve(&s, &r("c"), &NoModes).unwrap();
    assert!(!out.provenance.contains_key("x"));
    assert_eq!(out.provenance["x.y"].0, "c@1.0");
}

#[test]
fn mode_table_goes_to_merger() {
    let s = store(&["id='a'\nversion='1.0'\n[blocks.h]\nmode='append'\nlines=['x']"]);
    let out = resolve(&s, &r("a"), &Append).unwrap();
    assert!(get(&out.table, "blocks.h.mode").is_none());
    assert!(matches!(
        resolve(&s, &r("a"), &NoModes),
        Err(DefError::Merge { .. })
    ));
}

#[test]
fn overrides_by_path() {
    let s = store(&["id='p'\nversion='1.0'\n[words.F]\ndecimals=1\nprefix='F'\n[n]\nv=1"]);
    let mut out = resolve(&s, &r("p"), &NoModes).unwrap();
    let ov: Table = "'words.F.decimals' = 0\n'new.leaf' = true\n[words.S]\nprefix='S'"
        .parse()
        .unwrap();
    let layer = LayerId("machine:m".into());
    apply_overrides(&mut out, &ov, &layer).unwrap();
    assert_eq!(
        get(&out.table, "words.F.decimals").unwrap().as_integer(),
        Some(0)
    );
    assert_eq!(
        get(&out.table, "words.F.prefix").unwrap().as_str(),
        Some("F")
    );
    assert_eq!(get(&out.table, "new.leaf").unwrap().as_bool(), Some(true));
    assert_eq!(
        get(&out.table, "words.S.prefix").unwrap().as_str(),
        Some("S")
    );
    assert_eq!(out.provenance["words.F.decimals"], layer);
    assert_eq!(out.provenance["words.S.prefix"], layer);

    let through_scalar: Table = "'n.v.w' = 1".parse().unwrap();
    assert!(matches!(
        apply_overrides(&mut out, &through_scalar, &layer),
        Err(DefError::Merge { .. })
    ));
    let empty_seg: Table = "'a..b' = 1".parse().unwrap();
    assert!(apply_overrides(&mut out, &empty_seg, &layer).is_err());
}

#[test]
fn def_requires_id_and_version() {
    assert_eq!(
        Def::from_toml("version='1.0'"),
        Err(DefError::Missing("id"))
    );
    assert_eq!(Def::from_toml("id='a'"), Err(DefError::Missing("version")));
    assert!(matches!(Def::from_toml("id="), Err(DefError::Toml(_))));
}
