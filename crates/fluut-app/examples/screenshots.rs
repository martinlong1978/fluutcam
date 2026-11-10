// SPDX-License-Identifier: MIT OR Apache-2.0

//! The screenshots of the user documentation (the wiki in `docs/wiki/`),
//! rendered headless from the reference projects.
//!
//! ```text
//! cargo xtask screenshots              # or:
//! cargo run -p fluut-app --example screenshots -- [<out-dir>] [--list]
//! ```
//!
//! The real window runs under `egui_kittest` with its `wgpu` snapshot
//! renderer, so the 3-D viewport is in the picture. The size, the theme (dark
//! unless a shot switches it), the clock and the work folder are fixed, and
//! every path on screen is relative, so the same build gives the same
//! pictures. Native file dialogs cannot run headless: the window gets scripted
//! dialogs that answer the path a shot gives, and the shots click the
//! ribbon's buttons as a user would. Where a button needs a face or curve
//! picked (not in the window yet) the shot calls the job method.
//!
//! The table [`SHOTS`] is the list of pictures. A shot runs in order on one
//! window, so a shot starts from what the one before left. Add a shot by
//! adding a row and a function; then run the tool and commit the images.
//!
//! The program prints the name of every file it writes. Optimising the PNGs
//! further is `cargo xtask screenshots`'s job.

use eframe::egui::{self, Key};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use fluut_app::FluutApp;
use fluut_app::dialogs::{DialogKind, Dialogs};
use fluut_app::jobs::CalcTargets;
use fluut_app::ui::Tab;
use fluut_app::vm::Selection;
use fluut_model::ToolpathTypeId;
use fluut_model::project::DisplayUnit;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Window size in points; the pixel size is the same (scale 1).
const SIZE: (f32, f32) = (1280.0, 880.0);

type State = Option<FluutApp>;

struct Rig {
    h: Harness<'static, State>,
    /// The reference projects.
    samples: PathBuf,
}

/// One picture: its file name (without `.png`), what it shows, the part of
/// the window to keep (`x, y, width, height` in pixels; `None` keeps the
/// whole window) and the steps that bring the window to that state.
struct Shot {
    name: &'static str,
    shows: &'static str,
    crop: Option<(u32, u32, u32, u32)>,
    run: fn(&mut Rig),
}

