// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

fn file(data: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'2;1');\n\
         FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF {{ 1 0 10303 442 1 1 4 }}'));\n\
         ENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

fn one(data: &str) -> (RawModel, Vec<Param>) {
    let m = parse(file(data).as_bytes()).unwrap();
    let RawEntity::Simple { params, .. } = m.get(1).unwrap().clone() else {
        panic!("not simple")
    };
    (m, params)
}

fn err(data: &str) -> ParseError {
    parse(file(data).as_bytes()).unwrap_err()
}

#[test]
fn header() {
    let m = parse(file("").as_bytes()).unwrap();
    let Some([Param::List(l)]) = m.header_entity("FILE_SCHEMA") else {
        panic!()
    };
    assert!(matches!(&l[0], Param::Str(s) if s.starts_with("AP242_")));
    assert!(m.entities.is_empty());
}

#[test]
fn every_token_kind() {
    let (m, p) = one(
        "#1=THING('a''b',#22,-3,1.,1.5E-3,-.5,.T.,.UNSPECIFIED.,$,*,(1,(2)),\
         LENGTH_MEASURE(2.5),\"0FF\",/* note */ 7);",
    );
    assert_eq!(p.len(), 14);
    assert_eq!(p[0], Param::Str("a'b".into()));
    assert_eq!(p[1], Param::Ref(22));
    assert_eq!(p[2], Param::Int(-3));
    assert_eq!(p[3], Param::Real(1.0));
    assert_eq!(p[4], Param::Real(1.5e-3));
    assert_eq!(p[5], Param::Real(-0.5));
    assert_eq!(p[6], Param::Enum("T".into()));
    assert_eq!(p[7], Param::Enum("UNSPECIFIED".into()));
    assert_eq!(p[8], Param::Unset);
    assert_eq!(p[9], Param::Derived);
    assert_eq!(
        p[10],
        Param::List(vec![Param::Int(1), Param::List(vec![Param::Int(2)])])
    );
    let lm = m.keywords.get("LENGTH_MEASURE").unwrap();
    assert_eq!(p[11], Param::Typed(lm, Box::new(Param::Real(2.5))));
    assert_eq!(p[12], Param::Binary("0FF".into()));
    assert_eq!(p[13], Param::Int(7));
}

#[test]
fn string_escapes() {
    let (_, p) = one(r"#1=A('\X2\00E9\X0\t\X\E9 \\ \S\a \X4\0001F600\X0\');");
    assert_eq!(p[0], Param::Str("ét\u{e9} \\ \u{e1} \u{1F600}".into()));
}

#[test]
fn string_across_lines() {
    let (_, p) = one("#1=A('ab\ncd');");
    assert_eq!(p[0], Param::Str("abcd".into()));
}

#[test]
fn complex_instance_and_keywords_case() {
    let m = parse(
        file("#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3) length_unit() NAMED_UNIT(*));").as_bytes(),
    )
    .unwrap();
    let RawEntity::Complex { parts, .. } = m.get(1).unwrap() else {
        panic!()
    };
    let names: Vec<_> = parts.iter().map(|(k, _)| m.keywords.name(*k)).collect();
    assert_eq!(
        names,
        [
            "GEOMETRIC_REPRESENTATION_CONTEXT",
            "LENGTH_UNIT",
            "NAMED_UNIT"
        ]
    );
}

#[test]
fn positions() {
    let m = parse(file("#1=A();\n  #2=B();").as_bytes()).unwrap();
    // Header is 6 lines, DATA; line 6, #1 on line 7.
    assert_eq!(m.get(1).unwrap().pos(), Pos { line: 7, col: 1 });
    assert_eq!(m.get(2).unwrap().pos(), Pos { line: 8, col: 3 });
}

#[test]
fn errors_have_positions() {
    let e = err("#1=A('abc);");
    assert!(e.msg.contains("unterminated string"), "{e}");
    assert_eq!(e.pos.line, 7);
    assert!(err("#1=A(1,2;").msg.contains("expected ',' or ')'"));
    assert!(err("#1=A();\n#1=B();").msg.contains("defined twice"));
    assert!(err("#1=A(.T);").msg.contains("enumeration"));
    assert!(err("#1=A(1.2.3);").msg.contains("bad number"));
    assert!(err("#1=A(/* x);").msg.contains("unterminated comment"));
    assert!(err("#1=();").msg.contains("empty complex"));
    let deep = format!("#1=A({}1{});", "(".repeat(100), ")".repeat(100));
    assert!(err(&deep).msg.contains("nested too deeply"));
}

#[test]
fn truncated_files_never_panic() {
    let f = file("#1=A('x',(1,2),#3,.T.,LENGTH_MEASURE(1.));\n#3=B($);");
    for n in 0..f.len() {
        let _ = parse(&f.as_bytes()[..n]);
    }
    for n in 0..f.len() {
        let mut b = f.as_bytes().to_vec();
        b[n] = b'(';
        let _ = parse(&b);
        b[n] = b'\'';
        let _ = parse(&b);
    }
}

#[test]
fn several_data_sections() {
    let src = "ISO-10303-21;HEADER;FILE_SCHEMA(('X'));ENDSEC;\
               DATA('a',('X'));#1=A();ENDSEC;DATA;#2=B();ENDSEC;END-ISO-10303-21;";
    let m = parse(src.as_bytes()).unwrap();
    assert_eq!(m.entities.len(), 2);
}

