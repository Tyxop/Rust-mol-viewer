// GPU Compute Shader for Cylinder Frustum Culling + LOD Assignment
// Processes bond cylinders in parallel on GPU

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

// Cylinder data: start and end positions, radius, color
struct CylinderData {
    start: vec3<f32>,
    radius: f32,
    end: vec3<f32>,
    _padding: f32,
    color: vec4<f32>,
}

struct CameraData {
    position: vec3<f32>,
    _padding: f32,
}

// DrawIndexedIndirect command structure
struct DrawCommand {
    index_count: u32,
    instance_count: atomic<u32>,
    first_index: u32,
    base_vertex: u32,
    first_instance: u32,
}

// Input: cylinder data
@group(0) @binding(0) var<storage, read> cylinders: array<CylinderData>;

// Input: frustum planes
@group(0) @binding(1) var<uniform> frustum: Frustum;

// Input: camera position
@group(0) @binding(2) var<uniform> camera: CameraData;

// Input: LOD configuration
@group(0) @binding(3) var<uniform> lod_config: LodConfig;

// Output: draw commands for each LOD level (3 levels for cylinders)
@group(0) @binding(4) var<storage, read_write> draw_commands: array<DrawCommand, 3>;

// Output: visible cylinder indices for each LOD level
@group(0) @binding(5) var<storage, read_write> visible_indices_high: array<u32>;
@group(0) @binding(6) var<storage, read_write> visible_indices_medium: array<u32>;
@group(0) @binding(7) var<storage, read_write> visible_indices_low: array<u32>;

// Frustum culling test for cylinder (AABB approximation)
fn is_cylinder_visible(start: vec3<f32>, end: vec3<f32>, radius: f32) -> bool {
    // Create axis-aligned bounding box for cylinder
    let min_point = min(start, end) - vec3<f32>(radius, radius, radius);
    let max_point = max(start, end) + vec3<f32>(radius, radius, radius);

    // Test AABB against all frustum planes
    for (var i = 0u; i < 6u; i++) {
        let plane = frustum.planes[i];

        // Get positive vertex (farthest point in plane normal direction)
        let p_vertex = vec3<f32>(
            select(min_point.x, max_point.x, plane.x >= 0.0),
            select(min_point.y, max_point.y, plane.y >= 0.0),
            select(min_point.z, max_point.z, plane.z >= 0.0)
        );

        let distance = dot(plane.xyz, p_vertex) + plane.w;

        // If p_vertex is behind plane, entire box is outside
        if (distance < 0.0) {
            return false;
        }
    }

    return true;
}

// Compute LOD level based on distance to camera
fn compute_lod(distance: f32) -> u32 {
    if (distance < lod_config.distance_high) {
        return 0u; // High (16 sides)
    } else if (distance < lod_config.distance_medium) {
        return 1u; // Medium (8 sides)
    } else {
        return 2u; // Low (4 sides)
    }
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let cylinder_idx = global_id.x;

    // Bounds check
    if (cylinder_idx >= arrayLength(&cylinders)) {
        return;
    }

    let cylinder = cylinders[cylinder_idx];

    // STEP 1: Frustum culling
    if (!is_cylinder_visible(cylinder.start, cylinder.end, cylinder.radius)) {
        return; // Cylinder is not visible, skip
    }

    // STEP 2: Compute distance to camera (use cylinder center)
    let center = (cylinder.start + cylinder.end) * 0.5;
    let distance = length(center - camera.position);

    // STEP 3: Assign LOD level
    let lod_level = compute_lod(distance);

    // STEP 4: Atomically increment instance count and get insertion index
    var insertion_idx: u32;

    switch lod_level {
        case 0u: { // High (16 sides)
            insertion_idx = atomicAdd(&draw_commands[0].instance_count, 1u);
            visible_indices_high[insertion_idx] = cylinder_idx;
        }
        case 1u: { // Medium (8 sides)
            insertion_idx = atomicAdd(&draw_commands[1].instance_count, 1u);
            visible_indices_medium[insertion_idx] = cylinder_idx;
        }
        case 2u: { // Low (4 sides)
            insertion_idx = atomicAdd(&draw_commands[2].instance_count, 1u);
            visible_indices_low[insertion_idx] = cylinder_idx;
        }
        default: {}
    }
}
