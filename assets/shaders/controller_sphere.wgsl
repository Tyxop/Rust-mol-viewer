// controller_sphere.wgsl
// Simple shaded sphere for VR controller visualisation (red = right, blue = left).

struct CameraUniform {
    view_proj:     mat4x4<f32>,
    view:          mat4x4<f32>,
    proj:          mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view_pos:      vec3<f32>,
    _padding:      f32,
}
@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct CtrlUniform {
    model: mat4x4<f32>,   // mol-space transform (pre-multiplied with mol_to_world.inverse())
    color: vec4<f32>,     // RGBA controller colour
}
@group(1) @binding(0) var<uniform> ctrl: CtrlUniform;

struct VOut {
    @builtin(position) clip:   vec4<f32>,
    @location(0)       normal: vec3<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
) -> VOut {
    var out: VOut;
    let world_pos  = ctrl.model * vec4<f32>(position, 1.0);
    out.clip       = camera.view_proj * world_pos;
    // Transform normal by the upper-left 3×3 of the model matrix (no translation)
    out.normal     = normalize((ctrl.model * vec4<f32>(normal, 0.0)).xyz);
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let light = normalize(vec3<f32>(0.4, 1.0, 0.6));
    let ambient  = 0.35;
    let diffuse  = max(dot(normalize(in.normal), light), 0.0);
    let intensity = ambient + (1.0 - ambient) * diffuse;
    return vec4<f32>(ctrl.color.rgb * intensity, ctrl.color.a);
}
