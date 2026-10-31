// SPDX-License-Identifier: MIT OR Apache-2.0

//! Layered definitions: references, versions, stores and the generic merge
//! (design D06 §4, spec 05 §5.2).
//!
//! A definition is a TOML table with `id`, `version`, and optionally `extends`
//! (one parent) and `mixins` (fragments). Resolution applies the parent chain,
//! then each mixin, then the definition's own keys, each onto the result so
//! far. Tables deep-merge; scalars and arrays are replaced. A table with a
//! `mode` key, or at a path the [`ModeMerger`] claims, is handed to that
//! kind-specific merger (posts: D10 §2).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use toml::{Table, Value};

#[cfg(test)]
mod tests;

/// Maximum depth of `extends` and `mixins` nesting (spec 05 §5.2.2).
pub const MAX_DEPTH: usize = 16;

/// Keys that describe a layer rather than being merged into the result.
const META_KEYS: [&str; 5] = ["format", "id", "version", "extends", "mixins"];

/// A definition version, `major.minor` (D06 §4.2).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    /// Parses `"3.2"`. Both parts are required.
    pub fn parse(s: &str) -> Result<Version, DefError> {
        let bad = || DefError::BadVersion(s.to_owned());
        let (major, minor) = s.split_once('.').ok_or_else(bad)?;
        Ok(Version {
            major: parse_u32(major).ok_or_else(bad)?,
            minor: parse_u32(minor).ok_or_else(bad)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Digits only, so `"+3"` and `" 3"` are rejected.
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Which versions a reference accepts (D06 §4.2).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum VersionReq {
    /// `id`: the highest version available.
    Latest,
    /// `id@3`: the highest 3.x.
    Major(u32),
    /// `id@3.2`: exactly 3.2.
    Exact(Version),
}

impl VersionReq {
    pub fn matches(&self, v: Version) -> bool {
        match *self {
            VersionReq::Latest => true,
            VersionReq::Major(m) => v.major == m,
            VersionReq::Exact(e) => v == e,
        }
    }
}

/// A reference to a definition, e.g. `fluut/linuxcnc@3`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefRef {
    pub id: String,
    pub version: VersionReq,
}

impl DefRef {
    pub fn parse(s: &str) -> Result<DefRef, DefError> {
        let bad = || DefError::BadRef(s.to_owned());
        let (id, version) = match s.split_once('@') {
            None => (s, VersionReq::Latest),
            Some((id, v)) if v.contains('.') => {
                (id, VersionReq::Exact(Version::parse(v).map_err(|_| bad())?))
            }
            Some((id, v)) => (id, VersionReq::Major(parse_u32(v).ok_or_else(bad)?)),
        };
        if id.is_empty() || id.contains(|c: char| c.is_whitespace() || c == '@') {
            return Err(bad());
        }
        Ok(DefRef {
            id: id.to_owned(),
            version,
        })
    }
}

impl fmt::Display for DefRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.version {
            VersionReq::Latest => write!(f, "{}", self.id),
            VersionReq::Major(m) => write!(f, "{}@{m}", self.id),
            VersionReq::Exact(v) => write!(f, "{}@{v}", self.id),
        }
    }
}

/// One resolved layer, `id@major.minor`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LayerId(pub String);

impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A definition as read from its file.
#[derive(Clone, PartialEq, Debug)]
pub struct Def {
    pub id: String,
    pub version: Version,
    pub table: Table,
}

impl Def {
    /// Reads `id` and `version` from a parsed table.
    pub fn from_table(table: Table) -> Result<Def, DefError> {
        let id = match table.get("id") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => return Err(DefError::Missing("id")),
        };
        let version = match table.get("version") {
            Some(Value::String(s)) => Version::parse(s)?,
            _ => return Err(DefError::Missing("version")),
        };
        Ok(Def { id, version, table })
    }

    pub fn from_toml(src: &str) -> Result<Def, DefError> {
        let table: Table = src
            .parse()
            .map_err(|e: toml::de::Error| DefError::Toml(e.to_string()))?;
        Def::from_table(table)
    }

    pub fn layer_id(&self) -> LayerId {
        LayerId(format!("{}@{}", self.id, self.version))
    }
}

/// A library of definitions (built-in, user, imported, project; D06 §4.1).
pub trait DefStore {
    /// The highest version of `r.id` that `r.version` accepts.
    fn lookup(&self, r: &DefRef) -> Result<Arc<Def>, DefError>;
}