#[test]
fn escape_end_overlapping_its_start_is_an_error() {
    // Found by fuzzing: in `\X2\X0\` the end marker `\X0\` shares its
    // backslash with `\X2\`, which made the body slice run backwards.
    for s in ["\\X2\\X0\\", "\\X4\\X0\\"] {
        let e = err(&format!("#1=A('{s}');"));
        assert!(e.msg.contains("bad escape"), "{e:?}");
    }
    let raw = b"ISO-10303-21;HEADER;LE_H_N(('110.096&217:\xe3`/\\X4\\X0\\'\xa8D-I";
    assert!(parse(raw).is_err());
}

/// A DATA section of `n` varied instances: simple and complex ones, typed
/// values, strings, comments and binaries, some of them holding `;` and
/// `#n=` so that a cut may land inside them.
fn varied(n: usize) -> String {
    let mut s = String::new();
    for i in 1..=n {
        let id = 3 * i + 7;
        let line = match i % 9 {
            0 => format!("#{id}=CARTESIAN_POINT('p{i}',(1.5,-2.,3.E-2));"),
            1 => format!("#{id}=(NAMED_UNIT(*) LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));"),
            2 => format!("#{id}=A_{}('x;\n#{}=B();',#{});", i % 5, id + 1, id + 3),
            3 => format!(
                "/* a comment;\n#{}=C(); */ #{id}=MEASURE(LENGTH_MEASURE({i}.5),$,*);",
                id + 1
            ),
            4 => format!("#{id}=BIN(\"0;#{}=\",.T.);", id + 1),
            5 => format!("  #{id} = SPACED ( 'a''b' , ( 1 , ( 2 ) ) ) ;"),
            6 => format!("#{id}=K{}(#1,#2,(#3,#4));", i % 31),
            7 => format!(r"#{id}=STR('\X2\00E9\X0\;#9=');"),
            _ => format!("#{id}=EMPTY();"),
        };
        s.push_str(&line);
        s.push('\n');
    }
    s
}

fn chunked_matches_serial(src: &[u8], sizes: impl Iterator<Item = usize>) {
    let serial = parse_serial(src);
    for c in sizes {
        assert_eq!(parse_chunked(src, c), serial, "chunk size {c}");
    }
}

#[test]
fn chunked_parse_equals_serial() {
    let f = file(&varied(120));
    let m = parse_serial(f.as_bytes()).unwrap();
    assert_eq!(m.entities.len(), 120);
    chunked_matches_serial(f.as_bytes(), (1..300).chain([1000, 4000, 1 << 20]));
    assert_eq!(parse(f.as_bytes()), Ok(m));
}

#[test]
fn chunked_parse_splits_clean_files() {
    let data: String = (1..=500)
        .map(|i| format!("#{i}=CARTESIAN_POINT('',({i}.,0.,1.));\n"))
        .collect();
    let f = file(&data);
    let (r, info) = parse_with(f.as_bytes(), Split::Bytes(512));
    assert_eq!(r, parse_serial(f.as_bytes()));
    assert!(info.chunks > 10, "{info:?}");
    assert!(!info.retried, "{info:?}");
}

#[test]
fn cut_inside_a_string_is_retried_serially() {
    // The only `;` followed by `#` after the first entity is inside a string.
    let f = file("#1=A();\n#2=B('xxxxxxxx;\n#3=C();');\n#4=D();");
    let (r, info) = parse_with(f.as_bytes(), Split::Bytes(12));
    assert!(info.retried, "{info:?}");
    let m = r.unwrap();
    assert_eq!(m, parse_serial(f.as_bytes()).unwrap());
    assert!(m.get(3).is_none());
}

#[test]
fn chunked_parse_of_several_data_sections() {
    let src = format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('X'));ENDSEC;DATA('a',('X'));\n{}ENDSEC;\nDATA;\n{}ENDSEC;END-ISO-10303-21;",
        varied(40),
        varied(80).replace("#", "#9")
    );
    chunked_matches_serial(src.as_bytes(), 1..200);
}

#[test]
fn chunked_errors_equal_serial_errors() {
    let body = varied(60);
    let cases = [
        format!("{body}#1=A('abc);"),
        format!("#5=A();\n{body}#5=B();"),
        format!("{body}#1000=A(1,2;\n#1001=B();"),
        format!("{body}#1000=A({}1{});", "(".repeat(70), ")".repeat(70)),
        format!("{body}#1000=A(/* x);"),
        format!("{body}#1000=();"),
        format!("{body}#1000=A();ENDSEC"),
    ];
    for data in &cases {
        let f = file(data);
        let e = parse_serial(f.as_bytes()).unwrap_err();
        assert!(e.pos.line > 60, "{e}");
        chunked_matches_serial(f.as_bytes(), (1..120).step_by(7).chain([4000]));
    }
    // Truncated anywhere.
    let f = file(&varied(30));
    for n in (0..f.len()).step_by(3) {
        chunked_matches_serial(&f.as_bytes()[..n], [5, 64].into_iter());
    }
}

#[test]
fn binary_across_lines_keeps_line_numbers() {
    // Line 7: the binary opens; line 8: it closes; line 9: the error.
    let e = err("#1=A(\"0F\n0F\");\n#2=B(1,2;");
    assert_eq!(e.pos.line, 9, "{e}");
}
