// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S4: the FluutCAM window in `egui` (S 08 §8.2).
//!
//! Title bar, ribbon with a toolpath gallery, a Setup/Toolpath browser, the
//! `wgpu` viewport with an overlay handle, the inspector for a `pocket_2d`
//! toolpath and a timeline strip. Browser, viewport and inspector are
//! `egui_dock` tabs: they can be rearranged, docked elsewhere or floated.
//!
//! `cargo run --release`; see `s4_common::bench` for the measurement modes.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use eframe::egui_wgpu;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use s4_common::bench::{self, Bench};
use s4_common::camera::Camera;
use s4_common::mock::{self, Direction, OffsetOrder, Pattern, Pocket2d, Status, Units};
use s4_common::render::{Preview, Viewport};
use s4_common::{brand, glam, mesh, wgpu};
use std::time::Instant;

fn c(rgb: brand::Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn main() -> eframe::Result {
    let t0 = Instant::now();
    let mesh = mesh::relief(bench::grid());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FluutCAM — egui prototype")
            .with_inner_size([1440.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "s4-egui",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, &mesh, t0)))),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Browser,
    Viewport,
    Inspector,
}

/// Selected item in the browser: (setup, toolpath).
type Sel = (usize, Option<usize>);

struct App {
    dock: DockState<Tab>,
    state: State,
}

/// Everything the tabs read and change.
struct State {
    stage: usize,
    units: Units,
    decimal_comma: bool,
    search: String,
    setups: Vec<mock::Setup>,
    selected: Sel,
    params: Pocket2d,
    tool: mock::Tool,
    preview: bool,
    camera: Camera,
    zero: glam::Vec3,
    zero_hover: bool,
    viewport: Viewport,
    texture: Option<egui::TextureId>,
    /// What the viewport texture shows now; it is drawn again only on change.
    drawn: Option<(Camera, Preview)>,
    bench: Bench,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, mesh: &mesh::Mesh, t0: Instant) -> Self {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .expect("eframe built with wgpu");
        style(&cc.egui_ctx);
        let mut dock = DockState::new(vec![Tab::Viewport]);
        let tree = dock.main_surface_mut();
        let [rest, _] = tree.split_left(NodeIndex::root(), 0.18, vec![Tab::Browser]);
        tree.split_right(rest, 0.75, vec![Tab::Inspector]);
        Self {
            dock,
            state: State {
                stage: 2,
                units: Units::Mm,
                decimal_comma: false,
                search: String::new(),
                setups: bench::project(),
                selected: (0, Some(1)),
                params: Pocket2d::default(),
                tool: mock::tool(),
                preview: true,
                camera: Camera::framing(mesh.min, mesh.max),
                zero: glam::Vec3::new(mesh.min.x, mesh.min.y, mesh.max.z),
                zero_hover: false,
                viewport: Viewport::new(&rs.device, mesh),
                texture: None,
                drawn: None,
                bench: Bench::from_env(t0),
            },
        }
    }
}

fn style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|s| {
        let v = &mut s.visuals;
        v.panel_fill = c(brand::PANEL);
        v.window_fill = c(brand::PANEL);
        v.extreme_bg_color = c(brand::INK);
        v.faint_bg_color = c(brand::RAISED);
        v.selection.bg_fill = c(brand::CUTTER_ORANGE).gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, c(brand::CUTTER_ORANGE));
        v.hyperlink_color = c(brand::CUTTER_ORANGE);
        v.widgets.noninteractive.fg_stroke.color = c(brand::PAPER);
        v.widgets.noninteractive.bg_stroke.color = c(brand::LINE);
        v.widgets.inactive.bg_fill = c(brand::RAISED);
        v.widgets.inactive.weak_bg_fill = c(brand::RAISED);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, c(brand::CUTTER_ORANGE));
        v.widgets.active.bg_fill = c(brand::CUTTER_ORANGE);
        v.widgets.active.weak_bg_fill = c(brand::CUTTER_ORANGE);
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let s = &mut self.state;
        if s.bench.frame(&mut s.camera) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if s.bench.continuous() {
            ui.ctx().request_repaint();
        }
        egui::Panel::top("title")
            .frame(egui::Frame::new().fill(c(brand::INK)).inner_margin(6.0))
            .show(ui, |ui| title_bar(ui, s));
        egui::Panel::top("ribbon")
            .frame(egui::Frame::new().fill(c(brand::RAISED)).inner_margin(6.0))
            .show(ui, |ui| ribbon(ui, s));
        egui::Panel::bottom("timeline")
            .frame(egui::Frame::new().fill(c(brand::INK)).inner_margin(6.0))
            .show(ui, |ui| timeline(ui, s));
        let rs = frame.wgpu_render_state().expect("wgpu").clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(c(brand::INK)))
            .show(ui, |ui| {
                DockArea::new(&mut self.dock)
                    .show_close_buttons(false)
                    .show_inside(ui, &mut Tabs { s, rs: &rs });
            });
    }
}

