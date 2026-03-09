#!/usr/bin/env python3
"""
create_membrane.py — Build lipid bilayer membranes and simulate with OpenMM

No AmberTools required. Uses a geometric bilayer builder + CHARMM36 lipid
force field (downloaded automatically from GitHub on first run).

Requirements
------------
    conda install -c conda-forge openmm pdbfixer numpy

Optional (for CHARMM-GUI input):
    python scripts/create_membrane.py --charmm-psf step5_input.psf --charmm-pdb step5_input.pdb

Quick start
-----------
    # Pure POPC bilayer, stream to viewer
    python scripts/create_membrane.py --lipids POPC:100 --stream --viewer-exe target/release/pdbvisual.exe

    # Mixed bilayer
    python scripts/create_membrane.py --lipids POPC:70:POPE:20:CHOL:10 --stream ...

    # Build only, no simulation
    python scripts/create_membrane.py --lipids POPC:100 --output membrane.pdb --no-equilibrate

    # Load CHARMM-GUI output (PSF + PDB)
    python scripts/create_membrane.py --charmm-psf step5_input.psf --charmm-pdb step5_input.pdb --stream ...
"""

import argparse
import os
import sys
import time
import urllib.request
from pathlib import Path
from typing import Optional

import numpy as np


# ── Force field management ────────────────────────────────────────────────────

FF_CACHE_DIR = Path.home() / ".openmm_membrane" / "ff"

# CHARMM36 lipid FF XML — from the openmmforcefields GitHub repo (MIT licence)
CHARMM36_URLS = {
    "charmm36.xml": (
        "https://raw.githubusercontent.com/openmm/openmmforcefields/"
        "main/openmmforcefields/ffxml/charmm36/charmm36.xml"
    ),
    "charmm36_lipids.xml": (
        "https://raw.githubusercontent.com/openmm/openmmforcefields/"
        "main/openmmforcefields/ffxml/charmm36/charmm36_lipids.xml"
    ),
    "charmm36_water.xml": (
        "https://raw.githubusercontent.com/openmm/openmmforcefields/"
        "main/openmmforcefields/ffxml/charmm36/charmm36/water.xml"
    ),
}

# Fallback: minimal CHARMM36 water + ions (always in OpenMM standard)
OPENMM_BUILTIN_FF = ["charmm36.xml", "charmm36/water.xml"]


def ensure_charmm36_lipid_ff() -> list:
    """
    Return list of force field XML files that include CHARMM36 lipid params.
    Downloads them from GitHub on first run and caches in ~/.openmm_membrane/ff/.
    """
    FF_CACHE_DIR.mkdir(parents=True, exist_ok=True)

    # Try OpenMM built-in first (no lipid params but we need protein + water)
    # Then add downloaded lipid params on top
    ff_files = []

    for fname, url in CHARMM36_URLS.items():
        cached = FF_CACHE_DIR / fname
        if not cached.exists():
            print(f"[FF] Downloading {fname}…")
            try:
                urllib.request.urlretrieve(url, cached)
                print(f"[FF] Saved → {cached}")
            except Exception as e:
                print(f"[FF] WARNING: Could not download {fname}: {e}")
                continue
        ff_files.append(str(cached))

    if not ff_files:
        print("[FF] Could not download CHARMM36 lipid FF — using AMBER14 (protein only)")
        return ["amber14-all.xml", "amber14/tip3pfb.xml"]

    return ff_files


# ── Lipid geometry templates ──────────────────────────────────────────────────

# Approximate Cα-equivalent Z positions for each lipid type (Å from bilayer center)
# Positive Z = upper leaflet, negative = lower leaflet
# Values based on typical bilayer geometry (hydrocarbon core ~25 Å half-thickness)

