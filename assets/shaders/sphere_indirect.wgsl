// GPU-Driven Sphere Rendering with Indirect Drawing
// Vertex shader reads atom data from storage buffer using visible indices from compute shader

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec3<f32>,
}

struct AtomData {
    position: vec3<f32>,
    radius: f32,
    color: vec4<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var<storage, read> atoms: array<AtomData>;

@group(1) @binding(1)
var<storage, read> visible_indices: array<u32>;

@vertex
fn vs_main(
    vertex: VertexInput,
    @builtin(instance_index) instance_idx: u32,
) -> VertexOutput {
    var out: VertexOutput;

    // Get the actual atom index from visible indices
    let atom_idx = visible_indices[instance_idx];
    let atom = atoms[atom_idx];

    // Build model matrix from atom position and radius
    let scale_mat = mat4x4<f32>(
        vec4<f32>(atom.radius, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, atom.radius, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, atom.radius, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0)
    );

    let translate_mat = mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(atom.position, 1.0)
    );

    let model_matrix = translate_mat * scale_mat;

    // Transform vertex
    let world_position = model_matrix * vec4<f32>(vertex.position, 1.0);
    out.clip_position = camera.view_proj * world_position;
    out.world_position = world_position.xyz;

    // Transform normal (use upper 3x3 of model matrix)
    let normal_matrix = mat3x3<f32>(
        model_matrix[0].xyz,
        model_matrix[1].xyz,
        model_matrix[2].xyz
    );
    out.world_normal = normalize(normal_matrix * vertex.normal);

    out.color = atom.color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Phong lighting (matching sphere.wgsl)
    let light_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
    let view_dir = normalize(camera.view_pos - in.world_position);
    let reflect_dir = reflect(-light_dir, in.world_normal);

    let ambient = 0.3;
    let diffuse = max(dot(in.world_normal, light_dir), 0.0) * 0.6;
    let specular = pow(max(dot(view_dir, reflect_dir), 0.0), 32.0) * 0.3;

    let lighting = ambient + diffuse + specular;

    return vec4<f32>(in.color.rgb * lighting, in.color.a);
}