fn title_bar(ui: &mut egui::Ui, s: &mut State) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
        ui.painter()
            .circle_filled(r.center(), 6.0, c(brand::CUTTER_ORANGE));
        ui.label(
            egui::RichText::new("FluutCAM")
                .strong()
                .color(c(brand::PAPER)),
        );
        ui.label(egui::RichText::new("Relief.fluut").color(c(brand::STOCK_GREY)));
        ui.add_space(40.0);
        ui.add(
            egui::TextEdit::singleline(&mut s.search)
                .hint_text("Ctrl+K  Search commands…")
                .desired_width(320.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("⚙", |ui| {
                ui.checkbox(&mut s.decimal_comma, "Decimal comma");
            })
            .response
            .on_hover_text("Settings");
            let badge = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(s.units.suffix())
                            .strong()
                            .color(c(brand::INK)),
                    )
                    .fill(c(brand::STOCK_GREY)),
                )
                .on_hover_text("Project units: click to switch");
            if badge.clicked() {
                s.units = if s.units == Units::Mm {
                    Units::Inch
                } else {
                    Units::Mm
                };
            }
        });
    });
}

fn ribbon(ui: &mut egui::Ui, s: &mut State) {
    ui.horizontal(|ui| {
        for (i, name) in mock::STAGES.iter().enumerate() {
            let on = s.stage == i;
            let text = egui::RichText::new(name.to_uppercase())
                .size(12.0)
                .strong()
                .color(if on {
                    c(brand::CUTTER_ORANGE)
                } else {
                    c(brand::STOCK_GREY)
                });
            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                s.stage = i;
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for group in mock::ribbon(s.stage) {
            egui::Frame::new()
                .fill(c(brand::PANEL))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            for cmd in group.commands {
                                let size = if group.gallery {
                                    [72.0, 52.0]
                                } else {
                                    [64.0, 52.0]
                                };
                                let b = ui
                                    .add_sized(size, egui::Button::new(*cmd))
                                    .on_hover_text(format!("{} › {cmd}", group.name));
                                if group.gallery {
                                    // Placeholder pattern icon for the gallery tile.
                                    let r = Rect::from_center_size(
                                        b.rect.center_top() + Vec2::new(0.0, 12.0),
                                        Vec2::splat(12.0),
                                    );
                                    ui.painter().rect_stroke(
                                        r,
                                        2.0,
                                        Stroke::new(1.5, c(brand::CUTTER_ORANGE)),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                            }
                        });
                        ui.label(
                            egui::RichText::new(group.name)
                                .size(11.0)
                                .color(c(brand::STOCK_GREY)),
                        );
                    });
                });
        }
    });
}

fn timeline(ui: &mut egui::Ui, s: &State) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
    let p = ui.painter_at(rect);
    let total: usize = s.setups.iter().map(|st| st.toolpaths.len()).sum();
    let w = rect.width() / total.max(1) as f32;
    let mut x = rect.left();
    for (si, st) in s.setups.iter().enumerate() {
        for (ti, tp) in st.toolpaths.iter().enumerate() {
            let r = Rect::from_min_size(
                Pos2::new(x + 1.0, rect.top()),
                Vec2::new(w - 2.0, rect.height()),
            );
            let sel = s.selected == (si, Some(ti));
            p.rect_filled(
                r,
                4.0,
                if sel {
                    c(brand::CUTTER_ORANGE)
                } else {
                    c(brand::RAISED)
                },
            );
            p.text(
                r.left_center() + Vec2::new(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &tp.name,
                egui::FontId::proportional(11.0),
                c(brand::PAPER),
            );
            x += w;
        }
    }
}

struct Tabs<'a> {
    s: &'a mut State,
    rs: &'a egui_wgpu::RenderState,
}

impl TabViewer for Tabs<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Browser => "Browser",
            Tab::Viewport => "Viewport",
            Tab::Inspector => "Inspector",
        }
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Browser => browser(ui, self.s),
            Tab::Viewport => viewport(ui, self.s, self.rs),
            Tab::Inspector => inspector(ui, self.s),
        }
    }
}

