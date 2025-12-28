struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec3<f32>,
}

struct MaterialUniform {
    alpha: f32,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var<uniform> material: MaterialUniform;

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Vertices are already in world space (no model matrix needed)
    out.clip_position = camera.view_proj * vec4<f32>(vertex.position, 1.0);
    out.world_pos = vertex.position;
    out.normal = vertex.normal;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Two-sided lighting (flip normal if facing away from camera)
    let view_dir = normalize(camera.view_pos - in.world_pos);
    var normal = normalize(in.normal);

    // Flip normal if facing away from camera
    if (dot(normal, view_dir) < 0.0) {
        normal = -normal;
    }

    // Phong shading
    let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
    let reflect_dir = reflect(-light_dir, normal);

    let ambient = 0.3;
    let diffuse = max(dot(normal, light_dir), 0.0) * 0.6;
    let specular = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0) * 0.3;

    let lighting = ambient + diffuse + specular;

    // Semi-transparent blue surface
    let surface_color = vec3<f32>(0.3, 0.5, 0.8);

    return vec4<f32>(surface_color * lighting, material.alpha);
}
