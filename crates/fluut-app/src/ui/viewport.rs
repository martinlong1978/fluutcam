// SPDX-License-Identifier: MIT OR Apache-2.0

//! The viewport widget: the renderer's texture as an image, camera input,
//! picking on click, and the work-zero triad drawn by `egui` on top
//! (D13 §6: overlays drawn by `egui`).
//!
//! It shows the Setup of the selection (or the first Setup) in its work
//! frame, with that Setup's toolpaths.

use super::UiState;
use crate::brand::{self, Palette, c};
use crate::jobs::Jobs;
use crate::scene::SetupScene;
use crate::session::Snapshot;
use crate::text::{field_label, tr, tr_args};
use crate::viewport::{Camera, FrameState, Pick, PickMask, Renderer, Tints};
use crate::vm::Selection;
use crate::vm::pick::{PickMode, highlight};
use crate::vm::playback::StockColour;
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Vec2};
use eframe::egui_wgpu;
use fluut_core::fluut_sim::StockMesh;
use fluut_model::{Params, Sel};
use std::sync::Arc;

/// What has been uploaded: the Setup, its scene and its toolpath lines (by
/// pointer, since snapshots share them through `Arc`s). The renderer keeps
/// the stock blocks it shows itself.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Uploaded {
    setup: String,
    scene: usize,
    paths: Vec<usize>,
}

pub struct ViewportState {
    pub camera: Camera,
    renderer: Option<Renderer>,
    texture: Option<egui::TextureId>,
    uploaded: Option<Uploaded>,
    /// Toolpath ids in pick order.
    toolpaths: Vec<String>,
    /// Setup the camera was last fitted to.
    fitted: Option<String>,
    drawn: Option<FrameState>,
    /// What the tints were last worked out for.
    tinted: Option<TintKey>,
    /// Where the pointer was for the last hover pick.
    hover_at: Option<Pos2>,
}

/// The highlight changes only with the document or these.
type TintKey = (u64, String, Selection, Option<PickMode>);

impl std::fmt::Debug for ViewportState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewportState")
            .field("camera", &self.camera)
            .field("uploaded", &self.uploaded)
            .finish_non_exhaustive()
    }
}

impl ViewportState {
    pub fn new(render: Option<&egui_wgpu::RenderState>) -> ViewportState {
        ViewportState {
            camera: Camera::fit(None),
            renderer: render.map(|rs| Renderer::new(&rs.device)),
            texture: None,
            uploaded: None,
            toolpaths: Vec::new(),
            fitted: None,
            drawn: None,
            tinted: None,
            hover_at: None,
        }
    }
}

/// What a pick means in a pick mode.
fn pick_mask(mode: &PickMode) -> PickMask {
    PickMask {
        faces: true,
        edges: mode.filter.wants_edges(),
        toolpaths: false,
    }
}

/// The entity a pick hit, by its persistent id.
fn entity(scene: Option<&SetupScene>, p: Pick) -> Option<Sel> {
    let s = scene?;
    match p {
        Pick::Face(i) => s.faces.get(i as usize).cloned().map(Sel::Face),
        Pick::Edge(i) => s.edge_ids.get(i as usize).cloned().map(Sel::Edge),
        Pick::Toolpath(_) | Pick::None => None,
    }
}

/// The Setup the viewport shows for a selection.
pub fn shown_setup(s: &Snapshot, sel: &Selection) -> Option<String> {
    let id = match sel {
        Selection::Setup(id) | Selection::Face { setup: id, .. } => Some(id.clone()),
        Selection::Toolpath(t) => s.project.toolpath(t).map(|(st, _)| st.id.clone()),
        Selection::None => None,
    };
    id.or_else(|| s.project.setups.first().map(|st| st.id.clone()))
}

fn upload(
    vs: &mut ViewportState,
    rs: &egui_wgpu::RenderState,
    s: &Snapshot,
    setup: &str,
    stock: Option<&[Arc<StockMesh>]>,
) {
    let Some(scene) = s.scenes.get(setup) else {
        return;
    };
    let st = s.project.setup(setup);
    let ids: Vec<String> = st
        .map(|st| st.toolpaths.iter().map(|t| t.id.clone()).collect())
        .unwrap_or_default();
    let key = Uploaded {
        setup: setup.to_owned(),
        scene: Arc::as_ptr(scene) as usize,
        paths: ids
            .iter()
            .map(|id| s.paths.get(id).map_or(0, |p| Arc::as_ptr(p) as usize))
            .collect(),
    };
    let Some(r) = vs.renderer.as_mut() else {
        return;
    };
    // The stock blocks: only the changed ones are uploaded (D11 §4).
    if r.set_stock(&rs.device, stock) > 0 {
        vs.drawn = None;
    }
    if vs.uploaded.as_ref() == Some(&key) {
        return;
    }
    let same_scene = vs
        .uploaded
        .as_ref()
        .is_some_and(|u| u.setup == key.setup && u.scene == key.scene);
    if !same_scene {
        r.set_scene(&rs.device, &scene.mesh, &scene.edges, scene.stock.as_ref());
    }
    let lines: Vec<Option<&crate::scene::Lines>> = ids
        .iter()
        .map(|id| s.paths.get(id).map(|p| &p.lines))
        .collect();
    r.set_toolpaths(&rs.device, &lines);
    if vs.fitted.as_deref() != Some(setup) {
        vs.camera = Camera::fit(scene.bounds.as_ref());
        vs.fitted = Some(setup.to_owned());
    }
    vs.toolpaths = ids;
    vs.uploaded = Some(key);
    vs.drawn = None;
}

