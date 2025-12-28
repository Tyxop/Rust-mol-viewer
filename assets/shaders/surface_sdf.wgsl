// GPU Compute Shader for SDF (Signed Distance Field) calculation
// Massively parallel computation of molecular surface distance field

struct AtomData {
    position: vec3<f32>,
    radius: f32,  // VdW radius
}

struct GridParams {
    origin: vec3<f32>,
    spacing: f32,
    dimensions: vec3<u32>,  // nx, ny, nz
    probe_radius: f32,
}

@group(0) @binding(0)
var<storage, read> atoms: array<AtomData>;

@group(0) @binding(1)
var<uniform> params: GridParams;

@group(0) @binding(2)
var<storage, read_write> sdf_values: array<f32>;

// Compute SDF value at a single voxel
fn compute_sdf_at_voxel(voxel_pos: vec3<u32>) -> f32 {
    // Convert voxel coordinates to world position
    let world_pos = params.origin + vec3<f32>(voxel_pos) * params.spacing;

    // Find minimum distance to any atom surface (expanded by probe)
    var min_dist = 1e10;  // Large initial value

    let atom_count = arrayLength(&atoms);
    for (var i = 0u; i < atom_count; i++) {
        let atom = atoms[i];

        // Distance from voxel to atom center
        let dist_to_center = length(world_pos - atom.position);

        // SAS: distance to (VdW radius + probe radius)
        let expanded_radius = atom.radius + params.probe_radius;
        let dist_to_surface = dist_to_center - expanded_radius;

        min_dist = min(min_dist, dist_to_surface);
    }

    return min_dist;
}

// Main compute shader - one thread per voxel
@compute @workgroup_size(8, 8, 8)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    // Bounds check
    if (global_id.x >= params.dimensions.x ||
        global_id.y >= params.dimensions.y ||
        global_id.z >= params.dimensions.z) {
        return;
    }

    // Calculate linear index (must match CPU indexing: z * nx * ny + y * nx + x)
    let nx = params.dimensions.x;
    let ny = params.dimensions.y;
    let index = global_id.z * nx * ny + global_id.y * nx + global_id.x;

    // Compute and store SDF value
    sdf_values[index] = compute_sdf_at_voxel(global_id);
}
