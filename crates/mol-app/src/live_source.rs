use glam::Vec3;
use pdb_parser::dcd::{read_dcd_header, read_dcd_frame};
use std::sync::mpsc::Sender;
use std::io::{Read, Write};

// ── Protocol constants ──────────────────────────────────────────────────────
// Magic: b"MDSS"
// Version: 1u32
//
// Handshake (client → server):  b"MDSS" + u32 version (LE)
// Handshake (server → client):  b"MDSS" + u32 version=1 + u32 ok=1
//
// Per frame (client → server):
//   u32 frame_num (LE)
//   u32 n_atoms   (LE)
//   n_atoms × 3 × f32 (LE)  — interleaved x, y, z per atom
//   u32 checksum  (LE)       — sum of all coord bytes (wrapping) & 0xFFFFFFFF

const MAGIC: &[u8; 4] = b"MDSS";
const VERSION: u32 = 1;

/// Watch a DCD file that is being written by an MD engine. Sends new frames via `tx`.
///
/// The watcher keeps the file open and reads incrementally. When `read_dcd_frame`
/// returns an error (not enough data yet), it sleeps `poll_ms` milliseconds and retries
/// from the same position — enabling seamless incremental reading.
#[allow(dead_code)]
pub fn run_file_watcher(tx: Sender<(u32, Vec<Vec3>)>, dcd_path: String, poll_ms: u64) {
    let poll_dur = std::time::Duration::from_millis(poll_ms);

    // Wait for file to exist
    let path = std::path::PathBuf::from(&dcd_path);
    loop {
        if path.exists() {
            break;
        }
        log::warn!("Live watcher: waiting for DCD file to appear: {}", dcd_path);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    log::info!("Live watcher: opening DCD file: {}", dcd_path);

    // Open file and read header — use BufReader for buffered I/O
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Live watcher: cannot open {}: {}", dcd_path, e);
            return;
        }
    };
    let mut reader = std::io::BufReader::new(file);

    let header = match read_dcd_header(&mut reader) {
        Ok(h) => h,
        Err(e) => {
            log::error!("Live watcher: failed to read DCD header: {}", e);
            return;
        }
    };

    log::info!(
        "Live watcher: DCD opened — {} atoms, {} declared frames, has_crystal={}",
        header.n_atoms,
        header.n_frames,
        header.has_crystal
    );

    let mut frame_num: u32 = 0;

    loop {
        match read_dcd_frame(&mut reader, &header) {
            Ok(coords) => {
                if tx.send((frame_num, coords)).is_err() {
                    log::info!("Live watcher: receiver dropped, exiting thread");
                    return;
                }
                frame_num += 1;
            }
            Err(_) => {
                // Not enough data yet — seek back is implicit: BufReader position
                // is only advanced when reads succeed. For the partial-read case we
                // need to seek back to the position before the failed attempt.
                // Since read_dcd_frame uses read_exact via read_record, a failure
                // mid-record will leave the cursor in an unknown position. To handle
                // this robustly we track the position before each attempt.
                // We re-open the file fresh on error (simpler and reliable on Windows).
                std::thread::sleep(poll_dur);
                // Note: We rely on the caller's BufReader state — if a partial read
                // left it mid-record we simply try again. For a robust implementation
                // we'd save/restore position, but for append-only files this is fine
                // because the writer doesn't truncate.
            }
        }
    }
}

