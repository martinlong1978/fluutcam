// SPDX-License-Identifier: MIT OR Apache-2.0

//! Part 21 clear-text parser (design D04 §2–§3).
//!
//! A hand-written recursive-descent parser over the raw bytes; lexing is
//! done inline by the cursor. Input is untrusted (spec 06 §6.3): every error
//! is returned with its line and column, never a panic, and strings and
//! nesting are bounded.
//!
//! **Parallel parsing.** [`parse`] cuts each DATA section into chunks and
//! parses them with `rayon`. A cut is made after a `;` that is followed by
//! white space and `#`, without knowing whether that `;` is inside a string
//! or comment. Each chunk is parsed by the serial code from its absolute
//! position, with its end taken as the end of the input. The first chunk
//! starts between two instances. If the serial parser is between instances
//! at a chunk's end, the chunk parses exactly as it would serially; if not,
//! the chunk ends inside an instance, string or comment, and its parse
//! fails. So all chunks succeed only if every cut is a real boundary. Any
//! failure, and any duplicate id, makes the whole file be parsed again
//! serially: the result, and every error with its position, is always the
//! serial one. Chunks are merged in file order, keywords interned in the
//! order first seen, so the [`RawModel`] is the serial one too.

use memchr::memchr2;
use rayon::prelude::*;
use std::collections::BTreeMap;
use thiserror::Error;

/// Longest string literal accepted, in bytes.
pub const MAX_STRING: usize = 1 << 20;
/// Deepest nesting of parameter lists accepted.
pub const MAX_DEPTH: usize = 64;
/// Largest file accepted, in bytes.
pub const MAX_FILE: usize = 2 << 30;
/// Smallest chunk [`parse`] cuts a DATA section into, in bytes.
pub const MIN_CHUNK: usize = 128 << 10;

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
#[derive(Debug, Clone, Default, PartialEq)]
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
#[derive(Debug, Clone, Default, PartialEq)]
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

/// Parses a whole Part 21 file, in parallel on the current `rayon` pool
/// where the DATA section is large enough (at least two chunks of
/// [`MIN_CHUNK`] bytes; about four chunks per thread). The result equals
/// [`parse_serial`]'s.
pub fn parse(src: &[u8]) -> Result<RawModel, ParseError> {
    parse_with(src, Split::Auto).0
}

/// Parses a whole Part 21 file on the calling thread.
pub fn parse_serial(src: &[u8]) -> Result<RawModel, ParseError> {
    parse_with(src, Split::Serial).0
}

/// [`parse`] with DATA sections cut about every `chunk` bytes (at least
/// one), whatever their size; for tests and fuzzing.
pub fn parse_chunked(src: &[u8], chunk: usize) -> Result<RawModel, ParseError> {
    parse_with(src, Split::Bytes(chunk.max(1))).0
}

/// How DATA sections are cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Split {
    Serial,
    Auto,
    Bytes(usize),
}

/// What the parallel path did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SplitInfo {
    /// Chunks parsed in parallel, over all DATA sections.
    chunks: usize,
    /// A chunk failed or an id was repeated: parsed again serially.
    retried: bool,
}

/// A chunk of a DATA section, parsed.
struct Chunk {
    entities: Vec<RawEntity>,
    ids: Vec<u64>,
    keywords: Keywords,
}

fn parse_with(src: &[u8], split: Split) -> (Result<RawModel, ParseError>, SplitInfo) {
    let mut info = SplitInfo::default();
    let r = parse_inner(src, split, &mut info);
    if r == Err(Retry) {
        info.retried = true;
        let mut ignored = SplitInfo::default();
        return match parse_inner(src, Split::Serial, &mut ignored) {
            Ok(m) => (Ok(m), info),
            Err(Fail::Parse(e)) => (Err(e), info),
            Err(Retry) => unreachable!("the serial parser never retries"),
        };
    }
    (
        r.map_err(|e| match e {
            Fail::Parse(e) => e,
            Retry => unreachable!("handled above"),
        }),
        info,
    )
}

/// Why `parse_inner` stopped.
#[derive(Debug, PartialEq)]
enum Fail {
    Parse(ParseError),
    /// The parallel path cannot give the serial result: parse serially.
    Retry,
}
use Fail::Retry;

impl From<ParseError> for Fail {
    fn from(e: ParseError) -> Self {
        Fail::Parse(e)
    }
}

