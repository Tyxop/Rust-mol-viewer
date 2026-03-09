#!/usr/bin/env python3
"""
OpenMM MDSS Reporter — streams MD frames to PDB Visual in real time.

Usage
-----
1. Start the viewer with the same PDB file:
       mol-app.exe --live-stream 7777 protein.pdb

2. Run your simulation using MDSSReporter:
       python simulate.py protein.pdb

Protocol: MDSS v1 (TCP, little-endian)
  Handshake out : b"MDSS" + u32 version=1
  Handshake in  : b"MDSS" + u32 version=1 + u32 ok=1
  Per frame     : u32 frame_num + u32 n_atoms + n_atoms*3*f32 + u32 checksum
  End sentinel  : u32 0 + u32 0
"""

import socket
import struct
import subprocess
import sys
import os
import time
from typing import Optional

import numpy as np

# ── Protocol constants ───────────────────────────────────────────────────────
_MAGIC   = b"MDSS"
_VERSION = 1
_NM_TO_ANGSTROM = 10.0   # OpenMM uses nanometers; viewer uses Ångströms


class MDSSReporter:
    """
    OpenMM Reporter that streams atom positions to PDB Visual via MDSS protocol.

    Parameters
    ----------
    reportInterval : int
        Report every N integration steps.
    host : str
        Hostname where PDB Visual is listening (default: localhost).
    port : int
        TCP port (must match --live-stream <port> in the viewer).
    pdb_path : str | None
        If given and launch_viewer=True, the viewer is started automatically
        pointing at this PDB file.
    launch_viewer : bool
        If True, launch the viewer executable as a subprocess before connecting.
    viewer_exe : str
        Path to the mol-app executable (used only when launch_viewer=True).
    connect_retries : int
        How many times to retry the TCP connection (viewer may take a moment to start).
    """

    def __init__(
        self,
        reportInterval: int,
        host: str = "127.0.0.1",
        port: int = 7777,
        pdb_path: Optional[str] = None,
        launch_viewer: bool = False,
        viewer_exe: str = "mol-app",
        connect_retries: int = 20,
    ):
        self._interval    = reportInterval
        self._host        = host
        self._port        = port
        self._pdb_path    = pdb_path
        self._frame_num   = 0
        self._sock        = None
        self._proc        = None  # viewer subprocess

        if launch_viewer:
            if pdb_path is None:
                raise ValueError("pdb_path is required when launch_viewer=True")
            self._launch_viewer(viewer_exe, pdb_path, port)

        self._connect(connect_retries)

    # ── Viewer auto-launch ───────────────────────────────────────────────────

    def _launch_viewer(self, viewer_exe: str, pdb_path: str, port: int):
        # Resolve relative paths to absolute so subprocess can find them
        viewer_exe = os.path.abspath(viewer_exe)
        pdb_path   = os.path.abspath(pdb_path)
        cmd = [viewer_exe, "--live-stream", str(port), pdb_path]
        print(f"[MDSS] Launching viewer: {' '.join(cmd)}")
        # Launch detached so it doesn't block
        self._proc = subprocess.Popen(
            cmd,
            creationflags=subprocess.DETACHED_PROCESS if sys.platform == "win32" else 0,
        )
        time.sleep(2.0)  # give viewer time to bind the port

    # ── TCP connection ───────────────────────────────────────────────────────

    def _connect(self, retries: int):
        last_err = None
        for attempt in range(retries):
            try:
                s = socket.create_connection((self._host, self._port), timeout=5)
                # Handshake
                s.sendall(_MAGIC + struct.pack("<I", _VERSION))
                resp = s.recv(12)
                if len(resp) < 12 or resp[:4] != _MAGIC:
                    raise ConnectionError(f"Bad handshake response: {resp!r}")
                ok = struct.unpack_from("<I", resp, 8)[0]
                if ok != 1:
                    raise ConnectionError(f"Viewer rejected connection (ok={ok})")
                self._sock = s
                print(f"[MDSS] Connected to PDB Visual at {self._host}:{self._port}")
                return
            except (ConnectionRefusedError, OSError) as e:
                last_err = e
                if attempt < retries - 1:
                    print(f"[MDSS] Connection attempt {attempt+1}/{retries} failed, retrying…")
                    time.sleep(0.5)
        raise ConnectionError(
            f"Could not connect to PDB Visual at {self._host}:{self._port} "
            f"after {retries} attempts: {last_err}"
        )

    # ── OpenMM Reporter interface ────────────────────────────────────────────

    def describeNextReport(self, simulation):
        """Tell OpenMM when we want the next report."""
        steps = self._interval - simulation.currentStep % self._interval
        # (steps, positions, velocities, forces, energies, wrap)
        return (steps, True, False, False, False, False)

    def report(self, simulation, state):
        """Called by OpenMM every reportInterval steps."""
        if self._sock is None:
            return

        # Positions come in nanometers → convert to Ångströms
        pos_nm  = state.getPositions(asNumpy=True)._value   # numpy (N, 3) float64
        coords  = (pos_nm * _NM_TO_ANGSTROM).astype(np.float32)  # (N, 3) float32

        n_atoms = coords.shape[0]
        data    = coords.flatten().tobytes()                 # interleaved x,y,z per atom

        checksum = int(np.frombuffer(data, dtype=np.uint8).sum()) & 0xFFFFFFFF

        packet = (
            struct.pack("<II", self._frame_num, n_atoms)
            + data
            + struct.pack("<I", checksum)
        )

        try:
            self._sock.sendall(packet)
        except OSError as e:
            print(f"[MDSS] Send error on frame {self._frame_num}: {e}")
            self._sock = None

        self._frame_num += 1

    # ── Cleanup ──────────────────────────────────────────────────────────────

    def close(self):
        """Send end-of-stream sentinel and close the connection."""
        if self._sock:
            try:
                self._sock.sendall(struct.pack("<II", 0, 0))
            except OSError:
                pass
            self._sock.close()
            self._sock = None
        print(f"[MDSS] Connection closed. Sent {self._frame_num} frames.")

    def __del__(self):
        self.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


