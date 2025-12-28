use crate::geometry::Vertex;
use crate::representations::spheres::SphereInstance;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct CylinderInstance {
    pub model_matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
}

impl CylinderInstance {
    pub fn new(start: Vec3, end: Vec3, radius: f32, color: [f32; 4]) -> Self {
        let direction = (end - start).normalize();
        let length = start.distance(end);
        let center = (start + end) * 0.5;

        // Create rotation to align Y-axis (cylinder's default axis) with bond direction
        let y_axis = Vec3::Y;
        let rotation = Quat::from_rotation_arc(y_axis, direction);

        let model_matrix = Mat4::from_scale_rotation_translation(
            Vec3::new(radius, length * 0.5, radius), // Scale: radius for XZ, half-length for Y
            rotation,
            center,
        );

        Self {
            model_matrix: model_matrix.to_cols_array_2d(),
            color,
        }
    }

    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CylinderInstance>() as wgpu::BufferAddress,
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

pub struct BallStickRenderer {
    // Sphere rendering (for atoms)
    sphere_vertex_buffer: wgpu::Buffer,
    sphere_index_buffer: wgpu::Buffer,
    sphere_index_count: u32,
    sphere_instance_buffer: wgpu::Buffer,
    sphere_instance_count: u32,

    // Cylinder rendering (for bonds)
    pub cylinder_vertex_buffer: wgpu::Buffer,
    pub cylinder_index_buffer: wgpu::Buffer,
    pub cylinder_index_count: u32,
    pub cylinder_instance_buffer: wgpu::Buffer,
    pub cylinder_instance_count: u32,

    pub pipeline: wgpu::RenderPipeline,
    max_sphere_instances: u32,
    max_cylinder_instances: u32,
}

impl BallStickRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Create sphere geometry (smaller than VdW)
        let sphere_mesh = crate::geometry::create_icosphere(1); // Lower subdivision for performance

        let sphere_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ball-Stick Sphere Vertex Buffer"),
            contents: bytemuck::cast_slice(&sphere_mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let sphere_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ball-Stick Sphere Index Buffer"),
            contents: bytemuck::cast_slice(&sphere_mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let sphere_index_count = sphere_mesh.indices.len() as u32;

        // Create cylinder geometry
        let cylinder_mesh = create_cylinder(16); // 16 sides

        let cylinder_vertex_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Cylinder Vertex Buffer"),
                contents: bytemuck::cast_slice(&cylinder_mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let cylinder_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Cylinder Index Buffer"),
            contents: bytemuck::cast_slice(&cylinder_mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let cylinder_index_count = cylinder_mesh.indices.len() as u32;

        // Create instance buffers
        let max_sphere_instances = 100_000;
        let max_cylinder_instances = 150_000; // More bonds than atoms typically

        let sphere_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Ball-Stick Sphere Instance Buffer"),
            size: (max_sphere_instances as usize * std::mem::size_of::<SphereInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let cylinder_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cylinder Instance Buffer"),
            size: (max_cylinder_instances as usize * std::mem::size_of::<CylinderInstance>())
                as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Use same shader as spheres
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ball-Stick Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/sphere.wgsl").into(),
            ),
        });

        // Create pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ball-Stick Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Ball-Stick Render Pipeline"),
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
            "BallStickRenderer initialized: sphere {} verts, cylinder {} verts",
            sphere_mesh.vertices.len(),
            cylinder_mesh.vertices.len()
        );

        Self {
            sphere_vertex_buffer,
            sphere_index_buffer,
            sphere_index_count,
            sphere_instance_buffer,
            sphere_instance_count: 0,
            cylinder_vertex_buffer,
            cylinder_index_buffer,
            cylinder_index_count,
            cylinder_instance_buffer,
            cylinder_instance_count: 0,
            pipeline,
            max_sphere_instances,
            max_cylinder_instances,
        }
    }

    pub fn update_spheres(&mut self, queue: &wgpu::Queue, instances: &[SphereInstance]) {
        let count = instances.len().min(self.max_sphere_instances as usize);
        queue.write_buffer(
            &self.sphere_instance_buffer,
            0,
            bytemuck::cast_slice(&instances[..count]),
        );
        self.sphere_instance_count = count as u32;
    }

    pub fn update_cylinders(&mut self, queue: &wgpu::Queue, instances: &[CylinderInstance]) {
        let count = instances.len().min(self.max_cylinder_instances as usize);
        queue.write_buffer(
            &self.cylinder_instance_buffer,
            0,
            bytemuck::cast_slice(&instances[..count]),
        );
        self.cylinder_instance_count = count as u32;
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        render_pass.set_pipeline(&self.pipeline);

        // Render cylinders (bonds) first so they appear behind atoms
        if self.cylinder_instance_count > 0 {
            render_pass.set_vertex_buffer(0, self.cylinder_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.cylinder_instance_buffer.slice(..));
            render_pass.set_index_buffer(
                self.cylinder_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            render_pass.draw_indexed(0..self.cylinder_index_count, 0, 0..self.cylinder_instance_count);
        }

        // Render spheres (atoms)
        if self.sphere_instance_count > 0 {
            render_pass.set_vertex_buffer(0, self.sphere_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.sphere_instance_buffer.slice(..));
            render_pass.set_index_buffer(
                self.sphere_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            render_pass.draw_indexed(0..self.sphere_index_count, 0, 0..self.sphere_instance_count);
        }
    }
}

/// Create a cylinder mesh aligned along the Y axis with specified LOD
/// The cylinder goes from -1 to +1 in Y, with radius 1
pub fn create_cylinder(segments: u32) -> crate::geometry::Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    let angle_step = 2.0 * std::f32::consts::PI / segments as f32;

    // Create vertices for top and bottom circles
    for i in 0..segments {
        let angle = i as f32 * angle_step;
        let x = angle.cos();
        let z = angle.sin();

        // Bottom vertex
        vertices.push(Vertex::new(Vec3::new(x, -1.0, z), Vec3::new(x, 0.0, z)));

        // Top vertex
        vertices.push(Vertex::new(Vec3::new(x, 1.0, z), Vec3::new(x, 0.0, z)));
    }

    // Create side faces (two triangles per segment)
    for i in 0..segments {
        let next = (i + 1) % segments;

        let bottom_current = i * 2;
        let top_current = i * 2 + 1;
        let bottom_next = next * 2;
        let top_next = next * 2 + 1;

        // First triangle
        indices.push(bottom_current);
        indices.push(top_current);
        indices.push(bottom_next);

        // Second triangle
        indices.push(bottom_next);
        indices.push(top_current);
        indices.push(top_next);
    }

    // Add caps (top and bottom)
    let bottom_center_idx = vertices.len() as u32;
    vertices.push(Vertex::new(Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, -1.0, 0.0)));

    let top_center_idx = vertices.len() as u32;
    vertices.push(Vertex::new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)));

    // Bottom cap triangles
    for i in 0..segments {
        let next = (i + 1) % segments;
        indices.push(bottom_center_idx);
        indices.push(i * 2);
        indices.push(next * 2);
    }

    // Top cap triangles
    for i in 0..segments {
        let next = (i + 1) % segments;
        indices.push(top_center_idx);
        indices.push(next * 2 + 1);
        indices.push(i * 2 + 1);
    }

    crate::geometry::Mesh { vertices, indices }
}
