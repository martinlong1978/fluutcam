// SPDX-License-Identifier: MIT OR Apache-2.0

//! Part 21 clear-text parser (design D04 §2–§3).
//!
//! A hand-written recursive-descent parser over the raw bytes; lexing is
//! done inline by the cursor. Input is untrusted (spec 06 §6.3): every error
//! is returned with its line and column, never a panic, and strings and
//! nesting are bounded.

use memchr::memchr2;
use std::collections::BTreeMap;
use thiserror::Error;

/// Longest string literal accepted, in bytes.
pub const MAX_STRING: usize = 1 << 20;
/// Deepest nesting of parameter lists accepted.
pub const MAX_DEPTH: usize = 64;
/// Largest file accepted, in bytes.
pub const MAX_FILE: usize = 2 << 30;

/// Line and column (both from 1) of a byte in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Error, PartialEq)]
#[error("line {}, column {}: {msg}", pos.line, pos.col)]
pub struct ParseError {
    pub pos: Pos,
    pub msg: String,
}

/// Interned keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KwId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Ref(u64),
    Int(i64),
    Real(f64),
    Str(Box<str>),
    Enum(Box<str>),
    Binary(Box<str>),
    List(Vec<Param>),
    /// A typed value such as `LENGTH_MEASURE(1.5)`.
    Typed(KwId, Box<Param>),
    /// `$`
    Unset,
    /// `*`
    Derived,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RawEntity {
    Simple {
        kind: KwId,
        params: Vec<Param>,
        pos: Pos,
    },
    /// An external-mapping instance `(A(...) B(...))`.
    Complex {
        parts: Vec<(KwId, Vec<Param>)>,
        pos: Pos,
    },
}

impl RawEntity {
    pub fn pos(&self) -> Pos {
        match self {
            RawEntity::Simple { pos, .. } | RawEntity::Complex { pos, .. } => *pos,
        }
    }
}

/// Keyword interner. Ids are given in first-seen order, so they are
/// deterministic for a given file.
#[derive(Debug, Clone, Default)]
pub struct Keywords {
    map: BTreeMap<Box<str>, KwId>,
    names: Vec<Box<str>>,
}

impl Keywords {
    pub fn intern(&mut self, s: &str) -> KwId {
        if let Some(&k) = self.map.get(s) {
            return k;
        }
        let k = KwId(self.names.len() as u32);
        self.names.push(s.into());
        self.map.insert(s.into(), k);
        k
    }

    pub fn get(&self, s: &str) -> Option<KwId> {
        self.map.get(s).copied()
    }

    pub fn name(&self, k: KwId) -> &str {
        &self.names[k.0 as usize]
    }
}

/// A parsed file: header entities and the data section's entity table.
#[derive(Debug, Clone, Default)]
pub struct RawModel {
    pub header: Vec<(KwId, Vec<Param>)>,
    pub entities: Vec<RawEntity>,
    /// `#id` → index into `entities`.
    pub index: BTreeMap<u64, u32>,
    pub keywords: Keywords,
}

impl RawModel {
    pub fn get(&self, id: u64) -> Option<&RawEntity> {
        self.index.get(&id).map(|&i| &self.entities[i as usize])
    }

    /// The header entity of this name, e.g. `FILE_SCHEMA`.
    pub fn header_entity(&self, name: &str) -> Option<&[Param]> {
        let k = self.keywords.get(name)?;
        self.header
            .iter()
            .find(|(h, _)| *h == k)
            .map(|(_, p)| p.as_slice())
    }
}

/// Parses a whole Part 21 file.
pub fn parse(src: &[u8]) -> Result<RawModel, ParseError> {
    let mut p = Parser {
        s: src,
        i: 0,
        line: 1,
        line_start: 0,
        model: RawModel::default(),
    };
    if src.len() > MAX_FILE {
        return Err(p.err("file too large"));
    }
    p.ws()?;
    p.keyword_exact("ISO-10303-21")?;
    p.expect(b';')?;
    p.ws()?;
    p.keyword_exact("HEADER")?;
    p.expect(b';')?;
    loop {
        p.ws()?;
        if p.peek_keyword("ENDSEC") {
            p.keyword_exact("ENDSEC")?;
            p.expect(b';')?;
            break;
        }
        let name = p.keyword()?;
        let k = p.model.keywords.intern(&name);
        let params = p.param_list(0)?;
        p.expect(b';')?;
        p.model.header.push((k, params));
    }
    // One or more DATA sections (AP242 files may have several).
    loop {
        p.ws()?;
        if p.peek_keyword("END-ISO-10303-21") {
            break;
        }
        p.keyword_exact("DATA")?;
        p.ws()?;
        if p.peek() == Some(b'(') {
            // DATA('name', ('schema')) of a multi-section file.
            p.param_list(0)?;
        }
        p.expect(b';')?;
        loop {
            p.ws()?;
            if p.peek_keyword("ENDSEC") {
                p.keyword_exact("ENDSEC")?;
                p.expect(b';')?;
                break;
            }
            p.instance()?;
        }
    }
    Ok(p.model)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    line: u32,
    line_start: usize,
    model: RawModel,
}

