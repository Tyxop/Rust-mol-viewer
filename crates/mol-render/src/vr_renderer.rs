//! VR Renderer for Stereo Rendering
//!
//! This module handles VR-specific rendering with dual render passes (left and right eye),
//! integrating OpenXR session management with the main wgpu renderer.

use crate::camera::{Camera, Eye};
use crate::geometry::{create_icosphere, Vertex};
use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3, Vec4};
use log::{info, warn};
use mol_vr::{openxr, VrPerformanceMonitor, VrSession};
use wgpu;
use wgpu::util::DeviceExt;

/// GPU-side uniform for a single controller sphere (model matrix + colour).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CtrlUniform {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

/// Temporary bundle used while constructing controller sphere resources.
struct CtrlResources {
    pipeline: wgpu::RenderPipeline,
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    index_count: u32,
    left_buffer: wgpu::Buffer,
    right_buffer: wgpu::Buffer,
    left_bg: wgpu::BindGroup,
    right_bg: wgpu::BindGroup,
}

/// Temporary bundle used while constructing the menu panel resources.
struct MenuResources {
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    transform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// VR renderer with stereo support
pub struct VrRenderer {
    pub vr_session: VrSession,

    /// The wgpu texture format used by the VR swapchain (e.g. Rgba8UnormSrgb).
    /// Render pipelines used in VR passes must target this format.
    pub swapchain_format: wgpu::TextureFormat,

    // Depth textures for each eye (stored to keep alive, accessed via views)
    _left_depth_texture: wgpu::Texture,
    pub left_depth_view: wgpu::TextureView,
    _right_depth_texture: wgpu::Texture,
    pub right_depth_view: wgpu::TextureView,

    // Camera buffers for each eye
    pub left_camera_buffer: wgpu::Buffer,
    pub right_camera_buffer: wgpu::Buffer,
    pub left_camera_bind_group: wgpu::BindGroup,
    pub right_camera_bind_group: wgpu::BindGroup,

    // Performance monitoring
    performance_monitor: VrPerformanceMonitor,
    last_warning_frame: usize,

    // ── Floating VR menu panel ────────────────────────────────────────────
    // Offscreen texture where egui draws the menu (Rgba8Unorm, 512×256).
    // A quad pipeline samples this texture and draws it into each eye pass.
    pub vr_menu_texture: wgpu::Texture,
    pub vr_menu_texture_view: wgpu::TextureView,
    vr_menu_pipeline: wgpu::RenderPipeline,
    vr_menu_vertex_buffer: wgpu::Buffer,
    vr_menu_index_buffer: wgpu::Buffer,
    /// GPU buffer for the 4×4 world-space transform of the menu quad.
    pub vr_menu_transform_buffer: wgpu::Buffer,
    vr_menu_bind_group: wgpu::BindGroup,

