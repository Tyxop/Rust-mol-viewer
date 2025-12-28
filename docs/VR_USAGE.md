# VR Usage Guide - PDB Visual

## Overview

PDB Visual supports full virtual reality (VR) visualization of molecular structures using OpenXR. This allows you to explore protein structures in immersive 3D with natural hand-tracked controls.

## Supported Devices

Any OpenXR-compatible VR headset:
- **Meta Quest 2/3/Pro** (via Meta Quest Link or Air Link)
- **Valve Index**
- **HTC Vive/Vive Pro**
- **Windows Mixed Reality headsets**
- **Varjo XR-3**
- Any other OpenXR 1.0+ compatible device

## System Requirements

### Hardware
- VR headset with OpenXR runtime installed
- GPU capable of 90 FPS at your headset's resolution
- Recommended: RTX 3060 or better, 16GB RAM

### Software
- **Windows**: SteamVR or Oculus runtime
- **Linux**: Monado OpenXR runtime
- **macOS**: Not officially supported (OpenXR limited on macOS)

## Quick Start

### 1. Launch in VR Mode

```bash
# From project root
cargo run --package mol-app --release -- --vr path/to/protein.pdb

# Example with sample protein
cargo run --package mol-app --release -- --vr 9PZW.pdb
```

**Note**: The `--vr` flag is required to start in VR mode. Without it, the application runs in desktop mode.

### 2. First-Time Setup

1. **Start your VR runtime** before launching the app:
   - **Quest**: Launch Quest Link or Air Link
   - **SteamVR**: Open SteamVR
   - **Windows MR**: Start Mixed Reality Portal

2. **Put on your headset** - The app will automatically detect and use it

3. **Calibrate your play space** using your VR runtime's room setup

## VR Controls

### Controller Layout (Oculus Touch)

The application is configured for Oculus Touch controllers but works with any OpenXR-compatible controllers.

#### Left Controller
- **Thumbstick**: Move camera position
  - Left/Right: Strafe
  - Up/Down: Move forward/backward
- **Thumbstick Click**: Teleport forward 50 Ångströms
- **Grip Button**: *(Reserved for future use)*

#### Right Controller
- **Thumbstick**: Rotate molecule
  - Left/Right: Rotate around Y axis
  - Up/Down: Rotate around X axis
- **Grip Button**: Select atom (ray pick)
- **Trigger**: *(Reserved for UI interaction)*

### Movement Controls

#### Camera Translation (Left Thumbstick)
- Speed: 10 Ångströms/second
- Smooth, delta-time-based movement
- Moves camera while maintaining view direction

#### Molecule Rotation (Right Thumbstick)
- Rotation speed: 2 radians/second
- Orbits molecule around its center
- Maintains camera distance

#### Teleportation (Left Thumbstick Click)
- Instant jump 50 Å in controller's forward direction
- Use for quick repositioning
- Prevents motion sickness for large movements

### Atom Selection

1. Point right controller at an atom
2. Press and release **grip button**
3. Selected atom info appears in UI panel
4. Multi-select: Hold Shift/Ctrl while clicking *(requires keyboard)*

### Visual Feedback

- **Selected atoms**: Highlighted in yellow
- **Controller rays**: *(Future: visible laser pointer)*
- **UI panels**: Floating in 3D space

## Performance

### Target Frame Rate
- **90 FPS** (11.11ms per frame) for smooth VR experience
- Performance warnings logged if target not met

### Automatic Optimizations
- **LOD System**: 5 levels of detail based on distance
  - Distant atoms use simpler geometry (8 tris vs 512 tris)
- **Frustum Culling**: Off-screen atoms not rendered
- **GPU Compute**: Culling done on GPU when available

### Performance Tips

1. **Large molecules (>10,000 atoms)**:
   - Performance may vary based on GPU
   - LOD system automatically reduces detail

2. **If experiencing lag**:
   - Check VR runtime settings (reduce supersampling)
   - Ensure GPU drivers are up to date
   - Close other applications

