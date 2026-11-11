// SPDX-License-Identifier: MIT OR Apache-2.0

//! Post condition expressions (spec 05 §5.4.3, OQ-14; D10 §3.2).
//!
//! A hand-written lexer and recursive-descent parser that type-checks while
//! parsing, so every error is found at compile time. Evaluation is total:
//! division by zero gives 0 and a missing context value takes its type's
//! default.

use std::collections::BTreeMap;
use std::fmt;

#[cfg(test)]
mod tests;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Ty {
    Num,
    Str,
    Bool,
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Ty::Num => "number",
            Ty::Str => "string",
            Ty::Bool => "boolean",
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Value {
    fn default_of(ty: Ty) -> Value {
        match ty {
            Ty::Num => Value::Num(0.0),
            Ty::Str => Value::Str(String::new()),
            Ty::Bool => Value::Bool(false),
        }
    }

    fn ty(&self) -> Ty {
        match self {
            Value::Num(_) => Ty::Num,
            Value::Str(_) => Ty::Str,
            Value::Bool(_) => Ty::Bool,
        }
    }

    fn num(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            _ => 0.0,
        }
    }

    fn bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }
}

/// The context paths an expression may read, with their types.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Schema {
    paths: BTreeMap<String, Ty>,
}

impl Schema {
    pub fn new() -> Schema {
        Schema::default()
    }

    pub fn with(mut self, path: &str, ty: Ty) -> Schema {
        self.paths.insert(path.to_owned(), ty);
        self
    }

    pub fn get(&self, path: &str) -> Option<Ty> {
        self.paths.get(path).copied()
    }

    /// The paths posts can use (D10 §3.2).
    pub fn post() -> Schema {
        use Ty::*;
        [
            ("tool.number", Num),
            ("tool.name", Str),
            ("tool.diameter", Num),
            ("tool.corner_radius", Num),
            ("machine.name", Str),
            ("machine.tool_changer", Str),
            ("machine.has_rotary", Bool),
            ("setup.name", Str),
            ("setup.kind", Str),
            ("setup.index", Num),
            ("setup.index_a", Num),
            ("setup.index_b", Num),
            ("setup.work_offset", Str),
            ("toolpath.name", Str),
            ("toolpath.type", Str),
            ("toolpath.index", Num),
            ("state.coolant", Str),
            ("state.spindle_on", Bool),
            ("state.spindle_rpm", Num),
            ("state.feed", Num),
            ("state.rotary_clamped", Bool),
            ("state.first_tool", Bool),
            ("state.moved", Bool),
            ("marker.kind", Str),
            ("cycle.kind", Str),
        ]
        .into_iter()
        .fold(Schema::new(), |s, (p, t)| s.with(p, t))
    }
}

/// Values for the context paths at evaluation time.
pub trait Context {
    fn get(&self, path: &str) -> Option<Value>;
}

