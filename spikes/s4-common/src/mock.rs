// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mock project data for the browser, the ribbon and the inspector.
//!
//! The inspector edits the `passes` fields of a `pocket_2d` toolpath as
//! D15 §4.1 defines them, with the D15 §4.2 rules that need only the tool.
//! Strings are English literals here; the product takes them from Fluent
//! (D13 §9).

/// Workflow stages, the ribbon tabs of S 08 §8.3.
pub const STAGES: [&str; 6] = ["Model", "Setup", "Toolpaths", "Simulate", "Post", "Library"];

/// A ribbon group: a name and its commands.
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub name: &'static str,
    pub commands: &'static [&'static str],
    /// Shown as a gallery of toolpath types.
    pub gallery: bool,
}

const fn g(name: &'static str, commands: &'static [&'static str]) -> Group {
    Group {
        name,
        commands,
        gallery: false,
    }
}

const fn gallery(name: &'static str, commands: &'static [&'static str]) -> Group {
    Group {
        name,
        commands,
        gallery: true,
    }
}

/// Ribbon groups for a stage (index into [`STAGES`]).
#[must_use]
pub fn ribbon(stage: usize) -> &'static [Group] {
    const MODEL: &[Group] = &[
        g("Import", &["STEP", "STL", "DXF/SVG"]),
        g("View", &["Visibility", "Measure", "Section"]),
    ];
    const SETUP: &[Group] = &[
        g("New", &["3-axis", "Rotary", "Indexed"]),
        g("Setup", &["Stock", "Work zero", "Machine"]),
    ];
    const TOOLPATHS: &[Group] = &[
        gallery("2D", &["Contour", "Pocket", "Engrave", "V-carve"]),
        gallery("Holes", &["Drill", "Helical"]),
        gallery("3D", &["Parallel", "Waterline", "Scallop", "Pencil"]),
        g("Calculate", &["Selected", "All"]),
    ];
    const SIMULATE: &[Group] = &[
        g("Play", &["Play", "Step", "Speed"]),
        g("Check", &["Gouges", "Rest", "Compare"]),
    ];
    const POST: &[Group] = &[
        g("Post", &["Selected", "All"]),
        g("Output", &["Preview", "Save"]),
    ];
    const LIBRARY: &[Group] = &[g("Library", &["Tools", "Machines", "Posts"])];
    match stage {
        0 => MODEL,
        1 => SETUP,
        2 => TOOLPATHS,
        3 => SIMULATE,
        4 => POST,
        _ => LIBRARY,
    }
}

/// Toolpath state, as the browser shows it (S 08 §8.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotCalculated,
    Calculating(u8),
    Done,
    Stale,
    Warning,
    Error,
}

impl Status {
    /// A glyph for the browser. Colour is never the only signal, so each state
    /// has its own shape.
    #[must_use]
    pub fn glyph(self) -> &'static str {
        match self {
            Self::NotCalculated => "○",
            Self::Calculating(_) => "◔",
            Self::Done => "●",
            Self::Stale => "◍",
            Self::Warning => "△",
            Self::Error => "▲",
        }
    }

    /// Text for screen readers and tooltips.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::NotCalculated => "not calculated".into(),
            Self::Calculating(p) => format!("calculating, {p} %"),
            Self::Done => "done".into(),
            Self::Stale => "stale".into(),
            Self::Warning => "warning".into(),
            Self::Error => "error".into(),
        }
    }
}

/// A toolpath in the browser.
#[derive(Clone, Debug)]
pub struct Toolpath {
    pub name: String,
    pub kind: &'static str,
    pub status: Status,
}

/// A Setup in the browser.
#[derive(Clone, Debug)]
pub struct Setup {
    pub name: String,
    pub kind: &'static str,
    pub toolpaths: Vec<Toolpath>,
}

fn tp(name: &str, kind: &'static str, status: Status) -> Toolpath {
    Toolpath {
        name: name.into(),
        kind,
        status,
    }
}

