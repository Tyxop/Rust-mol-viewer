// GPU Compute Shader for SDF (Signed Distance Field) calculation
// Uses spatial acceleration grid: each voxel only checks 3x3x3 neighboring cells
// instead of iterating over all atoms — O(V × K) vs O(V × A)

struct AtomData {
    position: vec3<f32>,
    radius: f32,  // VdW radius
}

struct GridParams {
    // SDF voxel grid
    sdf_origin: vec3<f32>,
    sdf_spacing: f32,
    sdf_dimensions: vec3<u32>,
    probe_radius: f32,
    // Spatial acceleration grid
    spatial_origin: vec3<f32>,
    spatial_cell_size: f32,
    spatial_dims: vec3<u32>,
    _pad: u32,
}

@group(0) @binding(0)
var<storage, read> atoms: array<AtomData>;  // sorted by spatial cell

@group(0) @binding(1)
var<uniform> params: GridParams;

@group(0) @binding(2)
var<storage, read_write> sdf_values: array<f32>;

@group(0) @binding(3)
var<storage, read> cell_starts: array<u32>;  // start index in atoms[] per cell, 0xFFFFFFFF = empty

@group(0) @binding(4)
var<storage, read> cell_counts: array<u32>;  // atom count per cell

fn compute_sdf_at_voxel(voxel_pos: vec3<u32>) -> f32 {
    let world_pos = params.sdf_origin + vec3<f32>(voxel_pos) * params.sdf_spacing;

    // Map world position into spatial grid coordinates
    let rel = world_pos - params.spatial_origin;
    let cell_f = rel / params.spatial_cell_size;
    let cx = i32(floor(cell_f.x));
    let cy = i32(floor(cell_f.y));
    let cz = i32(floor(cell_f.z));

    let snx = i32(params.spatial_dims.x);
    let sny = i32(params.spatial_dims.y);
    let snz = i32(params.spatial_dims.z);

    var min_dist = 1e10f;

    // Check 3x3x3 neighboring cells — covers all atoms within max_expanded_radius
    for (var dz = -1; dz <= 1; dz++) {
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let ncx = cx + dx;
                let ncy = cy + dy;
                let ncz = cz + dz;

                // Bounds check
                if ncx < 0 || ncy < 0 || ncz < 0 || ncx >= snx || ncy >= sny || ncz >= snz {
                    continue;
                }

                let ci = u32(ncx + ncy * snx + ncz * snx * sny);
                let start = cell_starts[ci];
                if start == 0xFFFFFFFFu { continue; }  // empty cell

                let count = cell_counts[ci];
                for (var k = 0u; k < count; k++) {
                    let atom = atoms[start + k];
                    let dist_to_center = length(world_pos - atom.position);
                    let expanded_radius = atom.radius + params.probe_radius;
                    min_dist = min(min_dist, dist_to_center - expanded_radius);
                }
            }
        }
    }

    return min_dist;
}

// One thread per voxel
@compute @workgroup_size(8, 8, 4)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if (global_id.x >= params.sdf_dimensions.x ||
        global_id.y >= params.sdf_dimensions.y ||
        global_id.z >= params.sdf_dimensions.z) {
        return;
    }

    // Linear index: z * nx * ny + y * nx + x  (matches CPU indexing)
    let nx = params.sdf_dimensions.x;
    let ny = params.sdf_dimensions.y;
    let index = global_id.z * nx * ny + global_id.y * nx + global_id.x;

    sdf_values[index] = compute_sdf_at_voxel(global_id);
}