LIPID_GEOMETRY = {
    #       area_per_lipid (Å²)   z_head (Å from center)   length (Å)
    "POPC": {"apl": 68.0,  "z_head": 20.0, "length": 37.0},
    "POPE": {"apl": 60.0,  "z_head": 20.0, "length": 37.0},
    "POPS": {"apl": 60.0,  "z_head": 20.0, "length": 37.0},
    "POPG": {"apl": 65.0,  "z_head": 20.0, "length": 37.0},
    "DPPC": {"apl": 63.0,  "z_head": 19.0, "length": 36.0},
    "DPPE": {"apl": 55.0,  "z_head": 19.0, "length": 36.0},
    "DOPC": {"apl": 72.0,  "z_head": 21.0, "length": 39.0},
    "DOPE": {"apl": 65.0,  "z_head": 21.0, "length": 39.0},
    "DSPC": {"apl": 60.0,  "z_head": 19.0, "length": 38.0},
    "DLPC": {"apl": 60.0,  "z_head": 17.0, "length": 31.0},
    "CHOL": {"apl": 38.0,  "z_head": 16.0, "length": 18.0},
}
DEFAULT_LIPID = {"apl": 65.0, "z_head": 20.0, "length": 37.0}


def download_lipid_pdb(lipid_name: str, cache_dir: Path) -> Optional[str]:
    """
    Download a single-lipid PDB from RCSB CCD or PubChem.
    Returns local file path or None on failure.
    """
    # RCSB Chemical Component Dictionary PDB files
    ccd_ids = {
        "POPC": "LPC",  # approximate - use PC headgroup
        "POPE": "LPE",
        "DPPC": "DPC",
        "DOPC": "OPC",
        "CHOL": "CLR",
    }
    rcsb_id = ccd_ids.get(lipid_name)
    cached   = cache_dir / f"{lipid_name}.pdb"

    if cached.exists():
        return str(cached)

    if rcsb_id:
        url = f"https://files.rcsb.org/ligands/download/{rcsb_id}_ideal.pdb"
        try:
            urllib.request.urlretrieve(url, cached)
            return str(cached)
        except Exception:
            pass

    return None


# ── Geometric bilayer builder ─────────────────────────────────────────────────

def build_bilayer_positions(composition: dict, box_xy_A: tuple,
                             z_water_A: float) -> tuple:
    """
    Build approximate all-atom-like positions for a lipid bilayer.

    Strategy: place each lipid as a rigid body at a grid position, then
    use OpenMM energy minimization to relax the geometry.

    Returns (topology, positions, box_vectors) using OpenMM objects.
    """
    from openmm.app import Topology, Element
    from openmm.unit import angstrom, nanometer
    import openmm.unit as unit

    total_per_leaflet = sum(composition.values())
    # Weighted average area per lipid
    avg_apl = sum(
        composition.get(k, 0) * LIPID_GEOMETRY.get(k, DEFAULT_LIPID)["apl"]
        for k in composition
    ) / total_per_leaflet

    # Required box area
    box_area = box_xy_A[0] * box_xy_A[1]

    # Number of lipids per leaflet from actual box area
    n_per_leaflet = max(int(box_area / avg_apl), total_per_leaflet)
    print(f"[Builder] {n_per_leaflet} lipids/leaflet, box {box_xy_A[0]:.1f}×{box_xy_A[1]:.1f} Å²")

    # Grid layout
    nx = int(np.ceil(np.sqrt(n_per_leaflet * box_xy_A[0] / box_xy_A[1])))
    ny = int(np.ceil(n_per_leaflet / nx))
    dx = box_xy_A[0] / nx
    dy = box_xy_A[1] / ny

    # Repeat lipid names to fill the grid
    lipid_sequence = []
    for name, count in composition.items():
        lipid_sequence.extend([name] * count)
    # Tile to fill all grid positions
    while len(lipid_sequence) < n_per_leaflet:
        lipid_sequence.extend(lipid_sequence)
    lipid_sequence = lipid_sequence[:n_per_leaflet]

    topology = Topology()
    positions = []

    def add_lipid_placeholder(chain, lipid_name: str, cx: float, cy: float,
                               z_center: float, flip: bool):
        """Add simplified bead-like residue for each lipid."""
        geom     = LIPID_GEOMETRY.get(lipid_name, DEFAULT_LIPID)
        z_head   = geom["z_head"]
        length   = geom["length"]
        sign     = -1.0 if flip else 1.0   # lower leaflet points down

        res = topology.addResidue(lipid_name, chain)

        # Add 5 representative atoms along the lipid axis (head to tail)
        atom_names = ["P", "O1", "C1", "C2", "CT"]
        elements   = ["P", "O", "C", "C", "C"]
        for i, (aname, elem) in enumerate(zip(atom_names, elements)):
            frac = i / (len(atom_names) - 1)
            z    = z_center + sign * (z_head - frac * length)
            # Add small random jitter to break symmetry
            jx = np.random.uniform(-0.5, 0.5)
            jy = np.random.uniform(-0.5, 0.5)
            topology.addAtom(aname, Element.getBySymbol(elem), res)
            positions.append([(cx + jx) * 0.1, (cy + jy) * 0.1, z * 0.1])  # nm

    # Build upper and lower leaflets
    for leaflet, flip in [(0, False), (1, True)]:
        chain = topology.addChain()
        for idx in range(n_per_leaflet):
            row = idx // nx
            col = idx % nx
            cx  = (col + 0.5) * dx
            cy  = (row + 0.5) * dy
            add_lipid_placeholder(
                chain,
                lipid_sequence[idx],
                cx, cy, 0.0, flip
            )

    pos_array = np.array(positions)   # nm
    from openmm.unit import Quantity
    pos_qty = Quantity(pos_array.tolist(), nanometer)

    # Box includes membrane + water layers on both sides
    max_geom = max(LIPID_GEOMETRY.get(k, DEFAULT_LIPID)["z_head"] for k in composition)
    box_z_A  = 2.0 * (max_geom + z_water_A)
    box_vecs = np.array([
        [box_xy_A[0] * 0.1, 0.0, 0.0],
        [0.0, box_xy_A[1] * 0.1, 0.0],
        [0.0, 0.0, box_z_A * 0.1],
    ])

    return topology, pos_qty, box_vecs