fn status_colour(st: Status) -> Color32 {
    match st {
        Status::NotCalculated => c(brand::STOCK_GREY),
        Status::Calculating(_) | Status::Done => c(brand::GREEN),
        Status::Stale | Status::Warning => c(brand::AMBER),
        Status::Error => c(brand::RED),
    }
}

/// Status icon: a distinct shape per state, so colour is not the only signal.
fn status_icon(ui: &mut egui::Ui, st: Status) {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    resp.on_hover_text(st.label());
    let (p, o, col) = (ui.painter(), r.center(), status_colour(st));
    match st {
        Status::NotCalculated => {
            p.circle_stroke(o, 5.0, Stroke::new(1.5, col));
        }
        Status::Done => {
            p.circle_filled(o, 5.0, col);
        }
        Status::Stale => {
            p.circle_stroke(o, 5.0, Stroke::new(1.5, col));
            p.circle_filled(o, 2.5, col);
        }
        Status::Calculating(pct) => {
            p.circle_stroke(o, 5.0, Stroke::new(1.5, c(brand::LINE)));
            let a = f32::from(pct) / 100.0 * std::f32::consts::TAU;
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = a * i as f32 / 16.0 - std::f32::consts::FRAC_PI_2;
                    o + 5.0 * Vec2::new(t.cos(), t.sin())
                })
                .collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
        }
        Status::Warning | Status::Error => {
            let pts = vec![
                o + Vec2::new(0.0, -6.0),
                o + Vec2::new(6.0, 5.0),
                o + Vec2::new(-6.0, 5.0),
            ];
            if st == Status::Error {
                p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
            } else {
                p.add(egui::Shape::closed_line(pts, Stroke::new(1.5, col)));
            }
        }
    }
}

fn browser(ui: &mut egui::Ui, s: &mut State) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (si, setup) in s.setups.iter().enumerate() {
            egui::CollapsingHeader::new(format!("{}  ·  {}", setup.name, setup.kind))
                .id_salt(si)
                .default_open(true)
                .show(ui, |ui| {
                    for (ti, tp) in setup.toolpaths.iter().enumerate() {
                        ui.horizontal(|ui| {
                            status_icon(ui, tp.status);
                            let sel = s.selected == (si, Some(ti));
                            if ui.selectable_label(sel, &tp.name).clicked() {
                                s.selected = (si, Some(ti));
                            }
                        });
                    }
                });
        }
        egui::CollapsingHeader::new("Model").show(ui, |ui| {
            ui.label("Relief (1 body)");
        });
    });
}

