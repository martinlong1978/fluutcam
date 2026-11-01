// SPDX-License-Identifier: MIT OR Apache-2.0

//! A generated relief mesh, standing in for a display tessellation (D03 §7).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// One vertex: position and unit normal, model frame, mm.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

/// An indexed triangle mesh.
#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub min: Vec3,
    pub max: Vec3,
}

impl Mesh {
    /// Number of triangles.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Grid size that gives about one million triangles.
pub const GRID_1M: usize = 708;

/// Side of the relief in mm.
const SIZE: f32 = 120.0;

fn height(x: f32, y: f32) -> f32 {
    // A smooth relief: two domes, a ridge and fine ripples, 0 to about 30 mm.
    let d1 = ((x - 25.0).powi(2) + (y - 15.0).powi(2)) / 600.0;
    let d2 = ((x + 30.0).powi(2) + (y + 20.0).powi(2)) / 900.0;
    let ridge = (-(x + 0.6 * y).powi(2) / 300.0).exp();
    let ripple = 0.6 * (x * 0.35).sin() * (y * 0.3).cos();
    22.0 * (-d1).exp() + 15.0 * (-d2).exp() + 8.0 * ridge + ripple
}

fn gradient(x: f32, y: f32) -> (f32, f32) {
    let h = 1e-2;
    let dx = (height(x + h, y) - height(x - h, y)) / (2.0 * h);
    let dy = (height(x, y + h) - height(x, y - h)) / (2.0 * h);
    (dx, dy)
}

/// A relief on an `n × n` vertex grid, `2 (n − 1)²` triangles, centred on the
/// origin in X and Y with Z up.
///
/// # Panics
///
/// If `n < 2` or the vertex count does not fit in `u32`.
#[must_use]
pub fn relief(n: usize) -> Mesh {
    assert!(n >= 2, "grid needs at least 2 × 2 vertices");
    assert!(
        u32::try_from(n * n).is_ok(),
        "grid too large for u32 indices"
    );
    let step = SIZE / (n - 1) as f32;
    let half = SIZE / 2.0;
    let mut vertices = Vec::with_capacity(n * n);
    let (mut zmin, mut zmax) = (f32::MAX, f32::MIN);
    for j in 0..n {
        let y = j as f32 * step - half;
        for i in 0..n {
            let x = i as f32 * step - half;
            let z = height(x, y);
            zmin = zmin.min(z);
            zmax = zmax.max(z);
            let (dx, dy) = gradient(x, y);
            let normal = Vec3::new(-dx, -dy, 1.0).normalize();
            vertices.push(Vertex {
                pos: [x, y, z],
                normal: normal.into(),
            });
        }
    }
    let mut indices = Vec::with_capacity(6 * (n - 1) * (n - 1));
    let at = |i: usize, j: usize| (j * n + i) as u32;
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    Mesh {
        vertices,
        indices,
        min: Vec3::new(-half, -half, zmin),
        max: Vec3::new(half, half, zmax),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_count() {
        assert_eq!(relief(3).triangles(), 8);
        let m = relief(GRID_1M);
        assert_eq!(m.triangles(), 2 * 707 * 707);
        assert!(m.triangles() > 990_000 && m.triangles() < 1_010_000);
    }

    #[test]
    fn normals_are_unit_and_up() {
        for v in &relief(50).vertices {
            let n = Vec3::from(v.normal);
            assert!((n.length() - 1.0).abs() < 1e-5);
            assert!(n.z > 0.0);
        }
    }

    #[test]
    fn indices_in_range_and_ccw_from_above() {
        let m = relief(10);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
        for t in m.indices.chunks(3) {
            let p = |k: usize| Vec3::from(m.vertices[t[k] as usize].pos);
            let n = (p(1) - p(0)).cross(p(2) - p(0));
            assert!(n.z > 0.0);
        }
    }
}
