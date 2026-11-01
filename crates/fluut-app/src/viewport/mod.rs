// SPDX-License-Identifier: MIT OR Apache-2.0

//! The viewport (D13 §5): orbit camera and `wgpu` renderer. The widget that
//! shows it is in `ui::viewport`.

pub mod camera;
pub mod render;

pub use camera::Camera;
pub use render::{FrameState, Pick, Renderer};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::CalcTargets;
    use crate::session::tests::opened;

    /// Renders the boss project headless and picks a face and the background.
    /// Skips when the machine has no `wgpu` adapter.
    #[test]
    fn render_and_pick() {
        let Some((device, queue)) = render::headless() else {
            eprintln!("no wgpu adapter: render_and_pick skipped");
            return;
        };
        let mut j = opened("boss");
        j.calculate(CalcTargets::All);
        j.wait();
        let s = j.snapshot.clone().unwrap();
        let scene = &s.scenes["s1"];
        let mut r = Renderer::new(&device);
        r.set_scene(&device, &scene.mesh, &scene.edges, scene.stock.as_ref());
        r.set_toolpaths(&device, &[s.paths.get("t1").map(|p| &p.lines)]);
        let (w, h) = (320, 240);
        assert!(r.ensure_size(&device, w, h));
        assert!(!r.ensure_size(&device, w, h));
        // Frame the model only, so the middle of the view is a face.
        let mut b = crate::scene::Bounds::EMPTY;
        for p in &scene.mesh.positions {
            b.add(*p);
        }
        let mut camera = Camera::fit(Some(&b));
        camera.yaw = 0.3;
        camera.pitch = 1.2;
        let f = FrameState {
            camera,
            dark: true,
            face: None,
            toolpath: None,
        };
        r.render(&device, &queue, &f);
        let centre = r.pick(&device, &queue, &f, w / 2, h / 2);
        assert!(
            matches!(centre, Pick::Face(_) | Pick::Toolpath(0)),
            "{centre:?}"
        );
        assert_eq!(r.pick(&device, &queue, &f, 0, 0), Pick::None);
        assert_eq!(r.pick(&device, &queue, &f, w + 5, 0), Pick::None);
        // With the simulated stock shown the faces are hidden and not
        // pickable; the toolpath still is.
        j.simulate("s1");
        j.wait();
        while j.frame.is_none() {
            assert!(j.wait_event());
        }
        let mesh = j.frame.clone().unwrap().mesh.clone().unwrap();
        r.set_stock(&device, Some(&mesh));
        r.render(&device, &queue, &f);
        let centre = r.pick(&device, &queue, &f, w / 2, h / 2);
        assert!(!matches!(centre, Pick::Face(_)), "{centre:?}");
        r.set_stock(&device, None);
    }
}
