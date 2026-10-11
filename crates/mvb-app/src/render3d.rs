//! GPU side of the 3D view: a wgpu pipeline drawing the scene mesh inside egui's render pass.

use std::sync::Arc;

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::scene::Mesh;

/// Depth buffer requested from eframe; the pipeline must use the same format.
pub const DEPTH_BITS: u8 = 32;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Anti-aliasing samples requested from eframe; the pipeline must use the same count.
pub const MSAA_SAMPLES: u16 = 4;

/// Pipeline and buffers, stored in egui's callback resources.
pub struct Renderer3d {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    mesh: Option<GpuMesh>,
    max_buffer_size: u64,
}

struct GpuMesh {
    generation: u64,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl Renderer3d {
    /// Creates the pipeline and registers it with egui's renderer.
    pub fn install(render_state: &egui_wgpu::RenderState) {
        let device = &render_state.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("view3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("view3d.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view3d_uniforms"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view3d_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view3d_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("view3d_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let target = render_state.target_format;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("view3d_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: u32::from(MSAA_SAMPLES),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(if target.is_srgb() {
                    "fs_linear_framebuffer"
                } else {
                    "fs_gamma_framebuffer"
                }),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let renderer = Self {
            pipeline,
            uniforms,
            bind_group,
            mesh: None,
            max_buffer_size: device.limits().max_buffer_size,
        };
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(renderer);
    }
}

/// Largest mesh the GPU accepts, in bytes per buffer.
pub fn fits_in_gpu_buffer(mesh: &Mesh, max_buffer_size: u64) -> bool {
    (mesh.vertices.len() as u64) * 16 <= max_buffer_size
        && (mesh.indices.len() as u64) * 4 <= max_buffer_size
}

/// One frame of the 3D view, added to egui's paint list.
pub struct Paint3d {
    pub view_proj: [f32; 16],
    pub mesh: Arc<Mesh>,
    /// Changes whenever the mesh is rebuilt, so it is uploaded only then.
    pub generation: u64,
}

impl CallbackTrait for Paint3d {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = resources.get_mut::<Renderer3d>() else {
            return Vec::new();
        };
        let matrix: Vec<u8> = self
            .view_proj
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        queue.write_buffer(&renderer.uniforms, 0, &matrix);

        let up_to_date = renderer
            .mesh
            .as_ref()
            .is_some_and(|mesh| mesh.generation == self.generation);
        if !up_to_date {
            renderer.mesh = (!self.mesh.indices.is_empty()
                && fits_in_gpu_buffer(&self.mesh, renderer.max_buffer_size))
            .then(|| GpuMesh {
                generation: self.generation,
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("view3d_vertices"),
                    contents: &self.mesh.vertex_bytes(),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("view3d_indices"),
                    contents: &self.mesh.index_bytes(),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: self.mesh.indices.len() as u32,
            });
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(renderer) = resources.get::<Renderer3d>() else {
            return;
        };
        let Some(mesh) = &renderer.mesh else {
            return;
        };
        render_pass.set_pipeline(&renderer.pipeline);
        render_pass.set_bind_group(0, &renderer.bind_group, &[]);
        render_pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        render_pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..mesh.index_count, 0, 0..1);
    }
}