/// The mock project. `setups()[0].toolpaths[1]` is the `pocket_2d` toolpath
/// the inspector edits.
#[must_use]
pub fn setups() -> Vec<Setup> {
    vec![
        Setup {
            name: "Setup 1".into(),
            kind: "3-axis",
            toolpaths: vec![
                tp("Face top", "contour_2d", Status::Done),
                tp("Pocket flat", "pocket_2d", Status::Stale),
                tp("Finish relief", "parallel", Status::Calculating(42)),
            ],
        },
        Setup {
            name: "Setup 2 A90°".into(),
            kind: "Indexed",
            toolpaths: vec![
                tp("Side profile", "contour_2d", Status::Warning),
                tp("Slot", "pocket_2d", Status::NotCalculated),
                tp("Engrave", "contour_2d", Status::Error),
            ],
        },
    ]
}

/// A large mock project for list-performance checks: `setups` Setups of
/// `per_setup` toolpaths each.
#[must_use]
pub fn large_project(setups: usize, per_setup: usize) -> Vec<Setup> {
    (0..setups)
        .map(|s| Setup {
            name: format!("Setup {}", s + 1),
            kind: "3-axis",
            toolpaths: (0..per_setup)
                .map(|t| tp(&format!("Toolpath {}", t + 1), "parallel", Status::Done))
                .collect(),
        })
        .collect()
}

/// The tool of the edited toolpath.
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: &'static str,
    pub diameter_mm: f64,
    pub corner_radius_mm: f64,
    pub flute_length_mm: f64,
}

/// The mock tool: a 6 mm flat end mill.
#[must_use]
pub fn tool() -> Tool {
    Tool {
        name: "Flat 6 mm (user/router-bits#flat-6)",
        diameter_mm: 6.0,
        corner_radius_mm: 0.0,
        flute_length_mm: 18.0,
    }
}

/// `pattern` (D15 §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Offset,
    Raster,
}

/// `direction` (D15 §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Climb,
    Conventional,
}

/// `offset_order` (D15 §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetOrder {
    InsideOut,
    OutsideIn,
}

/// The `passes` group of a `pocket_2d` toolpath, resolved (defaults applied).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pocket2d {
    pub pattern: Pattern,
    pub stepover_mm: f64,
    pub step_down_mm: f64,
    pub direction: Direction,
    pub offset_order: OffsetOrder,
    pub pass_angle_deg: f64,
}

impl Default for Pocket2d {
    fn default() -> Self {
        Self {
            pattern: Pattern::Raster,
            stepover_mm: 2.4,
            step_down_mm: 1.5,
            direction: Direction::Climb,
            offset_order: OffsetOrder::InsideOut,
            pass_angle_deg: 30.0,
        }
    }
}

/// A validation message shown under its field (D15 §1.7).
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// Key of the field the message belongs under.
    pub field: &'static str,
    pub code: &'static str,
    pub error: bool,
    pub message: String,
}

impl Pocket2d {
    /// `offset_order` is shown only for the offset pattern, `pass_angle_deg`
    /// only for raster (D15 §4.1 "Depends on").
    #[must_use]
    pub fn visible(&self, key: &str) -> bool {
        match key {
            "offset_order" => self.pattern == Pattern::Offset,
            "pass_angle_deg" => self.pattern == Pattern::Raster,
            _ => true,
        }
    }

