use super::*;

/// A catalog over `src` alone, for tests that do not depend on the shipped
/// messages.
fn cat(src: &str) -> Catalog {
    Catalog::from_sources(&Locale::english(), &[src], false)
}

#[test]
fn lookup_without_arguments() {
    let c = cat("hello = Hello\n");
    assert_eq!(c.get("hello"), "Hello");
    assert!(c.has("hello"));
}

#[test]
fn lookup_with_arguments() {
    let c = cat("job = Calculating { $name } ({ $done } of { $total })\n");
    assert_eq!(
        c.format("job", &[("name", "Rough"), ("done", "1"), ("total", "3")]),
        "Calculating Rough (1 of 3)"
    );
}

#[test]
fn arguments_are_not_wrapped_in_isolation_marks() {
    let c = cat("a = x { $v } y\n");
    assert_eq!(c.format("a", &[("v", "7")]), "x 7 y");
}

#[test]
fn a_missing_argument_does_not_panic() {
    let c = cat("a = x { $v } y\n");
    let s = c.get("a");
    assert!(s.starts_with("x ") && s.ends_with(" y"), "{s}");
}

#[test]
fn missing_id_comes_back_as_the_id() {
    let c = cat("a = A\n");
    assert_eq!(c.get("no-such-id"), "no-such-id");
    assert_eq!(c.format("no-such-id", &[("x", "1")]), "no-such-id");
    assert!(!c.has("no-such-id"));
}

#[test]
fn unparsable_source_keeps_the_good_messages() {
    let c = cat("a = A\nthis is not fluent\nb = B\n");
    assert_eq!(c.get("a"), "A");
    assert_eq!(c.get("b"), "B");
}

#[test]
fn unknown_locale_falls_back_to_english() {
    let c = Catalog::new(&[Locale::parse("xx-YY").unwrap()]);
    assert_eq!(c.get("app-title"), "FluutCAM");
    let c = Catalog::new(&[Locale::parse("en-GB").unwrap()]);
    assert_eq!(c.get("ribbon-open"), "Open");
    assert_eq!(Catalog::new(&[]).get("ribbon-open"), "Open");
}

#[test]
fn chain_falls_back_message_by_message() {
    let first = bundle_from("fr", &["only-fr = Seulement\nboth = Fr\n"]);
    let second = bundle_from("en", &["both = En\nonly-en = Only\n"]);
    let c = Catalog {
        chain: vec![first, second],
        pseudo: false,
    };
    assert_eq!(c.get("both"), "Fr");
    assert_eq!(c.get("only-en"), "Only");
    assert_eq!(c.get("only-fr"), "Seulement");
    assert_eq!(c.get("neither"), "neither");
}

#[test]
fn pseudo_locale_wraps_accents_and_lengthens() {
    let c = Catalog::pseudo();
    let s = c.get("ribbon-open");
    assert!(s.starts_with('⟦') && s.ends_with('⟧'), "{s}");
    assert!(!s.contains("Open"), "letters are accented: {s}");
    assert!(s.chars().count() >= "Open".len() * 13 / 10 + 2, "{s}");
}

#[test]
fn pseudo_locale_leaves_placeables_and_arguments_alone() {
    let c = Catalog::from_sources(&Locale::english(), &["a = Open { $file } now\n"], true);
    let s = c.format("a", &[("file", "part.fluut")]);
    assert!(s.contains("part.fluut"), "{s}");
    assert!(!s.contains("$file"), "{s}");
    assert!(s.starts_with('⟦') && s.ends_with('⟧'), "{s}");
}

#[test]
fn pseudo_locale_shows_a_missing_id() {
    let s = Catalog::pseudo().get("no-such-id");
    assert!(s.contains("no-such-id"), "{s}");
}

#[test]
fn shipped_english_parses_and_has_unique_nonempty_messages() {
    let mut seen = std::collections::BTreeSet::new();
    for (name, src) in EN_FILES {
        let (_, errors) = parse_resource(src);
        assert!(errors.is_empty(), "{name}: {errors:?}");
        for id in message_ids(src) {
            assert!(seen.insert(id.clone()), "{name}: duplicate id {id}");
        }
        for line in src.lines() {
            let l = line.trim_end();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            // The pseudo-locale transforms single-line messages only.
            let (id, value) = l.split_once('=').unwrap_or_else(|| panic!("{name}: {l}"));
            assert!(
                !id.starts_with(char::is_whitespace) && !id.trim().is_empty(),
                "{name}: continuation line `{l}`"
            );
            assert!(!value.trim().is_empty(), "{name}: empty message `{l}`");
        }
    }
    let c = Catalog::english();
    for id in &seen {
        assert!(c.has(id), "{id}");
    }
}

#[test]
fn every_english_id_is_found_by_the_pseudo_locale() {
    let c = Catalog::pseudo();
    for (_, src) in EN_FILES {
        for id in message_ids(src) {
            assert!(c.has(&id), "{id}");
        }
    }
}

#[test]
fn message_ids_skip_comments_and_blank_lines() {
    let ids = message_ids("# c\n\na = A\nb-c = B { $x }\n");
    assert_eq!(ids, vec!["a".to_owned(), "b-c".to_owned()]);
}

#[test]
fn fallback_text_is_used_only_without_a_message() {
    let c = cat("a = A
");
    assert_eq!(c.get_or("a", "other"), "A");
    assert_eq!(c.get_or("b", "other"), "other");
    let p = Catalog::pseudo();
    let s = p.get_or("no-such-id", "other");
    assert!(s.starts_with('⟦') && s.contains("other"), "{s}");
}

/// `add_resource` refuses a second message with an id that a bundle has;
/// `bundle_from` ignores that at run time, so the shipped files are checked
/// here.
#[test]
fn shipped_english_files_have_no_duplicate_ids() {
    let mut seen = std::collections::BTreeMap::new();
    let mut duplicates = Vec::new();
    for (file, src) in EN_FILES {
        for id in message_ids(src) {
            if let Some(first) = seen.insert(id.clone(), file) {
                duplicates.push(format!("{id} ({first} and {file})"));
            }
        }
    }
    assert!(duplicates.is_empty(), "duplicate ids: {duplicates:?}");
}

#[test]
fn shipped_english_files_parse_and_load_without_overrides() {
    let mut b = FluentBundle::new_concurrent(vec!["en".parse().unwrap()]);
    for (file, src) in EN_FILES {
        let (r, errors) = parse_resource(src);
        assert!(errors.is_empty(), "{file}: {errors:?}");
        if let Err(e) = b.add_resource(r) {
            panic!("{file}: {e:?}");
        }
    }
}

#[test]
fn the_duplicate_check_sees_a_repeated_id() {
    let ids = message_ids("a = A\nb = B\na = Again\n");
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_ne!(ids.len(), sorted.len());
    // The bundle also refuses it.
    let (r, _) = parse_resource("a = A\na = Again\n");
    let mut b: Bundle = FluentBundle::new_concurrent(vec!["en".parse().unwrap()]);
    assert!(b.add_resource(r).is_err());
}
