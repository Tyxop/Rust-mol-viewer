//! VR UI Panel System
//!
//! This module provides 3D UI panels for VR that render egui interfaces
//! to textures and display them as textured quads in 3D space.

use anyhow::Result;
use glam::{Mat4, Quat, Vec3};
use wgpu;
use wgpu::util::DeviceExt;

/// A 3D UI panel that displays egui content in VR space
pub struct VrUiPanel {
    /// Offscreen texture for rendering UI
    pub texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,

    /// 3D positioning
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32, // Physical size in meters

    /// Rendering resources
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    transform_buffer: wgpu::Buffer,
}

/// Vertex data for UI quad
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadVertex {
    position: [f32; 3],
    uv: [f32; 2],
}

/// Transform uniform for positioning the quad in 3D space
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadTransform {
    model_matrix: [[f32; 4]; 4],
}

impl VrUiPanel {
    /// Create a new VR UI panel
    pub fn new(
        device: &wgpu::Device,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        width: u32,
        height: u32,
        position: Vec3,
        rotation: Quat,
        scale: f32,
    ) -> Result<Self> {
        // Create offscreen texture for UI rendering
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("VR UI Panel Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("VR UI Panel Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Create transform buffer
        let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("VR UI Panel Transform Buffer"),
            size: std::mem::size_of::<QuadTransform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group layout for panel (texture + transform)
        let panel_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("VR UI Panel Bind Group Layout"),
                entries: &[
                    // Transform uniform
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Texture
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Sampler
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("VR UI Panel Bind Group"),
            layout: &panel_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: transform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // Create shader module
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("VR UI Panel Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../assets/shaders/ui_quad.wgsl").into()),
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("VR UI Panel Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &panel_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("VR UI Panel Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<QuadVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // Position
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        // UV
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 12,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8UnormSrgb, // VR swapchain format
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
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
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create quad vertices (centered at origin, 1x1 in local space)
        let vertices = [
            QuadVertex {
                position: [-0.5, 0.5, 0.0],
                uv: [0.0, 0.0],
            }, // Top-left
            QuadVertex {
                position: [0.5, 0.5, 0.0],
                uv: [1.0, 0.0],
            }, // Top-right
            QuadVertex {
                position: [0.5, -0.5, 0.0],
                uv: [1.0, 1.0],
            }, // Bottom-right
            QuadVertex {
                position: [-0.5, -0.5, 0.0],
                uv: [0.0, 1.0],
            }, // Bottom-left
        ];

        let indices: [u16; 6] = [
            0, 1, 2, // First triangle
            0, 2, 3, // Second triangle
        ];

        // Create vertex buffer
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("VR UI Panel Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create index buffer
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("VR UI Panel Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Ok(Self {
            texture,
            texture_view,
            width,
            height,
            position,
            rotation,
            scale,
            pipeline,
            bind_group,
            vertex_buffer,
            index_buffer,
            transform_buffer,
        })
    }

    /// Update the panel's transform in GPU memory
    pub fn update_transform(&self, queue: &wgpu::Queue) {
        // Compute aspect ratio
        let aspect = self.width as f32 / self.height as f32;

        // Build model matrix: Translation * Rotation * Scale
        let model_matrix = Mat4::from_scale_rotation_translation(
            Vec3::new(self.scale * aspect, self.scale, 1.0), // Scale with aspect ratio
            self.rotation,
            self.position,
        );

        let transform = QuadTransform {
            model_matrix: model_matrix.to_cols_array_2d(),
        };

        queue.write_buffer(&self.transform_buffer, 0, bytemuck::bytes_of(&transform));
    }

    /// Render the UI panel quad to the given render pass
    pub fn render<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
    ) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, camera_bind_group, &[]);
        render_pass.set_bind_group(1, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..6, 0, 0..1);
    }
}
