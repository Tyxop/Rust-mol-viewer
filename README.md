<div align="center">

# Rust Mol Viewer

**A high-performance 3D molecular visualization engine written in Rust**

[![Rust](https://img.shields.io/badge/rust-2021_edition-orange?logo=rust)](https://www.rust-lang.org/)
[![wgpu](https://img.shields.io/badge/wgpu-22.0-blue)](https://wgpu.rs/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)](https://github.com/Tyxop/Rust-mol-viewer)

Renders proteins from PDB files using GPU-accelerated techniques — instanced rendering, compute shaders, LOD, frustum culling, raymarching, and VR support via OpenXR.

</div>

---

## Features

### Visualization Modes

| Mode | Key | Description |
|------|-----|-------------|
| **Van der Waals** | `1` | Atomic spheres with VdW radii, CPK color scheme |
| **Ball & Stick** | `2` | Small spheres + cylindrical bonds inferred by distance |
| **Ribbon / Cartoon** | `3` | Secondary structure — helices as ribbons, sheets as arrows |
| **Molecular Surface** | `4` | SAS surface via marching cubes on a signed distance field |

### Rendering Pipeline

- **GPU compute culling** — frustum culling dispatched on the GPU (3–20× speedup over CPU)
- **5-level LOD system** — High → Medium → Low → VeryLow → Billboard impostor, with hysteresis to prevent popping
- **Indirect draw** — atomic visibility flags written by compute shaders feed indirect draw calls
- **Raymarching** — sphere and surface representations with raymarched rendering
- **Axes renderer** — coordinate system visualization overlay

### Molecular Surface

- SAS (Solvent-Accessible Surface) with 1.4 Å probe radius
- Signed Distance Field on a 0.8 Å grid computed in parallel via Rayon
- Marching cubes with **vertex welding** (spatial hashing, 0.08 Å tolerance) → ~85% vertex reduction
- Laplacian smoothing (2 iterations) on the welded mesh
- Normals derived from SDF gradient (smooth shading)

### Animation

- Multi-frame PDB support (`MODEL`/`ENDMDL` records)
- Shared topology + per-frame coordinate storage (~95% less memory than full per-frame `Protein`)
- Play / Pause / Stop, frame slider, FPS control, Once / Loop / Ping-Pong modes
- GPU compute and LOD fully compatible with animations

### VR (OpenXR)

- Full OpenXR 1.0+ integration with stereo rendering (dual render passes)
- Asymmetric FOV and automatic IPD
- Controller input (Oculus Touch layout): rotate, translate, teleport, atom selection
- 3D-space UI panels rendered as world quads
- Compatible with Quest 2/3, Index, Vive, WMR

---

## Architecture

```
crates/
├── pdb-parser/       # nom-based PDB parser — ATOM, HETATM, HELIX, SHEET, CONECT, MODEL
│   ├── parser.rs     # Parser combinators
│   ├── structures.rs # Atom, Protein, Trajectory, Bond, Chain, SecondaryStructure
│   ├── bonds.rs      # Distance-based bond inference (VdW radii + tolerance)
│   └── spatial.rs    # Octree — O(log n) neighbor queries
│
├── mol-render/       # wgpu rendering engine
│   ├── renderer.rs   # GPU state, render passes, compute pipelines
│   ├── camera.rs     # Orbit camera (rotate / pan / zoom)
│   ├── lod.rs        # 5-level LOD with hysteresis
│   ├── culling.rs    # Frustum culling system
│   ├── marching_cubes.rs
│   ├── splines.rs    # Cubic splines for ribbon geometry
│   └── representations/
│       ├── spheres.rs          # Van der Waals
│       ├── ball_stick.rs       # Ball & Stick
│       ├── ribbon.rs           # Ribbon / Cartoon
│       ├── surface.rs          # Molecular surface
│       ├── sphere_raymarch.rs  # Raymarched spheres
│       ├── surface_raymarch.rs # Raymarched surface
│       └── axes.rs             # Coordinate axes
│
├── mol-ui/           # egui interface — panels, controls, animation, stats
├── mol-app/          # Entry point, winit event loop, input, ray picking
└── mol-vr/           # OpenXR VR integration
```

**Shaders** (`assets/shaders/`, WGSL format): `sphere.wgsl`, `sphere_indirect.wgsl`, `billboard.wgsl`, `ribbon.wgsl`, `surface.wgsl`, `sphere_raymarch.wgsl`, `surface_raymarch.wgsl`, `axes.wgsl`, plus compute shaders for culling and SDF generation.

---

## Getting Started

### Prerequisites

- **Rust** (2021 edition or later) — install via [rustup](https://rustup.rs/)
- **GPU** with Vulkan (Linux/Windows), Metal (macOS), or DirectX 12 (Windows)

**Linux** system dependencies:
```bash
# Debian / Ubuntu
sudo apt install libwayland-dev libxkbcommon-dev

# Arch
sudo pacman -S wayland libxkbcommon

# Fedora
sudo dnf install wayland-devel libxkbcommon-devel
```

### Build & Run

```bash
# Clone
git clone https://github.com/Tyxop/Rust-mol-viewer.git
cd Rust-mol-viewer

# Release build (always use release — debug is 10–100× slower)
cargo build --release

# Run with a PDB file
cargo run --package mol-app --release -- protein.pdb

# Run without a file (test sphere)
cargo run --package mol-app --release

# VR mode
cargo run --package mol-app --release -- --vr protein.pdb

# With logging
RUST_LOG=info cargo run --package mol-app --release -- protein.pdb
```

Download sample PDB files from [RCSB](https://www.rcsb.org/) (e.g. `9PZW`, `6TAV`).

---

## Controls

| Input | Action |
|-------|--------|
| Left mouse drag | Orbit camera |
| Right mouse drag | Pan camera |
| Scroll wheel | Zoom |
| `1` / `2` / `3` / `4` | Switch representation |
| `R` | Reset camera |
| `Ctrl`/`Shift` + click | Atom selection |
| `ESC` | Quit |

---

## Performance

Tested on a 27,525-atom protein (9PZW — NMDA receptor):

| Representation | FPS (RTX 3060 class) |
|----------------|----------------------|
| Van der Waals | 60+ FPS |
| Ball & Stick | 60+ FPS |
| Ribbon | 60+ FPS |
| Molecular Surface | 60+ FPS (after ~20s generation) |
| **VR stereo** | **90 FPS** (Quest 2/3) |

Surface generation for 9PZW:
- Grid: 150 × 271 × 226 voxels (9.2M)
- Generation time: ~19.5 s
- Vertices after welding: 354K (from 2.3M — 85% reduction)
- Triangles: 766K
- GPU memory: ~50 MB

---

## Tech Stack

| Category | Library |
|----------|---------|
| GPU abstraction | [wgpu 22.0](https://wgpu.rs/) (Vulkan / Metal / DX12) |
| Windowing & events | [winit 0.30](https://github.com/rust-windowing/winit) |
| UI | [egui 0.29](https://www.egui.rs/) |
| PDB parsing | [nom 7.1](https://github.com/rust-bakery/nom) |
| Linear algebra | [glam 0.29](https://github.com/bitshifter/glam-rs) + [nalgebra 0.33](https://nalgebra.org/) |
| Data parallelism | [rayon 1.10](https://github.com/rayon-rs/rayon) |
| VR | [openxr 0.19](https://github.com/Ralith/openxrs) |
| GPU data | [bytemuck](https://github.com/Lokathor/bytemuck) |

---

## Roadmap

- [x] PDB parser (ATOM, HETATM, HELIX, SHEET, CONECT, MODEL)
- [x] wgpu rendering engine (Vulkan / Metal / DirectX 12)
- [x] 4 molecular representations (VdW, Ball&Stick, Ribbon, Surface)
- [x] Orbit / pan / zoom camera
- [x] egui UI (panels, menus, atom selection)
- [x] Ray picking with octree
- [x] 5-level LOD with hysteresis
- [x] Frustum culling (CPU + GPU compute)
- [x] GPU indirect draw
- [x] Rayon parallelism (SDF, bond inference)
- [x] Multi-frame animation (MODEL/ENDMDL)
- [x] OpenXR VR support (stereo, controllers, 3D UI)
- [x] Raymarched sphere & surface representations
- [x] Axes renderer
- [ ] Multiple color schemes (by chain, residue, hydrophobicity)
- [ ] Distance and angle measurements
- [ ] PNG / OBJ / STL export
- [ ] DCD / XTC trajectory formats
- [ ] Per-frame bond recomputation for Ball&Stick animation

---

## License

MIT — see [LICENSE](LICENSE).

---

## References

- [RCSB Protein Data Bank](https://www.rcsb.org/)
- [PDB File Format](https://www.wwpdb.org/documentation/file-format)
- [wgpu](https://wgpu.rs/) · [egui](https://www.egui.rs/) · [OpenXR](https://www.khronos.org/openxr/)
- Lorensen & Cline, *Marching Cubes: A High Resolution 3D Surface Construction Algorithm* (1987)