/// An in-memory store.
#[derive(Default, Debug)]
pub struct MemStore {
    defs: Vec<Arc<Def>>,
}

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }

    pub fn insert(&mut self, def: Def) {
        self.defs.push(Arc::new(def));
    }

    pub fn insert_toml(&mut self, src: &str) -> Result<(), DefError> {
        self.insert(Def::from_toml(src)?);
        Ok(())
    }
}

impl DefStore for MemStore {
    fn lookup(&self, r: &DefRef) -> Result<Arc<Def>, DefError> {
        self.defs
            .iter()
            .filter(|d| d.id == r.id && r.version.matches(d.version))
            .max_by_key(|d| d.version)
            .cloned()
            .ok_or_else(|| DefError::NotFound(r.to_string()))
    }
}

/// Merges a table that carries a `mode` key (D06 §4.3).
pub trait ModeMerger {
    /// Whether tables at `path` go to [`ModeMerger::merge_mode`] even without
    /// a `mode` key, so the merger sees every layer's version (for example to
    /// record per-line provenance).
    fn handles(&self, _path: &str) -> bool {
        false
    }

    /// Returns the merged value for `path`. `parent` is the value so far, if
    /// any; `child` is the layer's table including its `mode` key.
    fn merge_mode(
        &self,
        path: &str,
        parent: Option<&Value>,
        child: &Table,
        layer: &LayerId,
    ) -> Result<Value, DefError>;
}

/// A merger for definition kinds without mode tables: any `mode` key is an
/// error.
#[derive(Debug)]
pub struct NoModes;

impl ModeMerger for NoModes {
    fn merge_mode(
        &self,
        path: &str,
        _parent: Option<&Value>,
        _child: &Table,
        layer: &LayerId,
    ) -> Result<Value, DefError> {
        Err(DefError::Merge {
            layer: layer.clone(),
            path: path.to_owned(),
            msg: "`mode` is not allowed here".to_owned(),
        })
    }
}

#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum DefError {
    #[error("definition not found: {0}")]
    NotFound(String),
    #[error("`extends`/`mixins` cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    #[error("definition nesting deeper than {MAX_DEPTH} at {0}")]
    TooDeep(String),
    #[error("bad definition reference `{0}`")]
    BadRef(String),
    #[error("bad version `{0}` (expected major.minor)")]
    BadVersion(String),
    #[error("missing `{0}`")]
    Missing(&'static str),
    #[error("TOML: {0}")]
    Toml(String),
    #[error("{layer}: {path}: {msg}")]
    Merge {
        layer: LayerId,
        path: String,
        msg: String,
    },
}

#[derive(Clone, PartialEq, Debug)]
pub enum DefWarning {
    /// Two mixins of one definition set the same key; the later one wins.
    MixinConflict {
        path: String,
        earlier: LayerId,
        later: LayerId,
    },
}

/// A fully merged definition.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Resolved {
    /// The merged keys, without layer metadata (`id`, `extends`, …).
    pub table: Table,
    /// Every layer applied, in order.
    pub layers: Vec<LayerId>,
    /// For each leaf path (dotted; arrays and mode tables are leaves), the
    /// layer that supplied it.
    pub provenance: BTreeMap<String, LayerId>,
    pub warnings: Vec<DefWarning>,
}

/// Resolves `root` and its parents and mixins (D06 §4.3).
pub fn resolve(
    store: &dyn DefStore,
    root: &DefRef,
    merger: &dyn ModeMerger,
) -> Result<Resolved, DefError> {
    let mut cx = Cx {
        store,
        merger,
        visiting: Vec::new(),
    };
    let mut out = Resolved::default();
    cx.apply(root, &mut out)?;
    Ok(out)
}

struct Cx<'a> {
    store: &'a dyn DefStore,
    merger: &'a dyn ModeMerger,
    visiting: Vec<String>,
}

