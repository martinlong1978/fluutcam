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
//! pictures. Dialogs cannot run headless: the shots call the job methods the
//! File menu's dialogs call, and open menus by clicking them.
//!
//! The table [`SHOTS`] is the list of pictures. A shot runs in order on one
//! window, so a shot starts from what the one before left. Add a shot by
//! adding a row and a function; then run the tool and commit the images.
//!
//! The program prints the name of every file it writes. Optimising the PNGs
//! further is `cargo xtask screenshots`'s job.

use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use egui_kittest::{Harness, kittest::NodeT};
use fluut_app::FluutApp;
use fluut_app::jobs::{CalcTargets, PostRequest};
use fluut_app::ui::Tab;
use fluut_app::vm::Selection;
use fluut_model::ToolpathTypeId;
use fluut_model::project::{DisplayUnit, SetupKind};
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
        name: "file-menu",
        shows: "The File menu before a project exists",
        crop: Some((0, 0, 420, 330)),
        run: |r| r.open_menu(&["File"]),
    },
    Shot {
        name: "new-project",
        shows: "A new, empty project",
        crop: None,
        run: |r| {
            r.app()
                .jobs
                .new_project("my-part.fluut".into(), DisplayUnit::Mm);
            r.settle();
        },
    },
    Shot {
        name: "model-imported",
        shows: "A STEP file imported: the model is in the Browser and the viewport",
        crop: None,
        run: |r| {
            let step = r.samples.join("block/block.step");
            r.app().jobs.import_model(step);
            r.settle();
        },
    },
    Shot {
        name: "add-setup-menu",
        shows: "File, Add Setup: the three kinds of Setup",
        crop: Some((0, 0, 560, 330)),
        run: |r| r.open_menu(&["File", "Add Setup"]),
    },
    Shot {
        name: "setup-added",
        shows: "A 3-axis Setup added: the inspector lists its stock and work zero",
        crop: None,
        run: |r| {
            r.app().jobs.add_setup(SetupKind::ThreeAxis, None);
            r.settle();
        },
    },
    Shot {
        name: "add-toolpath-menu",
        shows: "File, Add toolpath: the toolpath types",
        crop: Some((0, 0, 560, 560)),
        run: |r| r.open_menu(&["File", "Add toolpath"]),
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
        shows: "G-code written: the status bar names the file",
        crop: None,
        run: |r| {
            r.app().show_tab(Tab::Messages);
            r.app().jobs.post(PostRequest {
                post: "fluut/linuxcnc".into(),
                output: "my-part.ngc".into(),
                date: DATE.into(),
                time: TIME.into(),
            });
            r.settle();
        },
    },
    Shot {
        name: "saved",
        shows: "The project saved: no asterisk after the file name",
        crop: Some((0, 0, 640, 60)),
        run: |r| {
            r.app().jobs.save();
            r.settle();
        },
    },
    Shot {
        name: "reference-project",
        shows: "A reference project calculated, with a toolpath selected and its Passes tab in the inspector",
        crop: None,
        run: |r| {
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
            r.app().jobs.import_model("broken.step".into());
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

    /// Opens nested menus by clicking their buttons, the first being a menu
    /// bar button (the ribbon has a label "File" too).
    fn open_menu(&mut self, path: &[&str]) {
        for (i, label) in path.iter().enumerate() {
            let node = if i == 0 {
                self.h
                    .get_all_by_label(label)
                    .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
                    .expect("a menu button")
            } else {
                // A submenu button's label ends in the arrow glyph.
                self.h
                    .get_all_by_label_contains(label)
                    .next()
                    .expect("a menu item")
            };
            node.click();
            self.h.run_steps(4);
        }
        // Take the pointer off the menu so no item is highlighted.
        self.h.input_mut().events.push(egui::Event::PointerGone);
        self.h.run_steps(2);
    }

    /// Opens a reference project from the work folder (a copy of it).
    fn open_sample(&mut self, name: &str) {
        let from = self.samples.join(name);
        copy_dir(&from, Path::new(name));
        let path: PathBuf = [name, &format!("{name}.fluut")].iter().collect();
        // The ribbon's text fields keep the first project's names otherwise.
        let shown = path.display().to_string().replace('\\', "/");
        self.app().jobs.open(path);
        self.settle();
        self.app().ui.open_path = shown.clone();
        self.app().ui.post_output = shown.replace(".fluut", ".ngc");
        self.h.run_steps(2);
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
        // Escape closes an open menu; it does nothing else here.
        rig.h.run_steps(2);
        let img = rig.h.render().expect("render the window");
        let path = out.join(format!("{}.png", s.name));
        save_png(&img, s.crop, &path);
        println!("{}", path.display());
        rig.h.key_press(Key::Escape);
        // No pointer in the next picture.
        rig.h.input_mut().events.push(egui::Event::PointerGone);
        rig.h.run_steps(3);
    }
    let _ = std::env::set_current_dir(&out);
    let _ = std::fs::remove_dir_all(&work);
}
