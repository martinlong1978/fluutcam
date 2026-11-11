// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `wgpu` viewport renderer (D13 §5), grown from the S4 spike's. It
//! draws into its own texture, which the widget layer shows as an image.
//!
//! Layers: gradient background, faces (two-sided shading, the picked face
//! tinted), lines for edges, wires, the stock box and toolpaths (colour by
//! motion, S 08 §8.9). Picking renders ids into an `R32Uint` target for a
//! small square under the cursor only (§5.2).
//!
//! The simulated stock (D11 §4) is drawn instead of the faces, one indexed
//! buffer pair per mesh block; only blocks whose `Arc` changed are uploaded.
//! Each stock vertex carries its tag, and the shader colours it by rest
//! class or by toolpath as the frame asks, so the colouring changes
//! without an upload.

use super::camera::Camera;
use crate::brand::{self, Rgb};
use crate::scene::{Bounds, Lines, Mesh};
use crate::vm::playback::StockColour;
use bytemuck::{Pod, Zeroable};
use fluut_core::fluut_sim::StockMesh;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Colour format of the output texture. The shader writes sRGB-encoded values.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const PICK: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// Multisample count (S 08 §8.9: anti-aliased viewport).
pub const SAMPLES: u32 = 4;
/// Side of the square read back for a pick, pixels. Lines are thin, so a
/// pick takes the hit nearest the cursor within it.
pub const PICK_SIZE: u32 = 9;

/// Line tags: motion classes 0–3 (`scene::MotionClass`), then these.
pub const TAG_EDGE: u32 = 4;
pub const TAG_WIRE: u32 = 5;
pub const TAG_STOCK: u32 = 6;
pub const TAG_FIXTURE: u32 = 7;

/// What a pick found.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    None,
    /// Face by pick index (`SetupScene::faces`).
    Face(u32),
    /// Toolpath by its index in the drawn list.
    Toolpath(u32),
    /// Edge or wire curve by pick index (`SetupScene::edge_ids`).
    Edge(u32),
}

/// What a pick looks for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PickMask {
    pub faces: bool,
    pub edges: bool,
    pub toolpaths: bool,
}

impl PickMask {
    /// A click in the view: faces and toolpaths.
    pub const VIEW: PickMask = PickMask {
        faces: true,
        edges: false,
        toolpaths: true,
    };
}

/// Entities tinted over the scene, by pick index.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Tints {
    pub faces: Vec<u32>,
    pub edges: Vec<u32>,
    pub hover_face: Option<u32>,
    pub hover_edge: Option<u32>,
}

/// Marks stock vertices, whose face field holds their tag
/// (`fluut_sim::mesh::tag`); never a picked face.
const STOCK_BIT: u32 = 0x8000_0000;

const FACE_LAYER: u32 = 0x1000_0000;
const TOOLPATH_LAYER: u32 = 0x2000_0000;
const EDGE_LAYER: u32 = 0x3000_0000;
const INDEX_MASK: u32 = 0x0FFF_FFFF;

impl Pick {
    /// The id written to the pick target; 0 is nothing.
    pub fn encode(self) -> u32 {
        match self {
            Pick::None => 0,
            Pick::Face(i) => FACE_LAYER | (i & INDEX_MASK),
            Pick::Toolpath(i) => TOOLPATH_LAYER | (i & INDEX_MASK),
            Pick::Edge(i) => EDGE_LAYER | (i & INDEX_MASK),
        }
    }