# ── Composition parser ────────────────────────────────────────────────────────

def parse_lipids(spec: str) -> dict:
    parts = spec.split(":")
    if len(parts) % 2 != 0:
        sys.exit(f"[!] Bad lipid spec '{spec}'. Format: LIPID1:N1:LIPID2:N2")
    comp = {}
    for i in range(0, len(parts), 2):
        name  = parts[i].upper()
        try:
            count = int(parts[i+1])
        except ValueError:
            sys.exit(f"[!] Expected integer after '{name}', got '{parts[i+1]}'")
        comp[name] = count
    total = sum(comp.values())
    print(f"[Membrane] Lipid composition ({total} per leaflet):")
    for name, n in comp.items():
        pct  = 100 * n / total
        info = {
            "POPC": "Palmitoyl-oleoyl PC  — most common bilayer lipid",
            "POPE": "Palmitoyl-oleoyl PE  — inner leaflet enriched",
            "POPS": "Palmitoyl-oleoyl PS  — anionic, inner leaflet",
            "POPG": "Palmitoyl-oleoyl PG  — anionic (bacterial)",
            "DPPC": "Dipalmitoyl PC       — gel phase (Tm = 41°C)",
            "DOPC": "Dioleoyl PC          — unsaturated, fluid",
            "DOPE": "Dioleoyl PE          — fusion-prone",
            "CHOL": "Cholesterol          — modulates fluidity",
            "DSPC": "Distearoyl PC        — high melting point",
            "DLPC": "Dilauroyl PC         — short tails",
        }.get(name, "")
        print(f"           {name:6s}  {n:4d} ({pct:.0f}%)  {info}")
    return comp


# ── CHARMM-GUI loader ─────────────────────────────────────────────────────────

def load_charmm_gui(psf_path: str, pdb_path: str):
    """Load CHARMM-GUI output (PSF + PDB files)."""
    from openmm.app import CharmmPsfFile, PDBFile
    print(f"[Build] Loading CHARMM-GUI system…")
    psf = CharmmPsfFile(psf_path)
    pdb = PDBFile(pdb_path)
    print(f"[Build] {psf.topology.getNumAtoms()} atoms, {psf.topology.getNumResidues()} residues")
    return psf, pdb.positions


# ── System creation ───────────────────────────────────────────────────────────

