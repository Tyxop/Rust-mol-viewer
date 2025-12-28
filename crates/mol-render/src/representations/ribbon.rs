use crate::splines::{
    compute_binormals, compute_parallel_transport_normals, CatmullRomSpline,
};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct RibbonVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

impl RibbonVertex {
    pub fn new(position: Vec3, normal: Vec3, color: [f32; 4]) -> Self {
        Self {
            position: position.into(),
            normal: normal.into(),
            color,
        }
    }

    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RibbonVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 2 * std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum StructureType {
    Helix,
    Sheet,
    Coil,
}

impl StructureType {
    fn color(&self) -> [f32; 4] {
        match self {
            StructureType::Helix => [0.8, 0.2, 0.2, 1.0], // Red
            StructureType::Sheet => [0.9, 0.9, 0.2, 1.0], // Yellow
            StructureType::Coil => [0.6, 0.6, 0.6, 1.0],  // Gray
        }
    }
}

#[derive(Clone)]
struct CAlphaChain {
    positions: Vec<Vec3>,
    residue_numbers: Vec<i32>,
    chain_id: char,
}

pub struct RibbonRenderer {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    vertex_count: u32,
    max_vertices: u32,
    max_indices: u32,
    pipeline: wgpu::RenderPipeline,
}

impl RibbonRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        camera_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Pre-allocate large buffers for high-end hardware (M4 128GB / RTX 4090 24GB)
        let max_vertices = 1_000_000;  // 1M vertices (~36MB)
        let max_indices = 5_000_000;   // 5M indices (~20MB)