/// The list of pictures, in the order they are taken.
const SHOTS: &[Shot] = &[
    Shot {
        name: "start",
        shows: "The window with no project open",
        crop: None,
        run: |_| {},
    },
    Shot {
        name: "ribbon-model",
        shows: "The Model stage of the ribbon before a project exists: only New project and Open are enabled",
        crop: Some((0, 0, 1280, 215)),
        run: |_| {},
    },
    Shot {
        name: "unit-prompt",
        shows: "New project asks for the unit after the file dialog",
        crop: Some((440, 380, 400, 130)),
        run: |r| {
            r.ask(DialogKind::New(DisplayUnit::Mm), "my-part");
            r.click("New project");
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "new-project",
        shows: "A new, empty project",
        crop: None,
        run: |r| {
            r.click("Millimetres");
            r.settle();
        },
    },
    Shot {
        name: "model-imported",
        shows: "A STEP file imported: the model is in the Browser and the viewport",
        crop: None,
        run: |r| {
            let step = r.samples.join("block/block.step");
            r.ask(DialogKind::Import, step);
            r.click("Import model");
            r.settle();
        },
    },
    Shot {
        name: "ribbon-setup",
        shows: "The Setup stage of the ribbon: the three kinds of Setup, based-on, stock and work zero",
        crop: Some((0, 0, 1280, 215)),
        run: |r| r.click("SETUP"),
    },
    Shot {
        name: "setup-added",
        shows: "A 3-axis Setup added: the inspector lists its stock and work zero",
        crop: None,
        run: |r| {
            r.click("New 3-axis Setup");
            r.settle();
        },
    },
    Shot {
        name: "setup-handles",
        shows: "The handles of a selected Setup on the model: the work zero (a ring) and the block margins (orange squares)",
        crop: Some((265, 220, 720, 440)),
        run: |r| {
            r.app().ui.selection = Selection::Setup("s1".into());
            r.h.run_steps(4);
        },
    },
    Shot {
        name: "setup-bodies-orientation",
        shows: "The Bodies and Orientation groups of the Setup inspector: a check box for each body, and a list and Pick for each direction",
        crop: Some((988, 190, 284, 650)),
        run: |r| r.h.run_steps(2),
    },
    Shot {
        name: "ribbon-toolpaths",
        shows: "The Toolpaths stage of the ribbon: the galleries of toolpath types and the calculate buttons",
        crop: Some((0, 0, 1280, 215)),
        run: |r| r.click("TOOLPATHS"),
    },
    Shot {
        name: "toolpath-added",
        shows: "A 3-D toolpath added, not calculated yet",
        crop: None,
        run: |r| {
            r.app().jobs.add_toolpath("s1", ToolpathTypeId::Parallel);
            r.settle();
        },
    },
    Shot {
        name: "calculated",
        shows: "The toolpath calculated and drawn on the model",
        crop: None,
        run: |r| {
            r.app().jobs.calculate(CalcTargets::All);
            r.settle();
        },
    },
    Shot {
        name: "simulated",
        shows: "The Simulation tab: the stock after the cut, with the playback controls",
        crop: None,
        run: |r| {
            r.app().jobs.simulate("s1");
            r.settle();
            r.app().show_tab(Tab::Simulation);
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "posted",
        shows: "The Post stage with the post list, and the status bar naming the G-code file written",
        crop: None,
        run: |r| {
            r.app().show_tab(Tab::Messages);
            r.click("POST");
            r.ask(DialogKind::PostOutput, "my-part.ngc");
            r.click("Post");
            r.settle();
        },
    },
    Shot {
        name: "saved",
        shows: "The project saved: no asterisk after the file name",
        crop: Some((0, 0, 640, 60)),
        run: |r| {
            r.click("MODEL");
            r.click("Save");
            r.settle();
        },
    },
    Shot {
        name: "setup-inspector",
        shows: "The inspector of a Setup: name, material, machine, work offset, stock, work zero and clearance, all editable",
        crop: None,
        run: |r| {
            r.app().ui.selection = Selection::Setup("s1".into());
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "toolpath-tool",
        shows: "A 2-D pocket on the Tool tab: the tool dropdown, with the starter tools listed",
        crop: None,
        run: |r| {
            r.open_sample("plate2d");
            r.app().jobs.add_toolpath("s1", ToolpathTypeId::Pocket2d);
            r.settle();
            r.app().ui.inspector_tab = 0;
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "toolpath-heights",
        shows: "The Heights tab: each height is a reference and an offset",
        crop: None,
        run: |r| {
            r.app().ui.inspector_tab = 2;
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "pick-mode",
        shows: "Pick mode on the model: the banner, the picked faces tinted, and the selection list in the inspector",
        crop: None,
        run: |r| {
            r.app().ui.inspector_tab = 1;
            r.h.run_steps(3);
            r.h.get_all_by_label("Pick")
                .next()
                .expect("a Pick button")
                .click();
            r.h.run_steps(3);
            // Picks go in by id, as a click on the model would.
            for face in ["f-588.14", "f-588.0"] {
                let snap = r.app().jobs.snapshot.clone().expect("a project is open");
                let sel = fluut_model::Sel::Face(face.into());
                let edit = r
                    .app()
                    .ui
                    .pick
                    .as_mut()
                    .expect("pick mode")
                    .hit(&snap, &sel)
                    .expect("the pocket takes this face");
                r.app().jobs.edit(edit);
                r.settle();
            }
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "reference-project",
        shows: "A reference project calculated, with a toolpath selected and its Passes tab in the inspector",
        crop: None,
        run: |r| {
            // The pocket project has changes: save them, so Open does not ask.
            r.click("MODEL");
            r.click("Save");
            r.settle();
            r.open_sample("block");
            r.app().jobs.calculate(CalcTargets::All);
            r.settle();
            r.app().ui.selection = Selection::Toolpath("t2".into());
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "messages",
        shows: "The Messages tab: warnings from a calculation",
        crop: None,
        run: |r| {
            r.app().ui.selection = Selection::Toolpath("t3".into());
            r.app().show_tab(Tab::Messages);
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "import-failed",
        shows: "An import that fails: the status bar says why and the project is unchanged",
        crop: None,
        run: |r| {
            std::fs::write("broken.step", "this is not a STEP file\n").expect("write broken.step");
            r.ask(DialogKind::Import, "broken.step");
            r.click("Import model");
            r.settle();
        },
    },
    Shot {
        name: "rotary-project",
        shows: "A rotary Setup: a shaft turned about its axis, with a rotary finishing toolpath",
        crop: None,
        run: |r| {
            r.open_sample("shaft");
            r.app().jobs.calculate(CalcTargets::All);
            r.settle();
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "settings",
        shows: "Settings: the update check, which is off until you turn it on",
        crop: Some((400, 355, 480, 170)),
        run: |r| {
            r.app().ui.show_settings = true;
            r.h.run_steps(3);
        },
    },
    Shot {
        name: "light-theme",
        shows: "The light theme",
        crop: None,
        run: |r| {
            r.app().ui.show_settings = false;
            r.click("Light theme");
            r.h.run_steps(3);
        },
    },
];

const DATE: &str = "2026-11-04";
const TIME: &str = "12:00:00";

fn clock() -> (String, String) {
    (DATE.into(), TIME.into())
}

impl Rig {
    fn app(&mut self) -> &mut FluutApp {
        self.h
            .state_mut()
            .as_mut()
            .expect("built on the first frame")
    }

    /// Steps the window until no job runs; the worker has its own thread.
    fn settle(&mut self) {
        for _ in 0..4000 {
            self.h.step();
            if !self.app().jobs.is_busy() {
                self.h.run_steps(3);
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("a job did not finish");
    }

    fn click(&mut self, label: &str) {
        self.h.get_by_label(label).click();
        self.h.run_steps(3);
    }

    /// The next dialog of `kind` answers `path`.
    fn ask(&mut self, kind: DialogKind, path: impl Into<PathBuf>) {
        let a = self.app();
        if !a.dialogs.is_scripted() {
            a.dialogs = Dialogs::scripted();
        }
        a.dialogs.answer(kind, path);
    }

    /// Opens a reference project from the work folder (a copy of it).
    fn open_sample(&mut self, name: &str) {
        let from = self.samples.join(name);
        copy_dir(&from, Path::new(name));
        let path: PathBuf = [name, &format!("{name}.fluut")].iter().collect();
        self.click("MODEL");
        self.ask(DialogKind::Open, path);
        self.click("Open");
        self.settle();
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create folder");
    for e in std::fs::read_dir(from).expect("read folder") {
        let e = e.expect("entry");
        let dest = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).expect("copy file");
        }
    }
}

fn save_png(img: &RgbaImage, crop: Option<(u32, u32, u32, u32)>, path: &Path) {
    let view;
    let img = match crop {
        Some((x, y, w, h)) => {
            view = image::imageops::crop_imm(
                img,
                x,
                y,
                w.min(img.width() - x),
                h.min(img.height() - y),
            )
            .to_image();
            &view
        }
        None => img,
    };
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Best, FilterType::Adaptive)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .expect("encode png");
    std::fs::write(path, bytes).expect("write png");
}

fn main() {
    let mut out: Option<PathBuf> = None;
    for a in std::env::args().skip(1) {
        if a == "--list" {
            println!("| File | Shows |\n|---|---|");
            for s in SHOTS {
                println!("| `{}.png` | {} |", s.name, s.shows);
            }
            return;
        }
        out = Some(PathBuf::from(a));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::path::absolute(out.unwrap_or_else(|| root.join("docs/wiki/images")))
        .expect("absolute out dir");
    std::fs::create_dir_all(&out).expect("create out dir");
    let samples = std::path::absolute(root.join("crates/fluut-cli/tests/projects"))
        .expect("absolute samples dir");

    // A fresh work folder: every path on screen is relative to it.
    let work = std::env::temp_dir().join(format!("fluut-screenshots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("create work dir");
    std::env::set_current_dir(&work).expect("enter work dir");

    // The primary backends only: the OpenGL backend overflows its thread's
    // stack in debug builds on Windows. The adapter choice prefers a
    // software rasteriser, which keeps the pictures the same everywhere.
    let mut setup = default_wgpu_setup();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(c) = &mut setup {
        c.instance_descriptor.backends = eframe::egui_wgpu::wgpu::Backends::PRIMARY;
    }
    let render = create_render_state(setup, eframe::egui_wgpu::RendererOptions::PREDICTABLE);
    let shared = render.clone();
    let h = Harness::builder()
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .with_pixels_per_point(1.0)
        .with_theme(egui::Theme::Dark)
        .renderer(WgpuTestRenderer::from_render_state(render))
        .build_ui_state(
            move |ui, app: &mut State| {
                app.get_or_insert_with(|| {
                    let mut a = FluutApp::new(ui.ctx(), Some(shared.clone()), None, Instant::now());
                    a.ui.clock = clock;
                    a
                })
                .frame(ui);
            },
            None,
        );
    let mut rig = Rig { h, samples };
    rig.h.step();
    for s in SHOTS {
        (s.run)(&mut rig);
        // No pointer in the picture.
        rig.h.input_mut().events.push(egui::Event::PointerGone);
        rig.h.run_steps(2);
        let img = rig.h.render().expect("render the window");
        let path = out.join(format!("{}.png", s.name));
        save_png(&img, s.crop, &path);
        println!("{}", path.display());
        rig.h.key_press(Key::Escape);
        rig.h.run_steps(3);
    }
    let _ = std::env::set_current_dir(&out);
    let _ = std::fs::remove_dir_all(&work);
}
