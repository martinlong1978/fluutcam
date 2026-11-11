// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading the project file (D06 §7.2).
//!
//! The `format` key is checked first: an unknown major is an error, a newer
//! minor a warning, an older minor is migrated. Unknown keys are warnings
//! and are not written back; unknown keys in `passes` are errors.

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

/// Setup keys in canonical (file) order.
pub(crate) const SETUP_KEYS: [&str; 15] = [
    "id",
    "name",
    "based_on",
    "kind",
    "machine",
    "material",
    "work_offset",
    "geometry",
    "orientation",
    "index_angle",
    "stock",
    "zero",
    "region",
    "clearance",
    "fixtures",
];

/// Toolpath item keys, before the parameter groups (D15 §2.1).
pub(crate) const TOOLPATH_KEYS: [&str; 6] = ["id", "name", "type", "tool", "enabled", "template"];

/// Migrations from minor `i` to `i + 1`, applied to the parsed table
/// before it is read. Empty while the format is at minor 0.
const MIGRATIONS: &[fn(&mut Table)] = &[];

/// Parses a project file.
pub fn read(src: &str) -> R<Loaded> {
    let mut t: Table =
        toml::from_str(src).map_err(|e| LoadError(format!("not valid TOML: {e}")))?;
    let mut w = Vec::new();
    let minor = check_format(&t, &mut w)?;
    for m in MIGRATIONS.iter().skip(minor as usize) {
        m(&mut t);
    }
    let mut cx = Cx::new(&mut w);
    cx.unknown(
        &t,
        &[
            "format",
            "name",
            "units",
            "tool_libraries",
            "geometry",
            "setup",
            "view",
        ],
    );
    let name = cx.opt_str(&t, "name")?.unwrap_or_default();
    let units = match cx.opt_str(&t, "units")? {
        None => DisplayUnit::Mm,
        Some(u) => DisplayUnit::parse(&u)
            .ok_or_else(|| LoadError(format!("`units`: `{u}` is not mm or inch")))?,
    };
    let tool_libraries = cx.opt_strs(&t, "tool_libraries")?.unwrap_or_default();
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
        .map(|(i, s)| cx.at(&format!("setup[{i}]")).setup(s).map(Arc::new))
        .collect::<R<_>>()?;
    let views = cx.tables(&t, "view")?.into_iter().cloned().collect();
    let project = Project {
        name,
        units,
        tool_libraries,
        geometry,
        setups,
        views,
    };
    check_ids(&project)?;
    Ok(Loaded {
        project,
        warnings: w,
    })
}

/// `fluut.project/<major>[.<minor>]`: unknown major is an error, a newer
/// minor a warning (D06 §7.2). Returns the minor.
fn check_format(t: &Table, w: &mut Vec<String>) -> R<u32> {
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
    let minor: u32 = match minor {
        None => 0,
        Some(m) => m.parse().map_err(|_| bad())?,
    };
    if minor > PROJECT_MINOR {
        w.push(format!(
            "the project was written by a newer FluutCAM ({f}); newer fields are ignored \
             and will not be saved"
        ));
    }
    Ok(minor)
}

pub(crate) fn check_ids(p: &Project) -> R<()> {
    let mut seen = std::collections::BTreeSet::new();
    for id in p.ids() {
        if !seen.insert(id) {
            return err(format!("id `{id}` is used twice"));
        }
    }
    Ok(())
}

/// Reading context: the dotted path for messages and the warnings.
pub(crate) struct Cx<'a> {
    path: String,
    pub(crate) w: &'a mut Vec<String>,
}

