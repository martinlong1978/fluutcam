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
            stock_colour: Default::default(),
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
        let blocks = j.frame.clone().unwrap().blocks.clone().unwrap();
        assert_eq!(r.set_stock(&device, Some(&blocks)), blocks.len());
        // The same blocks again: nothing to upload.
        assert_eq!(r.set_stock(&device, Some(&blocks)), 0);
        r.render(&device, &queue, &f);
        let centre = r.pick(&device, &queue, &f, w / 2, h / 2);
        assert!(!matches!(centre, Pick::Face(_)), "{centre:?}");
        assert_eq!(r.set_stock(&device, None), 1);
    }

    /// Renders the simulated stock of `plate2d` from above, plain and by
    /// rest class. After the first toolpath the uncut stock shows blue
    /// (above tolerance); at the end the score line and chamfer, which the
    /// flat model does not have, show vermillion (gouge). Plain stock has
    /// neither. Skips without a `wgpu` adapter.
    #[test]
    fn stock_coloured_by_rest_material() {
        use crate::vm::playback::StockColour;
        let Some((device, queue)) = render::headless() else {
            eprintln!("no wgpu adapter: stock_coloured_by_rest_material skipped");
            return;
        };
        let mut j = opened("plate2d");
        j.simulate("s1");
        j.wait();
        while j.frame.is_none() {
            assert!(j.wait_event());
        }
        let s = j.snapshot.clone().unwrap();
        let scene = &s.scenes["s1"];
        let end = j.frame.clone().unwrap().blocks.clone().unwrap();
        j.scrub("s1", s.sims["s1"].clips[0].end);
        while j.scrubbing > 0 {
            assert!(j.wait_event());
        }
        let first = j.frame.clone().unwrap().blocks.clone().unwrap();
        let mut r = Renderer::new(&device);
        r.set_scene(&device, &scene.mesh, &scene.edges, None);
        let (w, h) = (320, 240);
        r.ensure_size(&device, w, h);
        let mut camera = Camera::fit(scene.bounds.as_ref());
        camera.pitch = 1.5;
        let count = |r: &mut Renderer, colour| {
            let f = FrameState {
                camera,
                dark: true,
                face: None,
                toolpath: None,
                stock_colour: colour,
            };
            r.render(&device, &queue, &f);
            let px = r.read_pixels(&device, &queue).unwrap();
            let blue = px
                .iter()
                .filter(|p| i32::from(p[2]) > i32::from(p[0]) + 60 && p[2] > p[1])
                .count();
            let red = px
                .iter()
                .filter(|p| i32::from(p[0]) > i32::from(p[2]) + 60 && p[0] > p[1])
                .count();
            (blue, red)
        };
        r.set_stock(&device, Some(&first));
        let (blue, _) = count(&mut r, StockColour::Rest);
        assert!(blue > 500, "after the first toolpath: {blue} blue");
        r.set_stock(&device, Some(&end));
        let (_, red) = count(&mut r, StockColour::Rest);
        assert!(red > 20, "at the end: {red} red");
        let (blue, red) = count(&mut r, StockColour::Plain);
        assert!(blue == 0 && red == 0, "plain: {blue} blue, {red} red");
    }
}
