struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec3<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Vertices are already in world space (no model matrix needed)
    out.clip_position = camera.view_proj * vec4<f32>(vertex.position, 1.0);
    out.world_pos = vertex.position;
    out.normal = vertex.normal;
    out.color = vertex.color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Phong shading
    let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
    let view_dir = normalize(camera.view_pos - in.world_pos);
    let normal = normalize(in.normal);
    let reflect_dir = reflect(-light_dir, normal);

    let ambient = 0.3;
    let diffuse = max(dot(normal, light_dir), 0.0) * 0.6;
    let specular = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0) * 0.3;

    let lighting = ambient + diffuse + specular;
    return vec4<f32>(in.color.rgb * lighting, in.color.a);
}
