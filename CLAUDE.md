# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**PDB Visual** is a high-performance 3D graphics engine in Rust for visualizing protein structures from PDB (Protein Data Bank) files. The project targets desktop and VR platforms with advanced rendering techniques including GPU compute shaders, LOD systems, and molecular surface generation.

**Key Technologies:**
- **wgpu 22.0** - Modern GPU abstraction (Vulkan/Metal/DirectX 12)
- **egui 0.29** - Immediate-mode GUI
- **nom 7.1** - Parser combinators for PDB format
- **rayon 1.10** - Data parallelism
- **nalgebra 0.33** - Linear algebra for splines and transformations

## Build & Run Commands

### Basic Commands
```bash
# Build in debug mode
cargo build

# Build in release mode (required for good performance)
cargo build --release

# Run with a PDB file (release mode recommended)
cargo run --package mol-app --release -- 9PZW.pdb

# Run without file (test sphere)
cargo run --package mol-app --release

# With debug logging
RUST_LOG=debug cargo run --package mol-app --release -- 9PZW.pdb
RUST_LOG=info cargo run --package mol-app --release -- 9PZW.pdb
```

### Testing
```bash
# Run all tests
cargo test --workspace

# Test specific crate
cargo test --package pdb-parser
cargo test --package mol-render
```

### Development Tools
```bash
# Linting
cargo clippy --workspace

# Code formatting
cargo fmt --all

# Check compilation without building
cargo check --workspace
```

### Release Profiles
The workspace defines two release profiles:
- `release` - Standard optimizations (LTO=thin, codegen-units=16)
- `release-fast` - Maximum optimization (LTO=fat, codegen-units=1, stripped)

## Architecture

### Workspace Structure

The project is organized as a Cargo workspace with 6 crates:

```
crates/
├── pdb-parser/     # PDB file parsing and data structures
├── mol-render/     # wgpu rendering engine
├── mol-ui/         # egui user interface
├── mol-app/        # Main application (event loop, integration)
├── mol-core/       # Shared utilities (placeholder)
└── mol-vr/         # OpenXR VR integration (placeholder)
```

### pdb-parser: PDB File Parser

**Purpose:** Parse PDB files and manage molecular data structures.

**Key modules:**
- `parser.rs` - nom-based parser for ATOM, HETATM, HELIX, SHEET, CONECT, MODEL/ENDMDL records
- `structures.rs` - Core data types: `Atom`, `Protein`, `Trajectory`, `Bond`, `Chain`, `SecondaryStructure`
- `bonds.rs` - Bond inference algorithms (distance-based with spatial partitioning)
- `spatial.rs` - Octree spatial data structure for O(log n) neighbor queries

**Key concepts:**
- **Protein** - Single frame with atoms, bonds, chains, secondary structures
- **Trajectory** - Multi-frame animation with shared topology + per-frame positions
- Bond inference uses Van der Waals radii + 0.4Å tolerance
- Octree partitions space for fast spatial queries (bond inference, atom selection, culling)

### mol-render: Rendering Engine

**Purpose:** High-performance molecular visualization with wgpu.

**Key modules:**
- `renderer.rs` - Main renderer with `Renderer` struct, manages GPU state and render passes
- `camera.rs` - Orbit camera (rotation, pan, zoom)
- `representations/` - Four visualization modes (see below)
- `geometry.rs` - Procedural geometry (icospheres, cylinders)
- `culling.rs` - Frustum culling system
- `lod.rs` - 5-level LOD system (High, Medium, Low, VeryLow, Impostor)
- `marching_cubes.rs` - Surface extraction algorithm
- `splines.rs` - Cubic splines for ribbon rendering
- `benchmark.rs` - CPU vs GPU performance tracking

**Representation System:**

The engine supports 4 visualization modes (`RepresentationType` enum):

1. **VanDerWaals (spheres.rs)** - Atomic spheres with VdW radii
   - GPU instanced rendering
   - Optional GPU compute culling (frustum + occlusion)
   - Billboard impostors for distant atoms (LOD system)

2. **BallAndStick (ball_stick.rs)** - Small spheres + cylindrical bonds
   - Hybrid: GPU for atom spheres, CPU for bond cylinders
   - Bond inference via octree spatial partitioning
   - GPU compute for atom culling

3. **Ribbon (ribbon.rs)** - Secondary structure visualization
   - Alpha helices as helical ribbons
   - Beta sheets as directional arrows
   - Cubic spline interpolation for smooth curves
   - Extrusion geometry

4. **Surface (surface.rs)** - Molecular surface
   - SAS (Solvent-Accessible Surface) algorithm
   - Marching cubes on SDF (Signed Distance Field)
   - Parallel SDF generation with rayon
   - Vertex welding (spatial hashing) to reduce duplicates by ~85%
   - Laplacian smoothing (2 iterations)

**GPU Compute Pipelines:**

The renderer uses compute shaders for performance-critical operations:
- `culling.wgsl` - Frustum culling for atom spheres
- `cylinder_culling.wgsl` - Culling for bond cylinders
- `surface_sdf.wgsl` - SDF generation for surface extraction
- Atomic visibility flags written to GPU buffer
- Indirect draw commands for efficient rendering

**LOD System:**

5-level automatic LOD based on camera distance:
- **High** - Full detail icosphere (subdivision 3)
- **Medium** - Subdivision 2
- **Low** - Subdivision 1
- **VeryLow** - Subdivision 0
- **Impostor** - Billboard quad with normal encoding

