//! Headless cross-platform 3D GPU renderer powered by `wgpu`.
//!
//! Provides offscreen hardware-accelerated rendering for the 3D face preview,
//! including perspective projection, Blinn-Phong studio lighting, specular
//! highlights, rim lighting, and depth-buffered rasterization with texture readback.

use std::sync::{Mutex, OnceLock};

use glam::Vec3;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use wgpu::util::DeviceExt;

use crate::face_mesh::{get_glb_mesh, GlbMesh};

pub const RENDER_WIDTH: u32 = 512;
pub const RENDER_HEIGHT: u32 = 512;

static WGPU_RENDERER: OnceLock<Mutex<Option<WgpuRenderer>>> = OnceLock::new();

const WGSL_SHADER_SOURCE: &str = r#"
struct Uniforms {
    cam_dist: f32,
    focal: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) view_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.view_position = input.position;
    out.normal = input.normal;

    let view_z = input.position.z;
    let perspective_scale = uniforms.focal / max(uniforms.cam_dist - view_z, 0.1);
    let ndc_x = input.position.x * perspective_scale * 2.0;
    let ndc_y = input.position.y * perspective_scale * 2.0;
    let ndc_z = clamp((view_z / 0.1) * 0.5 + 0.5, 0.0, 1.0);

    out.clip_position = vec4<f32>(ndc_x, ndc_y, ndc_z, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var normal = in.normal;
    let len_sq = dot(normal, normal);
    if (len_sq > 1e-6) {
        normal = normalize(normal);
    } else {
        normal = vec3<f32>(0.0, 0.0, 1.0);
    }

    let key_light_dir = normalize(vec3<f32>(0.35, 0.55, 0.75));
    let fill_light_dir = normalize(vec3<f32>(-0.45, -0.2, 0.6));
    let half_vector = normalize(vec3<f32>(key_light_dir.x, key_light_dir.y, key_light_dir.z + 1.0));

    let diffuse_key = max(dot(normal, key_light_dir), 0.0);
    let diffuse_fill = max(dot(normal, fill_light_dir), 0.0);
    let specular_dot = max(dot(normal, half_vector), 0.0);
    let specular_highlight = pow(specular_dot, 14.0);
    let rim_lighting = pow(1.0 - clamp(normal.z, 0.0, 1.0), 2.0);

    let color_r = (36.0 + diffuse_key * 170.0 + diffuse_fill * 55.0 + specular_highlight * 140.0 + rim_lighting * 35.0) / 255.0;
    let color_g = (34.0 + diffuse_key * 155.0 + diffuse_fill * 70.0 + specular_highlight * 140.0 + rim_lighting * 30.0) / 255.0;
    let color_b = (44.0 + diffuse_key * 175.0 + diffuse_fill * 95.0 + specular_highlight * 160.0 + rim_lighting * 45.0) / 255.0;

    return vec4<f32>(clamp(color_r, 0.0, 1.0), clamp(color_g, 0.0, 1.0), clamp(color_b, 0.0, 1.0), 1.0);
}
"#;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ShaderUniforms {
    cam_dist: f32,
    focal: f32,
    _pad0: f32,
    _pad1: f32,
}

/// Headless cross-platform 3D GPU renderer powered by `wgpu`.
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    bind_group: wgpu::BindGroup,
    color_texture: wgpu::Texture,
    depth_texture: wgpu::Texture,
    readback_buffer: wgpu::Buffer,
}

impl WgpuRenderer {
    /// Initialize headless wgpu device, pipeline, and offscreen render buffers.
    pub fn new(mesh: &GlbMesh) -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?;

