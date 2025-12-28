// GPU Compute Shader for Frustum Culling + LOD Assignment
// Processes all atoms in parallel on GPU, determining visibility and LOD level

struct Frustum {
    planes: array<vec4<f32>, 6>,
}

struct LodConfig {
    distance_high: f32,
    distance_medium: f32,
    distance_low: f32,
    distance_very_low: f32,
    hysteresis: f32,
    _padding: vec3<f32>,
}

struct AtomData {
    position: vec3<f32>,
    radius: f32,
    color: vec4<f32>,
}

struct CameraData {
    position: vec3<f32>,
    _padding: f32,
}

// DrawIndexedIndirect command structure
struct DrawCommand {
    index_count: u32,           // Number of indices per instance (constant)
    instance_count: atomic<u32>, // Atomically incremented by shader
    first_index: u32,           // Start index in index buffer (constant)
    base_vertex: u32,           // Vertex offset (constant)
    first_instance: u32,        // Instance offset (constant)
}

// Input: atom data
@group(0) @binding(0) var<storage, read> atoms: array<AtomData>;

// Input: frustum planes
@group(0) @binding(1) var<uniform> frustum: Frustum;

// Input: camera position
@group(0) @binding(2) var<uniform> camera: CameraData;

// Input: LOD configuration
@group(0) @binding(3) var<uniform> lod_config: LodConfig;

// Output: draw commands for each LOD level (5 levels)
@group(0) @binding(4) var<storage, read_write> draw_commands: array<DrawCommand, 5>;

// Output: visible atom indices for each LOD level
@group(0) @binding(5) var<storage, read_write> visible_indices_high: array<u32>;
@group(0) @binding(6) var<storage, read_write> visible_indices_medium: array<u32>;
@group(0) @binding(7) var<storage, read_write> visible_indices_low: array<u32>;
@group(0) @binding(8) var<storage, read_write> visible_indices_very_low: array<u32>;
@group(0) @binding(9) var<storage, read_write> visible_indices_impostor: array<u32>;

// Frustum culling test for sphere
fn is_sphere_visible(center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let plane = frustum.planes[i];
        let distance = dot(plane.xyz, center) + plane.w;

        // If sphere is completely behind any plane, it's not visible
        if (distance < -radius) {
            return false;
        }
    }
    return true;
}

// Compute LOD level based on distance to camera
fn compute_lod(distance: f32) -> u32 {
    if (distance < lod_config.distance_high) {
        return 0u; // High
    } else if (distance < lod_config.distance_medium) {
        return 1u; // Medium
    } else if (distance < lod_config.distance_low) {
        return 2u; // Low
    } else if (distance < lod_config.distance_very_low) {
        return 3u; // VeryLow
    } else {
        return 4u; // Impostor
    }
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let atom_idx = global_id.x;

    // Bounds check
    if (atom_idx >= arrayLength(&atoms)) {
        return;
    }

    let atom = atoms[atom_idx];

    // STEP 1: Frustum culling
    if (!is_sphere_visible(atom.position, atom.radius)) {
        return; // Atom is not visible, skip
    }

    // STEP 2: Compute distance to camera
    let distance = length(atom.position - camera.position);

    // STEP 3: Assign LOD level
    let lod_level = compute_lod(distance);

    // STEP 4: Atomically increment instance count and get insertion index
    var insertion_idx: u32;

    switch lod_level {
        case 0u: { // High
            insertion_idx = atomicAdd(&draw_commands[0].instance_count, 1u);
            visible_indices_high[insertion_idx] = atom_idx;
        }
        case 1u: { // Medium
            insertion_idx = atomicAdd(&draw_commands[1].instance_count, 1u);
            visible_indices_medium[insertion_idx] = atom_idx;
        }
        case 2u: { // Low
            insertion_idx = atomicAdd(&draw_commands[2].instance_count, 1u);
            visible_indices_low[insertion_idx] = atom_idx;
        }
        case 3u: { // VeryLow
            insertion_idx = atomicAdd(&draw_commands[3].instance_count, 1u);
            visible_indices_very_low[insertion_idx] = atom_idx;
        }
        case 4u: { // Impostor
            insertion_idx = atomicAdd(&draw_commands[4].instance_count, 1u);
            visible_indices_impostor[insertion_idx] = atom_idx;
        }
        default: {}
    }
}
