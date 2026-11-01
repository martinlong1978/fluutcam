// SPDX-License-Identifier: MIT OR Apache-2.0

//! The viewport renderer alone, without a toolkit or a window: render
//! `S4_BENCH` frames (default 200) of the `S4_GRID` mesh into a 1280 × 800
//! texture on a headless adapter, waiting for each frame to finish, and print
//! the frame times. `S4_PREVIEW=0` turns the draped pass preview off. With
//! `S4_PNG=<path>` it also writes the last frame (binary PPM).
//!
//! `cargo run --release --example viewport_bench`

use s4_common::camera::Camera;
use s4_common::render::{Preview, Viewport};
use s4_common::{bench, mesh, wgpu};
use std::time::Instant;

fn main() {
    let (w, h) = (1280, 800);
    let t = Instant::now();
    let mesh = mesh::relief(bench::grid());
    println!(
        "mesh: {} triangles in {:.0} ms",
        mesh.triangles(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("adapter");
    let info = adapter.get_info();
    println!(
        "adapter: {} ({:?}, {:?})",
        info.name, info.backend, info.device_type
    );
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let mut vp = Viewport::new(&device, &mesh);
    vp.ensure_size(&device, w, h);
    let mut cam = Camera::framing(mesh.min, mesh.max);
    let preview = Preview {
        enabled: std::env::var("S4_PREVIEW").map_or(true, |v| v != "0"),
        stepover_mm: 2.4,
        angle_deg: 30.0,
    };
    let n: usize = std::env::var("S4_BENCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let mut samples = Vec::with_capacity(n);
    for i in 0..n + 10 {
        let t = Instant::now();
        cam.orbit(6.0, 0.0);
        vp.render(&device, &queue, &cam, preview);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        if i >= 10 {
            samples.push(t.elapsed());
        }
    }
    println!("{}", bench::summary(&mut samples));
    if let Ok(path) = std::env::var("S4_PNG") {
        write_ppm(&device, &queue, &vp, w, h, &path);
    }
}

/// Read the texture back and write it as a binary PPM (no image crate needed).
fn write_ppm(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vp: &Viewport,
    w: u32,
    h: u32,
    path: &str,
) {
    let row = (w * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        vp.texture().expect("texture").as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    buf.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = buf.slice(..).get_mapped_range().expect("mapped");
    let mut out = format!("P6 {w} {h} 255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            let i = (y * row + x * 4) as usize;
            out.extend_from_slice(&data[i..i + 3]);
        }
    }
    std::fs::write(path, out).expect("write");
    println!("wrote {path}");
}
