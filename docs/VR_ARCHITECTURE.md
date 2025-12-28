# VR Architecture Documentation

## Overview

This document describes the technical architecture of the VR implementation in PDB Visual, designed for developers maintaining or extending the VR functionality.

## System Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────────┐
│                     mol-app                             │
│  ┌──────────────────────────────────────────────────┐  │
│  │  Main Event Loop (Desktop/VR Mode Switch)        │  │
│  └──────────────────────────────────────────────────┘  │
│           │                              │              │
│           ├──────Desktop─────┐    ┌─────VR──────┐      │
│           ↓                  ↓    ↓             ↓      │
│  ┌─────────────┐    ┌──────────────────┐  ┌─────────┐  │
│  │   winit     │    │   mol-render     │  │ mol-vr  │  │
│  │  (Window)   │    │   (Renderer)     │  │ (VR)    │  │
│  └─────────────┘    └──────────────────┘  └─────────┘  │
└─────────────────────────────────────────────────────────┘
                              │                   │
                              ↓                   ↓
                      ┌──────────────┐   ┌──────────────┐
                      │   wgpu       │   │  OpenXR      │
                      │  (Graphics)  │   │  (VR API)    │
                      └──────────────┘   └──────────────┘
                              │                   │
                              ↓                   ↓
                      ┌──────────────────────────────┐
                      │   Vulkan / Metal / DX12      │
                      └──────────────────────────────┘
```

## Module Breakdown

### 1. mol-vr Crate

**Purpose**: OpenXR integration and VR-specific functionality

**Location**: `crates/mol-vr/`

#### Module Structure

```
mol-vr/
├── src/
│   ├── lib.rs                 # Public API exports
│   ├── session.rs             # OpenXR session management
│   ├── input.rs               # VR controller input
│   ├── picking.rs             # Ray generation for selection
│   ├── ui_panel.rs            # 3D UI panels
│   ├── ui_interaction.rs      # Ray-quad intersection
│   └── performance.rs         # Frame timing monitoring
└── Cargo.toml
```

#### Key Types

##### VrSession (session.rs)
Manages the OpenXR lifecycle:
- Instance creation and system enumeration
- Session creation with Vulkan graphics binding
- Swapchain management (left + right eye)
- Frame synchronization
- Event handling

```rust
pub struct VrSession {
    pub instance: xr::Instance,
    pub system: xr::SystemId,
    pub session: xr::Session<xr::Vulkan>,
    pub session_state: SessionState,
    pub frame_waiter: xr::FrameWaiter,
    pub frame_stream: xr::FrameStream<xr::Vulkan>,
    pub stage_space: xr::Space,
    pub view_config_type: xr::ViewConfigurationType,
    pub left_swapchain: Option<VrSwapchain>,
    pub right_swapchain: Option<VrSwapchain>,
    pub views: Vec<xr::View>,
    pub input: VrInput,
}
```

**Lifecycle**:
1. `new()` - Initialize OpenXR instance and session
2. `create_swapchains()` - Create render targets for each eye
3. `poll_events()` - Handle OpenXR events (state changes)
4. `begin_frame()` - Wait for next frame, update views
5. *(Rendering happens)*
6. `end_frame()` - Submit to compositor

##### VrInput (input.rs)
Handles controller input via OpenXR action system:

```rust
pub struct VrInput {
    action_set: xr::ActionSet,
    left_hand_pose_action: xr::Action<xr::Posef>,
    right_hand_pose_action: xr::Action<xr::Posef>,
    left_grip_action: xr::Action<bool>,
    right_grip_action: xr::Action<bool>,
    left_joystick_action: xr::Action<xr::Vector2f>,
    right_joystick_action: xr::Action<xr::Vector2f>,
    teleport_action: xr::Action<bool>,
    // ... other actions
}
```

**Action Bindings** (Oculus Touch):
- `/user/hand/left/input/grip/pose` → left_hand_pose
- `/user/hand/right/input/grip/pose` → right_hand_pose
- `/user/hand/left/input/thumbstick` → left_joystick
- `/user/hand/right/input/thumbstick` → right_joystick
- `/user/hand/left/input/thumbstick/click` → teleport
- `/user/hand/left/input/squeeze/click` → left_grip
- `/user/hand/right/input/squeeze/click` → right_grip

##### VrPerformanceMonitor (performance.rs)
Tracks frame timing for VR:

```rust
pub struct VrPerformanceMonitor {
    last_frame: Option<Instant>,
    frame_times: VecDeque<f32>,  // Rolling window
    target_frame_time_ms: f32,   // 11.11ms for 90 FPS
    stats: VrPerformanceStats,
}
```

**Metrics**:
- Current/average/max frame time
- FPS calculation
- Dropped frame count
- Performance rating (% meeting target)

### 2. mol-render Integration

**Location**: `crates/mol-render/src/`

#### VrRenderer (vr_renderer.rs)

Bridges OpenXR and wgpu rendering:

```rust
pub struct VrRenderer {
    pub vr_session: VrSession,
    left_depth_texture: wgpu::Texture,
    left_depth_view: wgpu::TextureView,
    right_depth_texture: wgpu::Texture,
    right_depth_view: wgpu::TextureView,
    left_camera_buffer: wgpu::Buffer,
    right_camera_buffer: wgpu::Buffer,
    left_camera_bind_group: wgpu::BindGroup,
    right_camera_bind_group: wgpu::BindGroup,
    performance_monitor: VrPerformanceMonitor,
}
```

**Rendering Flow**:
1. Acquire swapchain images from OpenXR
2. Update camera uniforms for each eye
3. Render left eye pass
4. Render right eye pass
5. Submit commands to GPU
6. Release swapchain images back to OpenXR

#### Stereo Camera (camera.rs)

Extended Camera struct for stereo rendering:

```rust
pub struct Camera {
    // ... existing fields ...
    pub stereo_config: Option<StereoConfig>,
}

