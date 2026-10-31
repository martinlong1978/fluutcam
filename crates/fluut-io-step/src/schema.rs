// SPDX-License-Identifier: MIT OR Apache-2.0

//! Schema detection, units and the entity census (design D04 §4, §5.1–§5.2).

use crate::parse::{Param, RawEntity, RawModel};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schema {
    /// `CONFIG_CONTROL_DESIGN`
    Ap203e1,
    /// `AP203_CONFIGURATION_CONTROLLED_3D_DESIGN…`
    Ap203e2,
    /// `AUTOMOTIVE_DESIGN`
    Ap214,
    /// `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING…`
    Ap242,
    Other(String),
}

pub fn schema(m: &RawModel) -> Schema {
    let name = match m.header_entity("FILE_SCHEMA") {
        Some([Param::List(l), ..]) => match l.first() {
            Some(Param::Str(s)) => s.to_ascii_uppercase(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    let base = name.split([' ', '{']).next().unwrap_or("");
    if base == "CONFIG_CONTROL_DESIGN" {
        Schema::Ap203e1
    } else if base.starts_with("AP203_CONFIGURATION_CONTROLLED_3D_DESIGN") {
        Schema::Ap203e2
    } else if base == "AUTOMOTIVE_DESIGN" {
        Schema::Ap214
    } else if base.starts_with("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING") {
        Schema::Ap242
    } else {
        Schema::Other(name)
    }
}

/// The parts of an entity, as `(name, params)`; one part for a simple one.
pub fn parts<'a>(m: &'a RawModel, e: &'a RawEntity) -> Vec<(&'a str, &'a [Param])> {
    match e {
        RawEntity::Simple { kind, params, .. } => vec![(m.keywords.name(*kind), params)],
        RawEntity::Complex { parts, .. } => parts
            .iter()
            .map(|(k, p)| (m.keywords.name(*k), p.as_slice()))
            .collect(),
    }
}

/// The params of the part called `name`, if the entity has one.
pub fn part<'a>(m: &'a RawModel, e: &'a RawEntity, name: &str) -> Option<&'a [Param]> {
    parts(m, e)
        .into_iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| p)
}

/// Units of a geometric representation context.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Units {
    /// Millimetres per file length unit.
    pub length_mm: f64,
    /// Radians per file angle unit.
    pub angle_rad: f64,
    /// Length uncertainty in mm, if the context gives one.
    pub uncertainty_mm: Option<f64>,
    /// No length unit was found; mm was assumed.
    pub assumed: bool,
}

impl Default for Units {
    fn default() -> Self {
        Units {
            length_mm: 1.0,
            angle_rad: 1.0,
            uncertainty_mm: None,
            assumed: true,
        }
    }
}

fn si_prefix(p: &Param) -> f64 {
    match p {
        Param::Enum(e) => match &**e {
            "EXA" => 1e18,
            "PETA" => 1e15,
            "TERA" => 1e12,
            "GIGA" => 1e9,
            "MEGA" => 1e6,
            "KILO" => 1e3,
            "HECTO" => 1e2,
            "DECA" => 1e1,
            "DECI" => 1e-1,
            "CENTI" => 1e-2,
            "MILLI" => 1e-3,
            "MICRO" => 1e-6,
            "NANO" => 1e-9,
            "PICO" => 1e-12,
            _ => 1.0,
        },
        _ => 1.0,
    }
}

