use crate::geometry::Vertex;
use crate::marching_cubes::{extract_isosurface, VoxelGrid};
use glam::Vec3;
use rayon::prelude::*;
use wgpu::util::DeviceExt;
use bytemuck::{Pod, Zeroable};

struct BufferChunk {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    _vertex_count: u32,
    index_count: u32,
}

// GPU data structures for compute shader
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AtomDataGPU {
    position: [f32; 3],
    radius: f32,  // VdW radius
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GridParamsGPU {
    origin: [f32; 3],
    spacing: f32,
    dimensions: [u32; 3],  // nx, ny, nz
    probe_radius: f32,
}

#[derive(Clone)]
pub struct SurfaceConfig {
    pub probe_radius: f32,
    pub grid_spacing: f32,
    pub smoothing: bool,
    pub smoothing_iterations: usize,
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self {
            probe_radius: 1.4,
            grid_spacing: 0.8, // Balanced quality and performance
            smoothing: true, // Enable smoothing for continuous mesh
            smoothing_iterations: 2,
        }
    }
}

pub struct SurfaceRenderer {
    chunks: Vec<BufferChunk>,
    max_vertices_per_chunk: u32,
    max_indices_per_chunk: u32,
    pipeline: wgpu::RenderPipeline,
    material_buffer: wgpu::Buffer,
    material_bind_group: wgpu::BindGroup,
    alpha: f32,

    // GPU compute for SDF calculation
    compute_pipeline: Option<wgpu::ComputePipeline>,
    compute_bind_group_layout: Option<wgpu::BindGroupLayout>,
    gpu_compute_enabled: bool,
}

