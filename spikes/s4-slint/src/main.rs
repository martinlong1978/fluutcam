// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S4: the FluutCAM window in `slint` (S 08 §8.2).
//!
//! The UI is declared in `ui/app.slint`; this file holds the state, the
//! callbacks and the `wgpu` viewport, which renders into a texture that is
//! shown as a `slint::Image`.
//!
//! `cargo run --release`; see `s4_common::bench` for the measurement modes.

use s4_common::bench::{self, Bench};
use s4_common::camera::Camera;
use s4_common::mock::{self, Direction, OffsetOrder, Pattern, Pocket2d, Status, Units};
use s4_common::render::{Preview, Viewport};
use s4_common::{glam, mesh};
use slint::wgpu_30::{WGPUConfiguration, wgpu};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

slint::include_modules!();

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewport: Viewport,
}

struct State {
    units: Units,
    decimal_comma: bool,
    setups: Vec<mock::Setup>,
    expanded: Vec<bool>,
    selected: (usize, Option<usize>),
    params: Pocket2d,
    tool: mock::Tool,
    preview: bool,
    camera: Camera,
    zero: glam::Vec3,
    bench: Bench,
    /// The viewport must be drawn again.
    dirty: bool,
    gpu: Option<Gpu>,
    mesh: Option<mesh::Mesh>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let t0 = Instant::now();
    let mesh = mesh::relief(bench::grid());
    slint::BackendSelector::new()
        .require_wgpu_30(WGPUConfiguration::default())
        .select()?;
    let app = AppWindow::new()?;
    let setups = bench::project();
    let state = Rc::new(RefCell::new(State {
        units: Units::Mm,
        decimal_comma: false,
        expanded: vec![true; setups.len()],
        setups,
        selected: (0, Some(1)),
        params: Pocket2d::default(),
        tool: mock::tool(),
        preview: true,
        camera: Camera::framing(mesh.min, mesh.max),
        zero: glam::Vec3::new(mesh.min.x, mesh.min.y, mesh.max.z),
        bench: Bench::from_env(t0),
        dirty: true,
        gpu: None,
        mesh: Some(mesh),
    }));

    app.set_stages(ModelRc::new(VecModel::from(
        mock::STAGES
            .iter()
            .map(|s| SharedString::from(s.to_uppercase()))
            .collect::<Vec<_>>(),
    )));
    set_ribbon(&app, 2);
    sync(&app, &state.borrow());