    /// The D15 §4.2 rules that need only these fields and the tool.
    #[must_use]
    pub fn validate(&self, tool: &Tool) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let flat = tool.diameter_mm - 2.0 * tool.corner_radius_mm;
        if self.stepover_mm <= 0.0 {
            out.push(Diagnostic {
                field: "stepover",
                code: "param.passes.stepover.range",
                error: true,
                message: "Stepover must be greater than 0.".into(),
            });
        } else if self.stepover_mm > tool.diameter_mm {
            out.push(Diagnostic {
                field: "stepover",
                code: "param.passes.stepover.max",
                error: true,
                message: format!(
                    "Stepover {} is larger than the tool diameter {}.",
                    fmt_mm(self.stepover_mm),
                    fmt_mm(tool.diameter_mm)
                ),
            });
        } else if self.stepover_mm > flat {
            out.push(Diagnostic {
                field: "stepover",
                code: "param.passes.stepover.flat_width",
                error: false,
                message: "Stepover is wider than the flat of the tool; the floor will have ridges."
                    .into(),
            });
        }
        if self.step_down_mm <= 0.0 {
            out.push(Diagnostic {
                field: "step_down",
                code: "param.passes.step_down.range",
                error: true,
                message: "Step down must be greater than 0.".into(),
            });
        } else if self.step_down_mm > tool.flute_length_mm {
            out.push(Diagnostic {
                field: "step_down",
                code: "param.passes.step_down.flute",
                error: false,
                message: "Step down is longer than the flutes.".into(),
            });
        }
        out
    }
}

fn fmt_mm(v: f64) -> String {
    format!("{} mm", trim(v, 3))
}

fn trim(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Project display unit (S 08 §8.10a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Mm,
    Inch,
}

impl Units {
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Inch => "in",
        }
    }

    /// mm per display unit.
    #[must_use]
    pub fn scale(self) -> f64 {
        match self {
            Self::Mm => 1.0,
            Self::Inch => 25.4,
        }
    }
}

/// Format a length in mm for display in `units`, with `,` as the decimal
/// separator when `decimal_comma` is set.
#[must_use]
pub fn format_length(mm: f64, units: Units, decimal_comma: bool) -> String {
    let decimals = if units == Units::Inch { 4 } else { 3 };
    let s = trim(mm / units.scale(), decimals);
    if decimal_comma {
        s.replace('.', ",")
    } else {
        s
    }
}