pub struct StereoConfig {
    pub ipd: f32,              // Inter-pupillary distance
    pub left_fov: FovConfig,   // Asymmetric FOV per eye
    pub right_fov: FovConfig,
}
```

**Stereo Calculations**:
- Eye position = camera.position ± (ipd/2) along right vector
- Asymmetric frustum projection from OpenXR FOV angles
- Separate view-projection matrix per eye

### 3. mol-app Integration

**Location**: `crates/mol-app/src/main.rs`

#### Dual-Mode App Structure

```rust
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    trajectory: Option<Trajectory>,
    vr_mode: bool,              // NEW: VR mode flag
    prev_right_grip: bool,      // NEW: Edge detection
    // ... existing fields ...
}
```

**VR Mode Detection**:
```rust
fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let vr_mode = args.iter().any(|arg| arg == "--vr");

    if vr_mode {
        // VR event loop
    } else {
        // Desktop event loop
    }
}
```

## Data Flow

### VR Frame Loop

```
1. OpenXR Frame Sync
   ├─ poll_events() → SessionState updates
   ├─ begin_frame() → FrameState + predicted_display_time
   └─ locate_views() → Head pose + eye poses

2. Input Handling
   ├─ sync_actions() → Controller state update
   ├─ get_controller_state() → Poses + buttons + joysticks
   ├─ apply_vr_rotation() → Molecule rotation
   ├─ apply_vr_movement() → Camera translation
   ├─ apply_vr_teleport() → Instant repositioning
   └─ grip_selection → Atom picking

3. LOD & Culling (CPU or GPU)
   ├─ update_frustum() → From stereo view-projection
   ├─ compute_lod() → Distance-based LOD assignment
   └─ is_sphere_visible() → Frustum culling

4. Stereo Rendering
   ├─ acquire_images() → Left + right swapchain images
   ├─ LEFT EYE PASS
   │  ├─ update_camera_uniform(Eye::Left)
   │  ├─ begin_render_pass(left_swapchain, left_depth)
   │  └─ render_scene()
   ├─ RIGHT EYE PASS
   │  ├─ update_camera_uniform(Eye::Right)
   │  ├─ begin_render_pass(right_swapchain, right_depth)
   │  └─ render_scene()
   └─ release_images()