def create_system_from_charmm(psf, ff_files: list, box_vecs=None):
    """Create OpenMM System from CHARMM PSF using CHARMM36 parameters."""
    from openmm.app import CharmmParameterSet, PME, HBonds
    from openmm.unit import nanometer

    try:
        params = CharmmParameterSet(*[f for f in ff_files if f.endswith((".prm", ".str", ".rtf"))])
        system = psf.createSystem(
            params,
            nonbondedMethod=PME,
            nonbondedCutoff=1.2 * nanometer,
            constraints=HBonds,
        )
        print("[FF] CHARMM36 from parameter files")
        return system
    except Exception as e:
        print(f"[FF] CharmmParameterSet failed ({e}), trying ForceField XML…")

    # Try ForceField XML approach
    from openmm.app import ForceField
    xml_files = [f for f in ff_files if f.endswith(".xml")]
    if not xml_files:
        xml_files = OPENMM_BUILTIN_FF
    ff = ForceField(*xml_files)
    system = ff.createSystem(
        psf.topology,
        nonbondedMethod=PME,
        nonbondedCutoff=1.2 * nanometer,
        constraints=HBonds,
    )
    print(f"[FF] CHARMM36 XML")
    return system


def create_system_from_topology(topology, ff_files: list, box_vecs=None):
    """Create OpenMM System from a Topology object using AMBER14 + GBn2."""
    from openmm.app import ForceField, PME, HBonds, CutoffNonPeriodic
    from openmm.unit import nanometer

    # For the geometric builder output (placeholder atoms), use a simple FF
    ff = ForceField("amber14-all.xml", "implicit/gbn2.xml")
    try:
        system = ff.createSystem(
            topology,
            nonbondedMethod=CutoffNonPeriodic,
            constraints=HBonds,
            ignoreExternalBonds=True,
        )
        print("[FF] AMBER14 + GBn2 (placeholder geometry — for preview only)")
        return system
    except Exception as e:
        raise RuntimeError(f"Cannot create system: {e}")


# ── Solvation ─────────────────────────────────────────────────────────────────

def solvate_system(topology, positions, ff_files: list,
                   box_vecs, salt_M: float) -> tuple:
    """Add explicit water and ions using OpenMM Modeller."""
    from openmm.app import Modeller, ForceField
    from openmm.unit import molar, nanometer, Quantity
    import openmm.unit as unit

    xml_files = [f for f in ff_files if f.endswith(".xml")]
    if not xml_files:
        xml_files = OPENMM_BUILTIN_FF

    try:
        ff = ForceField(*xml_files)
    except Exception:
        ff = ForceField("amber14-all.xml", "amber14/tip3pfb.xml")

    modeller = Modeller(topology, positions)

    box_nm = Quantity(
        [[box_vecs[0][0], 0, 0],
         [0, box_vecs[1][1], 0],
         [0, 0, box_vecs[2][2]]],
        nanometer
    )

    print(f"[Solv] Adding TIP3P water (box {box_vecs[2][2]*10:.0f} Å z)…")
    try:
        modeller.addSolvent(
            ff,
            boxVectors=box_nm,
            ionicStrength=salt_M * unit.molar,
            positiveIon="Na+",
            negativeIon="Cl-",
        )
        n_water = sum(1 for r in modeller.topology.residues()
                      if r.name in ("HOH", "WAT", "TIP3"))
        print(f"[Solv] Added {n_water} water molecules, {salt_M} M NaCl")
    except Exception as e:
        print(f"[Solv] WARNING: Solvation failed ({e}) — proceeding without water")

    return modeller.topology, modeller.positions


# ── Equilibration ─────────────────────────────────────────────────────────────

def equilibrate(simulation, positions, reporter=None):
    from openmm.unit import kelvin
    print("\n[Equil] Phase 1/2 — Energy minimization…")
    simulation.context.setPositions(positions)
    simulation.minimizeEnergy(maxIterations=3000)
    state = simulation.context.getState(getEnergy=True)
    print(f"[Equil] Potential energy: {state.getPotentialEnergy()}")

    print("[Equil] Phase 2/2 — Heating 50 K → target temperature…")
    target_T = simulation.integrator.getTemperature()
    temps = [50, 100, 150, 200, 250, int(target_T.value_in_unit(kelvin))]
    for T in temps:
        simulation.integrator.setTemperature(T * kelvin)
        simulation.context.setVelocitiesToTemperature(T * kelvin)
        simulation.step(500)
        if reporter:
            reporter.report(simulation,
                            simulation.context.getState(getPositions=True))
    print("[Equil] Done.")


