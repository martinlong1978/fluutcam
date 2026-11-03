// SPDX-License-Identifier: MIT OR Apache-2.0

//! The strategy interface and registry (D08 §1, D09 §1).

use crate::geometry::SetupGeometry;
use crate::region::RegionFailure;
use crate::stock::LayeredStock;
use crate::strategy::{
    Bore, Chamfer2d, Contour, Contour2d, Drill, Engrave, Face, Horizontal, ManualNc,
    OptimisedLoad2d, OptimisedLoad3d, Parallel, Pocket2d, Pocket3d, RotaryAlongX, RotaryAroundA,
    RotaryRough, RotarySpiral, Scallop, Spiral, SteepAndShallow, Trace,
};
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
    /// The in-process stock after the earlier enabled toolpaths of the
    /// Setup (D08 §7); set for toolpaths with `rest_from = "previous"`.
    pub in_process: Option<&'a LayeredStock>,
    /// Rotary data of a `rotary` Setup.
    pub rotary: Option<RotaryInfo>,
}

/// What rotary types need of a `rotary` Setup (spec 03 §3.9). Heights in
/// `StrategyInput::env` are radii there (D15b §1.3).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RotaryInfo {
    /// Radius of the stock cylinder.
    pub stock_radius: f64,
    /// X of the stock cylinder ends.
    pub stock_x: (f64, f64),
    /// X extent of the Setup's bodies.
    pub model_x: (f64, f64),
    /// The machine's rotary axis: `None` when the Setup names no machine,
    /// `Some(None)` when its machine has no rotary axis.
    pub machine_axis: Option<Option<RotaryAxisInfo>>,
}

/// The machine's A axis (spec 02 §2.3).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RotaryAxisInfo {
    pub continuous: bool,
    /// `[min, max]` in degrees; `None` when unlimited.
    pub limits_deg: Option<(f64, f64)>,
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
static TRACE: Trace = Trace;
static CHAMFER_2D: Chamfer2d = Chamfer2d;
static BORE: Bore = Bore;
static DRILL: Drill = Drill;
static OPTIMISED_LOAD_2D: OptimisedLoad2d = OptimisedLoad2d;
static ENGRAVE: Engrave = Engrave;
static CONTOUR: Contour = Contour;
static HORIZONTAL: Horizontal = Horizontal;
static SPIRAL: Spiral = Spiral;
static SCALLOP: Scallop = Scallop;
static STEEP_AND_SHALLOW: SteepAndShallow = SteepAndShallow;
static OPTIMISED_LOAD_3D: OptimisedLoad3d = OptimisedLoad3d;
static POCKET_3D: Pocket3d = Pocket3d;
static ROTARY_ROUGH: RotaryRough = RotaryRough;
static ROTARY_ALONG_X: RotaryAlongX = RotaryAlongX;
static ROTARY_AROUND_A: RotaryAroundA = RotaryAroundA;
static ROTARY_SPIRAL: RotarySpiral = RotarySpiral;
static MANUAL_NC: ManualNc = ManualNc;
static REGISTRY: [&dyn ToolpathType; 22] = [
    &CONTOUR_2D,
    &POCKET_2D,
    &PARALLEL,
    &FACE,
    &TRACE,
    &CHAMFER_2D,
    &BORE,
    &DRILL,
    &OPTIMISED_LOAD_2D,
    &ENGRAVE,
    &CONTOUR,
    &HORIZONTAL,
    &SPIRAL,
    &SCALLOP,
    &STEEP_AND_SHALLOW,
    &OPTIMISED_LOAD_3D,
    &POCKET_3D,
    &ROTARY_ROUGH,
    &ROTARY_ALONG_X,
    &ROTARY_AROUND_A,
    &ROTARY_SPIRAL,
    &MANUAL_NC,
];

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
        ToolpathTypeId::Trace => &TRACE,
        ToolpathTypeId::Chamfer2d => &CHAMFER_2D,
        ToolpathTypeId::Bore => &BORE,
        ToolpathTypeId::Drill => &DRILL,
        ToolpathTypeId::OptimisedLoad2d => &OPTIMISED_LOAD_2D,
        ToolpathTypeId::Engrave => &ENGRAVE,
        ToolpathTypeId::Contour => &CONTOUR,
        ToolpathTypeId::Horizontal => &HORIZONTAL,
        ToolpathTypeId::Spiral => &SPIRAL,
        ToolpathTypeId::Scallop => &SCALLOP,
        ToolpathTypeId::SteepAndShallow => &STEEP_AND_SHALLOW,
        ToolpathTypeId::OptimisedLoad3d => &OPTIMISED_LOAD_3D,
        ToolpathTypeId::Pocket3d => &POCKET_3D,
        ToolpathTypeId::RotaryRough => &ROTARY_ROUGH,
        ToolpathTypeId::RotaryAlongX => &ROTARY_ALONG_X,
        ToolpathTypeId::RotaryAroundA => &ROTARY_AROUND_A,
        ToolpathTypeId::RotarySpiral => &ROTARY_SPIRAL,
        ToolpathTypeId::ManualNc => &MANUAL_NC,
    }
}

