// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading the project file (D06 §7.2).

use super::*;
use crate::params::{Num, Params, ToolpathTypeId};
use std::fmt;
use toml::{Table, Value};

/// A project file that cannot be loaded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LoadError(pub String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

/// A loaded project and the load warnings (unknown keys, newer minor format).
#[derive(Clone, PartialEq, Debug)]
pub struct Loaded {
    pub project: Project,
    pub warnings: Vec<String>,
}

type R<T> = Result<T, LoadError>;

fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(LoadError(msg.into()))
}

/// Parses a project file.
pub fn read(src: &str) -> R<Loaded> {
    let t: Table = toml::from_str(src).map_err(|e| LoadError(format!("not valid TOML: {e}")))?;
    let mut w = Vec::new();
    let format = check_format(&t, &mut w)?;
    let mut cx = Cx {
        path: String::new(),
        w: &mut w,
    };
    cx.unknown(
        &t,
        &["format", "name", "units", "geometry", "setup", "view"],
    );
    if t.contains_key("view") {
        cx.w.push("saved views are not supported yet and were ignored".into());
    }
    let name = cx.opt_str(&t, "name")?.unwrap_or_default();
    let units = match cx.opt_str(&t, "units")?.as_deref() {
        None | Some("mm") => DisplayUnit::Mm,
        Some("inch") => DisplayUnit::Inch,
        Some(u) => return err(format!("`units`: `{u}` is not mm or inch")),
    };
    let geometry = cx
        .tables(&t, "geometry")?
        .iter()
        .enumerate()
        .map(|(i, g)| cx.at(&format!("geometry[{i}]")).source(g))
        .collect::<R<_>>()?;
    let setups = cx
        .tables(&t, "setup")?
        .iter()
        .enumerate()
        .map(|(i, s)| cx.at(&format!("setup[{i}]")).setup(s))
        .collect::<R<_>>()?;
    let project = Project {
        format,
        name,
        units,
        geometry,
        setups,
    };
    check_ids(&project)?;
    Ok(Loaded {
        project,
        warnings: w,
    })
}

/// `fluut.project/<major>[.<minor>]`: unknown major is an error, a newer
/// minor a warning (D06 §7.2).
fn check_format(t: &Table, w: &mut Vec<String>) -> R<String> {
    let Some(Value::String(f)) = t.get("format") else {
        return err("`format` is missing");
    };
    let bad = || LoadError(format!("`format`: `{f}` is not a FluutCAM project"));
    let (id, ver) = f.split_once('/').ok_or_else(bad)?;
    if id != PROJECT_FORMAT {
        return Err(bad());
    }
    let (major, minor) = match ver.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (ver, None),
    };
    let major: u32 = major.parse().map_err(|_| bad())?;
    if major != PROJECT_MAJOR {
        return err(format!(
            "`format`: version {major} is not supported (this build reads {PROJECT_MAJOR})"
        ));
    }
    if let Some(m) = minor {
        let m: u32 = m.parse().map_err(|_| bad())?;
        if m > 0 {
            w.push(format!(
                "the project was written by a newer FluutCAM ({f}); newer fields are ignored"
            ));
        }
    }
    Ok(f.clone())
}

fn check_ids(p: &Project) -> R<()> {
    let mut seen = std::collections::BTreeSet::new();
    let ids = p
        .geometry
        .iter()
        .map(|g| &g.id)
        .chain(p.setups.iter().map(|s| &s.id))
        .chain(
            p.setups
                .iter()
                .flat_map(|s| s.toolpaths.iter().map(|t| &t.id)),
        );
    for id in ids {
        if !seen.insert(id) {
            return err(format!("id `{id}` is used twice"));
        }
    }
    Ok(())
}

struct Cx<'a> {
    path: String,
    w: &'a mut Vec<String>,
}

