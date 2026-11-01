// SPDX-License-Identifier: MIT OR Apache-2.0

//! Orbit camera: yaw and pitch about a target, Z up (CAM convention).

use glam::{Mat4, Vec2, Vec3, Vec4Swizzles};

/// Radians of orbit per pixel of drag.
const ORBIT_PER_PX: f32 = 0.008;
/// Pitch limit, short of straight up or down.
const PITCH_LIMIT: f32 = 1.55;

/// Orbit camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    /// Angle about Z, radians.
    pub yaw: f32,
    /// Angle above the XY plane, radians.
    pub pitch: f32,
    /// Distance from target to eye, mm.
    pub distance: f32,
    /// Vertical field of view, radians.
    pub fov_y: f32,
}

impl Camera {
    /// A camera that frames the box `min..max` from above at an angle.
    #[must_use]
    pub fn framing(min: Vec3, max: Vec3) -> Self {
        let radius = (max - min).length() * 0.5;
        let fov_y = 35f32.to_radians();
        Self {
            target: (min + max) * 0.5,
            yaw: -0.6,
            pitch: 0.75,
            distance: radius / (fov_y * 0.5).sin() * 0.85,
            fov_y,
        }
    }

    /// Eye position.
    #[must_use]
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + self.distance * Vec3::new(cp * cy, cp * sy, sp)
    }

    fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Z)
    }

    fn proj(&self, aspect: f32) -> Mat4 {
        let near = (self.distance * 0.01).max(0.01);
        let far = self.distance * 10.0;
        Mat4::perspective_rh(self.fov_y, aspect.max(1e-3), near, far)
    }

    /// Combined view and projection for `wgpu` clip space (depth 0..1).
    #[must_use]
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    /// Orbit by a drag of `dx`, `dy` pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ORBIT_PER_PX;
        self.pitch = (self.pitch + dy * ORBIT_PER_PX).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Pan by a drag of `dx`, `dy` pixels in a viewport `height` pixels tall:
    /// the point under the cursor at the target's depth follows the cursor.
    pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
        let mm_per_px = 2.0 * self.distance * (self.fov_y * 0.5).tan() / height.max(1.0);
        let view = self.view();
        let right = view.row(0).xyz();
        let up = view.row(1).xyz();
        self.target += (-dx * right + dy * up) * mm_per_px;
    }

    /// Zoom by `steps` (positive = closer); each step is 10 %.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.9f32.powf(steps)).clamp(1.0, 1e5);
    }

    /// Pixel position of a model point in a `size` viewport, origin top left.
    /// `None` when the point is behind the eye.
    #[must_use]
    pub fn project(&self, p: Vec3, size: Vec2) -> Option<Vec2> {
        let clip = self.view_proj(size.x / size.y) * p.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.xy() / clip.w;
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * size.x,
            (1.0 - ndc.y) * 0.5 * size.y,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::framing(Vec3::splat(-10.0), Vec3::splat(10.0))
    }

    #[test]
    fn orbit_keeps_distance_and_target() {
        let mut c = cam();
        let d = (c.eye() - c.target).length();
        c.orbit(120.0, -40.0);
        assert!(((c.eye() - c.target).length() - d).abs() < 1e-3);
        assert_eq!(c.target, cam().target);
    }

    #[test]
    fn pitch_is_clamped() {
        let mut c = cam();
        c.orbit(0.0, 1e6);
        assert!(c.pitch <= PITCH_LIMIT);
    }

    #[test]
    fn pan_moves_target_with_cursor() {
        let mut c = cam();
        let size = Vec2::new(800.0, 600.0);
        let before = c.project(Vec3::ZERO, size).unwrap();
        c.pan(50.0, 20.0, size.y);
        let after = c.project(Vec3::ZERO, size).unwrap();
        // The scene follows the drag: the origin moves by about the drag.
        assert!(
            (after - before - Vec2::new(50.0, 20.0)).length() < 1.0,
            "{before} {after}"
        );
    }

    #[test]
    fn zoom_moves_closer() {
        let mut c = cam();
        let d = c.distance;
        c.zoom(1.0);
        assert!(c.distance < d);
    }

    #[test]
    fn target_projects_to_centre() {
        let c = cam();
        let p = c.project(c.target, Vec2::new(640.0, 480.0)).unwrap();
        assert!((p - Vec2::new(320.0, 240.0)).length() < 1e-3);
    }
}