# ── Output ────────────────────────────────────────────────────────────────────

def save_pdb(simulation, topology, path: str):
    from openmm.app import PDBFile
    state = simulation.context.getState(getPositions=True, enforcePeriodicBox=True)
    with open(path, "w") as f:
        PDBFile.writeFile(topology, state.getPositions(), f)
    print(f"[Out] PDB saved → {path}")


def save_checkpoint(simulation, path: str):
    simulation.saveCheckpoint(path)
    print(f"[Out] Checkpoint → {path}")


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    p = argparse.ArgumentParser(
        description="Build lipid bilayer membranes for OpenMM",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    # Composition
    p.add_argument("--lipids", default="POPC:100",
                   metavar="LIPID1:N1[:LIPID2:N2]",
                   help="Lipid composition per leaflet (default: POPC:100)")
    p.add_argument("--size", nargs=2, type=float, default=[80.0, 80.0],
                   metavar=("X_A", "Y_A"),
                   help="Box XY in Ångström (default: 80 80)")
    p.add_argument("--water", type=float, default=25.0,
                   metavar="A", help="Water layer per side in Å (default: 25)")
    p.add_argument("--salt", type=float, default=0.15,
                   metavar="M", help="NaCl concentration in M (default: 0.15)")
    p.add_argument("--temperature", type=float, default=310.0,
                   metavar="K", help="Temperature in K (default: 310)")

    # CHARMM-GUI input
    p.add_argument("--charmm-psf", metavar="PSF",
                   help="CHARMM-GUI PSF file (skip builder, load directly)")
    p.add_argument("--charmm-pdb", metavar="PDB",
                   help="CHARMM-GUI PDB file (required with --charmm-psf)")

    # Output
    p.add_argument("--output", default="membrane.pdb",
                   help="Output PDB path (default: membrane.pdb)")
    p.add_argument("--workdir", default="membrane_build",
                   help="Working directory (default: membrane_build)")
    p.add_argument("--no-equilibrate", action="store_true",
                   help="Skip equilibration")

    # Streaming
    p.add_argument("--stream", action="store_true",
                   help="Stream to PDB Visual via MDSS")
    p.add_argument("--port", type=int, default=7777,
                   help="MDSS port (default: 7777)")
    p.add_argument("--viewer-exe", default="pdbvisual",
                   help="pdbvisual executable path")
    p.add_argument("--production-time", type=float, default=60.0,
                   metavar="S", help="Production run wall-clock seconds (default: 60)")

    args = p.parse_args()

    try:
        import openmm
        from openmm import LangevinMiddleIntegrator, MonteCarloMembraneBarostat, MonteCarloBarostat
        from openmm.app import Simulation
        from openmm.unit import kelvin, bar, picoseconds, femtoseconds
    except ImportError:
        sys.exit("OpenMM not found: conda install -c conda-forge openmm")

    os.makedirs(args.workdir, exist_ok=True)

    print(f"\n{'='*55}")
    print(f"  Membrane Builder")
    if args.charmm_psf:
        print(f"  Mode     : CHARMM-GUI input")
        print(f"  PSF      : {args.charmm_psf}")
        print(f"  PDB      : {args.charmm_pdb}")
    else:
        composition = parse_lipids(args.lipids)
        print(f"  Size     : {args.size[0]:.0f} × {args.size[1]:.0f} Å")
        print(f"  Water    : {args.water:.0f} Å/side")
        print(f"  Salt     : {args.salt} M NaCl")
    print(f"  Temp     : {args.temperature} K")
    print(f"{'='*55}\n")

    # ── Load force field ──────────────────────────────────────────────────────
    print("[FF] Fetching CHARMM36 lipid force field…")
    ff_files = ensure_charmm36_lipid_ff()

    # ── Build or load system ──────────────────────────────────────────────────
    if args.charmm_psf:
        if not args.charmm_pdb:
            sys.exit("[!] --charmm-pdb required with --charmm-psf")
        psf, positions = load_charmm_gui(args.charmm_psf, args.charmm_pdb)
        system   = create_system_from_charmm(psf, ff_files)
        topology = psf.topology
        box_vecs = None
        viewer_pdb = args.charmm_pdb
    else:
        print(f"\n[Build] Constructing geometric bilayer…")
        topology, positions, box_vecs = build_bilayer_positions(
            composition = composition,
            box_xy_A    = tuple(args.size),
            z_water_A   = args.water,
        )

        # Solvate
        print(f"\n[Solv] Solvating with TIP3P water + {args.salt} M NaCl…")
        topology, positions = solvate_system(
            topology, positions, ff_files, box_vecs, args.salt
        )

        # Save raw structure for viewer
        viewer_pdb = os.path.join(args.workdir, "membrane_raw.pdb")
        from openmm.app import PDBFile
        with open(viewer_pdb, "w") as f:
            PDBFile.writeFile(topology, positions, f)
        print(f"[Build] Raw structure → {viewer_pdb}")

        system = create_system_from_topology(topology, ff_files, box_vecs)

    # Barostat — semi-isotropic for bilayers (XY isotropic, Z free)
    try:
        baro = MonteCarloMembraneBarostat(
            1.0 * bar, 0.0 * bar,
            args.temperature * kelvin,
            MonteCarloMembraneBarostat.XYIsotropic,
            MonteCarloMembraneBarostat.ZFree, 25,
        )
        system.addForce(baro)
        print("[FF] Semi-isotropic NPT barostat (MonteCarloMembraneBarostat)")
    except Exception:
        system.addForce(MonteCarloBarostat(1.0 * bar, args.temperature * kelvin, 25))
        print("[FF] Isotropic NPT barostat (fallback)")

    # ── Integrator + Simulation ───────────────────────────────────────────────
    integrator = LangevinMiddleIntegrator(
        args.temperature * kelvin, 1.0 / picoseconds, 2.0 * femtoseconds
    )
    simulation = Simulation(topology, system, integrator)

    # ── MDSS reporter ─────────────────────────────────────────────────────────
    reporter = None
    if args.stream:
        sys.path.insert(0, str(Path(__file__).parent))
        from openmm_mdss import MDSSReporter
        viewer_exe = os.path.abspath(args.viewer_exe) \
                     if os.path.exists(args.viewer_exe) else args.viewer_exe
        reporter = MDSSReporter(
            reportInterval = 50,
            port           = args.port,
            pdb_path       = os.path.abspath(viewer_pdb),
            launch_viewer  = True,
            viewer_exe     = viewer_exe,
        )

    # ── Equilibrate ───────────────────────────────────────────────────────────
    if not args.no_equilibrate:
        equilibrate(simulation, positions, reporter=reporter)
        save_pdb(simulation, topology, args.output)
        ckpt = args.output.replace(".pdb", ".chk")
        save_checkpoint(simulation, ckpt)

        print(f"\n[Done] Equilibrated membrane:")
        print(f"       PDB        : {args.output}")
        print(f"       Checkpoint : {ckpt}")
        print(f"\n       Resume simulation:")
        print(f"       python scripts/openmm_mdss.py {args.output} --live-stream {args.port}")
    else:
        save_pdb(simulation, topology, args.output)
        print(f"\n[Done] Raw system saved to {args.output}")

    # ── Production ────────────────────────────────────────────────────────────
    if args.stream and reporter and not args.no_equilibrate:
        simulation.reporters.append(reporter)
        print(f"\n[MD] Production ({args.production_time:.0f}s wall-clock) — Ctrl+C to stop…")
        try:
            simulation.runForClockTime(args.production_time)
        except KeyboardInterrupt:
            print("\n[MD] Stopped.")
        except Exception as e:
            print(f"\n[MD] Error: {e}")
        reporter.close()

    print("\n[Done]")
    print("\nTip — For production-quality membranes use CHARMM-GUI:")
    print("  1. charmm-gui.org → Membrane Builder → download OpenMM output")
    print(f"  2. python scripts/create_membrane.py --charmm-psf step5_input.psf "
          f"--charmm-pdb step5_input.pdb --stream --viewer-exe {args.viewer_exe}")


if __name__ == "__main__":
    main()