    // ── Controller sphere visualisation ──────────────────────────────────
    ctrl_pipeline: wgpu::RenderPipeline,
    ctrl_vb: wgpu::Buffer,
    ctrl_ib: wgpu::Buffer,
    ctrl_index_count: u32,
    pub ctrl_left_buffer: wgpu::Buffer,   // CtrlUniform (model + color) for left  controller
    pub ctrl_right_buffer: wgpu::Buffer,  // CtrlUniform (model + color) for right controller
    ctrl_left_bg: wgpu::BindGroup,
    ctrl_right_bg: wgpu::BindGroup,
}

impl VrRenderer {
    /// Create a VR renderer from a pre-created `VrSession`.
    ///
    /// The session is created in `Renderer::new()` (together with the Vulkan context that
    /// wgpu is then initialized from), so we receive it here already fully constructed.
    pub fn new(
        mut vr_session: VrSession,
        device: &wgpu::Device,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        info!("Initializing VR renderer...");

        // Create swapchains — format is auto-selected from OpenXR's supported list
        let swapchain_format = vr_session
            .create_swapchains(device)
            .context("Failed to create VR swapchains")?;
        info!("VR swapchains created with format: {:?}", swapchain_format);

        // Get swapchain resolution
        let (left_width, left_height) = vr_session
            .left_swapchain
            .as_ref()
            .map(|s| s.resolution)
            .unwrap_or((2048, 2048));

        let (right_width, right_height) = vr_session
            .right_swapchain
            .as_ref()
            .map(|s| s.resolution)
            .unwrap_or((2048, 2048));

        info!(
            "VR resolution — Left: {}x{}, Right: {}x{}",
            left_width, left_height, right_width, right_height
        );

        // Create depth textures for each eye
        let left_depth_texture =
            Self::create_depth_texture(device, left_width, left_height, "VR Left Depth");
        let left_depth_view =
            left_depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let right_depth_texture =
            Self::create_depth_texture(device, right_width, right_height, "VR Right Depth");
        let right_depth_view =
            right_depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create camera uniform buffers for each eye
        let left_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("VR Left Camera Buffer"),
            size: std::mem::size_of::<crate::camera::CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let right_camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("VR Right Camera Buffer"),
            size: std::mem::size_of::<crate::camera::CameraUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind groups for each eye
        let left_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("VR Left Camera Bind Group"),
            layout: camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: left_camera_buffer.as_entire_binding(),
            }],
        });

        let right_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("VR Right Camera Bind Group"),
            layout: camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: right_camera_buffer.as_entire_binding(),
            }],
        });

        info!("VR renderer initialized successfully");

        // ── Build floating menu panel resources ───────────────────────────
        let menu = Self::create_menu_resources(device, camera_bind_group_layout, swapchain_format);

        // ── Build controller sphere resources ─────────────────────────────
        let ctrl = Self::create_controller_resources(device, camera_bind_group_layout, swapchain_format);

        Ok(Self {
            vr_session,
            swapchain_format,
            _left_depth_texture: left_depth_texture,
            left_depth_view,
            _right_depth_texture: right_depth_texture,
            right_depth_view,
            left_camera_buffer,
            right_camera_buffer,
            left_camera_bind_group,
            right_camera_bind_group,
            performance_monitor: VrPerformanceMonitor::new(),
            last_warning_frame: 0,
            vr_menu_texture: menu.texture,
            vr_menu_texture_view: menu.texture_view,
            vr_menu_pipeline: menu.pipeline,
            vr_menu_vertex_buffer: menu.vertex_buffer,
            vr_menu_index_buffer: menu.index_buffer,
            vr_menu_transform_buffer: menu.transform_buffer,
            vr_menu_bind_group: menu.bind_group,
            ctrl_pipeline: ctrl.pipeline,
            ctrl_vb: ctrl.vb,
            ctrl_ib: ctrl.ib,
            ctrl_index_count: ctrl.index_count,
            ctrl_left_buffer: ctrl.left_buffer,
            ctrl_right_buffer: ctrl.right_buffer,
            ctrl_left_bg: ctrl.left_bg,
            ctrl_right_bg: ctrl.right_bg,
        })
    }

    // ── Controller sphere creation ────────────────────────────────────────

    fn create_controller_resources(
        device: &wgpu::Device,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        swapchain_format: wgpu::TextureFormat,
    ) -> CtrlResources {
        // Low-poly icosphere (subdivision 1) — plenty for a 3 cm marker
        let mesh = create_icosphere(1);
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ctrl Sphere VB"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ctrl Sphere IB"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let index_count = mesh.indices.len() as u32;

        // Bind group layout for per-controller data (model + color)
        let ctrl_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Ctrl Sphere BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let make_buf = |color: [f32; 4]| -> wgpu::Buffer {
            let u = CtrlUniform { model: Mat4::IDENTITY.to_cols_array_2d(), color };
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Ctrl Uniform"),
                contents: bytemuck::cast_slice(&[u]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        // Left = blue, Right = red
        let left_buffer  = make_buf([0.2, 0.4, 1.0, 1.0]);
        let right_buffer = make_buf([1.0, 0.2, 0.2, 1.0]);

        let make_bg = |buf: &wgpu::Buffer| -> wgpu::BindGroup {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Ctrl Sphere BG"),
                layout: &ctrl_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buf.as_entire_binding(),
                }],
            })
        };
        let left_bg  = make_bg(&left_buffer);
        let right_bg = make_bg(&right_buffer);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ctrl Sphere Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../assets/shaders/controller_sphere.wgsl").into(),
            ),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ctrl Sphere Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &ctrl_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Ctrl Sphere Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: swapchain_format,
                    blend: None, // opaque
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
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

        CtrlResources { pipeline, vb, ib, index_count, left_buffer, right_buffer, left_bg, right_bg }
    }

    /// Upload per-frame controller transforms (mol-space, i.e. pre-multiplied by mol_to_world⁻¹).
    /// Call once per frame after computing mol_to_world.
    pub fn update_controller_transforms(
        &self,
        queue: &wgpu::Queue,
        left_model: Mat4,
        right_model: Mat4,
    ) {
        let left_u = CtrlUniform {
            model: left_model.to_cols_array_2d(),
            color: [0.2, 0.4, 1.0, 1.0], // blue
        };
        let right_u = CtrlUniform {
            model: right_model.to_cols_array_2d(),
            color: [1.0, 0.2, 0.2, 1.0], // red
        };
        queue.write_buffer(&self.ctrl_left_buffer,  0, bytemuck::cast_slice(&[left_u]));
        queue.write_buffer(&self.ctrl_right_buffer, 0, bytemuck::cast_slice(&[right_u]));
    }

    /// Render both controller spheres into a VR eye render pass.
    pub fn render_controllers<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        camera_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.ctrl_pipeline);
        pass.set_vertex_buffer(0, self.ctrl_vb.slice(..));
        pass.set_index_buffer(self.ctrl_ib.slice(..), wgpu::IndexFormat::Uint32);
        // Left controller (blue)
        pass.set_bind_group(0, camera_bg, &[]);
        pass.set_bind_group(1, &self.ctrl_left_bg, &[]);
        pass.draw_indexed(0..self.ctrl_index_count, 0, 0..1);
        // Right controller (red)
        pass.set_bind_group(1, &self.ctrl_right_bg, &[]);
        pass.draw_indexed(0..self.ctrl_index_count, 0, 0..1);
    }

    // ── Menu panel creation ───────────────────────────────────────────────

    fn create_menu_resources(
        device: &wgpu::Device,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
        swapchain_format: wgpu::TextureFormat,
    ) -> MenuResources {
        const W: u32 = 512;
        const H: u32 = 256;

        // Offscreen texture: egui draws the menu here
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("VR Menu Texture"),
            size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Sampler for the menu texture
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("VR Menu Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Transform buffer (mat4 in stage space)
        let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("VR Menu Transform"),
            size: 64, // mat4x4<f32>
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group layout for the panel (set 1): transform + texture + sampler
        let panel_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("VR Menu Panel BGL"),
            entries: &[
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("VR Menu Bind Group"),
            layout: &panel_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: transform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        // Shader (reuse ui_quad.wgsl)
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("VR Menu Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../assets/shaders/ui_quad.wgsl").into(),
            ),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("VR Menu Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &panel_bgl],
            push_constant_ranges: &[],
        });

        // Pipeline renders the menu quad INTO the VR swapchain (must match swapchain_format)
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("VR Menu Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 20, // 3×f32 pos + 2×f32 uv
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0,  shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 12, shader_location: 1 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: swapchain_format, // must match VR swapchain
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None, // visible from both sides
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false, // transparent UI, don't write depth
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Unit quad vertices (local space ±0.5); world scale applied via transform
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct QV { pos: [f32; 3], uv: [f32; 2] }
        let verts: [QV; 4] = [
            QV { pos: [-0.5,  0.5, 0.0], uv: [0.0, 0.0] },
            QV { pos: [ 0.5,  0.5, 0.0], uv: [1.0, 0.0] },
            QV { pos: [ 0.5, -0.5, 0.0], uv: [1.0, 1.0] },
            QV { pos: [-0.5, -0.5, 0.0], uv: [0.0, 1.0] },
        ];
        let idxs: [u16; 6] = [0, 1, 2, 0, 2, 3];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("VR Menu VB"), contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("VR Menu IB"), contents: bytemuck::cast_slice(&idxs),
            usage: wgpu::BufferUsages::INDEX,
        });

        MenuResources { texture, texture_view, pipeline, vertex_buffer, index_buffer, transform_buffer, bind_group }
    }

    /// Write the menu quad's world-space transform (Mat4, stage-space meters) to GPU.
    /// Call once per frame when the menu world position changes.
    pub fn update_menu_transform(&self, queue: &wgpu::Queue, world_mat: Mat4) {
        queue.write_buffer(&self.vr_menu_transform_buffer, 0, bytemuck::cast_slice(&world_mat.to_cols_array()));
    }

    /// Render the floating menu quad into a VR eye render pass.
    /// `camera_bg` must be the per-eye camera bind group (set 0).
    pub fn render_menu_quad<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        camera_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.vr_menu_pipeline);
        pass.set_bind_group(0, camera_bg, &[]);
        pass.set_bind_group(1, &self.vr_menu_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vr_menu_vertex_buffer.slice(..));
        pass.set_index_buffer(self.vr_menu_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    /// Create a depth texture for VR rendering
    fn create_depth_texture(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        label: &str,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// Update camera configuration from OpenXR views
    pub fn update_camera_from_vr(&mut self, camera: &mut Camera) -> Result<()> {
        // Get view configurations from OpenXR
        let view_configs = self.vr_session.get_view_configs()?;

        // Convert OpenXR FOV to our FOV format
        let left_fov = crate::camera::FovConfig {
            angle_left: view_configs[0].fov.angle_left,
            angle_right: view_configs[0].fov.angle_right,
            angle_up: view_configs[0].fov.angle_up,
            angle_down: view_configs[0].fov.angle_down,
        };

        let right_fov = crate::camera::FovConfig {
            angle_left: view_configs[1].fov.angle_left,
            angle_right: view_configs[1].fov.angle_right,
            angle_up: view_configs[1].fov.angle_up,
            angle_down: view_configs[1].fov.angle_down,
        };

        // Calculate IPD from view positions
        let left_pos = view_configs[0].position;
        let right_pos = view_configs[1].position;
        let ipd = (right_pos - left_pos).length();

        // Update camera stereo configuration
        camera.stereo_config = Some(crate::camera::StereoConfig {
            ipd,
            left_fov,
            right_fov,
        });

        // Update camera position from head pose (use average of both eyes)
        let head_position = (left_pos + right_pos) * 0.5;

        // Keep the current target, just update position to maintain viewing direction
        let current_forward = (camera.target - camera.position).normalize();
        let distance = camera.position.distance(camera.target);
        camera.position = head_position;
        camera.target = head_position + current_forward * distance;

        Ok(())
    }

    /// Render stereo frame for VR
    pub fn render_stereo<F>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        mut render_eye: F,
    ) -> Result<()>
    where
        F: FnMut(&wgpu::RenderPass, Eye),
    {
        // Update performance monitoring
        self.performance_monitor.tick();

        // Check for performance warnings (every 90 frames = ~1 second at 90 FPS)
        let stats = self.performance_monitor.stats();
        if stats.total_frames > self.last_warning_frame + 90 {
            if let Some(warning) = self.performance_monitor.get_warning() {
                warn!("{}", warning);
                self.last_warning_frame = stats.total_frames;
            }
        }

        // Acquire swapchain images
        let left_swapchain = self
            .vr_session
            .left_swapchain
            .as_mut()
            .context("Left swapchain not available")?;
        let right_swapchain = self
            .vr_session
            .right_swapchain
            .as_mut()
            .context("Right swapchain not available")?;

        let left_image_index = left_swapchain.acquire_image()?;
        let right_image_index = right_swapchain.acquire_image()?;

        // Create command encoder
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("VR Render Encoder"),
        });

        // Update camera uniforms for each eye
        let left_uniform = camera.uniform_stereo(Eye::Left);
        let right_uniform = camera.uniform_stereo(Eye::Right);

        queue.write_buffer(
            &self.left_camera_buffer,
            0,
            bytemuck::cast_slice(&[left_uniform]),
        );
        queue.write_buffer(
            &self.right_camera_buffer,
            0,
            bytemuck::cast_slice(&[right_uniform]),
        );

        // LEFT EYE RENDER PASS
        {
            let color_view = &left_swapchain.views[left_image_index];

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("VR Left Eye Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.02,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.left_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_bind_group(0, &self.left_camera_bind_group, &[]);
            render_eye(&render_pass, Eye::Left);
        }

        // RIGHT EYE RENDER PASS
        {
            let color_view = &right_swapchain.views[right_image_index];

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("VR Right Eye Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.02,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.right_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_bind_group(0, &self.right_camera_bind_group, &[]);
            render_eye(&render_pass, Eye::Right);
        }

        // Submit commands
        queue.submit(std::iter::once(encoder.finish()));

        // Release swapchain images
        self.vr_session.left_swapchain.as_mut().unwrap().release_image()?;
        self.vr_session.right_swapchain.as_mut().unwrap().release_image()?;

        Ok(())
    }

    /// Build a `CameraUniform` for one eye directly from an OpenXR view (6DoF).
    ///
    /// `mol_to_world` converts molecule coordinates (Angstroms, origin-centered) to
    /// VR stage space (meters). A good default:
    ///   `Mat4::from_scale_rotation_translation(Vec3::splat(0.002), Quat::IDENTITY, Vec3::new(0.0, 1.4, -1.5))`
    /// This scales 1 Å → 2 mm and places the molecule at eye height 1.4 m, 1.5 m ahead.
    ///
    /// `near`/`far` are in VR stage space (meters), e.g. 0.02 and 100.0.
    pub fn build_eye_uniform(
        view: &mol_vr::session::ViewConfig,
        near: f32,
        far: f32,
        mol_to_world: Mat4,
    ) -> crate::camera::CameraUniform {
        // Eye pose → world-from-eye transform
        let world_from_eye =
            Mat4::from_rotation_translation(view.orientation, view.position);
        // Invert: this transforms from world (stage) space into eye (view) space
        let eye_from_world = world_from_eye.inverse();

        // Full view: first bring molecule into stage space, then into eye space
        let view_matrix = eye_from_world * mol_to_world;

        // Asymmetric projection from OpenXR FOV angles (same formula as existing camera code)
        let l = near * view.fov.angle_left.tan();
        let r = near * view.fov.angle_right.tan();
        let b = near * view.fov.angle_down.tan();
        let t = near * view.fov.angle_up.tan();
        let w = r - l;
        let h = t - b;
        let d = far - near;
        let proj_matrix = Mat4::from_cols(
            Vec4::new(2.0 * near / w, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 * near / h, 0.0, 0.0),
            Vec4::new(
                (r + l) / w,
                (t + b) / h,
                -(far + near) / d,
                -1.0,
            ),
            Vec4::new(0.0, 0.0, -2.0 * far * near / d, 0.0),
        );

        let view_proj = proj_matrix * view_matrix;

        crate::camera::CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            view: view_matrix.to_cols_array_2d(),
            proj: proj_matrix.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            view_pos: view.position.into(),
            _padding: 0.0,
        }
    }

    /// Poll VR events and update session state
    pub fn poll_events(&mut self) -> Result<bool> {
        self.vr_session.poll_events()
    }

    /// Begin a VR frame
    pub fn begin_frame(&mut self) -> Result<openxr::FrameState> {
        self.vr_session.begin_frame()
    }

    /// End a VR frame and submit to compositor
    pub fn end_frame(&mut self, frame_state: &openxr::FrameState) -> Result<()> {
        self.vr_session.end_frame(frame_state)
    }

    /// Get current VR performance statistics
    pub fn performance_stats(&self) -> &mol_vr::VrPerformanceStats {
        self.performance_monitor.stats()
    }

    /// Check if VR performance is acceptable (>95% frames meeting 90 FPS target)
    pub fn is_performance_acceptable(&self) -> bool {
        self.performance_monitor.is_performance_acceptable()
    }
}