5. Frame Submission
   ├─ queue.submit(commands)
   └─ end_frame(frame_state) → Compositor
```

## Performance Optimizations

### LOD System

**5 Levels** based on camera distance:

| Level | Distance (Å) | Geometry | Triangles |
|-------|-------------|----------|-----------|
| High | 0-50 | Icosphere (subdiv 3) | 512 |
| Medium | 50-150 | Icosphere (subdiv 2) | 128 |
| Low | 150-500 | Icosphere (subdiv 1) | 32 |
| Very Low | 500-1000 | Octahedron | 8 |
| Impostor | >1000 | Billboard | 2 |

**Hysteresis**: 10% overlap prevents visual popping during transitions

### Frustum Culling

- VR benefits from narrower FOV per eye (~45° vs 90° desktop)
- More atoms culled → fewer draw calls
- Implemented in `CullingSystem` using view-projection planes

### GPU Compute Path

When available:
- Culling + LOD assignment done in compute shader
- Writes indirect draw commands directly
- CPU path used as fallback

**Compute Shader**: `crates/mol-render/assets/shaders/culling_compute.wgsl`

## OpenXR-wgpu Interop

### Challenge
OpenXR uses native graphics APIs (Vulkan, D3D12, OpenGL)
wgpu is a cross-platform abstraction

### Solution
1. Create wgpu with Vulkan backend: `wgpu::Backends::VULKAN`
2. OpenXR uses same Vulkan instance/device
3. Swapchain images shared between OpenXR and wgpu
4. **TODO**: Currently using placeholder textures, need proper wgpu-hal integration

### Proper Integration (Future)
```rust
// Get Vulkan handles from wgpu
let vk_instance = unsafe { wgpu_hal::vulkan::Instance::from_wgpu(&instance) };
let vk_device = unsafe { wgpu_hal::vulkan::Device::from_wgpu(&device) };

// Create OpenXR session with these handles
let session_create_info = xr::vulkan::SessionCreateInfo {
    instance: vk_instance.handle(),
    physical_device: vk_device.physical_device(),
    device: vk_device.handle(),
    // ...
};