impl Parser<'_> {
    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: (self.i - self.line_start + 1) as u32,
        }
    }

    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError {
            pos: self.pos(),
            msg: msg.into(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i += 1;
        if c == b'\n' {
            self.line += 1;
            self.line_start = self.i;
        }
        Some(c)
    }

    /// Skips whitespace and comments.
    fn ws(&mut self) -> Result<(), ParseError> {
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.bump();
                }
                b'/' if self.s.get(self.i + 1) == Some(&b'*') => {
                    let start = self.pos();
                    self.i += 2;
                    loop {
                        match self.bump() {
                            None => {
                                return Err(ParseError {
                                    pos: start,
                                    msg: "unterminated comment".into(),
                                });
                            }
                            Some(b'*') if self.peek() == Some(b'/') => {
                                self.i += 1;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        self.ws()?;
        match self.peek() {
            Some(x) if x == c => {
                self.bump();
                Ok(())
            }
            Some(x) => Err(self.err(format!(
                "expected '{}', found '{}'",
                c as char,
                x.escape_ascii()
            ))),
            None => Err(self.err(format!("expected '{}', found end of file", c as char))),
        }
    }

    fn peek_keyword(&self, kw: &str) -> bool {
        self.s[self.i..].starts_with(kw.as_bytes())
            && !self
                .s
                .get(self.i + kw.len())
                .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    }

    fn keyword_exact(&mut self, kw: &str) -> Result<(), ParseError> {
        if self.peek_keyword(kw) {
            self.i += kw.len();
            Ok(())
        } else {
            Err(self.err(format!("expected {kw}")))
        }
    }

    /// A standard or user-defined (`!NAME`) keyword.
    fn keyword(&mut self) -> Result<String, ParseError> {
        self.ws()?;
        let start = self.i;
        if self.peek() == Some(b'!') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.i += 1;
            } else {
                break;
            }
        }
        let k = &self.s[start..self.i];
        let first = k.iter().find(|&&c| c != b'!');
        if !first.is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i = start;
            return Err(self.err("expected a keyword"));
        }
        Ok(String::from_utf8_lossy(k).to_ascii_uppercase())
    }

    fn entity_id(&mut self) -> Result<u64, ParseError> {
        self.expect(b'#')?;
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| self.err("bad entity id"))
    }

    fn instance(&mut self) -> Result<(), ParseError> {
        let pos = self.pos();
        let id = self.entity_id()?;
        self.expect(b'=')?;
        self.ws()?;
        let e = if self.peek() == Some(b'(') {
            self.bump();
            let mut parts = Vec::new();
            loop {
                self.ws()?;
                if self.peek() == Some(b')') {
                    self.bump();
                    break;
                }
                let name = self.keyword()?;
                let k = self.model.keywords.intern(&name);
                parts.push((k, self.param_list(0)?));
            }
            if parts.is_empty() {
                return Err(ParseError {
                    pos,
                    msg: format!("#{id}: empty complex instance"),
                });
            }
            RawEntity::Complex { parts, pos }
        } else {
            let name = self.keyword()?;
            let kind = self.model.keywords.intern(&name);
            let params = self.param_list(0)?;
            RawEntity::Simple { kind, params, pos }
        };
        self.expect(b';')?;
        let slot = self.model.entities.len() as u32;
        if self.model.index.insert(id, slot).is_some() {
            return Err(ParseError {
                pos,
                msg: format!("#{id} defined twice"),
            });
        }
        self.model.entities.push(e);
        Ok(())
    }

    fn param_list(&mut self, depth: usize) -> Result<Vec<Param>, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("parameter lists nested too deeply"));
        }
        self.expect(b'(')?;
        let mut out = Vec::new();
        self.ws()?;
        if self.peek() == Some(b')') {
            self.bump();
            return Ok(out);
        }
        loop {
            out.push(self.param(depth)?);
            self.ws()?;
            match self.bump() {
                Some(b',') => {}
                Some(b')') => return Ok(out),
                Some(c) => {
                    return Err(
                        self.err(format!("expected ',' or ')', found '{}'", c.escape_ascii()))
                    );
                }
                None => return Err(self.err("unexpected end of file in a parameter list")),
            }
        }
    }

    fn param(&mut self, depth: usize) -> Result<Param, ParseError> {
        self.ws()?;
        let Some(c) = self.peek() else {
            return Err(self.err("unexpected end of file"));
        };
        match c {
            b'#' => Ok(Param::Ref(self.entity_id()?)),
            b'$' => {
                self.bump();
                Ok(Param::Unset)
            }
            b'*' => {
                self.bump();
                Ok(Param::Derived)
            }
            b'\'' => Ok(Param::Str(self.string()?.into())),
            b'"' => {
                self.bump();
                let start = self.i;
                match memchr::memchr(b'"', &self.s[self.i..]) {
                    Some(n) => {
                        self.i += n + 1;
                        Ok(Param::Binary(
                            String::from_utf8_lossy(&self.s[start..self.i - 1]).into(),
                        ))
                    }
                    None => Err(self.err("unterminated binary")),
                }
            }
            b'.' if self
                .s
                .get(self.i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic()) =>
            {
                self.bump();
                let start = self.i;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    self.i += 1;
                }
                let name = String::from_utf8_lossy(&self.s[start..self.i]).to_ascii_uppercase();
                if self.bump() != Some(b'.') {
                    return Err(self.err("unterminated enumeration"));
                }
                Ok(Param::Enum(name.into()))
            }
            b'(' => Ok(Param::List(self.param_list(depth + 1)?)),
            b'+' | b'-' | b'0'..=b'9' | b'.' => self.number(),
            c if c.is_ascii_alphabetic() || c == b'!' => {
                let name = self.keyword()?;
                let k = self.model.keywords.intern(&name);
                let mut inner = self.param_list(depth + 1)?;
                if inner.len() != 1 {
                    return Err(self.err(format!("{name}: a typed value takes one parameter")));
                }
                Ok(Param::Typed(k, Box::new(inner.remove(0))))
            }
            c => Err(self.err(format!("unexpected '{}'", c.escape_ascii()))),
        }
    }

    fn number(&mut self) -> Result<Param, ParseError> {
        let start = self.i;
        let mut real = false;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' => {}
                b'.' | b'E' | b'e' => real = true,
                b'+' | b'-' if matches!(self.s[self.i - 1], b'E' | b'e') => {}
                _ => break,
            }
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        let bad = || ParseError {
            pos: self.pos(),
            msg: format!("bad number '{t}'"),
        };
        if real {
            // Part 21 allows "1." and "1.E3"; Rust parses both.
            t.parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .map(Param::Real)
                .ok_or_else(bad)
        } else {
            t.parse::<i64>().map(Param::Int).map_err(|_| bad())
        }
    }

    /// A string literal, with `''` and the `\X\`, `\X2\`, `\X4\`, `\S\` and
    /// `\\` escapes decoded.
    fn string(&mut self) -> Result<String, ParseError> {
        let start_pos = self.pos();
        self.bump();
        let mut raw = Vec::new();
        loop {
            let Some(n) = memchr2(b'\'', b'\n', &self.s[self.i..]) else {
                return Err(ParseError {
                    pos: start_pos,
                    msg: "unterminated string".into(),
                });
            };
            raw.extend_from_slice(&self.s[self.i..self.i + n]);
            self.i += n;
            if self.s[self.i] == b'\n' {
                // Strings may span lines; the line break is not part of it.
                self.bump();
                continue;
            }
            self.i += 1;
            if self.peek() == Some(b'\'') {
                raw.push(b'\'');
                self.i += 1;
                continue;
            }
            break;
        }
        if raw.len() > MAX_STRING {
            return Err(ParseError {
                pos: start_pos,
                msg: "string too long".into(),
            });
        }
        decode(&raw).ok_or(ParseError {
            pos: start_pos,
            msg: "bad escape in string".into(),
        })
    }
}