impl Cx<'_> {
    /// Applies `r`'s layers (parent chain, mixins, own keys) onto `acc`.
    /// Mixins are applied onto the result so far, not resolved on their own,
    /// so their `append`/`patch` blocks act on the parent's blocks.
    fn apply(&mut self, r: &DefRef, acc: &mut Resolved) -> Result<(), DefError> {
        let def = self.store.lookup(r)?;
        if self.visiting.contains(&def.id) {
            let mut chain = self.visiting.clone();
            chain.push(def.id.clone());
            return Err(DefError::Cycle(chain));
        }
        if self.visiting.len() >= MAX_DEPTH {
            return Err(DefError::TooDeep(def.id.clone()));
        }
        self.visiting.push(def.id.clone());
        let layer = def.layer_id();

        if let Some(v) = def.table.get("extends") {
            let parent = match v {
                Value::String(s) => DefRef::parse(s)?,
                _ => return Err(DefError::BadRef(v.to_string())),
            };
            self.apply(&parent, acc)?;
        }

        let mixins = match def.table.get("mixins") {
            None => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| match v {
                    Value::String(s) => DefRef::parse(s),
                    _ => Err(DefError::BadRef(v.to_string())),
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(v) => return Err(DefError::BadRef(v.to_string())),
        };
        // Paths set by this definition's earlier mixins.
        let mut set_by_mixin: BTreeMap<String, LayerId> = BTreeMap::new();
        for m in &mixins {
            let before = acc.provenance.clone();
            self.apply(m, acc)?;
            for (path, l) in &acc.provenance {
                if before.get(path) == Some(l) {
                    continue;
                }
                if let Some(earlier) = set_by_mixin.get(path)
                    && earlier != l
                {
                    acc.warnings.push(DefWarning::MixinConflict {
                        path: path.clone(),
                        earlier: earlier.clone(),
                        later: l.clone(),
                    });
                }
                set_by_mixin.insert(path.clone(), l.clone());
            }
        }

        let own: Table = def
            .table
            .iter()
            .filter(|(k, _)| !META_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        merge_table(
            &mut acc.table,
            &mut acc.provenance,
            &own,
            "",
            &layer,
            self.merger,
        )?;
        acc.layers.push(layer);
        self.visiting.pop();
        Ok(())
    }
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

/// Drops provenance for `path` and everything below it.
fn forget(prov: &mut BTreeMap<String, LayerId>, path: &str) {
    let below = format!("{path}.");
    prov.retain(|k, _| k != path && !k.starts_with(&below));
}

fn merge_table(
    dst: &mut Table,
    prov: &mut BTreeMap<String, LayerId>,
    src: &Table,
    prefix: &str,
    layer: &LayerId,
    merger: &dyn ModeMerger,
) -> Result<(), DefError> {
    for (k, v) in src {
        let path = join(prefix, k);
        match v {
            Value::Table(t) if t.contains_key("mode") || merger.handles(&path) => {
                let merged = merger.merge_mode(&path, dst.get(k), t, layer)?;
                forget(prov, &path);
                dst.insert(k.clone(), merged);
                prov.insert(path, layer.clone());
            }
            Value::Table(t) => {
                if !matches!(dst.get(k), Some(Value::Table(_))) {
                    forget(prov, &path);
                    dst.insert(k.clone(), Value::Table(Table::new()));
                }
                let Some(Value::Table(d)) = dst.get_mut(k) else {
                    unreachable!("inserted above");
                };
                merge_table(d, prov, t, &path, layer, merger)?;
            }
            _ => {
                forget(prov, &path);
                dst.insert(k.clone(), v.clone());
                prov.insert(path, layer.clone());
            }
        }
    }
    Ok(())
}

/// Applies overrides keyed by dotted path (`"words.F.decimals" = 0`, spec 05
/// §5.2.3). Missing tables on the path are created. Nested tables merge as in
/// [`resolve`]; `mode` tables are not allowed.
pub fn apply_overrides(
    resolved: &mut Resolved,
    overrides: &Table,
    layer: &LayerId,
) -> Result<(), DefError> {
    for (key, v) in overrides {
        let parts: Vec<&str> = key.split('.').collect();
        let bad = |msg: &str| DefError::Merge {
            layer: layer.clone(),
            path: key.clone(),
            msg: msg.to_owned(),
        };
        if parts.iter().any(|p| p.is_empty()) {
            return Err(bad("empty path segment"));
        }
        let (last, dirs) = parts.split_last().expect("split yields one part");
        let mut t = &mut resolved.table;
        for (i, d) in dirs.iter().enumerate() {
            let entry = t
                .entry((*d).to_owned())
                .or_insert_with(|| Value::Table(Table::new()));
            match entry {
                Value::Table(next) => t = next,
                _ => {
                    return Err(bad(&format!("`{}` is not a table", parts[..=i].join("."))));
                }
            }
        }
        let mut one = Table::new();
        one.insert((*last).to_owned(), v.clone());
        merge_table(
            t,
            &mut resolved.provenance,
            &one,
            &dirs.join("."),
            layer,
            &NoModes,
        )?;
    }
    resolved.layers.push(layer.clone());
    Ok(())
}