pub fn show(
    ui: &mut egui::Ui,
    jobs: &mut Jobs,
    state: &mut UiState,
    vs: &mut ViewportState,
    render: Option<&egui_wgpu::RenderState>,
) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let label = tr("viewport-label");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
    let p = Palette::get(state.dark);
    let painter = ui.painter_at(rect);
    let (Some(rs), Some(snap)) = (render, jobs.snapshot.clone()) else {
        painter.rect_filled(rect, 0.0, c(p.base));
        if render.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr("viewport-no-gpu"),
                egui::FontId::proportional(13.0),
                c(p.weak_text),
            );
        }
        return;
    };
    // With no Setup yet, the model alone is shown.
    let Some(setup) = shown_setup(&snap, &state.selection).or_else(|| {
        snap.scenes
            .contains_key(crate::session::MODEL_SCENE)
            .then(|| crate::session::MODEL_SCENE.to_owned())
    }) else {
        return;
    };
    // The simulated stock of this Setup, while the simulation is current.
    let sim_frame = jobs
        .frame
        .clone()
        .filter(|f| f.setup == setup && snap.sims.contains_key(&setup));
    let stock = sim_frame
        .as_ref()
        .and_then(|f| f.blocks.as_deref())
        .filter(|_| state.show_stock && state.pick.is_none());
    upload(vs, rs, &snap, &setup, stock);
    // Camera input.
    let size = rect.size();
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        vs.camera.orbit(d.x, d.y);
    }
    if resp.dragged_by(egui::PointerButton::Middle)
        || resp.dragged_by(egui::PointerButton::Secondary)
    {
        let d = resp.drag_delta();
        vs.camera.pan(d.x, d.y, size.y);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            vs.camera.zoom(scroll / 50.0);
        }
    }
    if resp.double_clicked() {
        vs.camera = Camera::fit(snap.scenes.get(&setup).and_then(|s| s.bounds.as_ref()));
    }
    let selected_tp = match &state.selection {
        Selection::Toolpath(t) => vs.toolpaths.iter().position(|x| x == t).map(|i| i as u32),
        _ => None,
    };
    let scene = snap.scenes.get(&setup);
    let face = match &state.selection {
        Selection::Face { face, .. } => scene
            .and_then(|s| s.faces.iter().position(|f| f == face))
            .map(|i| i as u32),
        _ => None,
    };
    let frame = FrameState {
        camera: vs.camera,
        dark: state.dark,
        face,
        toolpath: selected_tp,
        stock_colour: if stock.is_some() {
            state.stock_colour
        } else {
            StockColour::Plain
        },
    };
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = ((size.x * ppp).round() as u32, (size.y * ppp).round() as u32);
    let Some(r) = vs.renderer.as_mut() else {
        return;
    };
    let resized = r.ensure_size(&rs.device, w, h);
    if resized || vs.texture.is_none() {
        let view = r.view().expect("sized");
        let mut er = rs.renderer.write();
        vs.texture = Some(match vs.texture {
            Some(id) => {
                er.update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => er.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear),
        });
    }
    // What is under the pointer, while picking; only when it moved.
    let pointer = resp.hover_pos();
    if let Some(mode) = state.pick.as_mut() {
        if pointer != vs.hover_at {
            vs.hover_at = pointer;
            mode.hover = pointer.and_then(|pos| {
                let local = (pos - rect.min) * ppp;
                let (x, y) = (local.x as u32, local.y as u32);
                let hit = r.pick_masked(&rs.device, &rs.queue, &frame, x, y, pick_mask(mode));
                entity(scene.map(|s| &**s), hit).filter(|e| mode.accepts(&snap, e))
            });
        }
    } else {
        vs.hover_at = None;
    }
    let key = (
        snap.revision,
        setup.clone(),
        state.selection.clone(),
        state.pick.clone(),
    );
    if vs.tinted.as_ref() != Some(&key) {
        let h = scene.map_or_else(Default::default, |s| {
            highlight(&snap, s, &state.selection, state.pick.as_ref())
        });
        if r.set_tints(Tints {
            faces: h.faces,
            edges: h.edges,
            hover_face: h.hover_face,
            hover_edge: h.hover_edge,
        }) {
            vs.drawn = None;
        }
        vs.tinted = Some(key);
    }
    if resized || vs.drawn != Some(frame) {
        r.render(&rs.device, &rs.queue, &frame);
        vs.drawn = Some(frame);
    }
    // Picking on a click (not a drag).
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let local = (pos - rect.min) * ppp;
        if let Some(mode) = state.pick.as_mut() {
            let (x, y) = (local.x as u32, local.y as u32);
            let hit = r.pick_masked(&rs.device, &rs.queue, &frame, x, y, pick_mask(mode));
            if let Some(e) = entity(scene.map(|s| &**s), hit).and_then(|s| mode.hit(&snap, &s)) {
                jobs.edit(e);
            }
        } else {
            let picked = r.pick(
                &rs.device,
                &rs.queue,
                &frame,
                local.x as u32,
                local.y as u32,
            );
            state.selection = match picked {
                Pick::Face(i) => scene.and_then(|s| s.faces.get(i as usize)).map_or(
                    Selection::Setup(setup.clone()),
                    |f| Selection::Face {
                        setup: setup.clone(),
                        face: f.clone(),
                    },
                ),
                Pick::Toolpath(i) => vs
                    .toolpaths
                    .get(i as usize)
                    .map_or(Selection::Setup(setup.clone()), |t| {
                        Selection::Toolpath(t.clone())
                    }),
                Pick::Edge(_) | Pick::None => Selection::Setup(setup.clone()),
            };
        }
    }
    if let Some(id) = vs.texture {
        painter.image(
            id,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    // Overlay: the work-zero triad at the origin of W.
    let px = |v: glam::Vec3| {
        vs.camera
            .project(v, glam::Vec2::new(size.x, size.y))
            .map(|q| rect.min + Vec2::new(q.x, q.y))
    };
    let len = scene.and_then(|s| s.bounds).map_or(10.0, |b| {
        ((b.max[0] - b.min[0]).max(b.max[1] - b.min[1]) * 0.12).max(1.0)
    });
    if let Some(o) = px(glam::Vec3::ZERO) {
        for (axis, col) in [
            (glam::Vec3::X, brand::RED),
            (glam::Vec3::Y, brand::GREEN),
            (glam::Vec3::Z, brand::BLUE),
        ] {
            if let Some(e) = px(axis * len) {
                painter.line_segment([o, e], Stroke::new(2.5, c(col)));
            }
        }
        painter.circle(o, 4.0, c(brand::PAPER), Stroke::new(1.5, c(brand::INK)));
    }
    // The tool at the playhead and the finding looked at.
    let tool = sim_frame
        .as_ref()
        .and_then(|f| f.tool)
        .filter(|_| stock.is_some());
    if let Some(q) = tool.and_then(|p| px(glam::Vec3::from(p))) {
        painter.circle(
            q,
            5.0,
            c(brand::CUTTER_ORANGE),
            Stroke::new(1.5, c(brand::INK)),
        );
    }
    if let Some(q) = state.focus.and_then(|p| px(glam::Vec3::from(p))) {
        painter.circle_stroke(q, 9.0, Stroke::new(2.5, c(brand::RED)));
    }
    // In pick mode: what is being picked, and how to leave.
    if let Some(mode) = &state.pick {
        let field = snap
            .project
            .toolpath(&mode.toolpath)
            .and_then(|(_, t)| Params::meta(t.kind(), &mode.path))
            .map_or_else(String::new, field_label);
        painter.text(
            rect.center_top() + Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_TOP,
            tr_args("pick-banner", &[("field", &field)]),
            egui::FontId::proportional(14.0),
            c(brand::CUTTER_ORANGE),
        );
    }
    // What is selected in the view, and the controls.
    let picked = match &state.selection {
        Selection::Face { face, .. } => Some(tr_args("viewport-picked-face", &[("id", face)])),
        Selection::Toolpath(t) => snap
            .project
            .toolpath(t)
            .map(|(_, tp)| tr_args("viewport-picked-toolpath", &[("name", &tp.name)])),
        _ => None,
    };
    if let Some(t) = picked {
        painter.text(
            rect.left_top() + Vec2::new(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            t,
            egui::FontId::proportional(13.0),
            c(brand::CUTTER_ORANGE),
        );
    }
    painter.text(
        rect.left_bottom() + Vec2::new(10.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        tr("viewport-help"),
        egui::FontId::proportional(12.0),
        c(p.weak_text),
    );
}
