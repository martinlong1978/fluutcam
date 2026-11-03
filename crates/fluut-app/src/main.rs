// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluutcam [project.fluut]`: the FluutCAM desktop application.
//!
//! `FLUUT_BENCH=<frames>` and `FLUUT_STARTUP=1` measure frame time and
//! startup (see `fluut_app::bench`).

use eframe::egui;
use fluut_app::updates::Updates;
use fluut_app::{FluutApp, brand, text};
use std::path::PathBuf;
use std::time::Instant;

fn main() -> eframe::Result {
    let t0 = Instant::now();
    crash_reports();
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
            let wake = cc.egui_ctx.clone();
            let updates = Updates::load(move || wake.request_repaint());
            let app = FluutApp::new(&cc.egui_ctx, cc.wgpu_render_state.clone(), open, t0);
            Ok(Box::new(app.with_updates(updates)))
        }),
    )
}

/// Installs the crash-report hook and notes reports from an earlier crash
/// on stderr (D14 §6). Nothing is sent anywhere. A dialog that offers the
/// report in the window waits for the translated texts.
fn crash_reports() {
    let Some(dir) = fluut_core::crash::install("fluutcam") else {
        return;
    };
    let _ = fluut_core::crash::offer_to_show(
        &dir,
        false,
        &mut std::io::empty(),
        &mut std::io::stderr(),
    );
}