    app.on_stage_selected({
        let app = app.as_weak();
        move |i| {
            let app = app.unwrap();
            app.set_stage(i);
            set_ribbon(&app, usize::try_from(i).unwrap_or(0));
        }
    });
    let update =
        |app: &slint::Weak<AppWindow>, state: &Rc<RefCell<State>>, f: &dyn Fn(&mut State)| {
            let app = app.unwrap();
            let mut s = state.borrow_mut();
            f(&mut s);
            s.dirty = true;
            sync(&app, &s);
            app.window().request_redraw();
        };
    macro_rules! on {
        ($setter:ident, |$s:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {{
            let (app, state) = (app.as_weak(), state.clone());
            app_handle(&app).$setter(move |$($arg: $ty),*| update(&app, &state, &|$s: &mut State| $body));
        }};
    }
    fn app_handle(w: &slint::Weak<AppWindow>) -> AppWindow {
        w.unwrap()
    }
    on!(on_toggle_units, |s| s.units = if s.units == Units::Mm {
        Units::Inch
    } else {
        Units::Mm
    });
    on!(on_toggle_comma, |s| s.decimal_comma = !s.decimal_comma);
    on!(on_preview_toggled, |s| s.preview = !s.preview);
    on!(
        on_pattern_selected,
        |s, i: i32| s.params.pattern = if i == 0 {
            Pattern::Offset
        } else {
            Pattern::Raster
        }
    );
    on!(
        on_direction_selected,
        |s, i: i32| s.params.direction = if i == 0 {
            Direction::Climb
        } else {
            Direction::Conventional
        }
    );
    on!(on_offset_order_selected, |s, i: i32| s
        .params
        .offset_order = if i == 0 {
        OffsetOrder::InsideOut
    } else {
        OffsetOrder::OutsideIn
    });
    on!(on_row_clicked, |s, si: i32, ti: i32| {
        let si = usize::try_from(si).unwrap_or(0);
        match usize::try_from(ti) {
            Ok(ti) => s.selected = (si, Some(ti)),
            Err(_) => s.expanded[si] = !s.expanded[si],
        }
    });
    on!(on_field_scrub, |s, key: SharedString, dx: f32| {
        let p = &mut s.params;
        match key.as_str() {
            "stepover" => {
                p.stepover_mm = (p.stepover_mm + f64::from(dx) * 0.01 * s.units.scale()).max(0.0)
            }
            "step_down" => {
                p.step_down_mm = (p.step_down_mm + f64::from(dx) * 0.01 * s.units.scale()).max(0.0)
            }
            _ => p.pass_angle_deg += f64::from(dx) * 0.5,
        }
    });
    on!(
        on_field_entered,
        |s, key: SharedString, text: SharedString| {
            if key == "pass_angle_deg" {
                if let Ok(v) = text.trim_end_matches('°').replace(',', ".").parse() {
                    s.params.pass_angle_deg = v;
                }
            } else if let Ok(v) = mock::parse_length(&text, s.units, &s.tool) {
                if key == "stepover" {
                    s.params.stepover_mm = v;
                } else {
                    s.params.step_down_mm = v;
                }
            }
        }
    );
    on!(
        on_viewport_drag,
        |s, button: slint::platform::PointerEventButton, dx: f32, dy: f32| {
            let h = s.camera_height();
            match button {
                slint::platform::PointerEventButton::Left => s.camera.orbit(dx, dy),
                _ => s.camera.pan(dx, dy, h),
            }
        }
    );
    on!(on_viewport_zoom, |s, dy: f32| s.camera.zoom(dy / 50.0));

    let weak = app.as_weak();
    let st = state.clone();
    app.window().set_rendering_notifier(move |rs, api| {
        let Some(app) = weak.upgrade() else { return };
        let mut s = st.borrow_mut();
        match (rs, api) {
            (
                slint::RenderingState::RenderingSetup,
                slint::GraphicsAPI::WGPU30 { device, queue, .. },
            ) => {
                let mesh = s.mesh.take().expect("mesh");
                s.gpu = Some(Gpu {
                    viewport: Viewport::new(device, &mesh),
                    device: device.clone(),
                    queue: queue.clone(),
                });
            }
            (slint::RenderingState::BeforeRendering, _) => before_rendering(&app, &mut s),
            _ => {}
        }
    })?;
    app.run()?;
    Ok(())
}

impl State {
    fn camera_height(&self) -> f32 {
        // Pan scale needs the viewport height in logical pixels; the camera
        // works in the same pixels as the drag.
        self.gpu
            .as_ref()
            .map_or(600.0, |_| VIEWPORT_H.with(|h| *h.borrow()))
    }
}

thread_local! {
    static VIEWPORT_H: RefCell<f32> = const { RefCell::new(600.0) };
}

fn before_rendering(app: &AppWindow, s: &mut State) {
    if s.bench.frame(&mut s.camera) {
        let _ = slint::quit_event_loop();
        return;
    }
    let scale = app.window().scale_factor();
    let (lw, lh) = (app.get_viewport_width(), app.get_viewport_height());
    VIEWPORT_H.with(|h| *h.borrow_mut() = lh);
    let (w, h) = ((lw * scale).round() as u32, (lh * scale).round() as u32);
    let preview = Preview {
        enabled: s.preview && s.params.pattern == Pattern::Raster,
        stepover_mm: s.params.stepover_mm as f32,
        angle_deg: s.params.pass_angle_deg as f32,
    };
    let continuous = s.bench.continuous();
    let cam = s.camera;
    let Some(gpu) = s.gpu.as_mut() else { return };
    let resized = gpu.viewport.ensure_size(&gpu.device, w, h);
    if resized || s.dirty || continuous {
        gpu.viewport.render(&gpu.device, &gpu.queue, &cam, preview);
        let tex = gpu.viewport.texture().expect("sized").clone();
        app.set_viewport_image(slint::Image::try_from(tex).expect("texture import"));
        set_triad(app, &cam, s.zero, lw, lh);
        s.dirty = false;
    }
    if continuous {
        app.window().request_redraw();
    }
}

fn set_triad(app: &AppWindow, cam: &Camera, zero: glam::Vec3, w: f32, h: f32) {
    let size = glam::Vec2::new(w, h);
    let Some(o) = cam.project(zero, size) else {
        return;
    };
    let line = |axis: glam::Vec3| {
        cam.project(zero + axis * 15.0, size)
            .map_or_else(SharedString::new, |e| {
                format!("M {} {} L {} {}", o.x, o.y, e.x, e.y).into()
            })
    };
    app.set_triad_x(line(glam::Vec3::X));
    app.set_triad_y(line(glam::Vec3::Y));
    app.set_triad_z(line(glam::Vec3::Z));
    app.set_zero_x(o.x);
    app.set_zero_y(o.y);
}

fn set_ribbon(app: &AppWindow, stage: usize) {
    let groups: Vec<RibbonGroup> = mock::ribbon(stage)
        .iter()
        .map(|g| RibbonGroup {
            name: g.name.into(),
            gallery: g.gallery,
            commands: ModelRc::new(VecModel::from(
                g.commands
                    .iter()
                    .map(|c| SharedString::from(*c))
                    .collect::<Vec<_>>(),
            )),
        })
        .collect();
    app.set_ribbon(ModelRc::new(VecModel::from(groups)));
}

fn status_code(st: Status) -> (i32, f32) {
    match st {
        Status::NotCalculated => (0, 0.0),
        Status::Calculating(p) => (1, f32::from(p) / 100.0),
        Status::Done => (2, 0.0),
        Status::Stale => (3, 0.0),
        Status::Warning => (4, 0.0),
        Status::Error => (5, 0.0),
    }
}

/// Push the state into the UI properties (the view-model step of D13 §1).
fn sync(app: &AppWindow, s: &State) {
    let mut rows = Vec::new();
    let mut clips = Vec::new();
    for (si, setup) in s.setups.iter().enumerate() {
        rows.push(TreeRow {
            depth: 0,
            text: format!("{}  ·  {}", setup.name, setup.kind).into(),
            status: -1,
            status_label: SharedString::new(),
            progress: 0.0,
            selected: false,
            expanded: s.expanded[si],
            setup: si as i32,
            toolpath: -1,
        });
        for (ti, tp) in setup.toolpaths.iter().enumerate() {
            let selected = s.selected == (si, Some(ti));
            if s.expanded[si] {
                let (status, progress) = status_code(tp.status);
                rows.push(TreeRow {
                    depth: 1,
                    text: tp.name.as_str().into(),
                    status,
                    status_label: tp.status.label().into(),
                    progress,
                    selected,
                    expanded: false,
                    setup: si as i32,
                    toolpath: ti as i32,
                });
            }
            clips.push(Clip {
                name: tp.name.as_str().into(),
                selected,
            });
        }
    }
    let same_rows = app.get_tree().row_count() == rows.len()
        && app.get_tree().iter().zip(&rows).all(|(a, b)| a == *b);
    if !same_rows {
        app.set_tree(ModelRc::new(VecModel::from(rows)));
        app.set_clips(ModelRc::new(VecModel::from(clips)));
    }

    app.set_units(s.units.suffix().into());
    app.set_decimal_comma(s.decimal_comma);
    let (si, ti) = s.selected;
    if let Some(tp) = ti.and_then(|t| s.setups.get(si)?.toolpaths.get(t)) {
        app.set_item_name(tp.name.as_str().into());
        app.set_item_kind(format!("{} · {}", tp.kind, s.setups[si].name).into());
    }
    app.set_tool_name(s.tool.name.into());
    let p = &s.params;
    app.set_pattern(i32::from(p.pattern == Pattern::Raster));
    app.set_direction(i32::from(p.direction == Direction::Conventional));
    app.set_offset_order(i32::from(p.offset_order == OffsetOrder::OutsideIn));
    let len = |mm| {
        format!(
            "{} {}",
            mock::format_length(mm, s.units, s.decimal_comma),
            s.units.suffix()
        )
    };
    app.set_stepover_text(len(p.stepover_mm).into());
    app.set_step_down_text(len(p.step_down_mm).into());
    app.set_angle_text(format!("{:.1}°", p.pass_angle_deg).into());
    let diags = p.validate(&s.tool);
    let msg = |key| diags.iter().find(|d| d.field == key);
    let stepover = msg("stepover");
    app.set_stepover_message(
        stepover
            .map(|d| d.message.clone())
            .unwrap_or_default()
            .into(),
    );
    app.set_stepover_error(stepover.is_some_and(|d| d.error));
    let step_down = msg("step_down");
    app.set_step_down_message(
        step_down
            .map(|d| d.message.clone())
            .unwrap_or_default()
            .into(),
    );
    app.set_step_down_error(step_down.is_some_and(|d| d.error));
    app.set_preview(s.preview);
}
