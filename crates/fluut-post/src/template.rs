// SPDX-License-Identifier: MIT OR Apache-2.0

//! Block templates (D10 §3.1) and the compiled post.
//!
//! Each block line is split into literal text, words (`[X]` resolving to a
//! `[words.X]` entry, formatted per §5.3) and built-in placeholders (spec 05
//! §5.4.2, formatted by the emitter). Unknown names are compile errors.

use crate::expr::{self, Expr, Schema};
use crate::post::{Block, LoadedPost, Post};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

/// Built-in placeholders (spec 05 §5.4.2, D10 §5), used when no word of the
/// same name is defined.
pub const PLACEHOLDERS: [&str; 54] = [
    "X",
    "Y",
    "Z",
    "A",
    "B",
    "I",
    "J",
    "K",
    "IA",
    "JA",
    "KA",
    "R",
    "F",
    "FP",
    "FC",
    "FI",
    "S",
    "T",
    "TP",
    "N",
    "TOOLNAME",
    "TDIA",
    "SAFEZ",
    "XH",
    "YH",
    "ZH",
    "XMIN",
    "YMIN",
    "ZMIN",
    "XMAX",
    "YMAX",
    "ZMAX",
    "DATE",
    "TIME",
    "TOOLPATH_NAME",
    "FILE_NOTES",
    "Radius",
    "PLANE",
    "MACHINE_NAME",
    "SETUP_NAME",
    "WORK_OFFSET",
    "INDEX_A",
    "INDEX_B",
    "POST_ID",
    "FLUUT_VERSION",
    "CYCLE_Z",
    "CYCLE_R",
    "CYCLE_Q",
    "CYCLE_P",
    "CYCLE_F",
    "COMMENT",
    "DWELL",
    "LBRACKET",
    "RBRACKET",
];

#[derive(Clone, PartialEq, Debug)]
pub enum Tok {
    Lit(String),
    /// Key of a `[words.*]` entry.
    Word(String),
    /// A built-in placeholder name.
    Placeholder(String),
}

#[derive(Clone, PartialEq, Debug)]
pub struct CompiledLine {
    pub id: Option<String>,
    pub toks: Vec<Tok>,
    pub when: Option<Expr>,
    pub layer: Option<String>,
}

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[error("blocks.{block} line {line}: {msg}")]
pub struct TemplateError {
    pub block: String,
    /// 1-based line number in the merged block.
    pub line: usize,
    pub msg: String,
}

/// Placeholder name → word key, from each word's `placeholder` (default:
/// its key).
pub fn word_names(post: &Post) -> BTreeMap<String, String> {
    post.words
        .iter()
        .map(|(k, w)| {
            (
                w.placeholder.clone().unwrap_or_else(|| k.clone()),
                k.clone(),
            )
        })
        .collect()
}

/// Splits a template line into tokens. `[NAME]` with `NAME` made of ASCII
/// letters, digits and `_` is a word or placeholder; any other bracket is
/// literal text.
pub fn tokenize(line: &str, words: &BTreeMap<String, String>) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if name_len == 0 || !after[name_len..].starts_with(']') {
            lit.push_str(&rest[..=open]);
            rest = after;
            continue;
        }
        let name = &after[..name_len];
        lit.push_str(&rest[..open]);
        if !lit.is_empty() {
            out.push(Tok::Lit(std::mem::take(&mut lit)));
        }
        if let Some(key) = words.get(name) {
            out.push(Tok::Word(key.clone()));
        } else if PLACEHOLDERS.contains(&name) {
            out.push(Tok::Placeholder(name.to_owned()));
        } else {
            return Err(format!("unknown placeholder `[{name}]`"));
        }
        rest = &after[name_len + 1..];
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        out.push(Tok::Lit(lit));
    }
    Ok(out)
}

pub fn compile_block(
    name: &str,
    block: &Block,
    words: &BTreeMap<String, String>,
    schema: &Schema,
) -> Result<Vec<CompiledLine>, TemplateError> {
    block
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let err = |msg: String| TemplateError {
                block: name.to_owned(),
                line: i + 1,
                msg,
            };
            let when = l
                .when
                .as_deref()
                .map(|w| expr::compile_condition(w, schema).map_err(|e| err(format!("when: {e}"))))
                .transpose()?;
            Ok(CompiledLine {
                id: l.id.clone(),
                toks: tokenize(&l.line, words).map_err(err)?,
                when,
                layer: l.layer.clone(),
            })
        })
        .collect()
}

/// A post ready for the emitter: typed, validated and with every block
/// compiled.
#[derive(Clone, PartialEq, Debug)]
pub struct CompiledPost {
    pub post: Post,
    pub blocks: BTreeMap<String, Vec<CompiledLine>>,
    pub warnings: Vec<String>,
}

impl CompiledPost {
    /// Compiles every block; all template errors are reported together.
    pub fn compile(loaded: LoadedPost) -> Result<CompiledPost, Vec<TemplateError>> {
        let schema = Schema::post();
        let words = word_names(&loaded.post);
        let mut blocks = BTreeMap::new();
        let mut errors = Vec::new();
        let mut warnings = loaded.warnings;
        for (name, b) in &loaded.post.blocks {
            match compile_block(name, b, &words, &schema) {
                Ok(lines) => {
                    for (i, l) in lines.iter().enumerate() {
                        for w in l.when.iter().flat_map(|e| &e.warnings) {
                            warnings.push(format!("blocks.{name} line {}: {w}", i + 1));
                        }
                    }
                    blocks.insert(name.clone(), lines);
                }
                Err(e) => errors.push(e),
            }
        }
        if errors.is_empty() {
            Ok(CompiledPost {
                post: loaded.post,
                blocks,
                warnings,
            })
        } else {
            Err(errors)
        }
    }
}