# ── Example simulation ───────────────────────────────────────────────────────

def prepare_model(pdb_path: str, ff):
    """
    Prepare a PDB file for simulation using PDBFixer (recommended) or
    falling back to plain Modeller if PDBFixer is not installed.

    PDBFixer handles:
      - Missing residues / loops
      - Missing heavy atoms
      - Non-standard residues
      - Terminal capping (ACE / NME caps)
      - Adding hydrogens at the right pH

    Install: conda install -c conda-forge pdbfixer
    """
    from openmm.app import Modeller

    try:
        from pdbfixer import PDBFixer
        from openmm.app import PDBFile as _PDBFile
        print("[OpenMM] Using PDBFixer to prepare model…")
        fixer = PDBFixer(filename=pdb_path)
        fixer.findMissingResidues()
        fixer.findNonstandardResidues()
        fixer.replaceNonstandardResidues()
        fixer.removeHeterogens(keepWater=False)
        fixer.findMissingAtoms()
        fixer.addMissingAtoms()
        fixer.addMissingHydrogens(pH=7.0)

        # Save the fixed model so the viewer and reporter share the same topology
        fixed_path = pdb_path.replace(".pdb", "_fixed.pdb")
        with open(fixed_path, "w") as f:
            _PDBFile.writeFile(fixer.topology, fixer.positions, f)
        print(f"[OpenMM] Saved prepared model → {fixed_path}")

        return fixer.topology, fixer.positions, fixed_path

    except ImportError:
        print("[OpenMM] PDBFixer not found — falling back to Modeller.addHydrogens()")
        print("         Install for better results:  conda install -c conda-forge pdbfixer")
        from openmm.app import PDBFile
        pdb = PDBFile(pdb_path)
        modeller = Modeller(pdb.topology, pdb.positions)
        modeller.addHydrogens(ff, pH=7.0)

        fixed_path = pdb_path.replace(".pdb", "_fixed.pdb")
        with open(fixed_path, "w") as f:
            PDBFile.writeFile(modeller.topology, modeller.positions, f)
        print(f"[OpenMM] Saved prepared model → {fixed_path}")

        return modeller.topology, modeller.positions, fixed_path


def run_example(pdb_path: str, port: int = 7777, launch_viewer: bool = False,
                viewer_exe: str = "mol-app"):
    """
    Run a simple NVT simulation of the given PDB file and stream to PDB Visual.

    Requirements:
        conda install -c conda-forge openmm pdbfixer
    """
    try:
        from openmm import LangevinMiddleIntegrator
        from openmm.app import (
            ForceField, Simulation, HBonds, CutoffNonPeriodic
        )
        from openmm.unit import kelvin, picosecond, picoseconds
    except ImportError:
        sys.exit("OpenMM not found. Install with:  conda install -c conda-forge openmm")

    print(f"[OpenMM] Loading {pdb_path}")

    ff = ForceField("amber14-all.xml", "implicit/gbn2.xml")
    # prepare_model returns the fixed PDB path — the viewer must load THIS file
    topology, positions, viewer_pdb = prepare_model(pdb_path, ff)

    system = ff.createSystem(
        topology,
        nonbondedMethod=CutoffNonPeriodic,
        constraints=HBonds,
    )

    integrator = LangevinMiddleIntegrator(300 * kelvin, 1 / picosecond, 0.004 * picoseconds)

    simulation = Simulation(topology, system, integrator)
    simulation.context.setPositions(positions)

    print("[OpenMM] Minimising energy…")
    simulation.minimizeEnergy(maxIterations=500)

    print("[OpenMM] Starting MD — streaming to PDB Visual…")
    print(f"         Viewer command:  pdbvisual.exe --live-stream {port} {viewer_pdb}")

    with MDSSReporter(
        reportInterval=10,          # stream every 10 steps (~0.04 ps per frame)
        port=port,
        pdb_path=viewer_pdb,        # launch viewer with the fixed PDB (atoms match)
        launch_viewer=launch_viewer,
        viewer_exe=viewer_exe,
    ) as reporter:
        simulation.reporters.append(reporter)
        simulation.runForClockTime(60)   # run for 60 seconds of wall-clock time

    print("[OpenMM] Done.")


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(
        description="Stream OpenMM simulation to PDB Visual via MDSS"
    )
    parser.add_argument("pdb", help="PDB file to simulate (same file passed to the viewer)")
    parser.add_argument("--port", type=int, default=7777, help="MDSS port (default: 7777)")
    parser.add_argument(
        "--launch-viewer", action="store_true",
        help="Auto-launch mol-app.exe before connecting"
    )
    parser.add_argument(
        "--viewer-exe", default="mol-app",
        help="Path to mol-app executable (used with --launch-viewer)"
    )
    args = parser.parse_args()

    run_example(args.pdb, port=args.port, launch_viewer=args.launch_viewer,
                viewer_exe=args.viewer_exe)