    pub fn decode(id: u32) -> Pick {
        match id & !INDEX_MASK {
            FACE_LAYER => Pick::Face(id & INDEX_MASK),
            TOOLPATH_LAYER => Pick::Toolpath(id & INDEX_MASK),
            EDGE_LAYER => Pick::Edge(id & INDEX_MASK),
            _ => Pick::None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    base: [f32; 4],
    accent: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    palette: [[f32; 4]; 8],
    /// Stock colours by rest class, then by toolpath.
    rest: [[f32; 4]; 4],
    clips: [[f32; 4]; 8],
    /// x: picked face + 1; y: stock colouring.
    selection: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Draw {
    style: [f32; 4],
    id: [u32; 4],
    tint: [f32; 4],
}

/// Highlight records after the toolpaths': one for faces and three for
/// lines (shifted a pixel, so they read thicker), selected then hovered.
const TINT_RECORDS: u64 = 8;
/// How much of the tint a highlight takes.
const FACE_TINT: f32 = 0.65;

/// Dynamic-offset stride of [`Draw`] records.
const DRAW_STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    face: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    pos: [f32; 3],
    tag: u32,
    pick: u32,
}

fn lin4(c: Rgb, scale: f32) -> [f32; 4] {
    let [r, g, b] = brand::to_linear(c);
    [r * scale, g * scale, b * scale, 1.0]
}

/// Line colours by tag: rapid, cut, plunge/ramp, lead; edge, wire, stock,
/// fixture.
fn palette(dark: bool) -> [[f32; 4]; 8] {
    let edge = if dark {
        [0x0E, 0x10, 0x14]
    } else {
        [0x30, 0x34, 0x3C]
    };
    [
        lin4(brand::RED, 1.0),
        lin4(brand::CUTTER_ORANGE, 1.0),
        lin4(brand::BLUE, 1.0),
        lin4(brand::GREEN, 1.0),
        lin4(edge, 1.0),
        lin4(brand::AMBER, 1.0),
        lin4(if dark { brand::PAPER } else { brand::INK }, 0.6),
        lin4(brand::BLUE, if dark { 1.0 } else { 0.8 }),
    ]
}

#[derive(Debug)]
struct Targets {
    width: u32,
    height: u32,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    pick: wgpu::Texture,
    pick_view: wgpu::TextureView,
    pick_depth_view: wgpu::TextureView,
}

#[derive(Debug)]
struct GpuLines {
    buffer: wgpu::Buffer,
    count: u32,
}

#[derive(Debug)]
struct GpuMesh {
    buffer: wgpu::Buffer,
    count: u32,
}

/// One block of the stock mesh, with the mesh it was uploaded from (kept,
/// so that its address is not reused while it is compared).
#[derive(Debug)]
struct StockBlock {
    src: Arc<StockMesh>,
    gpu: Option<(wgpu::Buffer, wgpu::Buffer, u32)>,
}

/// What a frame shows besides the scene.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FrameState {
    pub camera: Camera,
    pub dark: bool,
    /// Pick index of the picked face.
    pub face: Option<u32>,
    /// Index of the selected toolpath; the others are dimmed.
    pub toolpath: Option<u32>,
    pub stock_colour: StockColour,
}

#[derive(Debug)]
pub struct Renderer {
    mesh_pipeline: wgpu::RenderPipeline,
    mesh_pick: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    line_pick: wgpu::RenderPipeline,
    bg_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    globals: wgpu::BindGroup,
    draw_layout: wgpu::BindGroupLayout,
    draws: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    draw_capacity: u64,
    targets: Option<Targets>,
    mesh: Option<GpuMesh>,
    /// The simulated stock by block; drawn instead of the faces while set.
    stock: Option<Vec<StockBlock>>,
    edges: Option<GpuLines>,
    toolpaths: Vec<Option<GpuLines>>,
    /// Vertex ranges of faces and edges, by pick index.
    face_ranges: Vec<[u32; 2]>,
    edge_ranges: Vec<[u32; 2]>,
    tints: Tints,
}

fn uniform_layout(device: &wgpu::Device, dynamic: bool) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn draw_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    n: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fluut draws"),
        size: n * DRAW_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: wgpu::BufferSize::new(std::mem::size_of::<Draw>() as u64),
            }),
        }],
    });
    (buffer, group)
}