        let vertex_buffer_size = (max_vertices * std::mem::size_of::<RibbonVertex>()) as u64;
        let index_buffer_size = (max_indices * std::mem::size_of::<u32>()) as u64;

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Ribbon Vertex Buffer"),
            size: vertex_buffer_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Ribbon Index Buffer"),
            size: index_buffer_size,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ribbon Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/ribbon.wgsl").into(),
            ),
        });

        // Create render pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ribbon Pipeline Layout"),
            bind_group_layouts: &[camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Ribbon Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[RibbonVertex::desc()],
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
                cull_mode: None, // Don't cull back faces for ribbons
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

        log::info!("RibbonRenderer initialized");

        Self {
            vertex_buffer,
            index_buffer,
            index_count: 0,
            vertex_count: 0,
            max_vertices: max_vertices as u32,
            max_indices: max_indices as u32,
            pipeline,
        }
    }

    pub fn update_from_protein(
        &mut self,
        queue: &wgpu::Queue,
        protein: &pdb_parser::Protein,
    ) -> anyhow::Result<()> {
        // Extract C-alpha chains
        let ca_chains = self.extract_ca_chains(protein);

        if ca_chains.is_empty() {
            log::warn!("No C-alpha atoms found for ribbon rendering");
            self.vertex_count = 0;
            self.index_count = 0;
            return Ok(());
        }

        log::info!("Generating ribbon geometry for {} chains", ca_chains.len());

        // Generate geometry for each chain
        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();

        for chain in ca_chains {
            if chain.positions.len() < 4 {
                log::warn!(
                    "Chain {} has only {} CA atoms, skipping",
                    chain.chain_id,
                    chain.positions.len()
                );
                continue;
            }

            let (vertices, indices) =
                self.generate_chain_geometry(&chain, &protein.secondary_structure);

            // Offset indices by current vertex count
            let offset = all_vertices.len() as u32;
            all_vertices.extend(vertices);
            all_indices.extend(indices.iter().map(|&i| i + offset));
        }

        if all_vertices.is_empty() {
            log::warn!("No ribbon geometry generated");
            self.vertex_count = 0;
            self.index_count = 0;
            return Ok(());
        }

        log::info!(
            "Generated ribbon: {} vertices, {} triangles",
            all_vertices.len(),
            all_indices.len() / 3
        );

        // Upload to GPU
        if all_vertices.len() > self.max_vertices as usize {
            log::warn!(
                "Too many vertices: {} > {}. Truncating.",
                all_vertices.len(),
                self.max_vertices
            );
            all_vertices.truncate(self.max_vertices as usize);
        }

        if all_indices.len() > self.max_indices as usize {
            log::warn!(
                "Too many indices: {} > {}. Truncating.",
                all_indices.len(),
                self.max_indices
            );
            all_indices.truncate(self.max_indices as usize);
        }

        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&all_vertices));
        queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&all_indices));

        self.vertex_count = all_vertices.len() as u32;
        self.index_count = all_indices.len() as u32;

        Ok(())
    }

    fn extract_ca_chains(&self, protein: &pdb_parser::Protein) -> Vec<CAlphaChain> {
        use std::collections::HashMap;

        let mut chains_map: HashMap<char, CAlphaChain> = HashMap::new();

        // Extract C-alpha atoms grouped by chain
        let mut total_ca = 0;
        for atom in &protein.atoms {
            if atom.name.trim() == "CA" {
                total_ca += 1;
                let chain = chains_map
                    .entry(atom.residue.chain_id)
                    .or_insert(CAlphaChain {
                        positions: Vec::new(),
                        residue_numbers: Vec::new(),
                        chain_id: atom.residue.chain_id,
                    });

                chain.positions.push(atom.position);
                chain.residue_numbers.push(atom.residue.seq_num);
            }
        }

        log::info!("Found {} total C-alpha atoms", total_ca);

        let chains: Vec<CAlphaChain> = chains_map.into_values().collect();
        for chain in &chains {
            log::info!("  Chain {}: {} CA atoms", chain.chain_id, chain.positions.len());
        }

        chains
    }

    fn generate_chain_geometry(
        &self,
        chain: &CAlphaChain,
        secondary_structure: &pdb_parser::SecondaryStructure,
    ) -> (Vec<RibbonVertex>, Vec<u32>) {
        // Detect breaks in the chain (missing residues or large gaps)
        let segments = self.split_chain_at_breaks(chain);

        if segments.len() > 1 {
            log::info!("  Chain {}: Split into {} segments due to breaks",
                chain.chain_id, segments.len());
        }

        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();

        // Generate geometry for each continuous segment
        for segment in segments {
            if segment.positions.len() < 4 {
                continue; // Skip very short segments
            }

            let (vertices, indices) = self.generate_segment_geometry(
                &segment,
                secondary_structure,
                chain.chain_id,
            );

            // Offset indices by current vertex count
            let offset = all_vertices.len() as u32;
            all_vertices.extend(vertices);
            all_indices.extend(indices.iter().map(|&i| i + offset));
        }

        log::info!("  Chain {}: Generated {} vertices, {} triangles",
            chain.chain_id, all_vertices.len(), all_indices.len() / 3);

        (all_vertices, all_indices)
    }

    fn split_chain_at_breaks(&self, chain: &CAlphaChain) -> Vec<CAlphaChain> {
        let mut segments = Vec::new();
        let mut current_positions = Vec::new();
        let mut current_residues = Vec::new();

        for i in 0..chain.positions.len() {
            current_positions.push(chain.positions[i]);
            current_residues.push(chain.residue_numbers[i]);

            // Check if there's a break after this residue
            if i < chain.positions.len() - 1 {
                let distance = (chain.positions[i + 1] - chain.positions[i]).length();
                let residue_gap = (chain.residue_numbers[i + 1] - chain.residue_numbers[i]).abs();

                // Break if distance is too large (>4.5Å) or residue gap is >1
                if distance > 4.5 || residue_gap > 1 {
                    log::debug!("  Break detected: residue {} to {} (distance: {:.2}Å, gap: {})",
                        chain.residue_numbers[i],
                        chain.residue_numbers[i + 1],
                        distance,
                        residue_gap);

                    // Save current segment
                    if current_positions.len() >= 4 {
                        segments.push(CAlphaChain {
                            positions: current_positions.clone(),
                            residue_numbers: current_residues.clone(),
                            chain_id: chain.chain_id,
                        });
                    }

                    // Start new segment
                    current_positions.clear();
                    current_residues.clear();
                }
            }
        }

        // Add last segment
        if current_positions.len() >= 4 {
            segments.push(CAlphaChain {
                positions: current_positions,
                residue_numbers: current_residues,
                chain_id: chain.chain_id,
            });
        }

        // If no breaks found, return original chain
        if segments.is_empty() {
            vec![chain.clone()]
        } else {
            segments
        }
    }

    fn generate_segment_geometry(
        &self,
        segment: &CAlphaChain,
        secondary_structure: &pdb_parser::SecondaryStructure,
        _chain_id: char,
    ) -> (Vec<RibbonVertex>, Vec<u32>) {
        // Create spline from CA positions
        let spline = CatmullRomSpline::new(segment.positions.clone());

        // Subdivide spline (5 segments between each CA)
        let segments_per_interval = 5;
        let (positions, tangents) = spline.subdivide_with_tangents(segments_per_interval);

        // Compute normals using parallel transport
        let initial_normal = if tangents[0].abs_diff_eq(Vec3::Y, 0.1) {
            Vec3::X // Use X if tangent is close to Y
        } else {
            Vec3::Y // Otherwise use Y
        };

        let normals = compute_parallel_transport_normals(&positions, &tangents, initial_normal);
        let binormals = compute_binormals(&tangents, &normals);

        // Detect if we have any secondary structure information
        let has_secondary_structure = !secondary_structure.helices.is_empty()
            || !secondary_structure.sheets.is_empty();

        if !has_secondary_structure {
            log::info!("  Chain {}: No PDB secondary structure, using auto-detection", segment.chain_id);
        }

        // If no secondary structure, use simple detection
        let auto_structure = if !has_secondary_structure {
            let structure = detect_secondary_structure_simple(&segment.positions, &segment.residue_numbers);
            let helix_count = structure.iter().filter(|&&s| s == StructureType::Helix).count();
            let sheet_count = structure.iter().filter(|&&s| s == StructureType::Sheet).count();
            let coil_count = structure.iter().filter(|&&s| s == StructureType::Coil).count();
            log::info!("  Chain {}: Auto-detected {} helices, {} sheets, {} coils",
                segment.chain_id, helix_count, sheet_count, coil_count);
            Some(structure)
        } else {
            None
        };

        // Generate ribbon geometry
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for i in 0..positions.len() {
            // Determine structure type for this position
            let point_idx = i / segments_per_interval;
            let residue_num = if point_idx < segment.residue_numbers.len() {
                segment.residue_numbers[point_idx]
            } else {
                segment.residue_numbers[segment.residue_numbers.len() - 1]
            };

            let structure_type = if let Some(ref auto) = auto_structure {
                // Use auto-detected structure
                let idx = point_idx.min(auto.len().saturating_sub(1));
                auto.get(idx).copied().unwrap_or(StructureType::Coil)
            } else {
                // Use PDB structure records
                get_structure_type(residue_num, segment.chain_id, secondary_structure)
            };

            // Generate cross-section based on structure type
            let cross_section = self.generate_cross_section(
                positions[i],
                normals[i],
                binormals[i],
                &structure_type,
                point_idx,
                segment.residue_numbers.len(),
                secondary_structure,
                segment.chain_id,
                residue_num,
            );

            let color = structure_type.color();
            let base_idx = vertices.len() as u32;

            // Add cross-section vertices
            for (pos, normal) in &cross_section {
                vertices.push(RibbonVertex::new(*pos, *normal, color));
            }

            // Connect to previous cross-section
            if i > 0 {
                let prev_base = base_idx - cross_section.len() as u32;
                let n = cross_section.len() as u32;

                for j in 0..n {
                    let next_j = (j + 1) % n;

                    // Two triangles per quad
                    indices.extend_from_slice(&[
                        prev_base + j,
                        prev_base + next_j,
                        base_idx + j,
                        prev_base + next_j,
                        base_idx + next_j,
                        base_idx + j,
                    ]);
                }
            }
        }

        log::info!("  Chain {}: Generated {} vertices, {} triangles",
            segment.chain_id, vertices.len(), indices.len() / 3);

        (vertices, indices)
    }

    fn generate_cross_section(
        &self,
        center: Vec3,
        normal: Vec3,
        binormal: Vec3,
        structure_type: &StructureType,
        _point_idx: usize,
        _total_residues: usize,
        secondary_structure: &pdb_parser::SecondaryStructure,
        chain_id: char,
        residue_num: i32,
    ) -> Vec<(Vec3, Vec3)> {
        match structure_type {
            StructureType::Helix => {
                // Elliptical ribbon for helices
                let width = 1.5;
                let thickness = 0.3;
                self.create_ellipse_cross_section(center, normal, binormal, width, thickness, 8)
            }
            StructureType::Sheet => {
                // Check if we're near the end of the sheet (for arrow)
                let is_near_end = self.is_near_sheet_end(
                    residue_num,
                    chain_id,
                    secondary_structure,
                    2, // Look ahead 2 residues
                );

                let width = if is_near_end {
                    // Gradually widen to 3.0 for arrow
                    3.0
                } else {
                    2.0
                };

                let thickness = 0.2;
                self.create_rectangle_cross_section(center, normal, binormal, width, thickness)
            }
            StructureType::Coil => {
                // Circular tube for coils (made larger for better visibility)
                let radius = 1.0;
                self.create_circle_cross_section(center, normal, binormal, radius, 8)
            }
        }
    }

    fn create_ellipse_cross_section(
        &self,
        center: Vec3,
        normal: Vec3,
        binormal: Vec3,
        width: f32,
        thickness: f32,
        segments: usize,
    ) -> Vec<(Vec3, Vec3)> {
        let mut result = Vec::new();

        for i in 0..segments {
            let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let cos = angle.cos();
            let sin = angle.sin();

            let offset = normal * (width * 0.5 * cos) + binormal * (thickness * 0.5 * sin);
            let pos = center + offset;
            let n = offset.normalize();

            result.push((pos, n));
        }

        result
    }

    fn create_rectangle_cross_section(
        &self,
        center: Vec3,
        normal: Vec3,
        binormal: Vec3,
        width: f32,
        thickness: f32,
    ) -> Vec<(Vec3, Vec3)> {
        let hw = width * 0.5;
        let ht = thickness * 0.5;

        vec![
            (center + normal * hw + binormal * ht, Vec3::Z),  // Top right
            (center + normal * hw - binormal * ht, Vec3::Z),  // Bottom right
            (center - normal * hw - binormal * ht, Vec3::Z),  // Bottom left
            (center - normal * hw + binormal * ht, Vec3::Z),  // Top left
        ]
    }

    fn create_circle_cross_section(
        &self,
        center: Vec3,
        normal: Vec3,
        binormal: Vec3,
        radius: f32,
        segments: usize,
    ) -> Vec<(Vec3, Vec3)> {
        let mut result = Vec::new();

        for i in 0..segments {
            let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let cos = angle.cos();
            let sin = angle.sin();

            let offset = normal * (radius * cos) + binormal * (radius * sin);
            let pos = center + offset;
            let n = offset.normalize();

            result.push((pos, n));
        }

        result
    }

    fn is_near_sheet_end(
        &self,
        residue_num: i32,
        chain_id: char,
        secondary_structure: &pdb_parser::SecondaryStructure,
        lookahead: i32,
    ) -> bool {
        for sheet in &secondary_structure.sheets {
            if sheet.chain_id == chain_id
                && residue_num >= sheet.start_residue
                && residue_num <= sheet.end_residue
            {
                // Check if we're within lookahead residues of the end
                return residue_num >= sheet.end_residue - lookahead;
            }
        }
        false
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.index_count == 0 {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}

/// Simple secondary structure detection based on CA-CA distances
/// This is a simplified version of DSSP algorithm
fn detect_secondary_structure_simple(
    ca_positions: &[Vec3],
    _residue_numbers: &[i32],
) -> Vec<StructureType> {
    let n = ca_positions.len();
    let mut structure = vec![StructureType::Coil; n];

    if n < 4 {
        return structure;
    }

    // Detect helices based on i to i+3 and i to i+4 distances
    // Alpha helix: CA(i) to CA(i+4) distance ~6.0-6.5 Å
    // Also CA(i) to CA(i+1) ~3.8 Å
    for i in 0..n.saturating_sub(4) {
        let d_i_i1 = (ca_positions[i + 1] - ca_positions[i]).length();
        let d_i_i4 = (ca_positions[i + 4] - ca_positions[i]).length();

        // Check for alpha helix pattern
        if d_i_i1 > 3.6 && d_i_i1 < 4.0 && d_i_i4 > 5.5 && d_i_i4 < 7.0 {
            // Mark this as helix (need at least 4 consecutive residues)
            let mut is_helix = true;
            for j in 0..3 {
                if i + j + 1 >= n {
                    break;
                }
                let d = (ca_positions[i + j + 1] - ca_positions[i + j]).length();
                if d < 3.4 || d > 4.2 {
                    is_helix = false;
                    break;
                }
            }

            if is_helix {
                for j in i..=(i + 4).min(n - 1) {
                    structure[j] = StructureType::Helix;
                }
            }
        }
    }

    // Detect sheets based on CA-CA distances and hydrogen bonding patterns
    // Beta strand: CA(i) to CA(i+1) ~3.3-3.5 Å (more extended than helix)
    for i in 0..n.saturating_sub(3) {
        if structure[i] == StructureType::Helix {
            continue; // Already assigned as helix
        }

        let d_i_i1 = (ca_positions[i + 1] - ca_positions[i]).length();
        let d_i_i2 = (ca_positions[i + 2] - ca_positions[i]).length();

        // Extended conformation characteristic of beta strands
        if d_i_i1 > 3.2 && d_i_i1 < 3.6 && d_i_i2 > 6.0 && d_i_i2 < 7.5 {
            let mut is_sheet = true;
            for j in 0..2 {
                if i + j + 1 >= n {
                    break;
                }
                let d = (ca_positions[i + j + 1] - ca_positions[i + j]).length();
                if d < 3.0 || d > 3.8 {
                    is_sheet = false;
                    break;
                }
            }

            if is_sheet {
                for j in i..=(i + 3).min(n - 1) {
                    if structure[j] != StructureType::Helix {
                        structure[j] = StructureType::Sheet;
                    }
                }
            }
        }
    }

    structure
}

fn get_structure_type(
    residue_num: i32,
    chain_id: char,
    secondary_structure: &pdb_parser::SecondaryStructure,
) -> StructureType {
    // Check helices
    for helix in &secondary_structure.helices {
        if helix.chain_id == chain_id
            && residue_num >= helix.start_residue
            && residue_num <= helix.end_residue
        {
            return StructureType::Helix;
        }
    }

    // Check sheets
    for sheet in &secondary_structure.sheets {
        if sheet.chain_id == chain_id
            && residue_num >= sheet.start_residue
            && residue_num <= sheet.end_residue
        {
            return StructureType::Sheet;
        }
    }

    StructureType::Coil
}
