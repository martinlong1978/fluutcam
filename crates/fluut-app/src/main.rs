// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluutcam [project.fluut]`: the FluutCAM desktop application.
//!
//! `FLUUT_BENCH=<frames>` and `FLUUT_STARTUP=1` measure frame time and
//! startup (see `fluut_app::bench`).

use eframe::egui;
use fluut_app::{FluutApp, brand, text};
use std::path::PathBuf;
use std::time::Instant;

fn main() -> eframe::Result {
    let t0 = Instant::now();
    let open = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(text::tr("app-title"))
            .with_app_id("fluutcam")
            .with_inner_size([1440.0, 900.0])
            .with_icon(brand::icon_data()),
        ..Default::default()
    };
    eframe::run_native(
        "fluutcam",
        options,
        Box::new(move |cc| {
            Ok(Box::new(FluutApp::new(
                &cc.egui_ctx,
                cc.wgpu_render_state.clone(),
                open,
                t0,
            )))
        }),
    )
}
