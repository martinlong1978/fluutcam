// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use proptest::prelude::*;

fn cx() -> BTreeMap<String, Value> {
    [
        ("tool.number", Value::Num(3.0)),
        ("tool.diameter", Value::Num(6.0)),
        ("tool.name", Value::Str("flat 6".into())),
        ("machine.tool_changer", Value::Str("atc".into())),
        ("state.spindle_on", Value::Bool(true)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

fn ev(src: &str) -> Value {
    compile(src, &Schema::post()).unwrap().eval(&cx())
}

fn err(src: &str) -> String {
    compile(src, &Schema::post()).unwrap_err().to_string()
}

#[test]
fn precedence() {
    assert_eq!(ev("1 + 2 * 3"), Value::Num(7.0));
    assert_eq!(ev("(1 + 2) * 3"), Value::Num(9.0));
    assert_eq!(ev("10 - 4 - 3"), Value::Num(3.0));
    assert_eq!(ev("-2 * 3"), Value::Num(-6.0));
    assert_eq!(ev("1 + 1 == 2 && 3 > 2 || false"), Value::Bool(true));
    assert_eq!(ev("false && true || true"), Value::Bool(true));
    assert_eq!(ev("!false && false"), Value::Bool(false));
}

#[test]
fn comparisons_and_paths() {
    assert_eq!(ev("machine.tool_changer == 'atc'"), Value::Bool(true));
    assert_eq!(ev("machine.tool_changer != 'atc'"), Value::Bool(false));
    assert_eq!(ev("tool.diameter / 2 >= 3"), Value::Bool(true));
    assert_eq!(ev("state.spindle_on"), Value::Bool(true));
    // Missing values take their type's default.
    assert_eq!(ev("setup.kind == ''"), Value::Bool(true));
    assert_eq!(ev("state.rotary_clamped"), Value::Bool(false));
}

#[test]
fn functions() {
    assert_eq!(ev("abs(-2.5)"), Value::Num(2.5));
    assert_eq!(ev("round(2.5)"), Value::Num(3.0));
    assert_eq!(ev("round(-2.5)"), Value::Num(-3.0));
    assert_eq!(ev("round(1.2345, 2)"), Value::Num(1.23));
    assert_eq!(ev("min(4, tool.number, 9)"), Value::Num(3.0));
    assert_eq!(ev("max(4, tool.number)"), Value::Num(4.0));
}

#[test]
fn arity_and_unknown_function() {
    assert!(err("abs(1, 2)").contains("takes 1 arguments"));
    assert!(err("min(1)").contains("2 or more"));
    assert!(err("sqrt(4)").contains("unknown function `sqrt`"));
}

#[test]
fn unknown_path() {
    let e = compile("tool.colour == 'red'", &Schema::post()).unwrap_err();
    assert_eq!(e.col, 1);
    assert!(e.msg.contains("unknown path `tool.colour`"));
}

#[test]
fn type_mismatch() {
    assert!(err("tool.number == 'x'").contains("cannot compare a number with a string"));
    assert!(err("'a' < 'b'").contains("ordering comparison"));
    assert!(err("1 && true").contains("needs a boolean"));
    assert!(err("!1").contains("needs a boolean"));
    assert!(err("-true").contains("needs a number"));
    assert!(err("'a' + 1").contains("needs a number"));
    assert!(err("abs('x')").contains("needs a number"));
}

#[test]
fn syntax_errors() {
    assert!(err("1 +").contains("unexpected end"));
    assert!(err("(1").contains("expected `)`"));
    assert!(err("1 2").contains("unexpected `2`"));
    assert!(err("'open").contains("unterminated"));
    assert!(err("1 # 2").contains("unexpected character `#`"));
    assert!(err("tool.").contains("expected a name"));
    let e = compile("true && 5", &Schema::post()).unwrap_err();
    assert_eq!(e.col, 9);
}

#[test]
fn division_by_zero() {
    let e = compile("1 / 0", &Schema::post()).unwrap();
    assert_eq!(e.warnings.len(), 1);
    assert_eq!(e.eval(&cx()), Value::Num(0.0));
    // A zero only known at run time also gives 0, without a warning.
    let e = compile("1 / setup.index_a", &Schema::post()).unwrap();
    assert!(e.warnings.is_empty());
    assert_eq!(e.eval(&cx()), Value::Num(0.0));
}

#[test]
fn condition_must_be_boolean() {
    assert!(compile_condition("tool.number == 1", &Schema::post()).is_ok());
    assert!(
        compile_condition("tool.number", &Schema::post())
            .unwrap_err()
            .msg
            .contains("not a boolean")
    );
}

proptest! {
    #[test]
    fn compile_never_panics(s in "[ -~]{0,40}") {
        let _ = compile(&s, &Schema::post());
    }

    #[test]
    fn token_soup_never_panics(
        toks in prop::collection::vec(
            prop::sample::select(vec![
                "(", ")", "!", "-", "+", "*", "/", "==", "<", "&&", "||", ",", ".",
                "1", "0", "2.5", "'s'", "true", "tool", "number", "tool.number",
                "state.spindle_on", "abs", "round", "min",
            ]),
            0..16,
        )
    ) {
        let src = toks.join(" ");
        if let Ok(e) = compile(&src, &Schema::post()) {
            let _ = e.eval(&cx());
        }
    }
}