fn viewport(ui: &mut egui::Ui, s: &mut State, rs: &egui_wgpu::RenderState) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "3-D viewport"));
    let size = rect.size();
    let zero_px = s.camera.project(s.zero, glam::Vec2::new(size.x, size.y));
    let handle = zero_px.map(|p| rect.min + Vec2::new(p.x, p.y));
    s.zero_hover = handle
        .zip(resp.hover_pos())
        .is_some_and(|(h, m)| h.distance(m) < 10.0);
    if resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta();
        s.camera.orbit(d.x, d.y);
    }
    if resp.dragged_by(egui::PointerButton::Middle)
        || resp.dragged_by(egui::PointerButton::Secondary)
    {
        let d = resp.drag_delta();
        s.camera.pan(d.x, d.y, size.y);
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            s.camera.zoom(scroll / 50.0);
        }
    }
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = ((size.x * ppp).round() as u32, (size.y * ppp).round() as u32);
    let vp = &mut s.viewport;
    let resized = vp.ensure_size(&rs.device, w, h);
    if resized || s.texture.is_none() {
        let view = vp.view().expect("sized");
        let mut r = rs.renderer.write();
        s.texture = Some(match s.texture {
            Some(id) => {
                r.update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => r.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear),
        });
    }
    let preview = Preview {
        enabled: s.preview && s.params.pattern == Pattern::Raster,
        stepover_mm: s.params.stepover_mm as f32,
        angle_deg: s.params.pass_angle_deg as f32,
    };
    if resized || s.drawn != Some((s.camera, preview)) {
        vp.render(&rs.device, &rs.queue, &s.camera, preview);
        s.drawn = Some((s.camera, preview));
    }
    let p = ui.painter_at(rect);
    if let Some(id) = s.texture {
        p.image(
            id,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    // Overlay: work-zero triad with a handle at its origin.
    if let Some(o) = handle {
        for (axis, col) in [
            (glam::Vec3::X, brand::RED),
            (glam::Vec3::Y, brand::GREEN),
            (glam::Vec3::Z, [0x4C, 0x8D, 0xF6]),
        ] {
            if let Some(e) = s
                .camera
                .project(s.zero + axis * 15.0, glam::Vec2::new(size.x, size.y))
            {
                p.line_segment(
                    [o, rect.min + Vec2::new(e.x, e.y)],
                    Stroke::new(2.5, c(col)),
                );
            }
        }
        let fill = if s.zero_hover {
            c(brand::CUTTER_ORANGE)
        } else {
            c(brand::PAPER)
        };
        p.circle(o, 6.0, fill, Stroke::new(1.5, c(brand::INK)));
    }
    p.text(
        rect.left_bottom() + Vec2::new(10.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        "Drag: orbit · right/middle drag: pan · wheel: zoom",
        egui::FontId::proportional(12.0),
        c(brand::STOCK_GREY),
    );
}

fn inspector(ui: &mut egui::Ui, s: &mut State) {
    let (si, ti) = s.selected;
    let Some(tp) = ti.and_then(|t| s.setups.get(si)?.toolpaths.get(t)) else {
        ui.label("Select a toolpath.");
        return;
    };
    ui.heading(egui::RichText::new(&tp.name).color(c(brand::PAPER)));
    ui.label(
        egui::RichText::new(format!("{} · {}", tp.kind, s.setups[si].name))
            .color(c(brand::STOCK_GREY)),
    );
    ui.separator();
    ui.horizontal(|ui| {
        for t in ["Tool", "Geometry", "Heights", "Passes", "Linking"] {
            let _ = ui.selectable_label(t == "Passes", t);
        }
    });
    ui.separator();
    ui.label(egui::RichText::new(format!("Tool: {}", s.tool.name)).color(c(brand::STOCK_GREY)));
    ui.add_space(6.0);
    let diags = s.params.validate(&s.tool);
    let units = s.units;
    let comma = s.decimal_comma;
    let tool = s.tool.clone();
    let p = &mut s.params;
    egui::Grid::new("passes").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label("Pattern");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut p.pattern, Pattern::Offset, "Offset");
            ui.selectable_value(&mut p.pattern, Pattern::Raster, "Raster");
        });
        ui.end_row();
        for (key, label) in [("stepover", "Stepover"), ("step_down", "Step down")] {
            ui.label(label).on_hover_text("Drag the value to scrub; type an expression such as tool.diameter*0.4 or 1/8in");
            let v = if key == "stepover" { &mut p.stepover_mm } else { &mut p.step_down_mm };
            let t = tool.clone();
            ui.add(
                egui::DragValue::new(v)
                    .speed(0.01 * units.scale())
                    .range(0.0..=1000.0)
                    .custom_formatter(move |mm, _| format!("{} {}", mock::format_length(mm, units, comma), units.suffix()))
                    .custom_parser(move |text| mock::parse_length(text, units, &t).ok()),
            );
            ui.end_row();
            for d in diags.iter().filter(|d| d.field == key) {
                ui.label("");
                let (kind, col) = if d.error { ("Error", brand::RED) } else { ("Warning", brand::AMBER) };
                ui.scope(|ui| {
                    ui.set_max_width(200.0);
                    ui.add(egui::Label::new(egui::RichText::new(format!("{kind}: {}", d.message)).color(c(col)).size(12.0)).wrap())
                        .on_hover_text(d.code);
                });
                ui.end_row();
            }
        }
        ui.label("Direction");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut p.direction, Direction::Climb, "Climb");
            ui.selectable_value(&mut p.direction, Direction::Conventional, "Conventional");
        });
        ui.end_row();
        if p.visible("offset_order") {
            ui.label("Offset order");
            egui::ComboBox::from_id_salt("order")
                .selected_text(match p.offset_order {
                    OffsetOrder::InsideOut => "Inside out",
                    OffsetOrder::OutsideIn => "Outside in",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p.offset_order, OffsetOrder::InsideOut, "Inside out");
                    ui.selectable_value(&mut p.offset_order, OffsetOrder::OutsideIn, "Outside in");
                });
            ui.end_row();
        }
        if p.visible("pass_angle_deg") {
            ui.label("Pass angle");
            ui.add(egui::DragValue::new(&mut p.pass_angle_deg).speed(0.5).suffix("°"));
            ui.end_row();
        }
    });
    ui.add_space(8.0);
    ui.checkbox(&mut s.preview, "Show pass preview on the model");
}