3. **Monitor performance**:
   - Warnings appear in logs if <95% frames hit 90 FPS
   - Example: `WARN: VR Performance Warning: Only 87% of frames meeting target`

## Representations in VR

All molecular representations work in VR:

- **Van der Waals** (default): Full atomic spheres
- **Ball & Stick**: Atoms + bonds
- **Ribbon**: Protein backbone
- **Surface**: Molecular surface

Switch representations using keyboard shortcuts (1-4) or UI.

## UI in VR

*(Future feature - Phase 3 implementation pending integration)*

- UI panels render as 3D quads in VR space
- Interact with panels using controller ray + trigger
- Panels positioned at comfortable viewing distance

## Troubleshooting

### "OpenXR not available" Error

**Cause**: VR runtime not running or OpenXR not installed

**Solution**:
1. Start your VR runtime (SteamVR, Oculus, etc.)
2. Ensure headset is connected and detected
3. Check OpenXR is installed: `openxr_runtime.json` should exist
4. Try running the app again with `--vr` flag

### Black Screen in Headset

**Cause**: Render target mismatch or swapchain issues

**Solution**:
1. Restart the application
2. Restart your VR runtime
3. Check logs for OpenXR errors
4. Ensure GPU supports Vulkan 1.1+

### Poor Performance / Stuttering

**Cause**: GPU cannot maintain 90 FPS

**Solutions**:
1. Check performance warnings in logs
2. Reduce VR runtime supersampling
3. Close background applications
4. Use simpler representation (Van der Waals → Ball & Stick)

### Controllers Not Working

**Cause**: OpenXR action bindings not configured

**Solution**:
1. Ensure controllers are paired and tracking
2. Check SteamVR controller bindings
3. Restart the application
4. Check logs for input system errors

### Molecule Appears Too Large/Small

**Cause**: Camera distance not optimal for molecule size

**Solution**:
1. Use **teleport** (left thumbstick click) to adjust distance
2. Use **left thumbstick** to move closer/farther
3. Press **R** on keyboard to reset camera

## Advanced Usage

### Desktop + VR Hybrid

You can run the app on your monitor while in VR (companion window):
- Desktop window shows same view as VR
- Useful for demonstrations or recording

### Performance Monitoring

Enable verbose logging to see performance stats:
```bash
RUST_LOG=info cargo run --package mol-app --release -- --vr protein.pdb
```

Look for performance messages:
```
INFO: VR renderer initialized successfully
WARN: VR Performance Warning: Only 92.1% of frames meeting 90 FPS target
```

### Custom OpenXR Runtime

Set runtime via environment variable:
```bash
# Linux/macOS
export XR_RUNTIME_JSON=/path/to/openxr_runtime.json

# Windows
set XR_RUNTIME_JSON=C:\path\to\openxr_runtime.json
```

## Known Limitations

1. **UI Interaction**: 3D UI panels not yet interactive (Phase 3 pending)
2. **Hand Tracking**: Requires controllers, no hand tracking support yet
3. **Multi-user**: Single-user only
4. **Haptic Feedback**: Not yet implemented
5. **Passthrough**: No AR/passthrough support

## Tips for Best Experience

1. **Start with small molecules** (<5,000 atoms) to learn controls
2. **Use teleport** for large movements to avoid motion sickness
3. **Adjust camera distance** to comfortable viewing range (50-200 Å)
4. **Take breaks** - VR sessions should be <30 minutes initially
5. **Good lighting** - Ensure your play space is well-lit for tracking

## Support

For issues or questions:
- Check the logs for error messages
- Report issues on GitHub
- Include VR hardware info and logs

## Future Features (Roadmap)

- [ ] Interactive 3D UI panels
- [ ] Haptic feedback on atom selection
- [ ] Multi-user collaboration
- [ ] Hand tracking support
- [ ] Custom controller bindings
- [ ] VR-specific visualization modes
- [ ] Molecular editing in VR
