// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `wgpu` viewport renderer. It draws into its own texture, which each
//! toolkit then shows as an image, so the renderer is the same in both
//! prototypes.

use crate::brand;
use crate::camera::Camera;
use crate::mesh::{Mesh, Vertex};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Colour format of the output texture. The shader writes sRGB-encoded values.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Multisample count (S 08 §8.9 asks for an anti-aliased viewport).
pub const SAMPLES: u32 = 4;

/// The draped pass preview of CH-23.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview {
    pub enabled: bool,
    pub stepover_mm: f32,
    pub angle_deg: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    base: [f32; 4],
    accent: [f32; 4],
    preview: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
}

fn lin4(c: brand::Rgb, scale: f32) -> [f32; 4] {
    let [r, g, b] = brand::to_linear(c);
    [r * scale, g * scale, b * scale, 1.0]
}

#[derive(Debug)]
struct Targets {
    width: u32,
    height: u32,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
}

/// Mesh viewport.
#[derive(Debug)]
pub struct Viewport {
    mesh_pipeline: wgpu::RenderPipeline,
    bg_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    targets: Option<Targets>,
}

impl Viewport {
    /// Upload `mesh` and build the pipelines.
    ///
    /// # Panics
    ///
    /// If the mesh has more than `u32::MAX` indices.
    #[must_use]
    pub fn new(device: &wgpu::Device, mesh: &Mesh) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("s4 viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("s4 uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        };
        let multisample = wgpu::MultisampleState {
            count: SAMPLES,
            ..Default::default()
        };
        let target = [Some(wgpu::ColorTargetState::from(FORMAT))];
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("s4 mesh"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_mesh"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        });
        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("s4 background"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_bg"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_bg"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("s4 vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("s4 indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            mesh_pipeline,
            bg_pipeline,
            bind_group,
            uniforms,
            vertices,
            indices,
            index_count: u32::try_from(mesh.indices.len()).expect("too many indices"),
            targets: None,
        }
    }

    /// Make sure the output texture is `width × height` (at least 1 × 1).
    /// Returns `true` when a new texture was made; the toolkit must then
    /// re-import [`Self::texture`].
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
                label: Some("s4 target"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let resolve = make(
            FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let msaa = make(FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = make(DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        self.targets = Some(Targets {
            width,
            height,
            resolve_view: resolve.create_view(&Default::default()),
            resolve,
            msaa_view: msaa.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
        });
        true
    }

    /// The output texture, once [`Self::ensure_size`] has run.
    #[must_use]
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.resolve)
    }

    /// The output texture view, once [`Self::ensure_size`] has run.
    #[must_use]
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.resolve_view)
    }

    /// Render one frame into the output texture and submit it.
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cam: &Camera,
        preview: Preview,
    ) {
        let Some(t) = &self.targets else {
            return;
        };
        let u = Uniforms {
            view_proj: cam
                .view_proj(t.width as f32 / t.height as f32)
                .to_cols_array_2d(),
            eye: cam.eye().extend(1.0).into(),
            base: lin4(brand::STOCK_GREY, 1.0),
            accent: lin4(brand::CUTTER_ORANGE, 1.0),
            preview: [
                preview.stepover_mm,
                preview.angle_deg.to_radians(),
                f32::from(u8::from(preview.enabled)),
                0.0,
            ],
            bg_top: lin4(brand::RAISED, 1.0),
            bg_bottom: lin4(brand::INK, 0.8),
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("s4 viewport"),
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
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.bg_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}