/// Calculates one toolpath with its type.
pub fn calculate(input: &StrategyInput) -> Result<Generated, CamError> {
    guarded(input.toolpath, || {
        find(input.toolpath.kind()).generate(input)
    })
}

/// Runs `f`, turning a [`RegionFailure`] raised in it (also on a `rayon`
/// thread, which passes the payload on) into an error on toolpath `tp`.
/// Other panics are bugs and are not caught.
fn guarded(
    tp: &Toolpath,
    f: impl FnOnce() -> Result<Generated, CamError>,
) -> Result<Generated, CamError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(e) => match e.downcast::<RegionFailure>() {
            Ok(fail) => Err(CamError::Invalid(vec![
                Diagnostic::error(
                    fail.code(),
                    format!("toolpath \"{}\" could not be calculated: {fail}", tp.name),
                )
                .on(&tp.id),
            ])),
            Err(e) => std::panic::resume_unwind(e),
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{Region2, fault};
    use rayon::prelude::*;

    /// Forces region failures on this thread until dropped.
    struct Forced;

    impl Forced {
        fn on() -> Forced {
            fault::set(true);
            Forced
        }
    }

    impl Drop for Forced {
        fn drop(&mut self) {
            fault::set(false);
        }
    }

    fn toolpath() -> Toolpath {
        Toolpath::new("t7", "Roughing", ToolpathTypeId::Pocket2d, "lib#f6")
    }

    fn codes(r: Result<Generated, CamError>) -> Vec<(String, Option<String>)> {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e
                .diagnostics()
                .into_iter()
                .map(|d| (d.code, d.toolpath))
                .collect(),
        }
    }

    #[test]
    fn a_region_failure_is_an_error_on_the_toolpath() {
        let square = Region2::rect(0.0, 0.0, 10.0, 10.0);
        let t = toolpath();
        let on_t7 = |code: &str| vec![(code.to_owned(), Some("t7".to_owned()))];
        let r = guarded(&t, || {
            let _f = Forced::on();
            square.offset(-1.0);
            unreachable!()
        });
        assert_eq!(codes(r), on_t7("cam.region.offset_failed"));
        let r = guarded(&t, || {
            let _f = Forced::on();
            square.subtract(&Region2::rect(2.0, 2.0, 4.0, 4.0));
            unreachable!()
        });
        assert_eq!(codes(r), on_t7("cam.region.boolean_failed"));
        // From a `rayon` worker too.
        let r = guarded(&t, || {
            let areas: Vec<f64> = (0..4)
                .into_par_iter()
                .map(|i| {
                    let _f = (i == 3).then(Forced::on);
                    square.offset(-f64::from(i)).area()
                })
                .collect();
            unreachable!("{areas:?}")
        });
        assert_eq!(codes(r), on_t7("cam.region.offset_failed"));
        // Without the failure, nothing changes.
        let r = guarded(&t, || {
            square.offset(-1.0);
            Err(CamError::Unavailable("x".into()))
        });
        assert_eq!(codes(r), vec![("cam.type.unavailable".to_owned(), None)]);
    }

    #[test]
    fn other_panics_are_not_caught() {
        let e = std::panic::catch_unwind(|| guarded(&toolpath(), || panic!("a bug"))).unwrap_err();
        assert_eq!(e.downcast_ref::<&str>(), Some(&"a bug"));
    }
}