fn hex(b: &[u8]) -> Option<u32> {
    u32::from_str_radix(std::str::from_utf8(b).ok()?, 16).ok()
}

fn decode(raw: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != b'\\' {
            out.push(raw[i] as char);
            i += 1;
            continue;
        }
        let rest = &raw[i..];
        if rest.starts_with(b"\\\\") {
            out.push('\\');
            i += 2;
        } else if rest.starts_with(b"\\X2\\") || rest.starts_with(b"\\X4\\") {
            let w = if rest[2] == b'2' { 4 } else { 8 };
            let end = rest.windows(4).position(|x| x == b"\\X0\\")?;
            let body = &rest[4..end];
            if body.len() % w != 0 {
                return None;
            }
            let units: Option<Vec<u32>> = body.chunks(w).map(hex).collect();
            let units = units?;
            if w == 4 {
                let u16s: Vec<u16> = units.into_iter().map(|u| u as u16).collect();
                out.push_str(&String::from_utf16(&u16s).ok()?);
            } else {
                for u in units {
                    out.push(char::from_u32(u)?);
                }
            }
            i += end + 4;
        } else if rest.starts_with(b"\\X\\") && rest.len() >= 5 {
            out.push(char::from_u32(hex(&rest[3..5])?)?);
            i += 5;
        } else if rest.starts_with(b"\\S\\") && rest.len() >= 4 {
            out.push(char::from_u32(rest[3] as u32 + 128)?);
            i += 4;
        } else if rest.starts_with(b"\\P") && rest.len() >= 4 && rest[3] == b'\\' {
            // Code page switch (\PA\ …): ignored; ISO 8859-1 assumed.
            i += 4;
        } else {
            out.push('\\');
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests;