// Wrap OpenXR swapchain images as wgpu textures
for xr_image in xr_images {
    let texture = unsafe {
        device.create_texture_from_hal(
            wgpu_hal::vulkan::Texture::from_raw(xr_image),
            &texture_desc
        )
    };
}
```

## Shader Coordination

### Camera Uniform

Shared between desktop and VR:

```wgsl
struct CameraUniform {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    position: vec3<f32>,
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;
```

**VR**: Updated twice per frame (left + right camera bind groups)
**Desktop**: Updated once per frame

### UI Quad Shader

`assets/shaders/ui_quad.wgsl`:
- Renders UI panels as textured quads in 3D
- Uses panel transform + camera view-projection
- Alpha blending for transparency

## File Structure Summary

```
pdbvisual/
├── crates/
│   ├── mol-vr/                    # VR-specific functionality
│   │   ├── src/
│   │   │   ├── session.rs         # OpenXR session (300 lines)
│   │   │   ├── input.rs           # Controller input (350 lines)
│   │   │   ├── picking.rs         # Ray generation (80 lines)
│   │   │   ├── ui_panel.rs        # 3D UI panels (310 lines)
│   │   │   ├── ui_interaction.rs  # Ray-quad test (145 lines)
│   │   │   └── performance.rs     # Frame timing (200 lines)
│   │   └── Cargo.toml
│   │
│   ├── mol-render/                # Rendering
│   │   ├── src/
│   │   │   ├── vr_renderer.rs     # VR stereo renderer (340 lines)
│   │   │   ├── camera.rs          # Stereo camera (+140 lines)
│   │   │   ├── lod.rs             # LOD system (existing)
│   │   │   └── renderer.rs        # Main renderer (+vr_renderer field)
│   │   └── Cargo.toml
│   │
│   └── mol-app/                   # Application
│       ├── src/
│       │   └── main.rs            # Dual-mode event loop (+90 lines)
│       └── Cargo.toml
│
├── assets/
│   └── shaders/
│       └── ui_quad.wgsl           # UI quad shader (50 lines)
│
└── docs/
    ├── VR_USAGE.md                # User guide
    └── VR_ARCHITECTURE.md         # This file
```

## Key Design Decisions

### 1. Dual-Mode Application
**Decision**: Single binary supports both desktop and VR modes

**Rationale**:
- No need for separate VR build
- Easy testing and development
- Code reuse between modes

**Implementation**: `--vr` flag + mode detection

### 2. OpenXR Over Platform-Specific APIs
**Decision**: Use OpenXR instead of Oculus SDK, SteamVR, etc.

**Rationale**:
- Cross-platform standard (Khronos)
- Single implementation for all headsets
- Future-proof

**Trade-off**: Requires OpenXR runtime installation

### 3. Action System for Input
**Decision**: Use OpenXR action system instead of raw input

**Rationale**:
- Device-agnostic (works with any controller)
- User-remappable bindings
- Semantic actions (grip, trigger) vs raw buttons

**Implementation**: Action set with suggested bindings for Oculus Touch

### 4. Separate Camera Bind Groups per Eye
**Decision**: Maintain separate uniform buffers for left/right eye

**Rationale**:
- Avoids re-binding during render passes
- Cleaner code (each pass is independent)
- Minimal overhead (2 small buffers)

**Alternative**: Single buffer updated between passes (more complex)

### 5. Performance Monitoring in VrRenderer
**Decision**: Integrate monitoring directly into renderer

**Rationale**:
- Automatic tracking (developers can't forget)
- Accurate timing (around actual render calls)
- Warnings logged for debugging

**Trade-off**: Slight coupling, but worth it for reliability

## Extension Points

### Adding New Controller Profiles

1. In `VrInput::new()`, add new interaction profile:
```rust
let new_profile = instance.string_to_path(
    "/interaction_profiles/valve/index_controller"
)?;

instance.suggest_interaction_profile_bindings(
    new_profile,
    &[/* bindings */],
)?;
```

### Adding New Input Actions

1. Add action field to `VrInput`
2. Create action in `VrInput::new()`
3. Add binding in interaction profile
4. Query state in `get_controller_state()`

### Custom LOD Distances

Modify `LodConfig::default()` in `lod.rs`:
```rust
impl Default for LodConfig {
    fn default() -> Self {
        Self {
            distance_high: 75.0,     // Changed from 50.0
            distance_medium: 200.0,  // Changed from 150.0
            // ...
        }
    }
}
```

## Testing

### Without VR Hardware

1. **OpenXR Null Driver**: Simulates headset
2. **Monado**: Open-source OpenXR runtime (Linux)
3. **SteamVR Null Driver**: For Windows

### With VR Hardware

1. Start VR runtime
2. Launch with `--vr` flag
3. Monitor logs for performance warnings
4. Test all input actions

## Future Work

### Immediate
- [ ] Proper wgpu-hal Vulkan interop (remove placeholder textures)
- [ ] Interactive UI panels (event injection)
- [ ] Haptic feedback on atom selection

### Short-term
- [ ] Fixed foveated rendering (Quest optimization)
- [ ] Asynchronous spacewarp (ASW) support
- [ ] VR-specific LOD configuration

### Long-term
- [ ] Hand tracking (no controllers)
- [ ] Multi-user collaboration
- [ ] Molecular editing in VR
- [ ] Custom controller bindings UI

## References

- [OpenXR Specification](https://www.khronos.org/registry/OpenXR/)
- [openxr-rs Documentation](https://docs.rs/openxr/)
- [wgpu Documentation](https://docs.rs/wgpu/)
- [Meta Quest Development](https://developer.oculus.com/)
