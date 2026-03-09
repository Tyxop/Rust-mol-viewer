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
- **Controller tracking** — coloured spheres mark each hand in real time (blue = left, red = right)
- **Floating radial menu** — press left grip to open a billboard menu at controller position; press again to close. Menu stays locked at the position where it was opened, freeing the left hand
- **All 4 representations in VR** — switch between VdW, Ball & Stick, Ribbon and Surface from the VR menu; dedicated VR-format render pipelines ensure correct swapchain format (`Rgba8UnormSrgb`)
- **Molecule scale** — zoom +/− buttons in the VR menu scale the molecule in world space
- **Camera reset** — reset button restores the default view
- **Ray-based pointer** — right controller casts a ray onto the menu quad; accurate billboard-aware UV mapping (projects onto local X/Y axes of the billboard)
- **Reliable trigger input** — click fires at the last highlighted button position, so slight hand movement while squeezing does not miss the target (threshold: 50% trigger travel)
- **Atom selection** — right grip ray-picks the nearest atom when the menu is closed
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

**Shaders** (`assets/shaders/`, WGSL format): `sphere.wgsl`, `sphere_indirect.wgsl`, `billboard.wgsl`, `ribbon.wgsl`, `surface.wgsl`, `sphere_raymarch.wgsl`, `surface_raymarch.wgsl`, `axes.wgsl`, `controller_sphere.wgsl`, plus compute shaders for culling and SDF generation.

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

## Molecular Dynamics Integration (Python / OpenMM)

PDB Visual includes Python scripts for building membrane systems, running MD simulations with OpenMM, and streaming atom positions in real time to the viewer.

### Installation — conda

```bash
# Create a dedicated environment (recommended)
conda create -n molviewer python=3.11
conda activate molviewer

# OpenMM + scientific stack
conda install -c conda-forge openmm numpy scipy

# Optional: for clash detection in audit_structure()
conda install -c conda-forge scipy
```

Verify OpenMM is working:

```bash
python -c "import openmm; print(openmm.__version__)"
python -m openmm.testInstallation
```

---

### Simulation + Audit — `scripts/openmm_mdss.py`

The script has a built-in pipeline (audit → prepare → FF selection → equilibrate → production). Two ways to run it:

**Option A — script launches the viewer automatically:**
```bash
python scripts/openmm_mdss.py 9PZW.pdb --launch-viewer --viewer-exe target/release/mol-app
```

**Option B — open the viewer manually first, then run the script:**
```bash
# Terminal 1: open viewer in streaming mode
cargo run --package mol-app --release -- --live-stream 7777 9PZW.pdb

# Terminal 2: run simulation (audits the molecule automatically before simulating)
python scripts/openmm_mdss.py 9PZW.pdb --port 7777
```

If the audit finds blocking issues (NaN coordinates, etc.) the script exits before simulating.

#### CLI options

| Flag | Default | Description |
|------|---------|-------------|
| `pdb` | — | PDB file to simulate (same file passed to the viewer) |
| `--port N` | `7777` | MDSS TCP port |
| `--launch-viewer` | off | Launch the viewer as a subprocess automatically |
| `--viewer-exe PATH` | `mol-app` | Path to the viewer executable |

---

### Live Streaming API — `MDSSReporter`

`MDSSReporter` is an OpenMM reporter that streams atom coordinates to PDB Visual in real time over TCP using the MDSS protocol. Every N integration steps, positions are sent to the viewer and rendered immediately.

#### Quickstart

**1. Start the viewer** in live-stream mode pointing at your PDB file:

```bash
# Windows
target\release\mol-app.exe --live-stream 7777 protein.pdb

# Linux / macOS
./target/release/mol-app --live-stream 7777 protein.pdb
```

**2. Run your simulation** using `MDSSReporter`:

```python
from scripts.openmm_mdss import MDSSReporter
from openmm.app import *
from openmm import *
from openmm.unit import *

pdb      = PDBFile("protein.pdb")
ff       = ForceField("amber14-all.xml", "amber14/tip3pfb.xml")
system   = ff.createSystem(pdb.topology, nonbondedMethod=PME,
                            nonbondedCutoff=1.0*nanometer, constraints=HBonds)
integrator = LangevinMiddleIntegrator(300*kelvin, 1/picosecond, 0.002*picoseconds)

simulation = Simulation(pdb.topology, system, integrator)
simulation.context.setPositions(pdb.positions)
simulation.minimizeEnergy()

# Stream every 10 steps to PDB Visual (viewer must already be running)
simulation.reporters.append(MDSSReporter(reportInterval=10, port=7777))
simulation.step(10000)
```

Or let the reporter launch the viewer automatically:

```python
simulation.reporters.append(MDSSReporter(
    reportInterval=10,
    pdb_path="protein.pdb",
    launch_viewer=True,
    viewer_exe="target/release/mol-app",
    port=7777,
))
```

#### MDSS Protocol

```
Handshake (client → server):  "MDSS" + u32 version=1
Handshake (server → client):  "MDSS" + u32 version=1 + u32 ok=1
Per frame:                     u32 frame_num + u32 n_atoms + n_atoms×3×f32 + u32 checksum
End sentinel:                  u32 0 + u32 0
```