fn number(p: &Param) -> Option<f64> {
    match p {
        Param::Real(x) => Some(*x),
        Param::Int(i) => Some(*i as f64),
        Param::Typed(_, inner) => number(inner),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum UnitValue {
    /// Length in metres per unit.
    Length(f64),
    /// Angle in radians per unit.
    Angle(f64),
}

/// Value of a unit entity; `depth` bounds conversion chains.
fn unit_value(m: &RawModel, id: u64, depth: u32) -> Option<UnitValue> {
    if depth > 8 {
        return None;
    }
    let e = m.get(id)?;
    if let Some(p) = part(m, e, "SI_UNIT") {
        let scale = p.first().map_or(1.0, si_prefix);
        return match p.get(1) {
            Some(Param::Enum(n)) if &**n == "METRE" => Some(UnitValue::Length(scale)),
            Some(Param::Enum(n)) if &**n == "RADIAN" => Some(UnitValue::Angle(scale)),
            _ => None,
        };
    }
    if let Some(p) = part(m, e, "CONVERSION_BASED_UNIT") {
        let Some(Param::Ref(mw)) = p.get(1) else {
            return None;
        };
        let mwe = m.get(*mw)?;
        // (LENGTH_|PLANE_ANGLE_)MEASURE_WITH_UNIT(value, unit)
        let mp = parts(m, mwe)
            .into_iter()
            .find(|(n, _)| n.ends_with("MEASURE_WITH_UNIT"))?
            .1;
        let v = number(mp.first()?)?;
        let Some(Param::Ref(base)) = mp.get(1) else {
            return None;
        };
        return match unit_value(m, *base, depth + 1)? {
            UnitValue::Length(f) => Some(UnitValue::Length(v * f)),
            UnitValue::Angle(f) => Some(UnitValue::Angle(v * f)),
        };
    }
    None
}

/// Units of the representation context `ctx` (a complex entity with
/// `GLOBAL_UNIT_ASSIGNED_CONTEXT`).
pub fn units(m: &RawModel, ctx: u64) -> Units {
    let mut u = Units::default();
    let Some(e) = m.get(ctx) else {
        return u;
    };
    if let Some([Param::List(us)]) = part(m, e, "GLOBAL_UNIT_ASSIGNED_CONTEXT") {
        for p in us {
            let Param::Ref(id) = p else { continue };
            match unit_value(m, *id, 0) {
                Some(UnitValue::Length(f)) => {
                    u.length_mm = f * 1000.0;
                    u.assumed = false;
                }
                Some(UnitValue::Angle(f)) => u.angle_rad = f,
                None => {}
            }
        }
    }
    if let Some([Param::List(us)]) = part(m, e, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT") {
        for p in us {
            let Param::Ref(id) = p else { continue };
            let Some(ue) = m.get(*id) else { continue };
            if let Some(up) = part(m, ue, "UNCERTAINTY_MEASURE_WITH_UNIT")
                && let Some(v) = up.first().and_then(number)
            {
                u.uncertainty_mm = Some(v * u.length_mm);
            }
        }
    }
    u
}

/// Body roots (design D04 §5.1, "Bodies").
pub const BODY_KINDS: [&str; 4] = [
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
    "SHELL_BASED_SURFACE_MODEL",
    "FACETED_BREP",
];

/// Geometry and topology entities the mapper supports (design D04 §5.1).
pub const SUPPORTED: [&str; 50] = [
    "CARTESIAN_POINT",
    "DIRECTION",
    "VECTOR",
    "AXIS1_PLACEMENT",
    "AXIS2_PLACEMENT_2D",
    "AXIS2_PLACEMENT_3D",
    "LINE",
    "CIRCLE",
    "ELLIPSE",
    "PARABOLA",
    "HYPERBOLA",
    "POLYLINE",
    "TRIMMED_CURVE",
    "B_SPLINE_CURVE",
    "B_SPLINE_CURVE_WITH_KNOTS",
    "BEZIER_CURVE",
    "QUASI_UNIFORM_CURVE",
    "UNIFORM_CURVE",
    "RATIONAL_B_SPLINE_CURVE",
    "BOUNDED_CURVE",
    "OFFSET_CURVE_3D",
    "SURFACE_CURVE",
    "SEAM_CURVE",
    "PCURVE",
    "COMPOSITE_CURVE",
    "COMPOSITE_CURVE_SEGMENT",
    "PLANE",
    "CYLINDRICAL_SURFACE",
    "CONICAL_SURFACE",
    "SPHERICAL_SURFACE",
    "TOROIDAL_SURFACE",
    "DEGENERATE_TOROIDAL_SURFACE",
    "SURFACE_OF_LINEAR_EXTRUSION",
    "SURFACE_OF_REVOLUTION",
    "B_SPLINE_SURFACE",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "BEZIER_SURFACE",
    "RATIONAL_B_SPLINE_SURFACE",
    "BOUNDED_SURFACE",
    "RECTANGULAR_TRIMMED_SURFACE",
    "OFFSET_SURFACE",
    "VERTEX_POINT",
    "EDGE_CURVE",
    "ORIENTED_EDGE",
    "EDGE_LOOP",
    "VERTEX_LOOP",
    "POLY_LOOP",
    "FACE_BOUND",
    "FACE_OUTER_BOUND",
    "ADVANCED_FACE",
];

/// Further supported names that are parts of complex entities or bodies.
const ALSO_SUPPORTED: [&str; 16] = [
    "FACE_SURFACE",
    "CLOSED_SHELL",
    "OPEN_SHELL",
    "ORIENTED_CLOSED_SHELL",
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
    "SHELL_BASED_SURFACE_MODEL",
    "FACETED_BREP",
    "GEOMETRIC_REPRESENTATION_ITEM",
    "REPRESENTATION_ITEM",
    "CURVE",
    "SURFACE",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "QUASI_UNIFORM_SURFACE",
    "UNIFORM_SURFACE",
    "DEFINITIONAL_REPRESENTATION",
];

pub fn is_supported(name: &str) -> bool {
    SUPPORTED.contains(&name) || ALSO_SUPPORTED.contains(&name)
}

/// Entity kinds reachable from the body roots, by name. A complex entity
/// counts once under all its part names joined by `+`.
#[derive(Debug, Clone, Default)]
pub struct Census {
    pub bodies: BTreeMap<String, usize>,
    pub supported: BTreeMap<String, usize>,
    pub unsupported: BTreeMap<String, usize>,
}

fn refs(p: &Param, out: &mut Vec<u64>) {
    match p {
        Param::Ref(r) => out.push(*r),
        Param::List(l) => l.iter().for_each(|x| refs(x, out)),
        Param::Typed(_, x) => refs(x, out),
        _ => {}
    }
}

pub fn census(m: &RawModel) -> Census {
    let mut c = Census::default();
    let mut seen = BTreeSet::new();
    let mut stack = Vec::new();
    for (&id, &slot) in &m.index {
        let e = &m.entities[slot as usize];
        if let Some((name, _)) = parts(m, e)
            .into_iter()
            .find(|(n, _)| BODY_KINDS.contains(n))
        {
            *c.bodies.entry(name.to_owned()).or_default() += 1;
            stack.push(id);
        }
    }
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(e) = m.get(id) else { continue };
        let ps = parts(m, e);
        let name = ps.iter().map(|(n, _)| *n).collect::<Vec<_>>().join("+");
        let ok = ps.iter().all(|(n, _)| is_supported(n));
        let map = if ok {
            &mut c.supported
        } else {
            &mut c.unsupported
        };
        *map.entry(name).or_default() += 1;
        let mut out = Vec::new();
        for (_, p) in ps {
            p.iter().for_each(|x| refs(x, &mut out));
        }
        stack.extend(out);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn model(schema: &str, data: &str) -> RawModel {
        let src = format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('{schema}'));ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;"
        );
        parse(src.as_bytes()).unwrap()
    }

    #[test]
    fn schemas() {
        assert_eq!(schema(&model("CONFIG_CONTROL_DESIGN", "")), Schema::Ap203e1);
        assert_eq!(
            schema(&model("AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }", "")),
            Schema::Ap214
        );
        assert_eq!(
            schema(&model(
                "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }",
                ""
            )),
            Schema::Ap242
        );
        assert_eq!(
            schema(&model(
                "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF",
                ""
            )),
            Schema::Ap203e2
        );
    }

    const CTX: &str = "#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3) \
        GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#8)) \
        GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2,#3)) REPRESENTATION_CONTEXT('',''));\
        #8=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#1,'','');\
        #3=(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT());";

    #[test]
    fn millimetre_and_degree() {
        let m = model(
            "X",
            &format!(
                "{CTX}#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
                 #2=(CONVERSION_BASED_UNIT('DEGREE',#5) NAMED_UNIT(#6) PLANE_ANGLE_UNIT());\
                 #5=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925),#7);\
                 #6=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);\
                 #7=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));"
            ),
        );
        let u = units(&m, 9);
        assert_eq!(u.length_mm, 1.0);
        assert!(!u.assumed);
        assert!((u.angle_rad - 0.0174532925).abs() < 1e-15);
        assert_eq!(u.uncertainty_mm, Some(1e-5));
    }

    #[test]
    fn inch_and_metre() {
        let inch = model(
            "X",
            &format!(
                "{CTX}#1=(CONVERSION_BASED_UNIT('INCH',#5) LENGTH_UNIT() NAMED_UNIT(#6));\
                 #5=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#7);\
                 #6=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\
                 #7=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
                 #2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));"
            ),
        );
        let u = units(&inch, 9);
        assert!((u.length_mm - 25.4).abs() < 1e-12);
        assert!((u.uncertainty_mm.unwrap() - 25.4e-5).abs() < 1e-15);
        let metre = model(
            "X",
            &format!(
                "{CTX}#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));\
                 #2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));"
            ),
        );
        assert_eq!(units(&metre, 9).length_mm, 1000.0);
        assert!(units(&metre, 999).assumed);
    }

    #[test]
    fn census_follows_body_references() {
        let m = model(
            "X",
            "#1=MANIFOLD_SOLID_BREP('',#2);#2=CLOSED_SHELL('',(#3));\
             #3=ADVANCED_FACE('',(),#4,.T.);#4=WEIRD_SURFACE('');#5=PRODUCT('a','a','',());",
        );
        let c = census(&m);
        assert_eq!(c.bodies["MANIFOLD_SOLID_BREP"], 1);
        assert_eq!(c.supported["ADVANCED_FACE"], 1);
        assert_eq!(c.unsupported["WEIRD_SURFACE"], 1);
        assert!(!c.unsupported.contains_key("PRODUCT"));
    }
}