/// Parse a length entry into mm: numbers with an optional unit (`6`, `6mm`,
/// `0.25in`, `1/4"`), `+ - * /`, parentheses and `tool.diameter` /
/// `tool.radius`. Both `.` and `,` are accepted as the decimal separator.
/// A bare number is in the project unit. The expression is evaluated now and
/// only the number is kept (D13 §4).
///
/// # Errors
///
/// A message for the field when the entry does not parse.
pub fn parse_length(entry: &str, units: Units, tool: &Tool) -> Result<f64, String> {
    let src: Vec<char> = entry.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser {
        s: &src,
        i: 0,
        units,
        tool,
    };
    let v = p.sum()?;
    if p.i != src.len() {
        return Err(format!(
            "Unexpected '{}'.",
            src[p.i..].iter().collect::<String>()
        ));
    }
    if !v.is_finite() {
        return Err("Not a finite number.".into());
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    units: Units,
    tool: &'a Tool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            if self.eat('+') {
                v += self.product()?;
            } else if self.eat('-') {
                v -= self.product()?;
            } else {
                return Ok(v);
            }
        }
    }

    // A product's factors are numbers; at most one of them carries a unit,
    // and that unit applies to the whole product, so `1/4"` is a quarter
    // inch and `tool.diameter*0.4` is 2.4 mm. With no unit, the product is
    // in the project unit.
    fn product(&mut self) -> Result<f64, String> {
        let (mut v, mut scale) = self.factor()?;
        loop {
            let div = if self.eat('*') {
                false
            } else if self.eat('/') {
                true
            } else {
                break;
            };
            let (w, s) = self.factor()?;
            if div {
                if w == 0.0 {
                    return Err("Division by zero.".into());
                }
                v /= w;
            } else {
                v *= w;
            }
            scale = scale.or(s);
        }
        Ok(v * scale.unwrap_or(self.units.scale()))
    }

    /// A factor: its number and, if it has a unit, mm per that unit.
    fn factor(&mut self) -> Result<(f64, Option<f64>), String> {
        if self.eat('-') {
            let (v, l) = self.factor()?;
            return Ok((-v, l));
        }
        if self.eat('(') {
            let v = self.sum()?;
            if !self.eat(')') {
                return Err("Missing ')'.".into());
            }
            return Ok((v, Some(1.0)));
        }
        if self.peek().is_some_and(char::is_alphabetic) {
            let start = self.i;
            while self
                .peek()
                .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_')
            {
                self.i += 1;
            }
            let name: String = self.s[start..self.i].iter().collect();
            return match name.as_str() {
                "tool.diameter" => Ok((self.tool.diameter_mm, Some(1.0))),
                "tool.radius" => Ok((self.tool.diameter_mm / 2.0, Some(1.0))),
                _ => Err(format!("Unknown name '{name}'.")),
            };
        }
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',')
        {
            self.i += 1;
        }
        if start == self.i {
            return Err("Expected a number.".into());
        }
        let text: String = self.s[start..self.i].iter().collect();
        let v: f64 = text
            .replace(',', ".")
            .parse()
            .map_err(|_| format!("'{text}' is not a number."))?;
        let rest: String = self.s[self.i..].iter().collect();
        for (suffix, scale) in [("mm", 1.0), ("in", 25.4), ("\"", 25.4)] {
            if rest.starts_with(suffix) {
                self.i += suffix.chars().count();
                return Ok((v, Some(scale)));
            }
        }
        Ok((v, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepover_rules() {
        let t = tool();
        let mut p = Pocket2d::default();
        assert!(p.validate(&t).is_empty());
        p.stepover_mm = 7.0;
        let d = p.validate(&t);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, "param.passes.stepover.max");
        assert!(d[0].error);
        p.stepover_mm = 0.0;
        assert_eq!(p.validate(&t)[0].code, "param.passes.stepover.range");
        let bull = Tool {
            corner_radius_mm: 1.0,
            ..tool()
        };
        p.stepover_mm = 5.0;
        let d = p.validate(&bull);
        assert_eq!(d[0].code, "param.passes.stepover.flat_width");
        assert!(!d[0].error);
    }

    #[test]
    fn step_down_rule() {
        let p = Pocket2d {
            step_down_mm: 20.0,
            ..Default::default()
        };
        assert_eq!(p.validate(&tool())[0].code, "param.passes.step_down.flute");
    }

    #[test]
    fn visibility_follows_pattern() {
        let mut p = Pocket2d::default();
        assert!(p.visible("pass_angle_deg") && !p.visible("offset_order"));
        p.pattern = Pattern::Offset;
        assert!(!p.visible("pass_angle_deg") && p.visible("offset_order"));
    }

    #[test]
    fn length_entry() {
        let t = tool();
        let mm = |s| parse_length(s, Units::Mm, &t).unwrap();
        assert_eq!(mm("2.5"), 2.5);
        assert_eq!(mm("2,5"), 2.5);
        assert_eq!(mm("6/2"), 3.0);
        assert_eq!(mm("0.25in"), 6.35);
        assert_eq!(mm("1/4\""), 6.35);
        assert!((mm("tool.diameter*0.4") - 2.4).abs() < 1e-12);
        assert_eq!(mm("1mm + 1in"), 26.4);
        assert_eq!(parse_length("1", Units::Inch, &t).unwrap(), 25.4);
        assert_eq!(parse_length("6mm", Units::Inch, &t).unwrap(), 6.0);
        assert!(parse_length("2x", Units::Mm, &t).is_err());
        assert!(parse_length("1/0", Units::Mm, &t).is_err());
        assert!(parse_length("tool.length", Units::Mm, &t).is_err());
    }

    #[test]
    fn length_display() {
        assert_eq!(format_length(2.4, Units::Mm, false), "2.4");
        assert_eq!(format_length(2.4, Units::Mm, true), "2,4");
        assert_eq!(format_length(25.4, Units::Inch, false), "1");
        assert_eq!(format_length(6.35, Units::Inch, false), "0.25");
    }

    #[test]
    fn large_project_size() {
        let p = large_project(10, 20);
        assert_eq!(p.iter().map(|s| s.toolpaths.len()).sum::<usize>(), 200);
    }
}