impl SurfaceRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Multi-buffer chunking: each chunk limited by wgpu max buffer size (256MB)
        // Vertex struct = 40 bytes (3 pos + 3 normal + 4 color floats)
        let max_vertices_per_chunk = 6_000_000;  // 6M vertices (~240MB, under 256MB limit)
        let max_indices_per_chunk = 18_000_000;  // 18M indices (~72MB)

        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Surface Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/surface.wgsl").into(),
            ),
        });

        // Create material buffer (for alpha)
        let alpha = 1.0f32; // Fully opaque - fixes transparency artifacts
        let material_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Surface Material Buffer"),
            contents: bytemuck::cast_slice(&[alpha]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create material bind group layout
        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Surface Material Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Create material bind group
        let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Surface Material Bind Group"),
            layout: &material_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: material_buffer.as_entire_binding(),
            }],
        });

        // Create render pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Surface Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout, &material_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Surface Render Pipeline"),
            layout: Some(&pipeline_layout),
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
                    format: surface_format,
                    blend: None, // No blending needed for opaque surface
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // Don't cull back faces for surfaces
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

        log::info!("SurfaceRenderer initialized with multi-buffer chunking");
        log::info!("  Max vertices per chunk: {}", max_vertices_per_chunk);
        log::info!("  Max indices per chunk: {}", max_indices_per_chunk);

        // Initialize GPU compute for SDF calculation
        let (compute_pipeline, compute_bind_group_layout, gpu_compute_enabled) =
            Self::init_compute_pipeline(device);

        if gpu_compute_enabled {
            log::info!("✓ GPU compute enabled for surface SDF calculation (massive speedup!)");
        } else {
            log::warn!("✗ GPU compute not available, falling back to CPU (slower)");
        }

        Self {
            chunks: Vec::new(),
            max_vertices_per_chunk: max_vertices_per_chunk as u32,
            max_indices_per_chunk: max_indices_per_chunk as u32,
            pipeline,
            material_buffer,
            material_bind_group,
            alpha,
            compute_pipeline,
            compute_bind_group_layout,
            gpu_compute_enabled,
        }
    }

    /// Initialize GPU compute pipeline for SDF calculation
    fn init_compute_pipeline(
        device: &wgpu::Device,
    ) -> (Option<wgpu::ComputePipeline>, Option<wgpu::BindGroupLayout>, bool) {
        // Check if device supports compute shaders
        let features = device.features();
        if !features.contains(wgpu::Features::STORAGE_RESOURCE_BINDING_ARRAY) {
            return (None, None, false);
        }

        // Load compute shader
        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Surface SDF Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/surface_sdf.wgsl").into(),
            ),
        });

        // Create bind group layout for compute shader
        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Surface SDF Compute Bind Group Layout"),
                entries: &[
                    // Binding 0: Atom data (storage buffer, read-only)
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
                    // Binding 1: Grid params (uniform buffer)
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
                    // Binding 2: SDF values (storage buffer, read-write)
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
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
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Surface SDF Compute Pipeline Layout"),
                bind_group_layouts: &[&compute_bind_group_layout],
                push_constant_ranges: &[],
            });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Surface SDF Compute Pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &compute_shader,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });

        (Some(compute_pipeline), Some(compute_bind_group_layout), true)
    }

    /// Compute SDF values on GPU using compute shader
    fn compute_sdf_gpu(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        protein: &pdb_parser::Protein,
        grid: &VoxelGrid,
        config: &SurfaceConfig,
    ) -> anyhow::Result<Vec<f32>> {
        let (compute_pipeline, compute_bind_group_layout) = match (&self.compute_pipeline, &self.compute_bind_group_layout) {
            (Some(p), Some(l)) => (p, l),
            _ => anyhow::bail!("GPU compute not available"),
        };

        let (nx, ny, nz) = grid.dimensions;
        let total_voxels = (nx * ny * nz) as usize;

        // Prepare atom data for GPU
        let atom_data: Vec<AtomDataGPU> = protein
            .atoms
            .iter()
            .map(|atom| AtomDataGPU {
                position: atom.position.into(),
                radius: atom.element.vdw_radius(),
            })
            .collect();

        // Create atom data buffer
        let atom_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("SDF Atom Data Buffer"),
            contents: bytemuck::cast_slice(&atom_data),
            usage: wgpu::BufferUsages::STORAGE,
        });

        // Create grid params buffer
        let grid_params = GridParamsGPU {
            origin: grid.origin.into(),
            spacing: grid.spacing,
            dimensions: [nx as u32, ny as u32, nz as u32],
            probe_radius: config.probe_radius,
        };

        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("SDF Grid Params Buffer"),
            contents: bytemuck::cast_slice(&[grid_params]),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Create SDF values buffer (output)
        let sdf_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SDF Values Buffer"),
            size: (total_voxels * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        // Create staging buffer for readback
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SDF Staging Buffer"),
            size: (total_voxels * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SDF Compute Bind Group"),
            layout: compute_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: atom_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: sdf_buffer.as_entire_binding(),
                },
            ],
        });

        // Create command encoder and dispatch compute shader
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("SDF Compute Encoder"),
        });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("SDF Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(compute_pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            // Dispatch workgroups (8x8x8 threads per workgroup)
            let workgroups_x = ((nx + 7) / 8) as u32;
            let workgroups_y = ((ny + 7) / 8) as u32;
            let workgroups_z = ((nz + 7) / 8) as u32;

            log::debug!("Dispatching {} x {} x {} workgroups ({} total threads)",
                workgroups_x, workgroups_y, workgroups_z, nx * ny * nz);

            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, workgroups_z);
        }

        // Copy results to staging buffer
        encoder.copy_buffer_to_buffer(
            &sdf_buffer,
            0,
            &staging_buffer,
            0,
            (total_voxels * std::mem::size_of::<f32>()) as u64,
        );

        // Submit commands
        queue.submit(Some(encoder.finish()));

        // Read back results
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });

        // Wait for GPU to finish
        device.poll(wgpu::Maintain::Wait);

        // Check mapping result
        receiver.recv().unwrap()?;

        // Copy data
        let data = buffer_slice.get_mapped_range();
        let sdf_values: Vec<f32> = bytemuck::cast_slice(&data).to_vec();

        // Cleanup
        drop(data);
        staging_buffer.unmap();

        Ok(sdf_values)
    }

    pub fn generate_surface(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        protein: &pdb_parser::Protein,
        config: &SurfaceConfig,
    ) -> anyhow::Result<()> {
        let start = std::time::Instant::now();

        if protein.atoms.is_empty() {
            log::warn!("No atoms to generate surface");
            self.chunks.clear();
            return Ok(());
        }

        log::info!("Generating molecular surface with marching cubes...");
        log::info!("  Protein has {} atoms", protein.atoms.len());

        // Calculate bounding box with padding
        // Need extra padding to account for VdW radii (up to ~2.0 Å) + probe radius
        let (min, max) = protein.bounding_box();
        let padding = config.probe_radius + 3.0; // Probe + max VdW radius + margin
        let min_bound = min - Vec3::splat(padding);
        let max_bound = max + Vec3::splat(padding);

        // Create voxel grid
        let mut grid = VoxelGrid::new((min_bound, max_bound), config.grid_spacing);
        log::info!("Grid dimensions: {:?}", grid.dimensions);
        log::info!("Total voxels: {}", grid.values.len());

        // Compute SDF values - GPU or CPU path
        let (nx, ny, nz) = grid.dimensions;
        let sdf_start = std::time::Instant::now();

        if self.gpu_compute_enabled {
            // GPU COMPUTE PATH - MASSIVELY PARALLEL
            log::info!("Computing SDF on GPU...");
            grid.values = self.compute_sdf_gpu(device, queue, protein, &grid, config)?;
            log::info!("GPU SDF computation took {:?}", sdf_start.elapsed());
        } else {
            // CPU FALLBACK PATH - PARALLEL BUT SLOWER
            log::info!("Computing SDF on CPU (fallback)...");

            // Collect atom positions and radii for SDF computation
            let atoms: Vec<(Vec3, f32)> = protein
                .atoms
                .iter()
                .map(|atom| (atom.position, atom.element.vdw_radius()))
                .collect();

            // Compute SDF values in parallel
            // CRITICAL: Must match grid indexing order: z * nx * ny + y * nx + x
            let origin = grid.origin;
            let spacing = grid.spacing;
            let probe_radius = config.probe_radius;

            let values: Vec<f32> = (0..nz)
                .into_par_iter()
                .flat_map(|z| {
                    let atoms = atoms.clone();
                    (0..ny)
                        .into_par_iter()
                        .flat_map(move |y| {
                            let atoms = atoms.clone();
                            (0..nx)
                                .into_par_iter()
                                .map(move |x| {
                                    let world_pos = origin
                                        + Vec3::new(
                                            x as f32 * spacing,
                                            y as f32 * spacing,
                                            z as f32 * spacing,
                                        );
                                    compute_sdf_at_point(&world_pos, &atoms, probe_radius)
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .collect();

            grid.values = values;
            log::info!("CPU SDF computation took {:?}", sdf_start.elapsed());
        }

        // Extract isosurface using marching cubes
        let (vertices, indices) = extract_isosurface(&grid, 0.0);

        if vertices.is_empty() {
            log::warn!("No surface generated");
            self.chunks.clear();
            return Ok(());
        }

        log::info!(
            "Generated {} vertices (with duplicates), {} triangles",
            vertices.len(),
            indices.len() / 3
        );

        // Weld duplicate vertices to create continuous mesh
        log::info!("Welding duplicate vertices...");
        let (mut vertices, indices) = weld_vertices(vertices, indices, config.grid_spacing * 0.1);

        log::info!(
            "After welding: {} unique vertices, {} triangles",
            vertices.len(),
            indices.len() / 3
        );

        // Smooth mesh if requested
        if config.smoothing && vertices.len() > 0 {
            log::info!("Smoothing mesh...");
            smooth_mesh(&mut vertices, &indices, config.smoothing_iterations);
        }

        // Clear previous chunks
        self.chunks.clear();

        // Divide geometry into chunks
        // After welding, vertices are shared, so we upload all vertices once
        // and chunk only the indices
        let max_indices_per_chunk = self.max_indices_per_chunk as usize;

        // If mesh is small enough, use single chunk
        if vertices.len() <= self.max_vertices_per_chunk as usize &&
           indices.len() <= max_indices_per_chunk {
            // Single chunk
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Surface Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Surface Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            log::info!("Surface fits in single chunk: {} vertices, {} triangles",
                vertices.len(), indices.len() / 3);

            self.chunks.push(BufferChunk {
                vertex_buffer,
                index_buffer,
                _vertex_count: vertices.len() as u32,
                index_count: indices.len() as u32,
            });
        } else {
            // Too large - would need more complex chunking with vertex deduplication per chunk
            // For now, just warn and use all data (may hit limits)
            log::warn!("Surface too large for single chunk ({} vertices, {} indices)",
                vertices.len(), indices.len());
            log::warn!("Using single chunk anyway - may hit GPU limits");

            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Surface Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Surface Index Buffer"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.chunks.push(BufferChunk {
                vertex_buffer,
                index_buffer,
                _vertex_count: vertices.len() as u32,
                index_count: indices.len() as u32,
            });
        }

        log::info!("Surface divided into {} chunks", self.chunks.len());
        log::info!("Surface generation took {:?}", start.elapsed());

        Ok(())
    }

    pub fn set_alpha(&mut self, queue: &wgpu::Queue, alpha: f32) {
        if (self.alpha - alpha).abs() > 0.001 {
            self.alpha = alpha;
            queue.write_buffer(&self.material_buffer, 0, bytemuck::cast_slice(&[alpha]));
        }
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.chunks.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(1, &self.material_bind_group, &[]);

        // Render each chunk
        for chunk in &self.chunks {
            render_pass.set_vertex_buffer(0, chunk.vertex_buffer.slice(..));
            render_pass.set_index_buffer(chunk.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..chunk.index_count, 0, 0..1);
        }
    }
}

/// Weld duplicate vertices using spatial hashing for O(n) performance
fn weld_vertices(vertices: Vec<Vertex>, indices: Vec<u32>, tolerance: f32) -> (Vec<Vertex>, Vec<u32>) {
    use std::collections::HashMap;

    let grid_size = tolerance;
    let inv_grid_size = 1.0 / grid_size;

    // Spatial hash: grid cell -> list of vertex indices
    let mut spatial_hash: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    let mut unique_vertices: Vec<Vertex> = Vec::new();
    let mut vertex_map: HashMap<usize, usize> = HashMap::new();

    // Helper: compute grid cell for a position
    let get_cell = |pos: Vec3| -> (i32, i32, i32) {
        (
            (pos.x * inv_grid_size).floor() as i32,
            (pos.y * inv_grid_size).floor() as i32,
            (pos.z * inv_grid_size).floor() as i32,
        )
    };

    let tolerance_sq = tolerance * tolerance;

    // Build vertex map using spatial hashing
    for (old_idx, vertex) in vertices.iter().enumerate() {
        let pos = Vec3::from(vertex.position);
        let cell = get_cell(pos);

        // Check vertices in same cell and adjacent cells
        let mut found_idx = None;
        'outer: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let check_cell = (cell.0 + dx, cell.1 + dy, cell.2 + dz);

                    if let Some(nearby) = spatial_hash.get(&check_cell) {
                        for &idx in nearby {
                            let existing_pos = Vec3::from(unique_vertices[idx].position);
                            let dist_sq = (pos - existing_pos).length_squared();

                            if dist_sq < tolerance_sq {
                                found_idx = Some(idx);
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        let new_idx = if let Some(idx) = found_idx {
            idx
        } else {
            let idx = unique_vertices.len();
            unique_vertices.push(*vertex);
            spatial_hash.entry(cell).or_insert_with(Vec::new).push(idx);
            idx
        };

        vertex_map.insert(old_idx, new_idx);
    }

    // Remap indices
    let new_indices: Vec<u32> = indices
        .iter()
        .map(|&old_idx| *vertex_map.get(&(old_idx as usize)).unwrap() as u32)
        .collect();

    // Recalculate normals for welded mesh
    recalculate_normals(&mut unique_vertices, &new_indices);

    (unique_vertices, new_indices)
}

/// Compute signed distance field at a point
fn compute_sdf_at_point(point: &Vec3, atoms: &[(Vec3, f32)], probe_radius: f32) -> f32 {
    // Find minimum distance to any atom surface (expanded by probe)
    let mut min_dist = f32::MAX;

    for (atom_pos, vdw_radius) in atoms {
        let dist_to_center = (*point - *atom_pos).length();
        // SAS: distance to (VdW radius + probe radius)
        let expanded_radius = vdw_radius + probe_radius;
        let dist_to_surface = dist_to_center - expanded_radius;

        if dist_to_surface < min_dist {
            min_dist = dist_to_surface;
        }
    }

    min_dist
}

/// Smooth mesh using Laplacian smoothing
fn smooth_mesh(vertices: &mut [Vertex], indices: &[u32], iterations: usize) {
    if vertices.is_empty() || indices.is_empty() {
        return;
    }

    // Build vertex neighbors
    let neighbors = build_vertex_neighbors(vertices.len(), indices);

    for _ in 0..iterations {
        // Compute new positions
        let new_positions: Vec<Vec3> = vertices
            .par_iter()
            .enumerate()
            .map(|(i, vertex)| {
                if neighbors[i].is_empty() {
                    return Vec3::from(vertex.position);
                }

                // Average neighbor positions
                let sum: Vec3 = neighbors[i]
                    .iter()
                    .map(|&ni| Vec3::from(vertices[ni].position))
                    .sum();

                let avg = sum / neighbors[i].len() as f32;

                // Blend with current position (0.5 = 50% smoothing)
                Vec3::from(vertex.position).lerp(avg, 0.5)
            })
            .collect();

        // Update positions
        for (i, vertex) in vertices.iter_mut().enumerate() {
            vertex.position = new_positions[i].into();
        }

        // Recalculate normals after smoothing
        recalculate_normals(vertices, indices);
    }
}

/// Build adjacency list of vertex neighbors
fn build_vertex_neighbors(vertex_count: usize, indices: &[u32]) -> Vec<Vec<usize>> {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); vertex_count];

    // Iterate over triangles
    for triangle in indices.chunks(3) {
        if triangle.len() != 3 {
            continue;
        }

        let i0 = triangle[0] as usize;
        let i1 = triangle[1] as usize;
        let i2 = triangle[2] as usize;

        // Add edges
        add_neighbor(&mut neighbors, i0, i1);
        add_neighbor(&mut neighbors, i1, i0);

        add_neighbor(&mut neighbors, i1, i2);
        add_neighbor(&mut neighbors, i2, i1);

        add_neighbor(&mut neighbors, i2, i0);
        add_neighbor(&mut neighbors, i0, i2);
    }

    neighbors
}

fn add_neighbor(neighbors: &mut [Vec<usize>], from: usize, to: usize) {
    if from >= neighbors.len() || to >= neighbors.len() {
        return;
    }

    if !neighbors[from].contains(&to) {
        neighbors[from].push(to);
    }
}

/// Recalculate normals from triangle geometry
fn recalculate_normals(vertices: &mut [Vertex], indices: &[u32]) {
    // Reset normals
    for vertex in vertices.iter_mut() {
        vertex.normal = [0.0, 0.0, 0.0];
    }

    // Accumulate face normals
    for triangle in indices.chunks(3) {
        if triangle.len() != 3 {
            continue;
        }

        let i0 = triangle[0] as usize;
        let i1 = triangle[1] as usize;
        let i2 = triangle[2] as usize;

        if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
            continue;
        }

        let v0 = Vec3::from(vertices[i0].position);
        let v1 = Vec3::from(vertices[i1].position);
        let v2 = Vec3::from(vertices[i2].position);

        let edge1 = v1 - v0;
        let edge2 = v2 - v0;
        let normal = edge1.cross(edge2);

        // Accumulate (weighted by triangle area)
        let normal_arr: [f32; 3] = normal.into();

        for i in 0..3 {
            vertices[i0].normal[i] += normal_arr[i];
            vertices[i1].normal[i] += normal_arr[i];
            vertices[i2].normal[i] += normal_arr[i];
        }
    }

    // Normalize
    for vertex in vertices.iter_mut() {
        let normal = Vec3::from(vertex.normal);
        let len = normal.length();

        if len > 1e-6 {
            vertex.normal = (normal / len).into();
        } else {
            vertex.normal = [0.0, 1.0, 0.0]; // Default up
        }
    }
}
