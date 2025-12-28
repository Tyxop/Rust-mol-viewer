// VR UI Quad Shader
// Renders UI panels as textured quads in 3D space

// Camera uniforms (bind group 0)
struct CameraUniform {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    position: vec3<f32>,
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

// Panel transform (bind group 1, binding 0)
struct PanelTransform {
    model: mat4x4<f32>,
}

@group(1) @binding(0)
var<uniform> panel: PanelTransform;

// UI texture (bind group 1, binding 1)
@group(1) @binding(1)
var ui_texture: texture_2d<f32>;

// Sampler (bind group 1, binding 2)
@group(1) @binding(2)
var ui_sampler: sampler;

// Vertex input
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Transform vertex to world space, then to clip space
    let world_pos = panel.model * vec4<f32>(in.position, 1.0);
    out.clip_position = camera.view_proj * world_pos;
    out.uv = in.uv;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the UI texture
    let color = textureSample(ui_texture, ui_sampler, in.uv);

    // Return with alpha blending
    return color;
}
