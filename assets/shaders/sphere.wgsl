struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec3<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct InstanceInput {
    @location(2) model_matrix_0: vec4<f32>,
    @location(3) model_matrix_1: vec4<f32>,
    @location(4) model_matrix_2: vec4<f32>,
    @location(5) model_matrix_3: vec4<f32>,
    @location(6) color: vec4<f32>,
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
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let world_pos = model_matrix * vec4<f32>(vertex.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.world_pos = world_pos.xyz;

    // Transform normal to world space (assuming uniform scaling)
    let normal_matrix = mat3x3<f32>(
        model_matrix[0].xyz,
        model_matrix[1].xyz,
        model_matrix[2].xyz,
    );
    out.normal = normalize(normal_matrix * vertex.normal);

    out.color = instance.color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Phong shading
    let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
    let view_dir = normalize(camera.view_pos - in.world_pos);
    let reflect_dir = reflect(-light_dir, normalize(in.normal));

    let ambient = 0.3;
    let diffuse = max(dot(normalize(in.normal), light_dir), 0.0) * 0.6;
    let specular = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0) * 0.3;

    let lighting = ambient + diffuse + specular;
    return vec4<f32>(in.color.rgb * lighting, in.color.a);
}