impl Cx<'_> {
    fn at(&mut self, p: &str) -> Cx<'_> {
        let path = if self.path.is_empty() {
            p.to_owned()
        } else {
            format!("{}.{p}", self.path)
        };
        Cx { path, w: self.w }
    }

    fn key(&self, k: &str) -> String {
        if self.path.is_empty() {
            k.to_owned()
        } else {
            format!("{}.{k}", self.path)
        }
    }

    fn unknown(&mut self, t: &Table, known: &[&str]) {
        for k in t.keys() {
            if !known.contains(&k.as_str()) {
                let p = self.key(k);
                self.w.push(format!("unknown key `{p}` ignored"));
            }
        }
    }

    fn opt_str(&self, t: &Table, k: &str) -> R<Option<String>> {
        match t.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(v) => err(format!(
                "`{}`: expected a string, found {}",
                self.key(k),
                v.type_str()
            )),
        }
    }

    fn req_str(&self, t: &Table, k: &str) -> R<String> {
        self.opt_str(t, k)?
            .ok_or_else(|| LoadError(format!("`{}` is missing", self.key(k))))
    }

    fn opt_bool(&self, t: &Table, k: &str) -> R<Option<bool>> {
        match t.get(k) {
            None => Ok(None),
            Some(Value::Boolean(b)) => Ok(Some(*b)),
            Some(v) => err(format!(
                "`{}`: expected true or false, found {}",
                self.key(k),
                v.type_str()
            )),
        }
    }

    fn num(&self, v: &Value, k: &str) -> R<Num> {
        match v {
            Value::Integer(i) => Ok(Num::int(*i)),
            Value::Float(x) if x.is_finite() => Ok(Num::float(*x)),
            _ => err(format!("`{}`: expected a number", self.key(k))),
        }
    }

    fn num3(&self, v: Option<&Value>, k: &str) -> R<[Num; 3]> {
        match v.and_then(Value::as_array).map(Vec::as_slice) {
            Some([x, y, z]) => Ok([self.num(x, k)?, self.num(y, k)?, self.num(z, k)?]),
            _ => err(format!("`{}`: expected [x, y, z]", self.key(k))),
        }
    }

    fn inline<'t>(&self, t: &'t Table, k: &str) -> R<Option<&'t Table>> {
        match t.get(k) {
            None => Ok(None),
            Some(Value::Table(x)) => Ok(Some(x)),
            Some(v) => err(format!(
                "`{}`: expected a table, found {}",
                self.key(k),
                v.type_str()
            )),
        }
    }

    fn tables<'t>(&self, t: &'t Table, k: &str) -> R<Vec<&'t Table>> {
        match t.get(k) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_table().ok_or_else(|| {
                        LoadError(format!("`{}` must be [[{k}]] tables", self.key(k)))
                    })
                })
                .collect(),
            Some(_) => err(format!("`{}` must be [[{k}]] tables", self.key(k))),
        }
    }

    fn pick<T: Copy>(&self, k: &str, s: &str, all: &[T], name: impl Fn(T) -> &'static str) -> R<T> {
        all.iter().copied().find(|&x| name(x) == s).ok_or_else(|| {
            let names: Vec<_> = all.iter().map(|&x| name(x)).collect();
            LoadError(format!(
                "`{}`: `{s}` is not one of {}",
                self.key(k),
                names.join(", ")
            ))
        })
    }

    fn source(&mut self, t: &Table) -> R<GeometrySource> {
        self.unknown(t, &["id", "path", "kind", "hash", "unit", "body"]);
        let kind = self.req_str(t, "kind")?;
        let bodies = self
            .tables(t, "body")?
            .iter()
            .map(|b| {
                self.unknown(b, &["id", "name"]);
                Ok(BodyEntry {
                    id: self.req_str(b, "id")?,
                    name: self.opt_str(b, "name")?,
                })
            })
            .collect::<R<_>>()?;
        Ok(GeometrySource {
            id: self.req_str(t, "id")?,
            path: self.req_str(t, "path")?,
            kind: self.pick("kind", &kind, &SourceKind::ALL, SourceKind::as_str)?,
            hash: self.opt_str(t, "hash")?,
            unit: self.opt_str(t, "unit")?,
            bodies,
        })
    }

    fn setup(&mut self, t: &Table) -> R<Setup> {
        self.unknown(
            t,
            &[
                "id",
                "name",
                "kind",
                "machine",
                "material",
                "work_offset",
                "geometry",
                "orientation",
                "stock",
                "zero",
                "clearance",
                "toolpath",
            ],
        );
        let kind = match self.opt_str(t, "kind")? {
            None => SetupKind::ThreeAxis,
            Some(k) => self.pick("kind", &k, &SetupKind::ALL, SetupKind::as_str)?,
        };
        let bodies = match self.inline(t, "geometry")? {
            None => None,
            Some(g) => {
                self.at("geometry").unknown(g, &["bodies"]);
                let ids = match g.get("bodies") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|v| v.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>(),
                    _ => None,
                };
                Some(ids.ok_or_else(|| {
                    LoadError(format!(
                        "`{}`: expected bodies = [ids]",
                        self.key("geometry")
                    ))
                })?)
            }
        };
        let orientation = match self.inline(t, "orientation")? {
            None => None,
            Some(o) => {
                self.at("orientation").unknown(o, &["tool_axis", "x_axis"]);
                let ax = |k: &str| -> R<AxisDir> {
                    let s = self.req_str(o, k)?;
                    self.pick(k, &s, &AxisDir::ALL, AxisDir::as_str)
                };
                Some(Orientation {
                    tool_axis: ax("tool_axis")?,
                    x_axis: ax("x_axis")?,
                })
            }
        };
        let stock = match self.inline(t, "stock")? {
            None => None,
            Some(s) => {
                self.at("stock").unknown(s, &["kind", "margin"]);
                match self.req_str(s, "kind")?.as_str() {
                    "block" => Some(StockSpec::Block {
                        margin: match s.get("margin") {
                            None => [Num::int(0); 3],
                            v => self.num3(v, "stock.margin")?,
                        },
                    }),
                    k => {
                        return err(format!(
                            "`{}`: stock kind `{k}` is not supported yet (block only)",
                            self.key("stock")
                        ));
                    }
                }
            }
        };
        let zero = match self.inline(t, "zero")? {
            None => None,
            Some(z) => {
                self.at("zero").unknown(z, &["method", "corner", "at"]);
                let corner = || -> R<Corner> {
                    match self.opt_str(z, "corner")? {
                        None => Ok(Corner::FrontLeft),
                        Some(c) => self.pick("zero.corner", &c, &Corner::ALL, Corner::as_str),
                    }
                };
                Some(match self.req_str(z, "method")?.as_str() {
                    "stock_top_corner" => ZeroSpec::StockTopCorner(corner()?),
                    "stock_bottom_corner" => ZeroSpec::StockBottomCorner(corner()?),
                    "stock_top_centre" => ZeroSpec::StockTopCentre,
                    "point" => ZeroSpec::Point(self.num3(z.get("at"), "zero.at")?),
                    m => {
                        return err(format!(
                            "`{}`: `{m}` is not stock_top_corner, stock_bottom_corner, \
                             stock_top_centre or point",
                            self.key("zero.method")
                        ));
                    }
                })
            }
        };
        let clearance = match self.inline(t, "clearance")? {
            None => None,
            Some(c) => {
                self.at("clearance").unknown(c, &["kind", "offset"]);
                if self.req_str(c, "kind")? != "plane" {
                    return err(format!(
                        "`{}`: only kind = \"plane\" is supported for three_axis Setups",
                        self.key("clearance")
                    ));
                }
                Some(ClearanceSpec {
                    offset: match c.get("offset") {
                        None => Num::float(ClearanceSpec::DEFAULT_OFFSET),
                        Some(v) => self.num(v, "clearance.offset")?,
                    },
                })
            }
        };
        let toolpaths = self
            .tables(t, "toolpath")?
            .iter()
            .enumerate()
            .map(|(i, tp)| self.at(&format!("toolpath[{i}]")).toolpath(tp))
            .collect::<R<_>>()?;
        Ok(Setup {
            id: self.req_str(t, "id")?,
            name: self.opt_str(t, "name")?.unwrap_or_default(),
            kind,
            machine: self.opt_str(t, "machine")?,
            material: self.opt_str(t, "material")?,
            work_offset: self.opt_str(t, "work_offset")?,
            bodies,
            orientation,
            stock,
            zero,
            clearance,
            toolpaths,
        })
    }

    fn toolpath(&mut self, t: &Table) -> R<Toolpath> {
        let groups = ["tool_params", "geometry", "heights", "passes", "linking"];
        let mut known = vec!["id", "name", "type", "tool", "enabled", "template"];
        known.extend(groups);
        self.unknown(t, &known);
        let ty = self.req_str(t, "type")?;
        let kind = ToolpathTypeId::parse(&ty).ok_or_else(|| {
            LoadError(format!(
                "`{}`: toolpath type `{ty}` is not supported by this build",
                self.key("type")
            ))
        })?;
        let mut w = Vec::new();
        let params = Params::from_toml(kind, t, &mut w)
            .map_err(|e| LoadError(format!("{}: {e}", self.path)))?;
        for m in w {
            self.w.push(format!("{}: {m}", self.path));
        }
        Ok(Toolpath {
            id: self.req_str(t, "id")?,
            name: self.opt_str(t, "name")?.unwrap_or_default(),
            tool: self.req_str(t, "tool")?,
            enabled: self.opt_bool(t, "enabled")?,
            template: self.opt_str(t, "template")?,
            params,
        })
    }
}