fn parse_inner(src: &[u8], split: Split, info: &mut SplitInfo) -> Result<RawModel, Fail> {
    let mut p = Parser::new(src, 0, 1, 0, Keywords::default(), false);
    if src.len() > MAX_FILE {
        return Err(p.err("file too large").into());
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
        let k = p.keyword()?;
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
        if split != Split::Serial {
            info.chunks += p.data_parallel(split)?;
        }
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
    /// Ids in entity order, for a chunk; `None` fills `model.index`.
    ids: Option<Vec<u64>>,
    /// Parameters of the lists being parsed.
    stack: Vec<Param>,
}

impl<'a> Parser<'a> {
    /// A parser over `s` from byte `i`, which is on line `line` (from 1),
    /// a line starting at byte `line_start`.
    fn new(s: &'a [u8], i: usize, line: u32, line_start: usize, kw: Keywords, chunk: bool) -> Self {
        Parser {
            s,
            i,
            line,
            line_start,
            model: RawModel {
                keywords: kw,
                ..RawModel::default()
            },
            ids: chunk.then(Vec::new),
            stack: Vec::new(),
        }
    }

    /// Parses the instances of the DATA section that starts here, up to
    /// the first `ENDSEC` in the bytes, in parallel chunks, and moves past
    /// them. Returns the number of chunks (0 if the section is too small
    /// to cut).
    fn data_parallel(&mut self, split: Split) -> Result<usize, Fail> {
        let threads = rayon::current_num_threads();
        if split == Split::Serial || (split == Split::Auto && threads < 2) {
            return Ok(0);
        }
        let s = self.s;
        let start = self.i;
        let end = memchr::memmem::find(&s[start..], b"ENDSEC").map_or(s.len(), |n| start + n);
        let len = end - start;
        let chunk = match split {
            Split::Bytes(c) => c,
            _ if len < 2 * MIN_CHUNK => return Ok(0),
            _ => MIN_CHUNK.max(len / (4 * threads)),
        };
        let mut cuts = vec![start];
        let mut t = start + chunk;
        while t < end {
            match next_cut(&s[..end], t) {
                Some(c) => {
                    cuts.push(c);
                    t = c + chunk;
                }
                None => break,
            }
        }
        cuts.push(end);
        let spans: Vec<(usize, usize)> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
        // Line of each chunk start: newlines counted per chunk, then summed.
        let newlines: Vec<u32> = spans
            .par_iter()
            .map(|&(a, b)| memchr::memchr_iter(b'\n', &s[a..b]).count() as u32)
            .collect();
        let mut lines = Vec::with_capacity(spans.len() + 1);
        lines.push(self.line);
        for n in &newlines {
            lines.push(lines.last().copied().unwrap_or(1).saturating_add(*n));
        }
        let line_start = |i: usize| memchr::memrchr(b'\n', &s[..i]).map_or(0, |n| n + 1);
        let seed = &self.model.keywords;
        let chunks: Vec<Result<Chunk, ParseError>> = spans
            .par_iter()
            .enumerate()
            .map(|(k, &(a, b))| {
                let mut p = Parser::new(&s[..b], a, lines[k], line_start(a), seed.clone(), true);
                p.chunk_instances()?;
                Ok(Chunk {
                    entities: p.model.entities,
                    ids: p.ids.unwrap_or_default(),
                    keywords: p.model.keywords,
                })
            })
            .collect();
        let mut chunks: Vec<Chunk> = chunks
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(|_| Retry)?;
        // Keywords in first-seen order, chunk by chunk; then ids remapped.
        let seeded = seed.names.len();
        let maps: Vec<Vec<KwId>> = chunks
            .iter()
            .map(|c| {
                c.keywords.names[seeded..]
                    .iter()
                    .map(|n| self.model.keywords.intern(n))
                    .collect()
            })
            .collect();
        chunks.par_iter_mut().zip(&maps).for_each(|(c, map)| {
            let identity = map
                .iter()
                .enumerate()
                .all(|(j, k)| k.0 as usize == seeded + j);
            if !identity {
                let f = |k: KwId| {
                    if (k.0 as usize) < seeded {
                        k
                    } else {
                        map[k.0 as usize - seeded]
                    }
                };
                for e in &mut c.entities {
                    remap_entity(e, &f);
                }
            }
        });
        // Entities in file order; the index from sorted (id, slot) pairs.
        let total: usize = chunks.iter().map(|c| c.ids.len()).sum();
        let mut pairs: Vec<(u64, u32)> = Vec::with_capacity(self.model.index.len() + total);
        pairs.extend(std::mem::take(&mut self.model.index));
        self.model.entities.reserve(total);
        for c in chunks {
            for (id, e) in c.ids.into_iter().zip(c.entities) {
                pairs.push((id, self.model.entities.len() as u32));
                self.model.entities.push(e);
            }
        }
        pairs.par_sort_unstable();
        if pairs.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(Retry);
        }
        self.model.index = pairs.into_iter().collect();
        self.i = end;
        self.line = *lines.last().unwrap_or(&self.line);
        self.line_start = line_start(end);
        Ok(spans.len())
    }

    /// The instance loop of a chunk: instances up to the end of `s`.
    fn chunk_instances(&mut self) -> Result<(), ParseError> {
        loop {
            self.ws()?;
            if self.i >= self.s.len() {
                return Ok(());
            }
            self.instance()?;
        }
    }

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

    /// A standard or user-defined (`!NAME`) keyword, interned.
    fn keyword(&mut self) -> Result<KwId, ParseError> {
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
        // Only ASCII letters, digits, `_` and `!` were taken.
        let k = std::str::from_utf8(k).unwrap_or_default();
        Ok(if k.bytes().any(|c| c.is_ascii_lowercase()) {
            self.model.keywords.intern(&k.to_ascii_uppercase())
        } else {
            self.model.keywords.intern(k)
        })
    }

    fn entity_id(&mut self) -> Result<u64, ParseError> {
        self.expect(b'#')?;
        let start = self.i;
        let mut id = Some(0u64);
        while let Some(c) = self.peek().filter(u8::is_ascii_digit) {
            id = id
                .and_then(|n| n.checked_mul(10))
                .and_then(|n| n.checked_add(u64::from(c - b'0')));
            self.i += 1;
        }
        id.filter(|_| self.i > start)
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
                let k = self.keyword()?;
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
            let kind = self.keyword()?;
            let params = self.param_list(0)?;
            RawEntity::Simple { kind, params, pos }
        };
        self.expect(b';')?;
        let slot = self.model.entities.len() as u32;
        if let Some(ids) = &mut self.ids {
            ids.push(id);
        } else if self.model.index.insert(id, slot).is_some() {
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
        self.ws()?;
        if self.peek() == Some(b')') {
            self.bump();
            return Ok(Vec::new());
        }
        // Parameters go on a stack shared by all lists, so that each list
        // is allocated once, at its size.
        let base = self.stack.len();
        loop {
            let p = self.param(depth)?;
            self.stack.push(p);
            self.ws()?;
            match self.bump() {
                Some(b',') => {}
                Some(b')') => return Ok(self.stack.drain(base..).collect()),
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
                        // Keep the line count right across line breaks.
                        let body = &self.s[self.i..self.i + n];
                        if let Some(last) = memchr::memrchr(b'\n', body) {
                            self.line += memchr::memchr_iter(b'\n', body).count() as u32;
                            self.line_start = self.i + last + 1;
                        }
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
                let k = self.keyword()?;
                let mut inner = self.param_list(depth + 1)?;
                if inner.len() != 1 {
                    let name = self.model.keywords.name(k);
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

/// The first cut at or after `t`: just after a `;` that is followed by
/// white space and `#`.
fn next_cut(s: &[u8], mut t: usize) -> Option<usize> {
    while t < s.len() {
        let semi = t + memchr::memchr(b';', &s[t..])?;
        let mut j = semi + 1;
        while j < s.len() && matches!(s[j], b' ' | b'\t' | b'\r' | b'\n') {
            j += 1;
        }
        if s.get(j) == Some(&b'#') {
            return Some(semi + 1);
        }
        t = semi + 1;
    }
    None
}

fn remap_entity(e: &mut RawEntity, f: &impl Fn(KwId) -> KwId) {
    match e {
        RawEntity::Simple { kind, params, .. } => {
            *kind = f(*kind);
            params.iter_mut().for_each(|p| remap_param(p, f));
        }
        RawEntity::Complex { parts, .. } => {
            for (k, ps) in parts {
                *k = f(*k);
                ps.iter_mut().for_each(|p| remap_param(p, f));
            }
        }
    }
}

fn remap_param(p: &mut Param, f: &impl Fn(KwId) -> KwId) {
    match p {
        Param::Typed(k, inner) => {
            *k = f(*k);
            remap_param(inner, f);
        }
        Param::List(ps) => ps.iter_mut().for_each(|p| remap_param(p, f)),
        _ => {}
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
            // The end marker starts after the opening one.
            let end = 4 + rest[4..].windows(4).position(|x| x == b"\\X0\\")?;
            let body = &rest[4..end];
            if !body.len().is_multiple_of(w) {
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
