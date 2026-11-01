// SPDX-License-Identifier: MIT OR Apache-2.0

//! The strategy interface and registry (D08 §1, D09 §1).

use crate::geometry::SetupGeometry;
use crate::strategy::{Contour2d, Face, Parallel, Pocket2d};
use fluut_geom::{Aabb3, Affine, Point2, Point3};
use fluut_model::params::{FieldMeta, ToolpathTypeId};
use fluut_model::project::{SetupKind, Toolpath};
use fluut_model::resolve::{HeightEnv, ResolvedHeights, ResolvedParams, resolve};
use fluut_model::tools::Tool;
use fluut_model::validate::{MachineLimits, PostCaps, ValidateCx, validate};
use fluut_model::{Diagnostic, Severity};
use fluut_path::{PathError, ToolpathResult};
use std::fmt;

/// What a type is for (spec 03 §3.10.4).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Role {
    Rough,
    Finish,
    Hole,
    Utility,
}

/// Inputs of one toolpath calculation.
#[derive(Debug)]
pub struct StrategyInput<'a> {
    pub toolpath: &'a Toolpath,
    /// The Setup's bodies in the work frame W.
    pub geometry: &'a SetupGeometry,
    pub tool: &'a Tool,
    pub material: Option<&'a str>,
    pub setup_kind: SetupKind,
    /// Height references of the Setup; `selection` is filled by the type.
    pub env: HeightEnv,
    /// The Setup stock in W, when the Setup has one.
    pub stock: Option<Aabb3>,
    /// Model frame M → work frame W, for points stored in M.
    pub to_work: Affine,
    pub machine: Option<MachineLimits>,
    pub post: Option<PostCaps>,
}

impl StrategyInput<'_> {
    /// A model-frame point in W, projected to XY.
    pub fn point_xy(&self, p: [f64; 3]) -> Point2 {
        let w = self.to_work.apply_point(Point3::new(p[0], p[1], p[2]));
        Point2::new(w.x(), w.y())
    }
}

/// A calculated toolpath and its warnings.
#[derive(Clone, Debug)]
pub struct Generated {
    pub result: ToolpathResult,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub enum CamError {
    /// Validation or calculation errors (and the warnings found so far).
    Invalid(Vec<Diagnostic>),
    /// The type exists but cannot be calculated by this build.
    Unavailable(String),
    /// The result broke a path invariant (a bug).
    Path(PathError),
}

impl CamError {
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        match self {
            CamError::Invalid(d) => d.clone(),
            CamError::Unavailable(m) => vec![Diagnostic::error("cam.type.unavailable", m.clone())],
            CamError::Path(e) => vec![Diagnostic::error(
                "cam.internal.path",
                format!("internal error, the toolpath is invalid: {e}"),
            )],
        }
    }
}

impl fmt::Display for CamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.diagnostics();
        let msgs: Vec<String> = d.iter().map(ToString::to_string).collect();
        f.write_str(&msgs.join("; "))
    }
}

impl From<Diagnostic> for CamError {
    fn from(d: Diagnostic) -> Self {
        CamError::Invalid(vec![d])
    }
}

/// A toolpath type (D08 §1). Strategies supply their drive pattern; the
/// framework does validation, heights, linking and output.
pub trait ToolpathType: Send + Sync + fmt::Debug {
    fn id(&self) -> ToolpathTypeId;
    fn role(&self) -> Role;
    fn allowed_setups(&self) -> &'static [SetupKind] {
        &[SetupKind::ThreeAxis, SetupKind::Indexed]
    }
    fn fields(&self) -> &'static [FieldMeta] {
        self.id().fields()
    }
    fn generate(&self, input: &StrategyInput) -> Result<Generated, CamError>;
}

static CONTOUR_2D: Contour2d = Contour2d;
static POCKET_2D: Pocket2d = Pocket2d;
static PARALLEL: Parallel = Parallel;
static FACE: Face = Face;
static REGISTRY: [&dyn ToolpathType; 4] = [&CONTOUR_2D, &POCKET_2D, &PARALLEL, &FACE];

/// Every type of this build (D09 §1).
pub fn registry() -> &'static [&'static dyn ToolpathType] {
    &REGISTRY
}

pub fn find(id: ToolpathTypeId) -> &'static dyn ToolpathType {
    match id {
        ToolpathTypeId::Contour2d => &CONTOUR_2D,
        ToolpathTypeId::Pocket2d => &POCKET_2D,
        ToolpathTypeId::Parallel => &PARALLEL,
        ToolpathTypeId::Face => &FACE,
    }
}

/// Calculates one toolpath with its type.
pub fn calculate(input: &StrategyInput) -> Result<Generated, CamError> {
    find(input.toolpath.kind()).generate(input)
}

/// Resolved parameters and heights after validation, and the warnings.
#[derive(Debug)]
pub struct Prepared {
    pub params: ResolvedParams,
    pub heights: ResolvedHeights,
    pub warnings: Vec<Diagnostic>,
}

/// Resolves the parameters and heights (with the drive selection's Z range)
/// and runs every parameter rule. Errors stop the calculation.
pub fn prepare(input: &StrategyInput, selection: Option<(f64, f64)>) -> Result<Prepared, CamError> {
    let tp = input.toolpath;
    let params = resolve(&tp.params, input.tool, input.material);
    let env = HeightEnv {
        selection,
        ..input.env
    };
    let heights = params.heights.resolve(&env).ok();
    let cx = ValidateCx {
        tool: Some(input.tool),
        material: input.material,
        setup_kind: input.setup_kind,
        heights: heights.as_ref(),
        stock_bottom: Some(input.env.stock_bottom),
        machine: input.machine.as_ref(),
        post: input.post.as_ref(),
    };
    let diags = validate(tp, &cx);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(CamError::Invalid(diags));
    }
    let Some(heights) = heights else {
        let e = params.heights.resolve(&env).err().into_iter();
        return Err(CamError::Invalid(diags.into_iter().chain(e).collect()));
    };
    Ok(Prepared {
        params,
        heights,
        warnings: diags,
    })
}

/// Tags diagnostics with the toolpath id.
pub fn tag(diags: Vec<Diagnostic>, tp: &Toolpath) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .map(|d| {
            if d.toolpath.is_some() {
                d
            } else {
                d.on(&tp.id)
            }
        })
        .collect()
}