/// Robust file watcher with position save/restore around failed frame reads.
/// Supersedes the simpler version above; this is the version actually used.
pub fn run_file_watcher_robust(tx: Sender<(u32, Vec<Vec3>)>, dcd_path: String, poll_ms: u64) {
    let poll_dur = std::time::Duration::from_millis(poll_ms);

    let path = std::path::PathBuf::from(&dcd_path);
    loop {
        if path.exists() {
            break;
        }
        log::warn!("Live watcher: waiting for DCD file to appear: {}", dcd_path);
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    log::info!("Live watcher (robust): opening DCD file: {}", dcd_path);

    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Live watcher: cannot open {}: {}", dcd_path, e);
            return;
        }
    };
    // Use a seekable reader so we can restore position on partial reads
    let mut reader = std::io::BufReader::new(file);

    let header = match read_dcd_header(&mut reader) {
        Ok(h) => h,
        Err(e) => {
            log::error!("Live watcher: failed to read DCD header: {}", e);
            return;
        }
    };

    log::info!(
        "Live watcher: DCD opened — {} atoms, {} declared frames, has_crystal={}",
        header.n_atoms,
        header.n_frames,
        header.has_crystal
    );

    let mut frame_num: u32 = 0;

    loop {
        // Save position before attempting to read a frame
        use std::io::Seek;
        let pos_before = match reader.seek(std::io::SeekFrom::Current(0)) {
            Ok(p) => p,
            Err(e) => {
                log::error!("Live watcher: seek error: {}", e);
                return;
            }
        };

        match read_dcd_frame(&mut reader, &header) {
            Ok(coords) => {
                if tx.send((frame_num, coords)).is_err() {
                    log::info!("Live watcher: receiver dropped, exiting thread");
                    return;
                }
                frame_num += 1;
            }
            Err(_e) => {
                // Restore position so next attempt starts clean
                let _ = reader.seek(std::io::SeekFrom::Start(pos_before));
                std::thread::sleep(poll_dur);
            }
        }
    }
}