Positions are sent in Ångströms (converted from OpenMM's nanometers).

---

### Structure Audit — `audit_structure()`

Before running a simulation, call `audit_structure()` to detect common problems that would cause the simulation to crash or produce nonsense results.

```python
from scripts.openmm_mdss import audit_structure
from openmm.app import PDBFile

pdb = PDBFile("protein.pdb")
issues, warnings = audit_structure(pdb.topology, pdb.positions)

for w in warnings:
    print(f"[WARN]  {w}")
for e in issues:
    print(f"[ERROR] {e}")
```

What it checks:

| Check | Severity |
|-------|----------|
| Unknown residues (not protein/nucleic/water/ion) | Warning |
| NaN or Inf coordinates | Error |
| Severe atomic clashes (atoms < 0.5 Å apart, `scipy` required) | Warning |

---

### Membrane Builder — `scripts/create_membrane.py`

Builds lipid bilayer membranes using a built-in coarse-grained 5-bead force field (inspired by MARTINI 2, no extra packages beyond OpenMM + NumPy). For production quality, it also accepts CHARMM-GUI output.

#### Supported lipids

```
POPC  POPE  POPS  POPG  POPA    DPPC  DPPE  DPPS  DPPG  DPPA
DOPC  DOPE  DOPS  DOPG  DOPA    DLPC  DLPE  DLPS  DLPG
DSPC  DSPE  DSPS  DSPG  DSPA    CHOL
```

```bash
# List all supported lipids with area-per-lipid estimates
python scripts/create_membrane.py --list-lipids
```

#### Usage examples

```bash
# Pure POPC bilayer, save to PDB (no equilibration)
python scripts/create_membrane.py --lipids POPC:100 --no-equilibrate --output popc.pdb

# Plasma membrane-like mixed bilayer — stream to the viewer
python scripts/create_membrane.py \
    --lipids POPC:40:POPE:30:CHOL:20:POPS:10 \
    --size 100 100 --salt 0.15 --stream \
    --viewer-exe target/release/mol-app

# Custom box, temperature, longer production run
python scripts/create_membrane.py \
    --lipids DPPC:100 \
    --size 120 120 --water 30 --temperature 323 \
    --production-time 120 --output dppc_323K.pdb

# Load CHARMM-GUI all-atom system (production quality)
python scripts/create_membrane.py \
    --charmm-psf step5_input.psf --charmm-pdb step5_input.pdb \
    --stream --viewer-exe target/release/mol-app
```

#### All options

| Flag | Default | Description |
|------|---------|-------------|
| `--lipids LIPID:N[:LIPID:N]` | `POPC:100` | Composition per leaflet |
| `--size X Y` | `80 80` | Box XY dimensions in Å |
| `--water Å` | `25` | Water layer thickness per side in Å |
| `--salt M` | `0.15` | NaCl concentration in mol/L |
| `--temperature K` | `310` | Simulation temperature in K |
| `--no-equilibrate` | off | Skip minimization + heating |
| `--output FILE` | `membrane.pdb` | Output PDB path |
| `--workdir DIR` | `membrane_build` | Working directory for intermediates |
| `--stream` | off | Stream frames to PDB Visual via MDSS |
| `--port N` | `7777` | MDSS TCP port |
| `--viewer-exe PATH` | `pdbvisual` | Path to the viewer executable |
| `--production-time s` | `60` | Wall-clock seconds for production run |
| `--charmm-psf FILE` | — | CHARMM-GUI PSF file (all-atom mode) |
| `--charmm-pdb FILE` | — | CHARMM-GUI PDB file (required with PSF) |
| `--charmm-ff FILE…` | — | Extra CHARMM FF files (`.xml`/`.prm`/`.str`) |

#### CG model details

- **5-bead model**: `P` (headgroup) → `O1` (glycerol) → `C1` → `C2` → `CT` (tail end)
- **CHOL**: 4-bead model (`O1 → C1 → C2 → CT`, no glycerol bead)
- TIP3P water + NaCl ions at the specified salt concentration
- Equilibration: energy minimization → gradual heating (50 K → target T in 6 stages)
- PBC not used in CG mode (non-periodic cutoff); CHARMM-GUI mode uses PME

---

## Controls

### Desktop

| Input | Action |
|-------|--------|
| Left mouse drag | Orbit camera |
| Right mouse drag | Pan camera |
| Scroll wheel | Zoom |
| `1` / `2` / `3` / `4` | Switch representation |
| `R` | Reset camera |
| `Ctrl`/`Shift` + click | Atom selection |
| `ESC` | Quit |

### VR (OpenXR)

| Input | Action |
|-------|--------|
| Left grip (press) | Open / close floating menu at controller position |
| Right trigger (aim at button) | Click highlighted menu button |
| Right grip (menu closed) | Select nearest atom under controller ray |
| Menu → VdW / Ball+Stick / Ribbon / Surface | Switch molecular representation |
| Menu → Zoom + / Zoom − | Scale molecule up / down |
| Menu → Reset | Reset camera to default view |

The floating menu billboards toward the user's head and stays fixed at the position where it was opened. Controller positions are shown as coloured spheres (blue = left, red = right) so you always know where your hands are.

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
- [x] OpenXR VR support (stereo, controllers, billboard menu, representation switching, scale, atom selection)
- [x] Raymarched sphere & surface representations
- [x] Axes renderer
- [ ] Multiple color schemes (by chain, residue, hydrophobicity)
- [ ] Distance and angle measurements
- [ ] PNG / OBJ / STL export
- [x] OpenMM live-streaming via MDSS reporter (`scripts/openmm_mdss.py`)
- [x] Lipid membrane builder with CG force field (`scripts/create_membrane.py`)
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