impl Context for BTreeMap<String, Value> {
    fn get(&self, path: &str) -> Option<Value> {
        BTreeMap::get(self, path).cloned()
    }
}

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[error("column {col}: {msg}")]
pub struct ExprError {
    /// 1-based character column.
    pub col: usize,
    pub msg: String,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Func {
    Abs,
    Round,
    Min,
    Max,
}

#[derive(Clone, PartialEq, Debug)]
enum Node {
    Lit(Value),
    Path(String, Ty),
    Not(Box<Node>),
    Neg(Box<Node>),
    Bin(BinOp, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

/// A compiled, type-checked expression.
#[derive(Clone, PartialEq, Debug)]
pub struct Expr {
    root: Node,
    pub ty: Ty,
    /// e.g. division by a literal zero.
    pub warnings: Vec<String>,
}

/// Compiles `src` against `schema`.
pub fn compile(src: &str, schema: &Schema) -> Result<Expr, ExprError> {
    let toks = lex(src)?;
    let mut p = Parser {
        toks,
        i: 0,
        schema,
        warnings: Vec::new(),
        end: src.chars().count() + 1,
    };
    let (root, ty) = p.or()?;
    if let Some(t) = p.toks.get(p.i) {
        return Err(p.err_at(t.col, format!("unexpected `{}`", t.tok)));
    }
    Ok(Expr {
        root,
        ty,
        warnings: p.warnings,
    })
}

/// Compiles a line condition, which must be boolean.
pub fn compile_condition(src: &str, schema: &Schema) -> Result<Expr, ExprError> {
    let e = compile(src, schema)?;
    if e.ty != Ty::Bool {
        return Err(ExprError {
            col: 1,
            msg: format!("condition is a {}, not a boolean", e.ty),
        });
    }
    Ok(e)
}

impl Expr {
    pub fn eval(&self, cx: &dyn Context) -> Value {
        eval(&self.root, cx)
    }
}

fn eval(n: &Node, cx: &dyn Context) -> Value {
    match n {
        Node::Lit(v) => v.clone(),
        Node::Path(p, ty) => match cx.get(p) {
            Some(v) if v.ty() == *ty => v,
            _ => Value::default_of(*ty),
        },
        Node::Not(a) => Value::Bool(!eval(a, cx).bool()),
        Node::Neg(a) => Value::Num(-eval(a, cx).num()),
        Node::Bin(op, a, b) => {
            // Short-circuit logic; everything else evaluates both sides.
            match op {
                BinOp::Or => return Value::Bool(eval(a, cx).bool() || eval(b, cx).bool()),
                BinOp::And => return Value::Bool(eval(a, cx).bool() && eval(b, cx).bool()),
                _ => {}
            }
            let (x, y) = (eval(a, cx), eval(b, cx));
            match op {
                BinOp::Eq => Value::Bool(x == y),
                BinOp::Ne => Value::Bool(x != y),
                BinOp::Lt => Value::Bool(x.num() < y.num()),
                BinOp::Le => Value::Bool(x.num() <= y.num()),
                BinOp::Gt => Value::Bool(x.num() > y.num()),
                BinOp::Ge => Value::Bool(x.num() >= y.num()),
                BinOp::Add => Value::Num(x.num() + y.num()),
                BinOp::Sub => Value::Num(x.num() - y.num()),
                BinOp::Mul => Value::Num(x.num() * y.num()),
                BinOp::Div if y.num() == 0.0 => Value::Num(0.0),
                BinOp::Div => Value::Num(x.num() / y.num()),
                BinOp::Or | BinOp::And => unreachable!("handled above"),
            }
        }
        Node::Call(f, args) => {
            let v: Vec<f64> = args.iter().map(|a| eval(a, cx).num()).collect();
            Value::Num(match f {
                Func::Abs => v[0].abs(),
                Func::Round if v.len() == 2 => {
                    let k = libm::pow(10.0, libm::round(v[1]).clamp(0.0, 9.0));
                    libm::round(v[0] * k) / k
                }
                Func::Round => libm::round(v[0]),
                Func::Min => v.iter().copied().fold(f64::INFINITY, f64::min),
                Func::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            })
        }
    }
}

// --- lexer -------------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Num(n) => write!(f, "{n}"),
            Tok::Str(s) => write!(f, "'{s}'"),
            Tok::Ident(s) => f.write_str(s),
            Tok::Op(o) => f.write_str(o),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Spanned {
    tok: Tok,
    col: usize,
}

const OPS: [&str; 17] = [
    "||", "&&", "==", "!=", "<=", ">=", "!", "<", ">", "+", "-", "*", "/", "(", ")", ",", ".",
];

fn lex(src: &str) -> Result<Vec<Spanned>, ExprError> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let col = i + 1;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = cs[start..i].iter().collect();
            let n = text.parse().map_err(|_| ExprError {
                col,
                msg: format!("bad number `{text}`"),
            })?;
            out.push(Spanned {
                tok: Tok::Num(n),
                col,
            });
        } else if c == '\'' {
            let start = i + 1;
            i = start;
            while i < cs.len() && cs[i] != '\'' {
                i += 1;
            }
            if i == cs.len() {
                return Err(ExprError {
                    col,
                    msg: "unterminated string".into(),
                });
            }
            out.push(Spanned {
                tok: Tok::Str(cs[start..i].iter().collect()),
                col,
            });
            i += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Spanned {
                tok: Tok::Ident(cs[start..i].iter().collect()),
                col,
            });
        } else {
            let op = OPS
                .iter()
                .find(|o| {
                    o.chars()
                        .enumerate()
                        .all(|(k, oc)| cs.get(i + k) == Some(&oc))
                })
                .ok_or_else(|| ExprError {
                    col,
                    msg: format!("unexpected character `{c}`"),
                })?;
            i += op.len();
            out.push(Spanned {
                tok: Tok::Op(op),
                col,
            });
        }
    }
    Ok(out)
}