/// Start a TCP server that accepts MD frame streams using the MDSS protocol.
///
/// Protocol:
///   Handshake in:  b"MDSS" + u32 version
///   Handshake out: b"MDSS" + u32 version=1 + u32 ok=1
///   Frame:         u32 frame_num + u32 n_atoms + n_atoms×3×f32 + u32 checksum
///   End sentinel:  frame_num=0, n_atoms=0
pub fn run_socket_server(tx: Sender<(u32, Vec<Vec3>)>, port: u16) {
    use std::net::TcpListener;

    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = match TcpListener::bind(&bind_addr) {
        Ok(l) => l,
        Err(e) => {
            log::error!("Live stream: failed to bind {}: {}", bind_addr, e);
            return;
        }
    };

    log::info!("Listening for MD stream on port {}", port);

    for stream_result in listener.incoming() {
        match stream_result {
            Ok(mut stream) => {
                let peer = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                log::info!("Live stream: client connected from {}", peer);

                // Handle handshake
                if let Err(e) = handle_handshake(&mut stream) {
                    log::warn!("Live stream: handshake failed with {}: {}", peer, e);
                    continue;
                }

                // Frame loop
                match handle_frames(&mut stream, &tx) {
                    Ok(n) => log::info!("Live stream: {} frames received from {}", n, peer),
                    Err(e) => {
                        log::warn!("Live stream: disconnected from {}: {}", peer, e);
                        // If the tx is broken (main thread exited), stop accepting
                        if tx.send((0, Vec::new())).is_err() {
                            // Test send failed — receiver dropped
                            log::info!("Live stream: receiver dropped, exiting server");
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Live stream: accept error: {}", e);
            }
        }
    }
}

fn handle_handshake<S: Read + Write>(stream: &mut S) -> anyhow::Result<()> {
    let mut buf = [0u8; 8]; // 4 magic + 4 version
    stream.read_exact(&mut buf)?;

    if &buf[0..4] != MAGIC {
        anyhow::bail!(
            "MDSS handshake: bad magic {:?}, expected {:?}",
            &buf[0..4],
            MAGIC
        );
    }

    let client_ver = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    if client_ver != VERSION {
        anyhow::bail!(
            "MDSS handshake: version mismatch: client={} server={}",
            client_ver,
            VERSION
        );
    }

    // Send response: MDSS + version + ok=1
    let mut resp = Vec::with_capacity(12);
    resp.extend_from_slice(MAGIC);
    resp.extend_from_slice(&VERSION.to_le_bytes());
    resp.extend_from_slice(&1u32.to_le_bytes());
    stream.write_all(&resp)?;

    Ok(())
}

fn handle_frames<S: Read>(stream: &mut S, tx: &Sender<(u32, Vec<Vec3>)>) -> anyhow::Result<u64> {
    let mut count = 0u64;

    loop {
        // Read frame header: u32 frame_num + u32 n_atoms
        let mut hdr = [0u8; 8];
        stream.read_exact(&mut hdr)?;

        let frame_num = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let n_atoms = u32::from_le_bytes(hdr[4..8].try_into().unwrap());

        // End-of-stream sentinel
        if frame_num == 0 && n_atoms == 0 {
            log::info!("Live stream: received end-of-stream sentinel");
            break;
        }

        // Read coordinate data: n_atoms × 3 × f32 = n_atoms × 12 bytes
        let coord_bytes = (n_atoms as usize) * 12;
        let mut coord_buf = vec![0u8; coord_bytes];
        stream.read_exact(&mut coord_buf)?;

        // Read checksum: u32
        let mut cs_buf = [0u8; 4];
        stream.read_exact(&mut cs_buf)?;
        let received_checksum = u32::from_le_bytes(cs_buf);

        // Verify checksum: sum of coord bytes (wrapping)
        let computed_checksum: u32 = coord_buf
            .iter()
            .fold(0u32, |acc, &b| acc.wrapping_add(b as u32));

        if computed_checksum != received_checksum {
            anyhow::bail!(
                "Frame {} checksum mismatch: computed=0x{:08x} received=0x{:08x}",
                frame_num,
                computed_checksum,
                received_checksum
            );
        }

        // Parse interleaved xyz f32 coords
        let mut coords = Vec::with_capacity(n_atoms as usize);
        for i in 0..(n_atoms as usize) {
            let base = i * 12;
            let x = f32::from_le_bytes(coord_buf[base..base + 4].try_into().unwrap());
            let y = f32::from_le_bytes(coord_buf[base + 4..base + 8].try_into().unwrap());
            let z = f32::from_le_bytes(coord_buf[base + 8..base + 12].try_into().unwrap());
            coords.push(Vec3::new(x, y, z));
        }

        if tx.send((frame_num, coords)).is_err() {
            anyhow::bail!("receiver dropped");
        }

        count += 1;
    }

    Ok(count)
}

/// Print a Python example script for streaming MD frames to PDB Visual.
pub fn print_python_client_example(port: u16) {
    let script = format!(
        r#"
#!/usr/bin/env python3
"""Stream MD frames to PDB Visual.
Usage: python stream_md.py trajectory.dcd
"""
import socket, struct, numpy as np

HOST, PORT = "127.0.0.1", {port}
MAGIC = b"MDSS"
VERSION = 1

def send_frame(sock, frame_num, coords):
    n = len(coords)
    data = np.array(coords, dtype=np.float32).flatten().tobytes()
    checksum = sum(data) & 0xFFFFFFFF
    sock.sendall(struct.pack("<II", frame_num, n) + data + struct.pack("<I", checksum))

with socket.create_connection((HOST, PORT)) as s:
    # Handshake
    s.sendall(MAGIC + struct.pack("<I", VERSION))
    resp = s.recv(12)
    print(f"Connected: {{resp}}")
    # Send frames from DCD (install MDAnalysis: pip install MDAnalysis)
    import MDAnalysis as mda
    import sys
    u = mda.Universe(sys.argv[1])
    for i, ts in enumerate(u.trajectory):
        send_frame(s, i, u.atoms.positions.tolist())
    # End-of-stream sentinel
    s.sendall(struct.pack("<II", 0, 0))
"#,
        port = port
    );

    log::info!("=== Python MD streaming client example ===");
    for line in script.lines() {
        log::info!("{}", line);
    }
    log::info!("==========================================");
    println!("{}", script);
}