impl Renderer {
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fluut viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluut uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let global_layout = uniform_layout(device, false);
        let draw_layout = uniform_layout(device, true);
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &global_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&global_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32, 2 => Uint32];
        let mesh_vb = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attrs,
        };
        let line_vb = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };
        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        vb: Option<wgpu::VertexBufferLayout<'_>>,
                        topology: wgpu::PrimitiveTopology,
                        depth: (bool, wgpu::CompareFunction),
                        pick: bool| {
            let format = if pick { PICK } else { FORMAT };
            let target = [Some(wgpu::ColorTargetState::from(format))];
            let buffers = [vb];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: if buffers[0].is_some() { &buffers } else { &[] },
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth.0),
                    depth_compare: Some(depth.1),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: if pick { 1 } else { SAMPLES },
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction::{Always, LessEqual};
        use wgpu::PrimitiveTopology::{LineList, TriangleList};
        let mesh_pipeline = pipeline(
            "fluut mesh",
            "vs_mesh",
            "fs_mesh",
            Some(mesh_vb.clone()),
            TriangleList,
            (true, LessEqual),
            false,
        );
        let mesh_pick = pipeline(
            "fluut mesh pick",
            "vs_mesh",
            "fs_mesh_pick",
            Some(mesh_vb),
            TriangleList,
            (true, LessEqual),
            true,
        );
        let line_pipeline = pipeline(
            "fluut lines",
            "vs_line",
            "fs_line",
            Some(line_vb.clone()),
            LineList,
            (true, LessEqual),
            false,
        );
        let line_pick = pipeline(
            "fluut lines pick",
            "vs_line",
            "fs_line_pick",
            Some(line_vb),
            LineList,
            (true, LessEqual),
            true,
        );
        let bg_pipeline = pipeline(
            "fluut background",
            "vs_bg",
            "fs_bg",
            None,
            TriangleList,
            (false, Always),
            false,
        );
        let draw_capacity = 16;
        let (draws, draw_group) = draw_buffer(device, &draw_layout, draw_capacity);
        Renderer {
            mesh_pipeline,
            mesh_pick,
            line_pipeline,
            line_pick,
            bg_pipeline,
            uniforms,
            globals,
            draw_layout,
            draws,
            draw_group,
            draw_capacity,
            targets: None,
            mesh: None,
            stock: None,
            edges: None,
            toolpaths: Vec::new(),
            face_ranges: Vec::new(),
            edge_ranges: Vec::new(),
            tints: Tints::default(),
        }
    }

    /// Sets what is tinted. Returns whether it changed; nothing is uploaded.
    pub fn set_tints(&mut self, t: Tints) -> bool {
        let changed = self.tints != t;
        self.tints = t;
        changed
    }

    /// Uploads a Setup's faces, edges and stock box.
    pub fn set_scene(
        &mut self,
        device: &wgpu::Device,
        mesh: &Mesh,
        edges: &Lines,
        stock: Option<&Bounds>,
    ) {
        let verts: Vec<MeshVertex> = (0..mesh.positions.len())
            .map(|i| MeshVertex {
                pos: mesh.positions[i],
                normal: mesh.normals[i],
                face: mesh.face[i],
            })
            .collect();
        self.mesh = (!verts.is_empty()).then(|| GpuMesh {
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fluut faces"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            count: verts.len() as u32,
        });
        self.face_ranges.clone_from(&mesh.ranges);
        self.edge_ranges.clone_from(&edges.ranges);
        let mut lines = edges.clone();
        if let Some(b) = stock {
            box_lines(b, &mut lines);
        }
        self.edges = upload_lines(device, &lines, |t| match t {
            1 => TAG_WIRE,
            2 => TAG_STOCK,
            crate::scene::FIXTURE_TAG => TAG_FIXTURE,
            _ => TAG_EDGE,
        });
    }

    /// Shows the simulated stock's blocks (D11 §4) instead of the faces;
    /// `None` shows the faces again. Uploads only the blocks that are not
    /// the ones shown (by `Arc`), and returns how many it uploaded, or 1
    /// when the stock was hidden. The stock is not pickable.
    pub fn set_stock(&mut self, device: &wgpu::Device, blocks: Option<&[Arc<StockMesh>]>) -> usize {
        let Some(blocks) = blocks else {
            return usize::from(self.stock.take().is_some());
        };
        let shown = self.stock.get_or_insert_with(Vec::new);
        if shown.len() != blocks.len() {
            shown.clear();
        }
        let mut n = 0;
        for (k, b) in blocks.iter().enumerate() {
            if shown.get(k).is_some_and(|s| Arc::ptr_eq(&s.src, b)) {
                continue;
            }
            let gpu = (!b.indices.is_empty()).then(|| {
                let verts: Vec<MeshVertex> = (0..b.positions.len())
                    .map(|i| MeshVertex {
                        pos: b.positions[i],
                        normal: b.normals[i],
                        face: STOCK_BIT | b.tags[i],
                    })
                    .collect();
                let init = |label, contents: &[u8], usage| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents,
                        usage,
                    })
                };
                (
                    init(
                        "fluut stock",
                        bytemuck::cast_slice(&verts),
                        wgpu::BufferUsages::VERTEX,
                    ),
                    init(
                        "fluut stock indices",
                        bytemuck::cast_slice(&b.indices),
                        wgpu::BufferUsages::INDEX,
                    ),
                    b.indices.len() as u32,
                )
            });
            let block = StockBlock {
                src: b.clone(),
                gpu,
            };
            if k < shown.len() {
                shown[k] = block;
            } else {
                shown.push(block);
            }
            n += 1;
        }
        n
    }

    /// Uploads toolpath lines; `None` entries keep their index but draw
    /// nothing (so pick indices match the caller's list).
    pub fn set_toolpaths(&mut self, device: &wgpu::Device, paths: &[Option<&Lines>]) {
        self.toolpaths = paths
            .iter()
            .map(|l| l.and_then(|l| upload_lines(device, l, |t| t)))
            .collect();
    }

    /// Makes the output `width × height` (at least 1 × 1). Returns `true`
    /// when a new texture was made; it must then be registered again.
    pub fn ensure_size(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.width == width && t.height == height)
        {
            return false;
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("fluut target"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        use wgpu::TextureUsages as U;
        let resolve = make(
            FORMAT,
            1,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC,
        );
        let msaa = make(FORMAT, SAMPLES, U::RENDER_ATTACHMENT);
        let depth = make(DEPTH, SAMPLES, U::RENDER_ATTACHMENT);
        let pick = make(PICK, 1, U::RENDER_ATTACHMENT | U::COPY_SRC);
        let pick_depth = make(DEPTH, 1, U::RENDER_ATTACHMENT);
        self.targets = Some(Targets {
            width,
            height,
            resolve_view: resolve.create_view(&Default::default()),
            resolve,
            msaa_view: msaa.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            pick_view: pick.create_view(&Default::default()),
            pick,
            pick_depth_view: pick_depth.create_view(&Default::default()),
        });
        true
    }

    /// The output texture view, once [`Self::ensure_size`] has run.
    #[must_use]
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.resolve_view)
    }

    fn write_uniforms(&self, queue: &wgpu::Queue, f: &FrameState, t: &Targets) {
        let u = Uniforms {
            view_proj: f
                .camera
                .view_proj(t.width as f32 / t.height as f32)
                .to_cols_array_2d(),
            eye: f.camera.eye().extend(1.0).into(),
            base: lin4(brand::STOCK_GREY, 1.0),
            accent: lin4(brand::CUTTER_ORANGE, 1.0),
            bg_top: lin4(
                if f.dark {
                    brand::RAISED
                } else {
                    brand::FLUTE_WHITE
                },
                1.0,
            ),
            bg_bottom: lin4(
                if f.dark {
                    brand::INK
                } else {
                    brand::LIGHT_LINE
                },
                0.9,
            ),
            palette: palette(f.dark),
            rest: brand::REST_COLOURS.map(|c| lin4(c, 1.0)),
            clips: std::array::from_fn(|i| {
                lin4(
                    brand::TOOLPATH_COLOURS[i % brand::TOOLPATH_COLOURS.len()],
                    1.0,
                )
            }),
            selection: [f.face.map_or(0, |i| i + 1), f.stock_colour.code(), 0, 0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
    }

    /// Writes one [`Draw`] per line set: edges first, then toolpaths, then
    /// the highlight records.
    fn write_draws(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: &FrameState) {
        let n = 1 + self.toolpaths.len() as u64 + TINT_RECORDS;
        if n > self.draw_capacity {
            self.draw_capacity = n.next_power_of_two();
            (self.draws, self.draw_group) =
                draw_buffer(device, &self.draw_layout, self.draw_capacity);
        }
        let mut bytes = vec![0u8; (n * DRAW_STRIDE) as usize];
        let mut put = |i: u64, d: Draw| {
            let at = (i * DRAW_STRIDE) as usize;
            bytes[at..at + std::mem::size_of::<Draw>()].copy_from_slice(bytemuck::bytes_of(&d));
        };
        put(
            0,
            Draw {
                style: [1.0, 0.0, 0.0, 0.0],
                id: [0; 4],
                tint: [0.0; 4],
            },
        );
        for i in 0..self.toolpaths.len() as u32 {
            let lit = f.toolpath.is_none_or(|s| s == i);
            put(
                1 + u64::from(i),
                Draw {
                    style: [if lit { 1.0 } else { 0.35 }, 0.0, 0.0, 0.0],
                    id: [Pick::Toolpath(i).encode(), 0, 0, 0],
                    tint: [0.0; 4],
                },
            );
        }
        let base = 1 + self.toolpaths.len() as u64;
        let (w, h) = self
            .targets
            .as_ref()
            .map_or((1.0, 1.0), |t| (t.width as f32, t.height as f32));
        let (px, py) = (2.0 / w, 2.0 / h);
        let colours = [lin4(brand::CUTTER_ORANGE, 1.0), lin4(brand::BLUE, 1.0)];
        for (k, c) in colours.into_iter().enumerate() {
            let at = base + 4 * k as u64;
            let tint = [c[0], c[1], c[2], FACE_TINT];
            put(
                at,
                Draw {
                    style: [1.0, 0.0, 0.0, 0.0],
                    id: [0; 4],
                    tint,
                },
            );
            let solid = [c[0], c[1], c[2], 1.0];
            for (j, (x, y)) in [(0.0, 0.0), (px, 0.0), (0.0, py)].into_iter().enumerate() {
                put(
                    at + 1 + j as u64,
                    Draw {
                        style: [1.0, 0.0, x, y],
                        id: [0; 4],
                        tint: solid,
                    },
                );
            }
        }
        queue.write_buffer(&self.draws, 0, &bytes);
    }

    fn draw_scene(&self, pass: &mut wgpu::RenderPass<'_>, pick: Option<PickMask>) {
        let mask = pick.unwrap_or(PickMask {
            faces: true,
            edges: true,
            toolpaths: true,
        });
        let picking = pick.is_some();
        pass.set_bind_group(0, &self.globals, &[]);
        pass.set_bind_group(1, &self.draw_group, &[0]);
        if let Some(blocks) = &self.stock {
            if !picking {
                pass.set_pipeline(&self.mesh_pipeline);
                for (vb, ib, count) in blocks.iter().filter_map(|b| b.gpu.as_ref()) {
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*count, 0, 0..1);
                }
            }
        } else if let Some(m) = self.mesh.as_ref() {
            pass.set_pipeline(if picking {
                &self.mesh_pick
            } else {
                &self.mesh_pipeline
            });
            pass.set_vertex_buffer(0, m.buffer.slice(..));
            pass.draw(0..m.count, 0..1);
        }
        pass.set_pipeline(if picking {
            &self.line_pick
        } else {
            &self.line_pipeline
        });
        if let Some(e) = self.edges.as_ref().filter(|_| mask.edges) {
            pass.set_vertex_buffer(0, e.buffer.slice(..));
            pass.draw(0..e.count, 0..1);
        }
        for (i, tp) in self.toolpaths.iter().enumerate() {
            if let Some(l) = tp.as_ref().filter(|_| mask.toolpaths) {
                let offset = ((1 + i as u64) * DRAW_STRIDE) as u32;
                pass.set_bind_group(1, &self.draw_group, &[offset]);
                pass.set_vertex_buffer(0, l.buffer.slice(..));
                pass.draw(0..l.count, 0..1);
            }
        }
        if !picking && self.stock.is_none() {
            self.draw_tints(pass);
        }
    }

    /// Redraws the highlighted faces and edges over themselves, from their
    /// ranges in the buffers already uploaded.
    fn draw_tints(&self, pass: &mut wgpu::RenderPass<'_>) {
        let base = 1 + self.toolpaths.len() as u64;
        let record = |i: u64| ((base + i) * DRAW_STRIDE) as u32;
        let hover_face = self.tints.hover_face;
        if let Some(m) = self.mesh.as_ref() {
            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_vertex_buffer(0, m.buffer.slice(..));
            let groups = [(0, self.tints.faces.as_slice()), (4, hover_face.as_slice())];
            for (at, faces) in groups {
                pass.set_bind_group(1, &self.draw_group, &[record(at)]);
                for r in faces
                    .iter()
                    .filter_map(|f| self.face_ranges.get(*f as usize))
                {
                    pass.draw(r[0]..r[1], 0..1);
                }
            }
        }
        if let Some(e) = self.edges.as_ref() {
            pass.set_pipeline(&self.line_pipeline);
            pass.set_vertex_buffer(0, e.buffer.slice(..));
            let hover_edge = self.tints.hover_edge;
            let groups = [(1, self.tints.edges.as_slice()), (5, hover_edge.as_slice())];
            for (at, edges) in groups {
                for j in 0..3 {
                    pass.set_bind_group(1, &self.draw_group, &[record(at + j)]);
                    for r in edges
                        .iter()
                        .filter_map(|i| self.edge_ranges.get(*i as usize))
                    {
                        pass.draw(r[0]..r[1], 0..1);
                    }
                }
            }
        }
    }

    /// Renders one frame into the output texture and submits it.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: &FrameState) {
        let Some(t) = &self.targets else {
            return;
        };
        self.write_uniforms(queue, f, t);
        self.write_draws(device, queue, f);
        let t = self.targets.as_ref().expect("sized");
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluut viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&t.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals, &[]);
            pass.set_bind_group(1, &self.draw_group, &[0]);
            pass.set_pipeline(&self.bg_pipeline);
            pass.draw(0..3, 0..1);
            self.draw_scene(&mut pass, None);
        }
        queue.submit([encoder.finish()]);
    }

    /// The output's pixels, row by row from the top, sRGB-encoded RGBA (for
    /// tests). Blocks until the GPU is done.
    pub fn read_pixels(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<[u8; 4]>> {
        let t = self.targets.as_ref()?;
        let row = (t.width * 4).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluut pixels readback"),
            size: u64::from(row * t.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(t.height),
                },
            },
            wgpu::Extent3d {
                width: t.width,
                height: t.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((t.width * t.height) as usize);
        for j in 0..t.height {
            for i in 0..t.width {
                let at = (j * row + i * 4) as usize;
                out.push([data[at], data[at + 1], data[at + 2], data[at + 3]]);
            }
        }
        Some(out)
    }

    /// What is under pixel `(x, y)` of the output (origin top left): the id
    /// pass is drawn for a [`PICK_SIZE`] square around it and read back; the
    /// hit nearest the pixel wins. Blocks until the GPU is done (one small
    /// pass, D13 §5.4 "≤ 1 frame").
    pub fn pick(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        f: &FrameState,
        x: u32,
        y: u32,
    ) -> Pick {
        self.pick_masked(device, queue, f, x, y, PickMask::VIEW)
    }

    /// As [`Self::pick`], for what `mask` allows. An edge anywhere in the
    /// square beats a face, since faces fill the square around their edges.
    pub fn pick_masked(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        f: &FrameState,
        x: u32,
        y: u32,
        mask: PickMask,
    ) -> Pick {
        let Some(t) = &self.targets else {
            return Pick::None;
        };
        if x >= t.width || y >= t.height {
            return Pick::None;
        }
        self.write_uniforms(queue, f, t);
        self.write_draws(device, queue, f);
        let t = self.targets.as_ref().expect("sized");
        let half = PICK_SIZE / 2;
        let (x0, y0) = (x.saturating_sub(half), y.saturating_sub(half));
        let (w, h) = (PICK_SIZE.min(t.width - x0), PICK_SIZE.min(t.height - y0));
        let row = 256u32;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluut pick readback"),
            size: u64::from(row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluut pick"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.pick_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.pick_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(x0, y0, w, h);
            self.draw_scene(&mut pass, Some(mask));
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.pick,
                mip_level: 0,
                origin: wgpu::Origin3d { x: x0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        if device.poll(wgpu::PollType::wait_indefinitely()).is_err() {
            return Pick::None;
        }
        let Ok(data) = slice.get_mapped_range() else {
            return Pick::None;
        };
        let mut best = (u32::MAX, 0u32);
        let mut edge = (u32::MAX, 0u32);
        for j in 0..h {
            for i in 0..w {
                let at = (j * row + i * 4) as usize;
                let id = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
                let (dx, dy) = ((x0 + i).abs_diff(x), (y0 + j).abs_diff(y));
                let dist = dx * dx + dy * dy;
                if matches!(Pick::decode(id), Pick::Edge(_)) && dist < edge.0 {
                    edge = (dist, id);
                }
                // Toolpath lines win over faces at the same distance: they
                // are drawn on top.
                let better = dist < best.0
                    || (dist == best.0
                        && Pick::decode(id) != Pick::None
                        && Pick::decode(best.1) == Pick::None);
                if id != 0 && better {
                    best = (dist, id);
                }
            }
        }
        Pick::decode(if edge.1 != 0 { edge.1 } else { best.1 })
    }
}

fn upload_lines(device: &wgpu::Device, l: &Lines, tag: impl Fn(u32) -> u32) -> Option<GpuLines> {
    let verts: Vec<LineVertex> = l
        .points
        .iter()
        .zip(&l.tag)
        .enumerate()
        .map(|(i, (p, t))| LineVertex {
            pos: *p,
            tag: tag(*t),
            pick: l.pick.get(i).copied().unwrap_or(0),
        })
        .collect();
    (!verts.is_empty()).then(|| GpuLines {
        buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fluut lines"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        count: verts.len() as u32,
    })
}

/// The 12 edges of a box, tagged 2 (stock).
fn box_lines(b: &Bounds, out: &mut Lines) {
    let c = |i: usize| {
        [
            if i & 1 == 0 { b.min[0] } else { b.max[0] },
            if i & 2 == 0 { b.min[1] } else { b.max[1] },
            if i & 4 == 0 { b.min[2] } else { b.max[2] },
        ]
    };
    for (a, z) in [
        (0, 1),
        (0, 2),
        (1, 3),
        (2, 3),
        (4, 5),
        (4, 6),
        (5, 7),
        (6, 7),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        out.points.extend([c(a), c(z)]);
        out.tag.extend([2, 2]);
    }
}

/// A headless device for tests and benchmarks, or `None` when the machine
/// has no adapter (CI without a GPU or a software driver).
pub fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
    // Primary backends only: the GL backend (WGL on Windows) runs a thread
    // that overflows its stack in debug test builds, and a test machine
    // that has only GL is not one that can run these tests.
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    desc.backends &= wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(desc);
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_ids_round_trip() {
        for p in [
            Pick::None,
            Pick::Face(0),
            Pick::Face(12345),
            Pick::Toolpath(0),
            Pick::Toolpath(7),
            Pick::Edge(0),
            Pick::Edge(99),
        ] {
            assert_eq!(Pick::decode(p.encode()), p);
        }
        assert_eq!(Pick::None.encode(), 0);
        assert_ne!(Pick::Face(3).encode(), Pick::Toolpath(3).encode());
        assert_ne!(Pick::Edge(3).encode(), Pick::Toolpath(3).encode());
    }

    #[test]
    fn box_has_twelve_edges() {
        let mut l = Lines::default();
        box_lines(
            &Bounds {
                min: [0.0; 3],
                max: [1.0; 3],
            },
            &mut l,
        );
        assert_eq!(l.segment_count(), 12);
        // Every edge is axis-aligned and of unit length.
        for s in l.points.chunks(2) {
            let d: f32 = (0..3).map(|i| (s[1][i] - s[0][i]).abs()).sum();
            assert_eq!(d, 1.0);
        }
    }
}
