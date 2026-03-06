use crate::benchmark::{BenchmarkStats, BenchmarkTimer};
use crate::camera::Camera;
use crate::representations::{AxesRenderer, BallStickRenderer, BillboardRenderer, RepresentationType, SpheresRenderer, RibbonRenderer, SurfaceRenderer, SurfaceRaymarchRenderer, SphereRaymarchRenderer, RaymarchConfig};
use crate::representations::spheres::SphereInstance;
use crate::representations::billboards::BillboardInstance;
use crate::lod::{LodSystem, LodGroups, LodLevel};
use crate::culling::CullingSystem;
use anyhow::{Context, Result};
use glam::Vec3;
use wgpu::util::DeviceExt;
use std::collections::HashMap;
use bytemuck::{Pod, Zeroable};

// GPU compute shader data structures
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AtomDataGPU {
    position: [f32; 3],
    radius: f32,
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrustumGPU {
    planes: [[f32; 4]; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraDataGPU {
    position: [f32; 3],
    _padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LodConfigGPU {
    distance_high: f32,
    distance_medium: f32,
    distance_low: f32,
    distance_very_low: f32,
    hysteresis: f32,
    _padding: [f32; 3],
}

// DrawIndexedIndirect command structure
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawIndexedIndirectCommand {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

// Cylinder data for GPU compute (bonds in Ball-and-Stick)
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CylinderDataGPU {
    start: [f32; 3],
    radius: f32,
    end: [f32; 3],
    _padding: f32,
    color: [f32; 4],
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,

    camera_buffer: wgpu::Buffer,
    pub camera_bind_group: wgpu::BindGroup,
    camera_bind_group_layout: wgpu::BindGroupLayout,

    depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,

    pub camera: Camera,

    // Representation renderers (spheres always needed, others lazy-loaded)
    pub spheres_renderer: SpheresRenderer,
    pub ball_stick_renderer: Option<BallStickRenderer>,
    pub ribbon_renderer: Option<RibbonRenderer>,
    pub surface_renderer: Option<SurfaceRenderer>,
    pub surface_raymarch_renderer: Option<SurfaceRaymarchRenderer>,
    pub sphere_raymarch_renderer: Option<SphereRaymarchRenderer>,
    pub axes_renderer: AxesRenderer,

    // Multi-LOD sphere renderers (CPU path)
    spheres_high: SpheresRenderer,      // subdivision 3 (512 tris)
    spheres_medium: SpheresRenderer,    // subdivision 2 (128 tris)
    spheres_low: SpheresRenderer,       // subdivision 1 (32 tris)
    spheres_very_low: SpheresRenderer,  // subdivision 0 (20 tris)
    billboard_impostor: BillboardRenderer, // billboards for very distant atoms

    // GPU-driven sphere renderers (Phase 3.4)
    gpu_spheres_high: Option<SpheresRenderer>,
    gpu_spheres_medium: Option<SpheresRenderer>,
    gpu_spheres_low: Option<SpheresRenderer>,
    gpu_spheres_very_low: Option<SpheresRenderer>,

    // VR renderers compiled with the VR swapchain format (Rgba8UnormSrgb).
    // Each mirrors its desktop counterpart but targets the correct swapchain format.
    pub vr_spheres_renderer: Option<SpheresRenderer>,
    pub vr_ribbon_renderer: Option<RibbonRenderer>,
    pub vr_ball_stick_renderer: Option<BallStickRenderer>,
    pub vr_surface_renderer: Option<SurfaceRenderer>,

    // LOD and culling systems
    lod_system: LodSystem,
    lod_groups: LodGroups,
    culling_system: CullingSystem,
    lod_previous: HashMap<usize, LodLevel>, // Track previous LODs for hysteresis

    // GPU compute support
    gpu_compute_enabled: bool,

    // GPU compute culling infrastructure (Phase 3.2)
    compute_pipeline: Option<wgpu::ComputePipeline>,
    compute_bind_group_layout: Option<wgpu::BindGroupLayout>,
    compute_bind_group: Option<wgpu::BindGroup>,

    // Compute buffers
    atom_data_buffer: Option<wgpu::Buffer>,
    frustum_buffer: Option<wgpu::Buffer>,
    camera_position_buffer: Option<wgpu::Buffer>,
    lod_config_buffer: Option<wgpu::Buffer>,
    draw_commands_buffer: Option<wgpu::Buffer>,
    visible_indices_buffers: Option<Vec<wgpu::Buffer>>, // 5 buffers for 5 LOD levels
    atom_count: usize,

    // GPU-driven rendering infrastructure (Phase 3.4)
    gpu_render_bind_group_layout: Option<wgpu::BindGroupLayout>,
    gpu_render_bind_groups: Option<Vec<wgpu::BindGroup>>, // 5 bind groups, one per LOD

    // Current representation mode
    pub representation: RepresentationType,

    // Track which geometries have been loaded (to avoid regenerating)
    ball_stick_loaded: bool,
    ribbon_loaded: bool,
    surface_loaded: bool,
    surface_raymarch_loaded: bool,
    sphere_raymarch_loaded: bool,
    // Track which protein (by atom count) was used to generate each geometry
    ball_stick_atom_count: usize,
    ribbon_atom_count: usize,
    surface_atom_count: usize,
    surface_raymarch_atom_count: usize,
    sphere_raymarch_atom_count: usize,

    // Benchmarking stats
    pub benchmark_stats: BenchmarkStats,

    // VR support (optional)
    pub vr_renderer: Option<crate::vr_renderer::VrRenderer>,
}

impl Renderer {
    pub async fn new(window: std::sync::Arc<winit::window::Window>, vr_mode: bool) -> Result<Self> {
        let size = window.inner_size();

        // ── VR path: create Vulkan + OpenXR session first, then wrap in wgpu ─────
        // This order is REQUIRED: the OpenXR runtime (Oculus) must inject its required
        // Vulkan extensions (VK_KHR_external_semaphore_win32 etc.) before the VkDevice
        // is created, otherwise xrCreateSession crashes in the runtime DLL.
        let (instance, adapter, device, queue, initial_vr_session) = if vr_mode {
            // Create VrSession (also creates VkInstance + VkDevice with OpenXR extensions)
            let (vr_session, vk_ctx) = mol_vr::VrSession::new()
                .context("Failed to create OpenXR VR session")?;

            // Wrap the VkInstance in wgpu::hal
            let hal_instance = unsafe {
                wgpu::hal::vulkan::Instance::from_raw(
                    vk_ctx.entry,
                    vk_ctx.instance,
                    vk_ctx.api_version,
                    0, // android_sdk_version
                    None, // debug_utils
                    vk_ctx.instance_extensions,
                    wgpu::InstanceFlags::empty(),
                    false, // has_nv_optimus
                    None, // drop_guard
                )
                .context("Failed to wrap VkInstance in wgpu HAL")?
            };

            // Create wgpu::Instance from the HAL instance
            let wgpu_instance = unsafe {
                wgpu::Instance::from_hal::<wgpu::hal::api::Vulkan>(hal_instance)
            };

            // Expose the physical device as a wgpu Adapter
            let hal_adapter = unsafe {
                wgpu_instance
                    .as_hal::<wgpu::hal::api::Vulkan>()
                    .and_then(|i| i.expose_adapter(vk_ctx.physical_device))
                    .ok_or_else(|| anyhow::anyhow!("Failed to expose VkPhysicalDevice as wgpu adapter"))?
            };
            let wgpu_adapter = unsafe {
                wgpu_instance.create_adapter_from_hal(hal_adapter)
            };

            // Check features available on this adapter
            let adapter_features = wgpu_adapter.features();
            let supports_compute =
                adapter_features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
            let mut features = wgpu::Features::empty();
            if supports_compute {
                features |= wgpu::Features::INDIRECT_FIRST_INSTANCE;
            }

            // Create the wgpu::Device from the pre-created VkDevice
            let open_device = unsafe {
                wgpu_adapter.as_hal::<wgpu::hal::api::Vulkan, _, Result<_, _>>(|hal_adapter| {
                    hal_adapter
                        .ok_or_else(|| anyhow::anyhow!("Adapter is not Vulkan"))?
                        .device_from_raw(
                            vk_ctx.device,
                            true, // handle_is_owned (we own the VkDevice)
                            &vk_ctx.device_extensions,
                            features,
                            &wgpu::MemoryHints::default(),
                            vk_ctx.queue_family_index,
                            0, // queue_index
                        )
                        .map_err(|e| anyhow::anyhow!("device_from_raw failed: {:?}", e))
                })
            }
            .context("Failed to create wgpu device from raw VkDevice")?;

            let (wgpu_device, wgpu_queue) = unsafe {
                wgpu_adapter.create_device_from_hal(
                    open_device,
                    &wgpu::DeviceDescriptor {
                        label: Some("VR Device"),
                        required_features: features,
                        required_limits: wgpu::Limits::default(),
                        memory_hints: Default::default(),
                    },
                    None,
                )
            }
            .map_err(|e| anyhow::anyhow!("create_device_from_hal failed: {}", e))?;

            log::info!("GPU Features detected (VR mode):");
            log::info!("  Indirect drawing: {}", supports_compute);
            if supports_compute {
                log::info!("  GPU compute culling: ENABLED");
            } else {
                log::warn!("  GPU compute culling: NOT SUPPORTED - using CPU fallback");
            }

            (wgpu_instance, wgpu_adapter, wgpu_device, wgpu_queue, Some(vr_session))
        } else {
            // ── Non-VR path: normal wgpu initialization ───────────────────────────
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..Default::default()
            });

            let surface = instance.create_surface(window.clone())?;

            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .ok_or_else(|| anyhow::anyhow!("Failed to find an appropriate adapter"))?;

            let adapter_features = adapter.features();
            let supports_compute =
                adapter_features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
            let mut features = wgpu::Features::empty();
            if supports_compute {
                features |= wgpu::Features::INDIRECT_FIRST_INSTANCE;
            }

            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Device"),
                        required_features: features,
                        required_limits: wgpu::Limits::default(),
                        memory_hints: Default::default(),
                    },
                    None,
                )
                .await?;

            log::info!("GPU Features detected:");
            log::info!("  Indirect drawing: {}", supports_compute);
            if supports_compute {
                log::info!("  GPU compute culling: ENABLED");
            } else {
                log::warn!("  GPU compute culling: NOT SUPPORTED - using CPU fallback");
            }

            (instance, adapter, device, queue, None)
        };

        // Derive gpu_compute_enabled from the device's actual features
        let gpu_compute_enabled = device
            .features()
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);

        // Create surface (works for both VR and non-VR since the instance has the win32 extension)
        let surface = instance.create_surface(window.clone())?;

        // Configure surface
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        // Create camera
        let aspect = size.width as f32 / size.height as f32;
        let camera = Camera::new(aspect);

        // Create camera buffer and bind group layout
        let camera_uniform = camera.uniform();
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Camera Bind Group Layout"),
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

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // Create depth texture
        let (depth_texture, depth_view) = Self::create_depth_texture(&device, &config);

        // Create only essential renderers at startup (lazy-load others on demand)
        log::info!("Creating essential renderers...");

        let spheres_renderer =
            SpheresRenderer::new(&device, surface_format, &camera_bind_group_layout);

        // Create axes renderer for coordinate system visualization
        let axes_renderer =
            AxesRenderer::new(&device, surface_format, &camera_bind_group_layout, 50.0);

        // Ball-stick, ribbon, and surface renderers will be lazy-loaded when first needed
        // This significantly speeds up startup time

        // Multi-LOD sphere renderers - only create if GPU compute NOT available
        // (if GPU available, we'll create GPU-driven renderers instead)
        let (spheres_high, spheres_medium, spheres_low, spheres_very_low, billboard_impostor) =
            if !gpu_compute_enabled {
                log::info!("GPU compute not available - creating CPU multi-LOD renderers...");
                (
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 3),
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 2),
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 1),
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 0),
                    BillboardRenderer::new(&device, surface_format, &camera_bind_group_layout),
                )
            } else {
                log::info!("GPU compute available - skipping CPU multi-LOD renderers (will use GPU path)...");
                // Create minimal dummy renderers (subdiv 0 = only 20 tris each, very fast)
                // These won't be used for Van der Waals mode when GPU is available
                (
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 0),
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 0),
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 0),
                    SpheresRenderer::new_with_subdivision(&device, surface_format, &camera_bind_group_layout, 0),
                    BillboardRenderer::new(&device, surface_format, &camera_bind_group_layout),
                )
            };

        // Initialize LOD and culling systems
        let lod_system = LodSystem::with_default_config();
        let lod_groups = LodGroups::new();
        let culling_system = CullingSystem::new();
        let lod_previous = HashMap::new();

        log::info!("Renderer initialized successfully");
        log::info!("  Surface format: {:?}", surface_format);
        log::info!("  Size: {}x{}", size.width, size.height);
        log::info!("  Multi-LOD rendering enabled");

        let mut renderer = Self {
            device,
            queue,
            surface,
            config,
            size,
            camera_buffer,
            camera_bind_group,
            camera_bind_group_layout,
            depth_texture,
            depth_view,
            camera,
            spheres_renderer,
            ball_stick_renderer: None,  // Lazy-loaded
            ribbon_renderer: None,      // Lazy-loaded
            surface_renderer: None,     // Lazy-loaded
            surface_raymarch_renderer: None, // Lazy-loaded
            sphere_raymarch_renderer: None,  // Lazy-loaded
            axes_renderer,
            spheres_high,
            spheres_medium,
            spheres_low,
            spheres_very_low,
            billboard_impostor,
            gpu_spheres_high: None,
            gpu_spheres_medium: None,
            gpu_spheres_low: None,
            gpu_spheres_very_low: None,
            lod_system,
            lod_groups,
            culling_system,
            lod_previous,
            gpu_compute_enabled,
            compute_pipeline: None,
            compute_bind_group_layout: None,
            compute_bind_group: None,
            atom_data_buffer: None,
            frustum_buffer: None,
            camera_position_buffer: None,
            lod_config_buffer: None,
            draw_commands_buffer: None,
            visible_indices_buffers: None,
            atom_count: 0,
            gpu_render_bind_group_layout: None,
            gpu_render_bind_groups: None,
            representation: RepresentationType::VanDerWaals,
            ball_stick_loaded: false,
            ribbon_loaded: false,
            surface_loaded: false,
            surface_raymarch_loaded: false,
            sphere_raymarch_loaded: false,
            ball_stick_atom_count: 0,
            ribbon_atom_count: 0,
            surface_atom_count: 0,
            surface_raymarch_atom_count: 0,
            sphere_raymarch_atom_count: 0,

            benchmark_stats: BenchmarkStats::new(),

            vr_renderer: None, // initialized below if vr_mode
            vr_spheres_renderer: None,
            vr_ribbon_renderer: None,
            vr_ball_stick_renderer: None,
            vr_surface_renderer: None,
        };

        // Initialize VR renderer now that the camera bind group layout is stored in the struct
        if let Some(vr_session) = initial_vr_session {
            match crate::vr_renderer::VrRenderer::new(
                vr_session,
                &renderer.device,
                &renderer.camera_bind_group_layout,
            ) {
                Ok(vr) => {
                    log::info!("VR renderer initialized successfully");
                    // Create VR-specific sphere renderer compiled with the VR swapchain format.
                    // The desktop sphere renderer uses Bgra8UnormSrgb; VR swapchain uses Rgba8UnormSrgb.
                    // Using the wrong format causes a pipeline validation error at render time.
                    let vr_fmt = vr.swapchain_format;
                    renderer.vr_spheres_renderer = Some(SpheresRenderer::new(
                        &renderer.device,
                        vr_fmt,
                        &renderer.camera_bind_group_layout,
                    ));
                    renderer.vr_ribbon_renderer = Some(RibbonRenderer::new(
                        &renderer.device,
                        vr_fmt,
                        &renderer.camera_bind_group_layout,
                    ));
                    renderer.vr_ball_stick_renderer = Some(BallStickRenderer::new(
                        &renderer.device,
                        vr_fmt,
                        &renderer.camera_bind_group_layout,
                    ));
                    renderer.vr_surface_renderer = Some(SurfaceRenderer::new(
                        &renderer.device,
                        vr_fmt,
                        &renderer.camera_bind_group_layout,
                    ));
                    renderer.vr_renderer = Some(vr);
                }
                Err(e) => {
                    log::error!("Failed to initialize VR renderer: {}", e);
                    return Err(e);
                }
            }
        }

        // Initialize benchmark stats
        renderer.benchmark_stats.gpu_enabled = gpu_compute_enabled;

        // Initialize GPU compute pipeline if supported
        if gpu_compute_enabled {
            log::info!("Initializing GPU compute culling pipeline...");
            renderer.init_compute_pipeline();
        }

        Ok(renderer)
    }

    fn create_depth_texture(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let size = wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        };

        let desc = wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let texture = device.create_texture(&desc);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        (texture, view)
    }

    fn init_compute_pipeline(&mut self) {
        // Load compute shader
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Culling Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../assets/shaders/culling.wgsl").into(),
            ),
        });

        // Create bind group layout for compute shader
        let compute_bind_group_layout =
            self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Compute Bind Group Layout"),
                entries: &[
                    // @binding(0): atoms storage buffer (read)
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(1): frustum uniform
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(2): camera position uniform
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(3): LOD config uniform
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(4): draw commands buffer (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(5): visible_indices_high (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(6): visible_indices_medium (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(7): visible_indices_low (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(8): visible_indices_very_low (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(9): visible_indices_impostor (read_write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        // Create compute pipeline
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Compute Pipeline Layout"),
            bind_group_layouts: &[&compute_bind_group_layout],
            push_constant_ranges: &[],
        });

        let compute_pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Culling Compute Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });

        self.compute_pipeline = Some(compute_pipeline);
        self.compute_bind_group_layout = Some(compute_bind_group_layout);

        // Create bind group layout for GPU-driven rendering (Phase 3.4)
        let gpu_render_bind_group_layout =
            self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("GPU Render Bind Group Layout"),
                entries: &[
                    // @binding(0): atoms storage buffer (read)
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // @binding(1): visible_indices storage buffer (read)
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        self.gpu_render_bind_group_layout = Some(gpu_render_bind_group_layout);

        // Create GPU-driven sphere renderers
        log::info!("Creating GPU-driven sphere renderers...");

        let gpu_spheres_high = SpheresRenderer::new_gpu_driven(
            &self.device,
            self.config.format,
            &self.camera_bind_group_layout,
            self.gpu_render_bind_group_layout.as_ref().unwrap(),
            3, // High detail
        );

        let gpu_spheres_medium = SpheresRenderer::new_gpu_driven(
            &self.device,
            self.config.format,
            &self.camera_bind_group_layout,
            self.gpu_render_bind_group_layout.as_ref().unwrap(),
            2, // Medium detail
        );

        let gpu_spheres_low = SpheresRenderer::new_gpu_driven(
            &self.device,
            self.config.format,
            &self.camera_bind_group_layout,
            self.gpu_render_bind_group_layout.as_ref().unwrap(),
            1, // Low detail
        );

        let gpu_spheres_very_low = SpheresRenderer::new_gpu_driven(
            &self.device,
            self.config.format,
            &self.camera_bind_group_layout,
            self.gpu_render_bind_group_layout.as_ref().unwrap(),
            0, // Very low detail
        );

        self.gpu_spheres_high = Some(gpu_spheres_high);
        self.gpu_spheres_medium = Some(gpu_spheres_medium);
        self.gpu_spheres_low = Some(gpu_spheres_low);
        self.gpu_spheres_very_low = Some(gpu_spheres_very_low);

        log::info!("GPU compute culling pipeline initialized successfully");
        log::info!("GPU-driven rendering enabled");
    }

    fn init_compute_buffers(&mut self, protein: &pdb_parser::Protein) {
        if !self.gpu_compute_enabled {
            return;
        }

        self.atom_count = protein.atoms.len();
        log::info!("Initializing GPU compute buffers for {} atoms", self.atom_count);

        // 1. Create atom data buffer
        let atom_data: Vec<AtomDataGPU> = protein.atoms.iter()
            .map(|atom| AtomDataGPU {
                position: atom.position.into(),
                radius: atom.element.vdw_radius(),
                color: atom.element.cpk_color(),
            })
            .collect();

        let atom_data_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Atom Data Buffer"),
            contents: bytemuck::cast_slice(&atom_data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        // 2. Create frustum uniform buffer (will be updated each frame)
        let frustum_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frustum Uniform Buffer"),
            size: std::mem::size_of::<FrustumGPU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 3. Create camera position uniform buffer (will be updated each frame)
        let camera_position_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Position Buffer"),
            size: std::mem::size_of::<CameraDataGPU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 4. Create LOD config uniform buffer
        let config = self.lod_system.get_config();
        let lod_config = LodConfigGPU {
            distance_high: config.distance_high,
            distance_medium: config.distance_medium,
            distance_low: config.distance_low,
            distance_very_low: config.distance_very_low,
            hysteresis: config.hysteresis,
            _padding: [0.0; 3],
        };

        let lod_config_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("LOD Config Buffer"),
            contents: bytemuck::cast_slice(&[lod_config]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // 5. Create draw commands buffer (5 commands: high, medium, low, very_low, impostor)
        // Initialize with index counts from geometry, instance_count will be filled by compute shader
        let draw_commands = vec![
            // High (subdiv 3)
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_high { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Medium (subdiv 2)
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_medium { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Low (subdiv 1)
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_low { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Very Low (subdiv 0)
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_very_low { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Impostor (billboards) - not implemented yet for GPU path
            DrawIndexedIndirectCommand {
                index_count: 0,
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
        ];

        let draw_commands_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Draw Commands Buffer"),
            contents: bytemuck::cast_slice(&draw_commands),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });

        // 6. Create visible indices buffers (5 buffers, one per LOD level)
        let visible_indices_buffers: Vec<wgpu::Buffer> = (0..5)
            .map(|i| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("Visible Indices Buffer {}", i)),
                    size: (self.atom_count * std::mem::size_of::<u32>()) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();

        // 7. Create bind group
        let compute_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Bind Group"),
            layout: self.compute_bind_group_layout.as_ref().unwrap(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: atom_data_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: frustum_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: camera_position_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: lod_config_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: draw_commands_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: visible_indices_buffers[0].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: visible_indices_buffers[1].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: visible_indices_buffers[2].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: visible_indices_buffers[3].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: visible_indices_buffers[4].as_entire_binding(),
                },
            ],
        });

        // 8. Create GPU-driven rendering bind groups (one per LOD level)
        let gpu_render_bind_groups: Vec<wgpu::BindGroup> = (0..5)
            .map(|i| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("GPU Render Bind Group LOD {}", i)),
                    layout: self.gpu_render_bind_group_layout.as_ref().unwrap(),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: atom_data_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: visible_indices_buffers[i].as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();

        // Store all buffers
        self.atom_data_buffer = Some(atom_data_buffer);
        self.frustum_buffer = Some(frustum_buffer);
        self.camera_position_buffer = Some(camera_position_buffer);
        self.lod_config_buffer = Some(lod_config_buffer);
        self.draw_commands_buffer = Some(draw_commands_buffer);
        self.visible_indices_buffers = Some(visible_indices_buffers);
        self.compute_bind_group = Some(compute_bind_group);
        self.gpu_render_bind_groups = Some(gpu_render_bind_groups);

        log::info!("GPU compute buffers initialized successfully");
        log::info!("GPU-driven rendering bind groups created");
    }

    /// Dispatch GPU compute shader for frustum culling and LOD assignment
    fn dispatch_compute_culling(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.gpu_compute_enabled || self.compute_pipeline.is_none() || self.atom_count == 0 {
            return;
        }

        // Start GPU benchmark timer
        let timer = BenchmarkTimer::start();

        // Update per-frame uniforms

        // 1. Update frustum uniform
        let view_proj = self.camera.view_projection();
        self.culling_system.update(view_proj);

        let frustum = self.culling_system.get_frustum();
        let frustum_data = FrustumGPU {
            planes: [
                [frustum.planes[0].normal.x, frustum.planes[0].normal.y, frustum.planes[0].normal.z, frustum.planes[0].distance],
                [frustum.planes[1].normal.x, frustum.planes[1].normal.y, frustum.planes[1].normal.z, frustum.planes[1].distance],
                [frustum.planes[2].normal.x, frustum.planes[2].normal.y, frustum.planes[2].normal.z, frustum.planes[2].distance],
                [frustum.planes[3].normal.x, frustum.planes[3].normal.y, frustum.planes[3].normal.z, frustum.planes[3].distance],
                [frustum.planes[4].normal.x, frustum.planes[4].normal.y, frustum.planes[4].normal.z, frustum.planes[4].distance],
                [frustum.planes[5].normal.x, frustum.planes[5].normal.y, frustum.planes[5].normal.z, frustum.planes[5].distance],
            ],
        };

        if let Some(ref buffer) = self.frustum_buffer {
            self.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[frustum_data]));
        }

        // 2. Update camera position uniform
        let camera_data = CameraDataGPU {
            position: self.camera.position.into(),
            _padding: 0.0,
        };

        if let Some(ref buffer) = self.camera_position_buffer {
            self.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[camera_data]));
        }

        // 3. Reset draw commands instance counts to zero (preserve index_count)
        let reset_commands = vec![
            // High
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_high { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Medium
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_medium { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Low
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_low { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Very Low
            DrawIndexedIndirectCommand {
                index_count: if let Some(ref r) = self.gpu_spheres_very_low { r.index_count } else { 0 },
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
            // Impostor
            DrawIndexedIndirectCommand {
                index_count: 0,
                instance_count: 0,
                first_index: 0,
                base_vertex: 0,
                first_instance: 0,
            },
        ];

        if let Some(ref buffer) = self.draw_commands_buffer {
            self.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&reset_commands));
        }

        // 4. Dispatch compute shader
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Culling Compute Pass"),
                timestamp_writes: None,
            });

            if let Some(ref pipeline) = self.compute_pipeline {
                compute_pass.set_pipeline(pipeline);
            }

            if let Some(ref bind_group) = self.compute_bind_group {
                compute_pass.set_bind_group(0, bind_group, &[]);
            }

            // Dispatch with workgroup size of 256 (from shader @workgroup_size(256))
            let workgroup_count = (self.atom_count as u32 + 255) / 256;
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        // Update GPU benchmark stats
        // Note: This measures CPU dispatch time, not actual GPU execution time
        // Real GPU time would require timestamp queries (more complex)
        self.benchmark_stats.gpu_compute_time_us = timer.elapsed_us();

        log::trace!("Dispatched GPU compute culling for {} atoms ({} workgroups), dispatch time={:.2}µs",
            self.atom_count, (self.atom_count as u32 + 255) / 256, self.benchmark_stats.gpu_compute_time_us);
    }

    /// Get current benchmark statistics
    pub fn get_benchmark_stats(&self) -> &BenchmarkStats {
        &self.benchmark_stats
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);

            // Update camera aspect ratio
            self.camera.aspect = new_size.width as f32 / new_size.height as f32;

            // Recreate depth texture
            (self.depth_texture, self.depth_view) =
                Self::create_depth_texture(&self.device, &self.config);
        }
    }

    pub fn update(&mut self) {
        // Update camera uniform buffer
        let camera_uniform = self.camera.uniform();
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[camera_uniform]));
    }

    pub fn load_protein(&mut self, protein: &pdb_parser::Protein) {
        // Skip if protein is already loaded (same atom count)
        // This prevents per-frame re-initialization in the render loop
        if protein.atoms.len() == self.atom_count && self.atom_count > 0 {
            return;
        }

        log::info!("Loading protein with {} atoms", protein.atoms.len());

        // Generate sphere instances for Van der Waals
        let sphere_instances: Vec<SphereInstance> = protein
            .atoms
            .iter()
            .map(|atom| {
                SphereInstance::new(
                    atom.position,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.spheres_renderer
            .update_instances(&self.queue, &sphere_instances);

        if let Some(ref mut vr_sph) = self.vr_spheres_renderer {
            vr_sph.update_instances(&self.queue, &sphere_instances);
        }

        // Ball-stick, ribbon, and surface geometries are now lazy-loaded
        // They will be generated when the user switches to those representation modes
        // This significantly speeds up protein loading

        // NOTE: Camera positioning is now handled by the App layer (focus_on_model, frame_all, etc.)
        // We don't auto-center the camera here to allow multi-model rendering without camera jumps

        // Initialize GPU compute buffers if available
        self.init_compute_buffers(protein);

        // Update atom_count to track which protein is loaded
        self.atom_count = protein.atoms.len();

        let (min, max): (Vec3, Vec3) = protein.bounding_box();
        let size = (max - min).length();

        log::info!("Protein loaded successfully");
        log::info!("  Center: {:?}", protein.center());
        log::info!("  Bounds: {:?} to {:?}", min, max);
        log::info!("  Size: {:.2} Å", size);
    }

    /// Update atom positions for animation frames
    /// Takes new coordinates and updates all loaded representations
    /// Note: topology (elements, colors, radii) must remain unchanged
    pub fn update_atom_positions(&mut self, protein: &pdb_parser::Protein, coords: &[Vec3]) {
        if coords.len() != protein.atoms.len() {
            log::warn!("Coordinate count mismatch: {} vs {}", coords.len(), protein.atoms.len());
            return;
        }

        // Update Van der Waals sphere positions
        let sphere_instances: Vec<SphereInstance> = protein
            .atoms
            .iter()
            .zip(coords.iter())
            .map(|(atom, &pos)| {
                SphereInstance::new(
                    pos,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.spheres_renderer.update_instances(&self.queue, &sphere_instances);

        // Update all LOD levels for CPU path
        self.spheres_high.update_instances(&self.queue, &sphere_instances);
        self.spheres_medium.update_instances(&self.queue, &sphere_instances);
        self.spheres_low.update_instances(&self.queue, &sphere_instances);
        self.spheres_very_low.update_instances(&self.queue, &sphere_instances);

        // Keep VR sphere renderer in sync (same instances, different pipeline format)
        if let Some(ref mut vr_sph) = self.vr_spheres_renderer {
            vr_sph.update_instances(&self.queue, &sphere_instances);
        }

        // TODO: Update Ball-and-Stick, Ribbon, and Surface if loaded
        // For now, animation only works well with Van der Waals representation
    }

    /// Update rendering to highlight selected atoms
    pub fn update_selection(
        &mut self,
        protein: &pdb_parser::Protein,
        selected: &std::collections::HashSet<usize>,
    ) {
        if selected.is_empty() {
            return;
        }

        // Regenerate Van der Waals instances with highlighted selection
        let sphere_instances: Vec<SphereInstance> = protein
            .atoms
            .iter()
            .enumerate()
            .map(|(idx, atom)| {
                let mut color = atom.element.cpk_color();

                // Highlight selected atoms by brightening them
                if selected.contains(&idx) {
                    // Mix with white to brighten (50% blend)
                    color[0] = (color[0] * 0.5 + 0.5).min(1.0);
                    color[1] = (color[1] * 0.5 + 0.5).min(1.0);
                    color[2] = (color[2] * 0.5 + 0.5).min(1.0);
                }

                SphereInstance::new(atom.position, atom.element.vdw_radius(), color)
            })
            .collect();

        self.spheres_renderer
            .update_instances(&self.queue, &sphere_instances);

        // Regenerate Ball-and-Stick instances with highlighted selection
        let ball_instances: Vec<SphereInstance> = protein
            .atoms
            .iter()
            .enumerate()
            .map(|(idx, atom)| {
                let mut color = atom.element.cpk_color();

                // Highlight selected atoms
                if selected.contains(&idx) {
                    color[0] = (color[0] * 0.5 + 0.5).min(1.0);
                    color[1] = (color[1] * 0.5 + 0.5).min(1.0);
                    color[2] = (color[2] * 0.5 + 0.5).min(1.0);
                }

                SphereInstance::new(atom.position, 0.3, color)
            })
            .collect();

        // Only update if ball-stick renderer has been created
        if let Some(ref mut renderer) = self.ball_stick_renderer {
            renderer.update_spheres(&self.queue, &ball_instances);
        }

        // Note: Bonds are not highlighted for now
        // Could potentially highlight bonds between selected atoms
    }

    /// Ensure ball-stick renderer is created and has data
    pub fn ensure_ball_stick(&mut self, protein: &pdb_parser::Protein) {
        // Check if already loaded for THIS protein (by atom count)
        let current_atom_count = protein.atoms.len();
        if self.ball_stick_loaded && self.ball_stick_atom_count == current_atom_count {
            log::debug!("Ball-stick geometry already loaded for this protein, skipping regeneration");
            return;
        }

        // If loaded but for different protein, log that we're regenerating
        if self.ball_stick_loaded && self.ball_stick_atom_count != current_atom_count {
            log::info!("Ball-stick geometry loaded for different protein ({} atoms), regenerating for {} atoms",
                self.ball_stick_atom_count, current_atom_count);
        }

        // Create renderer if needed
        if self.ball_stick_renderer.is_none() {
            log::info!("Creating ball-stick renderer...");
            self.ball_stick_renderer = Some(BallStickRenderer::new(
                &self.device,
                self.config.format,
                &self.camera_bind_group_layout,
            ));
        }

        // Generate and load ball-stick data
        log::info!("Generating ball-stick geometry for {} atoms...", protein.atoms.len());

        let ball_instances: Vec<SphereInstance> = protein
            .atoms
            .iter()
            .map(|atom| {
                SphereInstance::new(
                    atom.position,
                    0.3, // Fixed small radius for ball-and-stick
                    atom.element.cpk_color(),
                )
            })
            .collect();

        use crate::representations::ball_stick::CylinderInstance;
        let cylinder_instances: Vec<CylinderInstance> = protein
            .bonds
            .iter()
            .map(|bond| {
                let atom1 = &protein.atoms[bond.atom1];
                let atom2 = &protein.atoms[bond.atom2];
                CylinderInstance::new(
                    atom1.position,
                    atom2.position,
                    0.15, // Bond radius
                    [0.7, 0.7, 0.7, 1.0],
                )
            })
            .collect();

        if let Some(ref mut renderer) = self.ball_stick_renderer {
            renderer.update_spheres(&self.queue, &ball_instances);
            renderer.update_cylinders(&self.queue, &cylinder_instances);
        }
        // Mirror to VR ball-stick renderer
        if let Some(ref mut vr_ball) = self.vr_ball_stick_renderer {
            vr_ball.update_spheres(&self.queue, &ball_instances);
            vr_ball.update_cylinders(&self.queue, &cylinder_instances);
        }

        self.ball_stick_loaded = true;
        self.ball_stick_atom_count = protein.atoms.len();
        log::info!("Ball-stick geometry ready: {} atoms, {} bonds", ball_instances.len(), cylinder_instances.len());
    }

    /// Ensure ribbon renderer is created and has data
    pub fn ensure_ribbon(&mut self, protein: &pdb_parser::Protein) {
        // Check if already loaded for THIS protein (by atom count)
        let current_atom_count = protein.atoms.len();
        if self.ribbon_loaded && self.ribbon_atom_count == current_atom_count {
            log::debug!("Ribbon geometry already loaded for this protein, skipping regeneration");
            return;
        }

        // If loaded but for different protein, log that we're regenerating
        if self.ribbon_loaded && self.ribbon_atom_count != current_atom_count {
            log::info!("Ribbon geometry loaded for different protein ({} atoms), regenerating for {} atoms",
                self.ribbon_atom_count, current_atom_count);
        }

        // Create renderer if needed
        if self.ribbon_renderer.is_none() {
            log::info!("Creating ribbon renderer...");
            self.ribbon_renderer = Some(RibbonRenderer::new(
                &self.device,
                self.config.format,
                &self.camera_bind_group_layout,
            ));
        }

        // Generate ribbon geometry
        log::info!("Generating ribbon geometry...");
        if let Some(ref mut renderer) = self.ribbon_renderer {
            if let Err(e) = renderer.update_from_protein(&self.queue, protein) {
                log::error!("Failed to generate ribbon: {}", e);
            } else {
                self.ribbon_loaded = true;
                self.ribbon_atom_count = protein.atoms.len();
                log::info!("Ribbon geometry ready");
            }
        }
        // Mirror to VR ribbon renderer (same data, different pipeline format)
        if let Some(ref mut vr_ribbon) = self.vr_ribbon_renderer {
            if let Err(e) = vr_ribbon.update_from_protein(&self.queue, protein) {
                log::error!("Failed to generate VR ribbon: {}", e);
            }
        }
    }

    /// Ensure surface renderer is created and has data
    pub fn ensure_surface(&mut self, protein: &pdb_parser::Protein) {
        // Check if already loaded for THIS protein (by atom count)
        let current_atom_count = protein.atoms.len();
        if self.surface_loaded && self.surface_atom_count == current_atom_count {
            log::debug!("Surface geometry already loaded for this protein, skipping regeneration");
            return;
        }

        // If loaded but for different protein, log that we're regenerating
        if self.surface_loaded && self.surface_atom_count != current_atom_count {
            log::info!("Surface geometry loaded for different protein ({} atoms), regenerating for {} atoms",
                self.surface_atom_count, current_atom_count);
        }

        // Create renderer if needed
        if self.surface_renderer.is_none() {
            log::info!("Creating surface renderer...");
            self.surface_renderer = Some(SurfaceRenderer::new(
                &self.device,
                self.config.format,
                &self.camera_bind_group_layout,
            ));
        }

        // Generate surface geometry (this is slow!)
        log::info!("Generating molecular surface (this may take a while)...");
        use crate::representations::SurfaceConfig;
        let surface_config = SurfaceConfig::default();

        if let Some(ref mut renderer) = self.surface_renderer {
            if let Err(e) = renderer.generate_surface(&self.device, &self.queue, protein, &surface_config) {
                log::error!("Failed to generate surface: {}", e);
            } else {
                self.surface_loaded = true;
                self.surface_atom_count = protein.atoms.len();
                log::info!("Surface geometry ready");
            }
        }
        // Mirror to VR surface renderer
        if let Some(ref mut vr_surf) = self.vr_surface_renderer {
            if let Err(e) = vr_surf.generate_surface(&self.device, &self.queue, protein, &surface_config) {
                log::error!("Failed to generate VR surface: {}", e);
            }
        }
    }

    /// Ensure raymarching surface renderer is created and has data
    pub fn ensure_surface_raymarch(&mut self, protein: &pdb_parser::Protein) {
        // Check if already loaded for THIS protein (by atom count)
        let current_atom_count = protein.atoms.len();
        if self.surface_raymarch_loaded && self.surface_raymarch_atom_count == current_atom_count {
            log::debug!("Raymarching surface already loaded for this protein, skipping regeneration");
            return;
        }

        // If loaded but for different protein, log that we're regenerating
        if self.surface_raymarch_loaded && self.surface_raymarch_atom_count != current_atom_count {
            log::info!("Raymarching surface loaded for different protein ({} atoms), regenerating for {} atoms",
                self.surface_raymarch_atom_count, current_atom_count);
        }

        // Create renderer if needed
        if self.surface_raymarch_renderer.is_none() {
            log::info!("Creating raymarching surface renderer...");
            self.surface_raymarch_renderer = Some(SurfaceRaymarchRenderer::new(
                &self.device,
                self.config.format,
                &self.camera_bind_group_layout,
            ));
        }

        // Prepare raymarching data (instant compared to marching cubes!)
        log::info!("Preparing raymarching surface (instant preparation)...");
        let raymarch_config = RaymarchConfig::default();

        if let Some(ref mut renderer) = self.surface_raymarch_renderer {
            if let Err(e) = renderer.prepare_surface(&self.device, &self.queue, protein, &raymarch_config) {
                log::error!("Failed to prepare raymarching surface: {}", e);
            } else {
                self.surface_raymarch_loaded = true;
                self.surface_raymarch_atom_count = protein.atoms.len();
                log::info!("Raymarching surface ready (real-time rendering enabled)");
            }
        }
    }

    /// Ensure sphere raymarch renderer is created and has data
    pub fn ensure_sphere_raymarch(&mut self, protein: &pdb_parser::Protein) {
        let current_atom_count = protein.atoms.len();
        if self.sphere_raymarch_loaded && self.sphere_raymarch_atom_count == current_atom_count {
            log::debug!("Sphere raymarch already loaded for this protein, skipping regeneration");
            return;
        }

        if self.sphere_raymarch_loaded && self.sphere_raymarch_atom_count != current_atom_count {
            log::info!("Sphere raymarch loaded for different protein ({} atoms), regenerating for {} atoms",
                self.sphere_raymarch_atom_count, current_atom_count);
        }

        if self.sphere_raymarch_renderer.is_none() {
            log::info!("Creating sphere raymarch renderer...");
            self.sphere_raymarch_renderer = Some(SphereRaymarchRenderer::new(
                &self.device,
                self.config.format,
                &self.camera_bind_group_layout,
            ));
        }

        log::info!("Preparing sphere raymarch ({} atoms)...", protein.atoms.len());
        let cell_size = 8.0; // Grid cell size in Angstroms

        if let Some(ref mut renderer) = self.sphere_raymarch_renderer {
            if let Err(e) = renderer.prepare(&self.device, &self.queue, protein, cell_size) {
                log::error!("Failed to prepare sphere raymarch: {}", e);
            } else {
                self.sphere_raymarch_loaded = true;
                self.sphere_raymarch_atom_count = protein.atoms.len();
                log::info!("Sphere raymarch ready (analytic ray-sphere intersection)");
            }
        }
    }

    /// Update visible instances with LOD + Frustum Culling
    pub fn update_visible_instances(&mut self, protein: &pdb_parser::Protein) {
        // Start benchmark timer for CPU path
        let timer = BenchmarkTimer::start();

        // 1. Update frustum from camera
        let view_proj = self.camera.view_projection();
        self.culling_system.update(view_proj);

        // 2. Assign LOD levels with hysteresis based on distance to camera
        let lod_assignments: Vec<(usize, LodLevel)> = protein.atoms.iter()
            .enumerate()
            .map(|(idx, atom)| {
                let distance = atom.position.distance(self.camera.position);
                let previous = self.lod_previous.get(&idx).copied();
                let lod = self.lod_system.compute_lod_with_previous(distance, previous);
                (idx, lod)
            })
            .collect();

        // 3. Update previous LODs for next frame
        self.lod_previous.clear();
        for &(idx, lod) in &lod_assignments {
            self.lod_previous.insert(idx, lod);
        }

        // 4. Group by LOD level
        self.lod_groups = LodGroups::from_assignments(&lod_assignments);

        // 5. For each LOD group: frustum culling + create instances + update renderer

        // High detail (subdivision 3)
        let visible_high: Vec<_> = self.lod_groups.high.iter()
            .filter(|&&idx| {
                let atom = &protein.atoms[idx];
                self.culling_system.is_sphere_visible(
                    atom.position,
                    atom.element.vdw_radius()
                )
            })
            .copied()
            .collect();

        let instances_high: Vec<SphereInstance> = visible_high.iter()
            .map(|&idx| {
                let atom = &protein.atoms[idx];
                SphereInstance::new(
                    atom.position,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.spheres_high.update_instances(&self.queue, &instances_high);

        // Medium detail (subdivision 2)
        let visible_medium: Vec<_> = self.lod_groups.medium.iter()
            .filter(|&&idx| {
                let atom = &protein.atoms[idx];
                self.culling_system.is_sphere_visible(
                    atom.position,
                    atom.element.vdw_radius()
                )
            })
            .copied()
            .collect();

        let instances_medium: Vec<SphereInstance> = visible_medium.iter()
            .map(|&idx| {
                let atom = &protein.atoms[idx];
                SphereInstance::new(
                    atom.position,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.spheres_medium.update_instances(&self.queue, &instances_medium);

        // Low detail (subdivision 1)
        let visible_low: Vec<_> = self.lod_groups.low.iter()
            .filter(|&&idx| {
                let atom = &protein.atoms[idx];
                self.culling_system.is_sphere_visible(
                    atom.position,
                    atom.element.vdw_radius()
                )
            })
            .copied()
            .collect();

        let instances_low: Vec<SphereInstance> = visible_low.iter()
            .map(|&idx| {
                let atom = &protein.atoms[idx];
                SphereInstance::new(
                    atom.position,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.spheres_low.update_instances(&self.queue, &instances_low);

        // Very low detail (subdivision 0)
        let visible_very_low: Vec<_> = self.lod_groups.very_low.iter()
            .filter(|&&idx| {
                let atom = &protein.atoms[idx];
                self.culling_system.is_sphere_visible(
                    atom.position,
                    atom.element.vdw_radius()
                )
            })
            .copied()
            .collect();

        let instances_very_low: Vec<SphereInstance> = visible_very_low.iter()
            .map(|&idx| {
                let atom = &protein.atoms[idx];
                SphereInstance::new(
                    atom.position,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.spheres_very_low.update_instances(&self.queue, &instances_very_low);

        // Impostors (billboards for very distant atoms)
        let visible_impostors: Vec<_> = self.lod_groups.impostors.iter()
            .filter(|&&idx| {
                let atom = &protein.atoms[idx];
                self.culling_system.is_sphere_visible(
                    atom.position,
                    atom.element.vdw_radius()
                )
            })
            .copied()
            .collect();

        let instances_impostors: Vec<BillboardInstance> = visible_impostors.iter()
            .map(|&idx| {
                let atom = &protein.atoms[idx];
                BillboardInstance::new(
                    atom.position,
                    atom.element.vdw_radius(),
                    atom.element.cpk_color(),
                )
            })
            .collect();

        self.billboard_impostor.update_instances(&self.queue, &instances_impostors);

        // Update benchmark stats
        self.benchmark_stats.cpu_compute_time_us = timer.elapsed_us();
        self.benchmark_stats.atom_count = protein.atoms.len();
        self.benchmark_stats.frames_measured += 1;

        // Log statistics
        let total_visible = visible_high.len() + visible_medium.len() + visible_low.len() + visible_very_low.len() + visible_impostors.len();
        log::trace!(
            "LOD stats (CPU): High={}, Medium={}, Low={}, VeryLow={}, Impostor={}, Total visible={}/{}, Time={:.2}µs",
            visible_high.len(),
            visible_medium.len(),
            visible_low.len(),
            visible_very_low.len(),
            visible_impostors.len(),
            total_visible,
            protein.atoms.len(),
            self.benchmark_stats.cpu_compute_time_us
        );
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Dispatch GPU compute culling for Van der Waals and Ball-and-Stick (spheres only)
        if self.representation == RepresentationType::VanDerWaals
            || self.representation == RepresentationType::BallAndStick {
            self.dispatch_compute_culling(&mut encoder);
        }

        // Render 3D scene
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Set camera bind group for all renderers
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

            // Render based on current representation
            match self.representation {
                RepresentationType::VanDerWaals => {
                    // Use GPU-driven rendering if available, otherwise fall back to CPU path
                    if self.gpu_compute_enabled
                        && self.gpu_spheres_high.is_some()
                        && self.draw_commands_buffer.is_some()
                        && self.gpu_render_bind_groups.is_some()
                    {
                        // GPU-driven indirect rendering
                        let draw_buffer = self.draw_commands_buffer.as_ref().unwrap();
                        let bind_groups = self.gpu_render_bind_groups.as_ref().unwrap();
                        let cmd_size = std::mem::size_of::<DrawIndexedIndirectCommand>() as u64;

                        self.gpu_spheres_high
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[0], draw_buffer, 0 * cmd_size);

                        self.gpu_spheres_medium
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[1], draw_buffer, 1 * cmd_size);

                        self.gpu_spheres_low
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[2], draw_buffer, 2 * cmd_size);

                        self.gpu_spheres_very_low
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[3], draw_buffer, 3 * cmd_size);

                        // Billboard impostors not yet implemented for GPU path
                        // TODO: Add GPU billboard renderer
                    } else {
                        // CPU path (Phases 1 & 2)
                        self.spheres_high.render(&mut render_pass);
                        self.spheres_medium.render(&mut render_pass);
                        self.spheres_low.render(&mut render_pass);
                        self.spheres_very_low.render(&mut render_pass);
                        self.billboard_impostor.render(&mut render_pass);
                    }
                }
                RepresentationType::BallAndStick => {
                    // Lazy-load ball-stick renderer if needed
                    if self.ball_stick_renderer.is_none() {
                        log::info!("Lazy-loading ball-stick renderer...");
                        self.ball_stick_renderer = Some(BallStickRenderer::new(
                            &self.device,
                            self.config.format,
                            &self.camera_bind_group_layout,
                        ));
                    }

                    // Hybrid approach: GPU for spheres (atoms), CPU for cylinders (bonds)
                    if self.gpu_compute_enabled
                        && self.gpu_spheres_high.is_some()
                        && self.draw_commands_buffer.is_some()
                        && self.gpu_render_bind_groups.is_some()
                    {
                        // First render cylinders (bonds) using CPU - they go behind
                        if let Some(ref renderer) = self.ball_stick_renderer {
                            render_pass.set_pipeline(&renderer.pipeline);

                            // Render only cylinders
                            if renderer.cylinder_instance_count > 0 {
                                render_pass.set_vertex_buffer(0, renderer.cylinder_vertex_buffer.slice(..));
                                render_pass.set_vertex_buffer(1, renderer.cylinder_instance_buffer.slice(..));
                                render_pass.set_index_buffer(
                                    renderer.cylinder_index_buffer.slice(..),
                                    wgpu::IndexFormat::Uint32,
                                );
                                render_pass.draw_indexed(0..renderer.cylinder_index_count, 0, 0..renderer.cylinder_instance_count);
                            }
                        }

                        // Then render spheres (atoms) using GPU - they go on top
                        let draw_buffer = self.draw_commands_buffer.as_ref().unwrap();
                        let bind_groups = self.gpu_render_bind_groups.as_ref().unwrap();
                        let cmd_size = std::mem::size_of::<DrawIndexedIndirectCommand>() as u64;

                        self.gpu_spheres_high
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[0], draw_buffer, 0 * cmd_size);

                        self.gpu_spheres_medium
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[1], draw_buffer, 1 * cmd_size);

                        self.gpu_spheres_low
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[2], draw_buffer, 2 * cmd_size);

                        self.gpu_spheres_very_low
                            .as_ref()
                            .unwrap()
                            .render_indirect(&mut render_pass, &bind_groups[3], draw_buffer, 3 * cmd_size);
                    } else {
                        // CPU fallback: render everything with BallStickRenderer
                        if let Some(ref renderer) = self.ball_stick_renderer {
                            renderer.render(&mut render_pass);
                        }
                    }
                }
                RepresentationType::Ribbon => {
                    // Lazy-load ribbon renderer if needed
                    if self.ribbon_renderer.is_none() {
                        log::info!("Lazy-loading ribbon renderer...");
                        self.ribbon_renderer = Some(RibbonRenderer::new(
                            &self.device,
                            self.config.format,
                            &self.camera_bind_group_layout,
                        ));
                    }
                    if let Some(ref renderer) = self.ribbon_renderer {
                        renderer.render(&mut render_pass);
                    }
                }
                RepresentationType::Surface => {
                    // Lazy-load surface renderer if needed
                    if self.surface_renderer.is_none() {
                        log::info!("Lazy-loading surface renderer...");
                        self.surface_renderer = Some(SurfaceRenderer::new(
                            &self.device,
                            self.config.format,
                            &self.camera_bind_group_layout,
                        ));
                    }
                    if let Some(ref renderer) = self.surface_renderer {
                        renderer.render(&mut render_pass);
                    }
                }
                RepresentationType::SurfaceRaymarch => {
                    // Lazy-load raymarching surface renderer if needed
                    if self.surface_raymarch_renderer.is_none() {
                        log::info!("Lazy-loading raymarching surface renderer...");
                        self.surface_raymarch_renderer = Some(SurfaceRaymarchRenderer::new(
                            &self.device,
                            self.config.format,
                            &self.camera_bind_group_layout,
                        ));
                    }
                    if let Some(ref renderer) = self.surface_raymarch_renderer {
                        renderer.render(&mut render_pass);
                    }
                }
                RepresentationType::SphereRaymarch => {
                    // Lazy-load raymarched spheres renderer if needed
                    if self.sphere_raymarch_renderer.is_none() {
                        log::info!("Lazy-loading sphere raymarch renderer...");
                        self.sphere_raymarch_renderer = Some(SphereRaymarchRenderer::new(
                            &self.device,
                            self.config.format,
                            &self.camera_bind_group_layout,
                        ));
                    }
                    if let Some(ref renderer) = self.sphere_raymarch_renderer {
                        renderer.render(&mut render_pass);
                    }
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Clear all GPU resources for loaded protein data
    /// This is called when all models are removed from the scene
    pub fn clear_protein_data(&mut self) {
        log::info!("Clearing all protein data from renderer");

        // Clear lazy-loaded renderers (these are Option types, so setting to None frees memory)
        self.ball_stick_renderer = None;
        self.ribbon_renderer = None;
        self.surface_renderer = None;
        self.surface_raymarch_renderer = None;
        self.sphere_raymarch_renderer = None;

        // Reset loaded flags
        self.ball_stick_loaded = false;
        self.ribbon_loaded = false;
        self.surface_loaded = false;
        self.surface_raymarch_loaded = false;
        self.sphere_raymarch_loaded = false;

        // Clear GPU compute buffers
        self.atom_data_buffer = None;
        self.frustum_buffer = None;
        self.camera_position_buffer = None;
        self.lod_config_buffer = None;
        self.draw_commands_buffer = None;
        self.visible_indices_buffers = None;
        self.compute_bind_group = None;
        self.gpu_render_bind_groups = None;

        // Clear GPU-driven sphere renderers if they exist
        self.gpu_spheres_high = None;
        self.gpu_spheres_medium = None;
        self.gpu_spheres_low = None;
        self.gpu_spheres_very_low = None;

        self.atom_count = 0;

        log::info!("Protein data cleared successfully");
    }
}