impl<'a> Cx<'a> {
    pub(crate) fn new(w: &'a mut Vec<String>) -> Cx<'a> {
        Cx {
            path: String::new(),
            w,
        }
    }

    pub(crate) fn at(&mut self, p: &str) -> Cx<'_> {
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

    fn bad<T>(&self, k: &str, what: &str, v: &Value) -> R<T> {
        err(format!(
            "`{}`: expected {what}, found {}",
            self.key(k),
            v.type_str()
        ))
    }

    fn str_of(&self, k: &str, v: &Value) -> R<String> {
        match v {
            Value::String(s) => Ok(s.clone()),
            _ => self.bad(k, "a string", v),
        }
    }

    fn opt_str(&self, t: &Table, k: &str) -> R<Option<String>> {
        t.get(k).map(|v| self.str_of(k, v)).transpose()
    }

    /// An array of non-empty strings.
    fn opt_strs(&self, t: &Table, k: &str) -> R<Option<Vec<String>>> {
        let Some(v) = t.get(k) else {
            return Ok(None);
        };
        let Value::Array(a) = v else {
            return self.bad(k, "an array of strings", v);
        };
        let mut out = Vec::new();
        for (i, x) in a.iter().enumerate() {
            let s = match x {
                Value::String(s) if !s.trim().is_empty() => s.clone(),
                Value::String(_) => {
                    return err(format!("`{}[{i}]` is empty", self.key(k)));
                }
                _ => return self.bad(k, "an array of strings", v),
            };
            out.push(s);
        }
        Ok(Some(out))
    }

    fn req_str(&self, t: &Table, k: &str) -> R<String> {
        self.opt_str(t, k)?
            .ok_or_else(|| LoadError(format!("`{}` is missing", self.key(k))))
    }

    fn bool_of(&self, k: &str, v: &Value) -> R<bool> {
        match v {
            Value::Boolean(b) => Ok(*b),
            _ => self.bad(k, "true or false", v),
        }
    }

    fn num(&self, v: &Value, k: &str) -> R<Num> {
        match v {
            Value::Integer(i) => Ok(Num::int(*i)),
            Value::Float(x) if x.is_finite() => Ok(Num::float(*x)),
            _ => err(format!("`{}`: expected a number", self.key(k))),
        }
    }

    fn req_num(&self, t: &Table, k: &str) -> R<Num> {
        match t.get(k) {
            Some(v) => self.num(v, k),
            None => err(format!("`{}` is missing", self.key(k))),
        }
    }

    fn opt_num(&self, t: &Table, k: &str) -> R<Option<Num>> {
        t.get(k).map(|v| self.num(v, k)).transpose()
    }

    fn num3(&self, v: Option<&Value>, k: &str) -> R<[Num; 3]> {
        match v.and_then(Value::as_array).map(Vec::as_slice) {
            Some([x, y, z]) => Ok([self.num(x, k)?, self.num(y, k)?, self.num(z, k)?]),
            _ => err(format!("`{}`: expected [x, y, z]", self.key(k))),
        }
    }

    fn bool3(&self, v: &Value, k: &str) -> R<[bool; 3]> {
        match v.as_array().map(Vec::as_slice) {
            Some([x, y, z]) => Ok([
                self.bool_of(k, x)?,
                self.bool_of(k, y)?,
                self.bool_of(k, z)?,
            ]),
            _ => err(format!(
                "`{}`: expected [x, y, z] of true or false",
                self.key(k)
            )),
        }
    }

    fn table_of<'t>(&self, k: &str, v: &'t Value) -> R<&'t Table> {
        match v {
            Value::Table(x) => Ok(x),
            _ => self.bad(k, "a table", v),
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

    fn one_of(&self, k: &str, s: &str, all: &[&str]) -> R<()> {
        if all.contains(&s) {
            Ok(())
        } else {
            err(format!(
                "`{}`: `{s}` is not one of {}",
                self.key(k),
                all.join(", ")
            ))
        }
    }

    pub(crate) fn source(&mut self, t: &Table) -> R<GeometrySource> {
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

    pub(crate) fn setup(&mut self, t: &Table) -> R<Setup> {
        let mut known = SETUP_KEYS.to_vec();
        known.push("toolpath");
        self.unknown(t, &known);
        let mut s = Setup::new(&self.req_str(t, "id")?, "");
        for k in &SETUP_KEYS[1..] {
            self.setup_key(&mut s, k, t.get(*k))?;
        }
        s.toolpaths = self
            .tables(t, "toolpath")?
            .iter()
            .enumerate()
            .map(|(i, tp)| self.at(&format!("toolpath[{i}]")).toolpath(tp))
            .collect::<R<_>>()?;
        Ok(s)
    }

    /// Sets (`Some`) or resets (`None`) one Setup key from its file value.
    /// `id` and `toolpath` are not keys here.
    pub(crate) fn setup_key(&mut self, s: &mut Setup, k: &str, v: Option<&Value>) -> R<()> {
        let Some(v) = v else {
            match k {
                "name" => s.name.clear(),
                "based_on" => s.based_on = None,
                "kind" => s.kind = None,
                "machine" => s.machine = None,
                "material" => s.material = None,
                "work_offset" => s.work_offset = None,
                "geometry" => s.geometry = None,
                "orientation" => s.orientation = None,
                "index_angle" => s.index_angle = None,
                "stock" => s.stock = None,
                "zero" => s.zero = None,
                "region" => s.region = None,
                "clearance" => s.clearance = None,
                "fixtures" => s.fixtures = None,
                _ => return err(format!("`{}` is not a Setup field", self.key(k))),
            }
            return Ok(());
        };
        match k {
            "name" => s.name = self.str_of(k, v)?,
            "based_on" => s.based_on = Some(self.str_of(k, v)?),
            "kind" => {
                let x = self.str_of(k, v)?;
                s.kind = Some(self.pick(k, &x, &SetupKind::ALL, SetupKind::as_str)?);
            }
            "machine" => s.machine = Some(self.str_of(k, v)?),
            "material" => s.material = Some(self.str_of(k, v)?),
            "work_offset" => s.work_offset = Some(self.str_of(k, v)?),
            "geometry" => s.geometry = Some(self.geometry_sel(self.table_of(k, v)?)?),
            "orientation" => s.orientation = Some(self.orientation(self.table_of(k, v)?)?),
            "index_angle" => s.index_angle = Some(self.index_angle(self.table_of(k, v)?)?),
            "stock" => s.stock = Some(self.stock(self.table_of(k, v)?)?),
            "zero" => s.zero = Some(self.zero(self.table_of(k, v)?)?),
            "region" => {
                let r = self.table_of(k, v)?;
                let mut cx = self.at("region");
                cx.unknown(r, &["floor_z"]);
                s.region = Some(Region {
                    floor_z: cx.req_num(r, "floor_z")?,
                });
            }
            "clearance" => s.clearance = Some(self.clearance(self.table_of(k, v)?)?),
            "fixtures" => s.fixtures = Some(self.fixtures(v)?),
            _ => return err(format!("`{}` is not a Setup field", self.key(k))),
        }
        Ok(())
    }

    fn geometry_sel(&mut self, g: &Table) -> R<GeometrySel> {
        self.at("geometry")
            .unknown(g, &["bodies", "scale", "mirror"]);
        let bodies = match g.get("bodies") {
            None => None,
            Some(Value::Array(a)) => Some(
                a.iter()
                    .map(|v| v.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        LoadError(format!(
                            "`{}`: expected bodies = [ids]",
                            self.key("geometry")
                        ))
                    })?,
            ),
            Some(_) => {
                return err(format!(
                    "`{}`: expected bodies = [ids]",
                    self.key("geometry")
                ));
            }
        };
        Ok(GeometrySel {
            bodies,
            scale: g
                .get("scale")
                .map(|v| self.num3(Some(v), "geometry.scale"))
                .transpose()?,
            mirror: g
                .get("mirror")
                .map(|v| self.bool3(v, "geometry.mirror"))
                .transpose()?,
        })
    }

    fn axis_pick(&self, k: &str, v: &Value) -> R<AxisPick> {
        match v {
            Value::String(s) => Ok(AxisPick::Axis(self.pick(
                k,
                s,
                &AxisDir::ALL,
                AxisDir::as_str,
            )?)),
            Value::Table(t) if t.len() == 1 => match t.iter().next() {
                Some((f, Value::String(id))) if f == "face" => Ok(AxisPick::Face(id.clone())),
                Some((e, Value::String(id))) if e == "edge" => Ok(AxisPick::Edge(id.clone())),
                _ => err(format!(
                    "`{}`: expected an axis such as \"+Z\", {{ face = id }} or {{ edge = id }}",
                    self.key(k)
                )),
            },
            _ => err(format!(
                "`{}`: expected an axis such as \"+Z\", {{ face = id }} or {{ edge = id }}",
                self.key(k)
            )),
        }
    }

    fn orientation(&mut self, o: &Table) -> R<Orientation> {
        let mut cx = self.at("orientation");
        let req = |cx: &Cx, k: &str| -> R<AxisPick> {
            match o.get(k) {
                Some(v) => cx.axis_pick(k, v),
                None => err(format!("`{}` is missing", cx.key(k))),
            }
        };
        if o.contains_key("rotary_axis") {
            cx.unknown(o, &["rotary_axis", "a_zero_toward"]);
            Ok(Orientation::Rotary {
                rotary_axis: req(&cx, "rotary_axis")?,
                a_zero_toward: o
                    .get("a_zero_toward")
                    .map(|v| cx.axis_pick("a_zero_toward", v))
                    .transpose()?,
            })
        } else {
            cx.unknown(o, &["tool_axis", "x_axis"]);
            Ok(Orientation::ThreeAxis {
                tool_axis: req(&cx, "tool_axis")?,
                x_axis: req(&cx, "x_axis")?,
            })
        }
    }

    fn index_angle(&mut self, a: &Table) -> R<IndexAngle> {
        let mut cx = self.at("index_angle");
        cx.unknown(a, &["mode", "value"]);
        let mode = cx.req_str(a, "mode")?;
        let mode = cx.pick(
            "mode",
            &mode,
            &[AngleMode::Absolute, AngleMode::Relative],
            AngleMode::as_str,
        )?;
        Ok(IndexAngle {
            mode,
            value: cx.req_num(a, "value")?,
        })
    }

    fn stock(&mut self, s: &Table) -> R<StockSpec> {
        let mut cx = self.at("stock");
        let kind = cx.req_str(s, "kind")?;
        cx.one_of(
            "kind",
            &kind,
            &["block", "cylinder", "body", "from_previous"],
        )?;
        Ok(match kind.as_str() {
            "block" => {
                cx.unknown(s, &["kind", "margin", "size"]);
                let margin = s
                    .get("margin")
                    .map(|v| cx.num3(Some(v), "margin"))
                    .transpose()?;
                let size = s
                    .get("size")
                    .map(|v| cx.num3(Some(v), "size"))
                    .transpose()?;
                if margin.is_some() && size.is_some() {
                    return err(format!("`{}`: give `margin` or `size`, not both", cx.path));
                }
                StockSpec::Block { margin, size }
            }
            "cylinder" => {
                cx.unknown(s, &["kind", "radius", "length"]);
                StockSpec::Cylinder {
                    radius: cx.req_num(s, "radius")?,
                    length: cx.req_num(s, "length")?,
                }
            }
            "body" => {
                cx.unknown(s, &["kind", "body"]);
                StockSpec::Body {
                    body: cx.req_str(s, "body")?,
                }
            }
            _ => {
                cx.unknown(s, &["kind"]);
                StockSpec::FromPrevious
            }
        })
    }

    fn zero(&mut self, z: &Table) -> R<ZeroSpec> {
        let mut cx = self.at("zero");
        if !z.contains_key("method") && z.contains_key("x") {
            cx.unknown(z, &["x"]);
            let x = match &z["x"] {
                Value::String(s) => match s.as_str() {
                    "stock_left" => AxisX::StockLeft,
                    "stock_right" => AxisX::StockRight,
                    _ => {
                        return err(format!(
                            "`{}`: `{s}` is not stock_left, stock_right or a number",
                            cx.key("x")
                        ));
                    }
                },
                v => AxisX::At(cx.num(v, "x")?),
            };
            return Ok(ZeroSpec::OnAxis { x });
        }
        cx.unknown(z, &["method", "corner", "at"]);
        let corner = |cx: &Cx| -> R<Corner> {
            match cx.opt_str(z, "corner")? {
                None => Ok(Corner::FrontLeft),
                Some(c) => cx.pick("corner", &c, &Corner::ALL, Corner::as_str),
            }
        };
        Ok(match cx.req_str(z, "method")?.as_str() {
            "stock_top_corner" => ZeroSpec::StockTopCorner(corner(&cx)?),
            "stock_bottom_corner" => ZeroSpec::StockBottomCorner(corner(&cx)?),
            "stock_top_centre" => ZeroSpec::StockTopCentre,
            "point" => ZeroSpec::Point(cx.num3(z.get("at"), "at")?),
            m => {
                return err(format!(
                    "`{}`: `{m}` is not stock_top_corner, stock_bottom_corner, \
                     stock_top_centre or point",
                    cx.key("method")
                ));
            }
        })
    }

    /// `fixtures = [{ kind = "chuck", diameter = 100, length = 50 }, ...]`.
    fn fixtures(&mut self, v: &Value) -> R<Vec<Fixture>> {
        let Value::Array(items) = v else {
            return self.bad("fixtures", "an array of fixtures", v);
        };
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let Value::Table(t) = item else {
                return err(format!(
                    "`{}`: fixture {i} must be a table",
                    self.key("fixtures")
                ));
            };
            let mut cx = self.at(&format!("fixtures[{i}]"));
            let kind = cx.req_str(t, "kind")?;
            cx.one_of("kind", &kind, &Fixture::KINDS)?;
            let positive = |cx: &Cx, k: &str, n: Num| -> R<Num> {
                if n.v > 0.0 {
                    Ok(n)
                } else {
                    err(format!("`{}` must be above 0", cx.key(k)))
                }
            };
            out.push(match kind.as_str() {
                "chuck" => {
                    cx.unknown(t, &["kind", "diameter", "length"]);
                    Fixture::Chuck {
                        diameter: positive(&cx, "diameter", cx.req_num(t, "diameter")?)?,
                        length: positive(&cx, "length", cx.req_num(t, "length")?)?,
                    }
                }
                "tailstock" => {
                    cx.unknown(t, &["kind", "diameter", "length", "tip_diameter"]);
                    let tip = cx.opt_num(t, "tip_diameter")?;
                    if let Some(n) = tip
                        && n.v < 0.0
                    {
                        return err(format!("`{}` must not be below 0", cx.key("tip_diameter")));
                    }
                    Fixture::Tailstock {
                        diameter: positive(&cx, "diameter", cx.req_num(t, "diameter")?)?,
                        length: positive(&cx, "length", cx.req_num(t, "length")?)?,
                        tip_diameter: tip,
                    }
                }
                _ => {
                    cx.unknown(t, &["kind", "min", "max"]);
                    let min = cx.num3(t.get("min"), "min")?;
                    let max = cx.num3(t.get("max"), "max")?;
                    if (0..3).any(|a| max[a].v <= min[a].v) {
                        return err(format!(
                            "`{}`: every coordinate of `max` must be above `min`",
                            cx.key("max")
                        ));
                    }
                    Fixture::Clamp { min, max }
                }
            });
        }
        Ok(out)
    }