        let (device, queue) = pollster::block_on(
            adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("snenk_wgpu_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(wgpu::Limits::default()),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            ),
        )
        .ok()?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("snenk_face_shader"),
            source: wgpu::ShaderSource::Wgsl(WGSL_SHADER_SOURCE.into()),
        });

        let uniforms = ShaderUniforms {
            cam_dist: 0.48,
            focal: 1.95,
            _pad0: 0.0,
            _pad1: 0.0,
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniform_buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bind_group_layout"),
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
            label: Some("bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                            shader_location: 1,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vertex_buffer"),
            size: (mesh.base_positions.len() * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut flattened_indices = Vec::with_capacity(mesh.triangles.len() * 3);
        for triangle in &mesh.triangles {
            flattened_indices.push(triangle[0]);
            flattened_indices.push(triangle[1]);
            flattened_indices.push(triangle[2]);
        }
        let index_count = flattened_indices.len() as u32;

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("index_buffer"),
            contents: bytemuck::cast_slice(&flattened_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color_target"),
            size: wgpu::Extent3d {
                width: RENDER_WIDTH,
                height: RENDER_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth_target"),
            size: wgpu::Extent3d {
                width: RENDER_WIDTH,
                height: RENDER_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback_buffer"),
            size: (RENDER_WIDTH * RENDER_HEIGHT * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Some(Self {
            device,
            queue,
            pipeline,
            vertex_buffer,
            index_buffer,
            index_count,
            bind_group,
            color_texture,
            depth_texture,
            readback_buffer,
        })
    }

    /// Render deformed 3D vertices offscreen and read back pixel buffer.
    pub fn render(
        &mut self,
        mesh: &GlbMesh,
        view_space_vertices: &[[f32; 3]],
    ) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
        let mut normal_accum = vec![Vec3::ZERO; view_space_vertices.len()];
        for &[i0, i1, i2] in &mesh.triangles {
            let u0 = i0 as usize;
            let u1 = i1 as usize;
            let u2 = i2 as usize;
            if u0 < view_space_vertices.len()
                && u1 < view_space_vertices.len()
                && u2 < view_space_vertices.len()
            {
                let p0 = Vec3::from_array(view_space_vertices[u0]);
                let p1 = Vec3::from_array(view_space_vertices[u1]);
                let p2 = Vec3::from_array(view_space_vertices[u2]);
                let face_normal = (p1 - p0).cross(p2 - p0);
                normal_accum[u0] += face_normal;
                normal_accum[u1] += face_normal;
                normal_accum[u2] += face_normal;
            }
        }

        let mut vertices = Vec::with_capacity(view_space_vertices.len());
        for (pos, norm) in view_space_vertices.iter().zip(normal_accum.iter()) {
            let len = norm.length();
            let normal = if len > 1e-8 {
                *norm / len
            } else {
                Vec3::new(0.0, 0.0, 1.0)
            };

            vertices.push(Vertex {
                position: *pos,
                normal: normal.to_array(),
            });
        }

        self.queue
            .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        let color_view = self
            .color_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = self
            .depth_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(RENDER_WIDTH * 4),
                    rows_per_image: Some(RENDER_HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: RENDER_WIDTH,
                height: RENDER_HEIGHT,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = self.readback_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = sender.send(res);
        });

        self.device.poll(wgpu::Maintain::wait());
        receiver.recv().ok()?.ok()?;

        let mapped_data = buffer_slice.get_mapped_range();
        let mut pixel_buf = SharedPixelBuffer::<Rgba8Pixel>::new(RENDER_WIDTH, RENDER_HEIGHT);
        pixel_buf
            .make_mut_slice()
            .copy_from_slice(bytemuck::cast_slice(&mapped_data));
        drop(mapped_data);
        self.readback_buffer.unmap();

        Some(pixel_buf)
    }
}

/// Render deformed 3D view-space vertices using the static offscreen GPU renderer.
pub fn render_view_space_vertices(view_space_vertices: &[[f32; 3]]) -> Image {
    let mesh = get_glb_mesh();
    let pixel_buf = WGPU_RENDERER
        .get_or_init(|| Mutex::new(WgpuRenderer::new(mesh)))
        .lock()
        .ok()
        .and_then(|mut guard| {
            guard
                .as_mut()
                .and_then(|r| r.render(mesh, view_space_vertices))
        })
        .unwrap_or_else(|| SharedPixelBuffer::<Rgba8Pixel>::new(RENDER_WIDTH, RENDER_HEIGHT));

    Image::from_rgba8_premultiplied(pixel_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgpu_renderer_renders_mesh_without_panics() {
        let mesh = get_glb_mesh();
        let image = render_view_space_vertices(&mesh.base_positions);
        assert_eq!(image.size().width, RENDER_WIDTH);
        assert_eq!(image.size().height, RENDER_HEIGHT);
    }

    #[test]
    fn vertex_smooth_normals_are_computed() {
        let mesh = get_glb_mesh();
        let mut normal_accum = vec![Vec3::ZERO; mesh.base_positions.len()];
        for &[i0, i1, i2] in &mesh.triangles {
            let p0 = Vec3::from_array(mesh.base_positions[i0 as usize]);
            let p1 = Vec3::from_array(mesh.base_positions[i1 as usize]);
            let p2 = Vec3::from_array(mesh.base_positions[i2 as usize]);
            let face_normal = (p1 - p0).cross(p2 - p0);
            normal_accum[i0 as usize] += face_normal;
            normal_accum[i1 as usize] += face_normal;
            normal_accum[i2 as usize] += face_normal;
        }

        let non_zero_count = normal_accum.iter().filter(|n| n.length() > 1e-8).count();
        assert!(non_zero_count > 0);
        assert_eq!(non_zero_count, mesh.base_positions.len());
    }
}