Hysteresis prevents visual "popping" during LOD transitions.

**Shaders:**

All shaders in `assets/shaders/` are WGSL format:
- `sphere.wgsl` / `sphere_indirect.wgsl` - Atom rendering
- `ribbon.wgsl` - Ribbon/cartoon rendering
- `surface.wgsl` - Surface rendering
- `billboard.wgsl` - Impostor rendering
- Phong lighting model with ambient + diffuse + specular

### mol-ui: User Interface

**Purpose:** egui-based UI panels and controls.

**Key components:**
- `MolecularUI` - Main UI coordinator
- `UIState` - Shared state (animation controls, selection, config)
- Panels: Controls, Info (stats), Settings, Animation

**UI State Management:**
- UI state is synchronized with renderer state
- Representation changes trigger renderer updates
- Animation controls (play/pause/stop, frame slider, FPS control)
- Selection tracking (clicked atoms, ray picking results)

### mol-app: Main Application

**Purpose:** Application entry point, event loop, integration layer.

**Key responsibilities:**
- winit event loop
- Mouse/keyboard input handling
- Camera control (orbit, pan, zoom via mouse)
- Atom selection via ray picking
- egui integration with wgpu
- Animation frame timing and updates
- PDB file loading and trajectory management

**Input Controls:**
- Left mouse drag → orbit camera
- Right mouse drag → pan camera
- Mouse wheel → zoom
- Keys 1-4 → switch representation modes
- R → reset camera
- ESC → exit
- Ctrl/Shift + click → atom selection

## Important Implementation Details

### Animation System

The project supports multi-frame PDB files (MODEL/ENDMDL records):
- **Topology** is shared across frames (atom types, bonds, chains)
- **Positions** are stored per-frame as `Vec<Vec<glam::Vec3>>`
- Frame updates only modify position buffers (GPU efficient)
- Animation timing uses accumulator pattern for consistent FPS
- Memory efficient: ~95% less than storing full Protein per frame

**Limitations:**
- VanDerWaals works perfectly with animations
- BallAndStick/Ribbon/Surface have limited support (topology changes not handled)
- See `ANIMATION_IMPLEMENTATION.md` for details

### Surface Generation Pipeline

Critical implementation notes (bugs were fixed in December 2025):

1. **SDF Grid Order** - Grid must be filled in same order as indexed (z→y→x)
2. **Vertex Welding** - Marching cubes generates 3 verts per triangle (duplicates)
   - Spatial hashing with 0.08Å tolerance merges duplicates
   - Enables Laplacian smoothing on continuous mesh
3. **Normal Calculation** - Use SDF gradient, not face normals, for smooth results
4. **Transparency** - Surface is opaque (alpha=1.0) to avoid depth ordering issues

Parameters:
- Probe radius: 1.4Å (water molecule size)
- Grid spacing: 0.8Å
- Smoothing: 2 Laplacian iterations

### GPU Compute Culling

GPU compute provides 3-20x speedup vs CPU culling:
- Enabled by default for VanDerWaals
- Hybrid mode for BallAndStick (GPU atoms, CPU bonds)
- Uses atomic counters for indirect draw commands
- Benchmark stats available in UI panel

### Octree Spatial Partitioning

Used throughout the codebase for O(log n) spatial queries:
- Bond inference (finding atoms within bonding distance)
- Atom selection (ray-sphere intersection)
- Frustum culling (bounding box queries)
- Max depth: 10 levels, min objects per node: 8

## Common Development Patterns

### Adding a New Representation Mode

1. Create new file in `crates/mol-render/src/representations/`
2. Implement renderer struct with `new()` and `render()` methods
3. Add shader file in `assets/shaders/`
4. Update `RepresentationType` enum in `mol-render/src/representations/mod.rs`
5. Add renderer to `Renderer` struct in `renderer.rs`
6. Handle in `mol-app/src/main.rs` keyboard input

### Modifying Shaders

- Shaders are loaded at runtime from `assets/shaders/`
- Changes require restarting the application
- Use WGSL syntax (similar to GLSL but stricter)
- All shaders use Phong lighting model (ambient + diffuse + specular)

### Working with PDB Files

- Use `parse_pdb_trajectory()` for multi-frame support
- Use `parse_pdb_file()` for single-frame (legacy)
- Sample files: `9PZW.pdb` (27K atoms), `6TAV.pdb`, `8c9n.pdb` (large)
- Download from RCSB: https://www.rcsb.org/

### Performance Considerations

- **Always test in release mode** - Debug is 10-100x slower
- Enable `RUST_LOG=info` to see performance stats
- Large proteins (>50K atoms) benefit most from GPU compute
- Surface generation can take 10-30s for very large proteins
- Target: 60 FPS for 27K atoms on mid-range GPU

## Known Issues

- Surface generation slow for proteins >100K atoms (CPU-bound SDF generation)
- Ball-and-stick animation limited (doesn't recompute bonds per frame)
- Ribbon/Surface animation not implemented (topology assumed static)
- No file format support for DCD/XTC trajectories yet

## Future Phases

- **Phase 6 (VR):** OpenXR integration, stereo rendering, Quest 2/3 optimization
- **Phase 7 (Advanced):** Multiple color schemes, distance/angle measurements, export (PNG/OBJ/STL), DCD/XTC support

## References

Key documentation sources:
- PDB Format: https://www.wwpdb.org/documentation/file-format
- wgpu Guide: https://wgpu.rs/
- egui Docs: https://www.egui.rs/
- Marching Cubes: Lorensen & Cline (1987)
