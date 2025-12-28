struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec3<f32>,
}

struct BillboardInstance {
    position: vec3<f32>,
    radius: f32,
    color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_pos: vec3<f32>,
    @location(3) instance_pos: vec3<f32>,
    @location(4) instance_radius: f32,
}

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

// Vertex shader for billboards
// Input: quad vertices in [-1,1] range
// Output: world-space billboard facing camera

@vertex
fn vs_main(
    @location(0) vertex_pos: vec2<f32>,       // Quad vertex position
    @location(1) instance_pos: vec3<f32>,     // Billboard center
    @location(2) instance_radius: f32,        // Billboard radius
    @location(3) instance_color: vec4<f32>,   // Billboard color
) -> VertexOutput {
    var out: VertexOutput;

    // Calculate billboard orientation (always face camera)
    let to_camera = normalize(camera.view_pos - instance_pos);
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let right = normalize(cross(up, to_camera));
    let billboard_up = cross(to_camera, right);

    // Scale and position the quad
    let world_pos = instance_pos
        + right * vertex_pos.x * instance_radius
        + billboard_up * vertex_pos.y * instance_radius;

    out.clip_position = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.tex_coord = vertex_pos * 0.5 + 0.5; // Convert [-1,1] to [0,1]
    out.color = instance_color;
    out.world_pos = world_pos;
    out.instance_pos = instance_pos;
    out.instance_radius = instance_radius;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;

    // Create circular impostor
    let center = vec2<f32>(0.5, 0.5);
    let dist = distance(in.tex_coord, center);

    // Discard pixels outside circle
    if (dist > 0.5) {
        discard;
    }

    // Reconstruct sphere normal from UV coordinates
    // Map UV [0,1] to sphere surface [-1,1]
    let x = (in.tex_coord.x - 0.5) * 2.0;
    let y = (in.tex_coord.y - 0.5) * 2.0;
    let z_sq = 1.0 - x*x - y*y;

    if (z_sq < 0.0) {
        discard; // Outside sphere (should not happen after dist check)
    }

    let z = sqrt(z_sq);
    let normal = normalize(vec3<f32>(x, y, z));

    // Phong lighting (match sphere.wgsl exactly)
    let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
    let view_dir = normalize(camera.view_pos - in.world_pos);
    let reflect_dir = reflect(-light_dir, normal);

    let ambient = 0.3;
    let diffuse = max(dot(normal, light_dir), 0.0) * 0.6;
    let specular = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0) * 0.3;

    let lighting = ambient + diffuse + specular;

    // Compute actual sphere surface position for depth correction
    let surface_offset = vec3<f32>(x, y, z) * in.instance_radius;
    let surface_world_pos = in.instance_pos + surface_offset;
    let surface_clip_pos = camera.view_proj * vec4<f32>(surface_world_pos, 1.0);

    // Output color and corrected depth
    out.color = vec4<f32>(in.color.rgb * lighting, in.color.a);
    out.depth = surface_clip_pos.z / surface_clip_pos.w;

    return out;
}
