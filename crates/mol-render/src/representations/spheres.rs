use crate::geometry::{create_icosphere, Vertex};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SphereInstance {
    pub model_matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
}

impl SphereInstance {
    pub fn new(position: Vec3, radius: f32, color: [f32; 4]) -> Self {
        let model_matrix = Mat4::from_scale_rotation_translation(
            Vec3::splat(radius),
            glam::Quat::IDENTITY,
            position,
        );

        Self {
            model_matrix: model_matrix.to_cols_array_2d(),
            color,
        }
    }

    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SphereInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // Model matrix (4x vec4)
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 2 * std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 3 * std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // Color
                wgpu::VertexAttribute {
                    offset: 4 * std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

pub struct SpheresRenderer {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    pub index_count: u32,  // Public for GPU-driven draw commands

    instance_buffer: wgpu::Buffer,
    instance_count: u32,
    max_instances: u32,

    pipeline: wgpu::RenderPipeline,
}

impl SpheresRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Create icosphere geometry (subdivision level 2)
        Self::new_with_subdivision(device, surface_format, camera_bind_group_layout, 2)
    }

    /// Create GPU-driven renderer using sphere_indirect.wgsl shader
    pub fn new_gpu_driven(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        gpu_data_bind_group_layout: &wgpu::BindGroupLayout,
        subdivision_level: u32,
    ) -> Self {
        let mesh = create_icosphere(subdivision_level);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GPU Sphere Vertex Buffer"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("GPU Sphere Index Buffer"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let index_count = mesh.indices.len() as u32;

        // No instance buffer needed for GPU-driven rendering
        // Instances are managed by visible_indices buffer
        let dummy_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Dummy Instance Buffer"),
            size: 64,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        // Load GPU-driven shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU Sphere Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/sphere_indirect.wgsl").into(),
            ),
        });

        // Create render pipeline with two bind groups: camera (group 0) and gpu data (group 1)
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GPU Sphere Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, gpu_data_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("GPU Sphere Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],  // Only vertex buffer, no instance buffer
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        log::info!(
            "GPU-driven SpheresRenderer initialized (subdiv {}): {} vertices, {} indices",
            subdivision_level,
            mesh.vertices.len(),
            index_count
        );

        Self {
            vertex_buffer,
            index_buffer,
            index_count,
            instance_buffer: dummy_instance_buffer,
            instance_count: 0,
            max_instances: 0,
            pipeline,
        }
    }

    pub fn new_with_subdivision(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        subdivision_level: u32,
    ) -> Self {
        // Create icosphere geometry with specified subdivision level
        let mesh = create_icosphere(subdivision_level);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sphere Vertex Buffer"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sphere Index Buffer"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let index_count = mesh.indices.len() as u32;

        // Create instance buffer (pre-allocate for many atoms)
        let max_instances = 100_000;
        let instance_buffer_size =
            (max_instances as usize * std::mem::size_of::<SphereInstance>()) as u64;

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sphere Instance Buffer"),
            size: instance_buffer_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sphere Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/sphere.wgsl").into(),
            ),
        });

        // Create render pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sphere Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sphere Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc(), SphereInstance::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        log::info!(
            "SpheresRenderer initialized (subdiv {}): {} vertices, {} indices",
            subdivision_level,
            mesh.vertices.len(),
            index_count
        );

        Self {
            vertex_buffer,
            index_buffer,
            index_count,
            instance_buffer,
            instance_count: 0,
            max_instances,
            pipeline,
        }
    }

    pub fn update_instances(&mut self, queue: &wgpu::Queue, instances: &[SphereInstance]) {
        if instances.len() > self.max_instances as usize {
            log::warn!(
                "Too many instances: {} > {}. Truncating.",
                instances.len(),
                self.max_instances
            );
            let truncated = &instances[..self.max_instances as usize];
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(truncated));
            self.instance_count = self.max_instances;
        } else {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
            self.instance_count = instances.len() as u32;
        }
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.instance_count == 0 {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.index_count, 0, 0..self.instance_count);
    }

    /// Render using GPU-driven indirect drawing
    /// bind_group should contain atom data and visible indices buffers
    /// draw_buffer should contain DrawIndexedIndirectCommand
    pub fn render_indirect<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        gpu_bind_group: &'a wgpu::BindGroup,
        draw_buffer: &'a wgpu::Buffer,
        draw_offset: u64,
    ) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_bind_group(1, gpu_bind_group, &[]);
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed_indirect(draw_buffer, draw_offset);
    }
}