    fn clearance(&mut self, c: &Table) -> R<ClearanceSpec> {
        let mut cx = self.at("clearance");
        let kind = cx.req_str(c, "kind")?;
        cx.one_of("kind", &kind, &["plane", "cylinder"])?;
        if kind == "plane" {
            cx.unknown(c, &["kind", "offset"]);
            Ok(ClearanceSpec::Plane {
                offset: cx.opt_num(c, "offset")?,
            })
        } else {
            cx.unknown(c, &["kind", "radius"]);
            Ok(ClearanceSpec::Cylinder {
                radius: cx.req_num(c, "radius")?,
            })
        }
    }

    pub(crate) fn toolpath(&mut self, t: &Table) -> R<Toolpath> {
        let groups = ["tool_params", "geometry", "heights", "passes", "linking"];
        let mut known = TOOLPATH_KEYS.to_vec();
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
            tool: self.opt_str(t, "tool")?.unwrap_or_default(),
            enabled: t
                .get("enabled")
                .map(|v| self.bool_of("enabled", v))
                .transpose()?,
            template: self.opt_str(t, "template")?,
            params,
        })
    }

    /// Sets (`Some`) or clears (`None`) a toolpath item key (`name`, `tool`,
    /// `enabled`, `template`).
    pub(crate) fn toolpath_key(&mut self, tp: &mut Toolpath, k: &str, v: Option<&Value>) -> R<()> {
        match (k, v) {
            ("name", None) => tp.name.clear(),
            ("name", Some(v)) => tp.name = self.str_of(k, v)?,
            ("tool", v) => {
                tp.tool = v
                    .map(|v| self.str_of(k, v))
                    .transpose()?
                    .unwrap_or_default()
            }
            ("enabled", v) => tp.enabled = v.map(|v| self.bool_of(k, v)).transpose()?,
            ("template", v) => tp.template = v.map(|v| self.str_of(k, v)).transpose()?,
            _ => return err(format!("`{}` is not a toolpath field", self.key(k))),
        }
        Ok(())
    }
}