// --- parser ------------------------------------------------------------------

struct Parser<'a> {
    toks: Vec<Spanned>,
    i: usize,
    schema: &'a Schema,
    warnings: Vec<String>,
    /// Column just past the end, for "unexpected end" errors.
    end: usize,
}

type Typed = (Node, Ty);

impl Parser<'_> {
    fn err_at(&self, col: usize, msg: String) -> ExprError {
        ExprError { col, msg }
    }

    fn col(&self) -> usize {
        self.toks.get(self.i).map_or(self.end, |t| t.col)
    }

    fn peek_op(&self) -> Option<&'static str> {
        match self.toks.get(self.i) {
            Some(Spanned {
                tok: Tok::Op(o), ..
            }) => Some(o),
            _ => None,
        }
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek_op() == Some(op) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, op: &str) -> Result<(), ExprError> {
        if self.eat(op) {
            Ok(())
        } else {
            Err(self.err_at(self.col(), format!("expected `{op}`")))
        }
    }

    fn want(&self, col: usize, got: Ty, want: Ty, what: &str) -> Result<(), ExprError> {
        if got == want {
            Ok(())
        } else {
            Err(self.err_at(col, format!("{what} needs a {want}, found a {got}")))
        }
    }

    fn logic(
        &mut self,
        op: &'static str,
        bin: BinOp,
        next: fn(&mut Self) -> Result<Typed, ExprError>,
    ) -> Result<Typed, ExprError> {
        let col = self.col();
        let (mut n, t) = next(self)?;
        let mut first = Some(t);
        while self.peek_op() == Some(op) {
            if let Some(t) = first.take() {
                self.want(col, t, Ty::Bool, op)?;
            }
            self.i += 1;
            let c = self.col();
            let (r, rt) = next(self)?;
            self.want(c, rt, Ty::Bool, op)?;
            n = Node::Bin(bin, Box::new(n), Box::new(r));
        }
        Ok((n, first.unwrap_or(Ty::Bool)))
    }

    fn or(&mut self) -> Result<Typed, ExprError> {
        self.logic("||", BinOp::Or, Self::and)
    }

    fn and(&mut self) -> Result<Typed, ExprError> {
        self.logic("&&", BinOp::And, Self::not)
    }

    fn not(&mut self) -> Result<Typed, ExprError> {
        let col = self.col();
        if self.eat("!") {
            let (n, t) = self.cmp()?;
            self.want(col, t, Ty::Bool, "`!`")?;
            return Ok((Node::Not(Box::new(n)), Ty::Bool));
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<Typed, ExprError> {
        let col = self.col();
        let (a, at) = self.sum()?;
        let op = match self.peek_op() {
            Some("==") => BinOp::Eq,
            Some("!=") => BinOp::Ne,
            Some("<") => BinOp::Lt,
            Some("<=") => BinOp::Le,
            Some(">") => BinOp::Gt,
            Some(">=") => BinOp::Ge,
            _ => return Ok((a, at)),
        };
        self.i += 1;
        let (b, bt) = self.sum()?;
        if at != bt {
            return Err(self.err_at(col, format!("cannot compare a {at} with a {bt}")));
        }
        if !matches!(op, BinOp::Eq | BinOp::Ne) {
            self.want(col, at, Ty::Num, "ordering comparison")?;
        }
        Ok((Node::Bin(op, Box::new(a), Box::new(b)), Ty::Bool))
    }

    fn arith(
        &mut self,
        ops: [(&'static str, BinOp); 2],
        next: fn(&mut Self) -> Result<Typed, ExprError>,
    ) -> Result<Typed, ExprError> {
        let col = self.col();
        let (mut n, t) = next(self)?;
        let mut first = Some(t);
        while let Some(&(sym, op)) = ops.iter().find(|(s, _)| self.peek_op() == Some(s)) {
            if let Some(t) = first.take() {
                self.want(col, t, Ty::Num, &format!("`{sym}`"))?;
            }
            self.i += 1;
            let c = self.col();
            let (r, rt) = next(self)?;
            self.want(c, rt, Ty::Num, &format!("`{sym}`"))?;
            if op == BinOp::Div && r == Node::Lit(Value::Num(0.0)) {
                self.warnings
                    .push(format!("column {c}: division by zero always gives 0"));
            }
            n = Node::Bin(op, Box::new(n), Box::new(r));
        }
        Ok((n, first.unwrap_or(Ty::Num)))
    }

    fn sum(&mut self) -> Result<Typed, ExprError> {
        self.arith([("+", BinOp::Add), ("-", BinOp::Sub)], Self::term)
    }

    fn term(&mut self) -> Result<Typed, ExprError> {
        self.arith([("*", BinOp::Mul), ("/", BinOp::Div)], Self::unary)
    }

    fn unary(&mut self) -> Result<Typed, ExprError> {
        let col = self.col();
        if self.eat("-") {
            let (n, t) = self.atom()?;
            self.want(col, t, Ty::Num, "`-`")?;
            return Ok((Node::Neg(Box::new(n)), Ty::Num));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Typed, ExprError> {
        let col = self.col();
        let Some(Spanned { tok, .. }) = self.toks.get(self.i).cloned() else {
            return Err(self.err_at(col, "unexpected end of expression".into()));
        };
        self.i += 1;
        match tok {
            Tok::Num(n) => Ok((Node::Lit(Value::Num(n)), Ty::Num)),
            Tok::Str(s) => Ok((Node::Lit(Value::Str(s)), Ty::Str)),
            Tok::Op("(") => {
                let e = self.or()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Op(o) => Err(self.err_at(col, format!("unexpected `{o}`"))),
            Tok::Ident(id) if id == "true" || id == "false" => {
                Ok((Node::Lit(Value::Bool(id == "true")), Ty::Bool))
            }
            Tok::Ident(id) if self.peek_op() == Some("(") => self.call(&id, col),
            Tok::Ident(id) => {
                let mut path = id;
                while self.eat(".") {
                    match self.toks.get(self.i) {
                        Some(Spanned {
                            tok: Tok::Ident(s), ..
                        }) => {
                            path.push('.');
                            path.push_str(s);
                            self.i += 1;
                        }
                        _ => {
                            return Err(self.err_at(self.col(), "expected a name after `.`".into()));
                        }
                    }
                }
                let ty = self
                    .schema
                    .get(&path)
                    .ok_or_else(|| self.err_at(col, format!("unknown path `{path}`")))?;
                Ok((Node::Path(path, ty), ty))
            }
        }
    }

    fn call(&mut self, name: &str, col: usize) -> Result<Typed, ExprError> {
        let (f, arity) = match name {
            "abs" => (Func::Abs, 1..=1),
            "round" => (Func::Round, 1..=2),
            "min" => (Func::Min, 2..=usize::MAX),
            "max" => (Func::Max, 2..=usize::MAX),
            _ => return Err(self.err_at(col, format!("unknown function `{name}`"))),
        };
        self.expect("(")?;
        let mut args = Vec::new();
        loop {
            let c = self.col();
            let (a, t) = self.or()?;
            self.want(c, t, Ty::Num, &format!("`{name}`"))?;
            args.push(a);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        if !arity.contains(&args.len()) {
            return Err(self.err_at(
                col,
                format!(
                    "`{name}` takes {} arguments, not {}",
                    describe(&arity),
                    args.len()
                ),
            ));
        }
        Ok((Node::Call(f, args), Ty::Num))
    }
}

fn describe(r: &std::ops::RangeInclusive<usize>) -> String {
    match (*r.start(), *r.end()) {
        (a, b) if a == b => a.to_string(),
        (a, usize::MAX) => format!("{a} or more"),
        (a, b) => format!("{a} to {b}"),
    }
}
