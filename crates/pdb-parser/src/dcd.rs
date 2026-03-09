use glam::Vec3;
use std::io::{Read, Seek};

pub struct DcdHeader {
    pub n_atoms: u32,
    pub n_frames: u32,       // may be 0 if not known at write time
    pub timestep_ps: f32,    // converted from AKMA: 1 AKMA = 0.04888 ps
    pub has_crystal: bool,   // unit cell info per frame
    pub frames_start: u64,   // byte offset after header (for seeking)
}

/// Read a Fortran-style record: [u32 len][data bytes][u32 trailing len]
fn read_record<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    // Read leading length
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF while reading Fortran record length",
            ));
        }
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf) as usize;

    // Read data
    let mut data = vec![0u8; len];
    r.read_exact(&mut data)?;

    // Read trailing length
    let mut trail_buf = [0u8; 4];
    r.read_exact(&mut trail_buf)?;
    let trail_len = u32::from_le_bytes(trail_buf) as usize;

    if trail_len != len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Fortran record length mismatch: leading={} trailing={}",
                len, trail_len
            ),
        ));
    }

    Ok(data)
}

pub fn read_dcd_header<R: Read + Seek>(r: &mut R) -> anyhow::Result<DcdHeader> {
    // ── Record 1: 84-byte block ──────────────────────────────────────────────
    // Layout (CHARMM DCD):
    //   [0..4]   "CORD" or "VELD" magic
    //   [4..8]   nframes (i32)
    //   [8..12]  first step (i32)
    //   [12..16] delta step (i32)
    //   [16..20] last step (i32)
    //   [20..40] 5 × i32 padding / flags
    //   [40..44] timestep (f32 in AKMA units)  ← some writers use f64 at byte 40
    //   [44..48] has_crystal flag (i32)
    //   [48..80] various padding
    //   [80..84] CHARMM version
    let rec1 = read_record(r)?;

    if rec1.len() < 52 {
        anyhow::bail!("DCD record 1 too short: {} bytes", rec1.len());
    }

    // Check magic
    let magic = &rec1[0..4];
    if magic == b"DROC" || magic == b"DLEV" {
        anyhow::bail!(
            "DCD file is big-endian — only little-endian DCD files are supported. \
             Convert with VMD or catdcd before loading."
        );
    }
    if magic != b"CORD" && magic != b"VELD" {
        anyhow::bail!(
            "DCD magic bytes invalid: {:?} — not a valid DCD file",
            magic
        );
    }

    let nframes = i32::from_le_bytes(rec1[4..8].try_into().unwrap()) as u32;

    // Timestep: CHARMM stores f32 at offset 40, NAMD stores f64 at offset 40.
    // We try f32 first (most common).  If it's unreasonably large, fall back.
    let timestep_akma = f32::from_le_bytes(rec1[40..44].try_into().unwrap());
    let timestep_ps = if timestep_akma.is_finite() && timestep_akma > 0.0 && timestep_akma < 1e6 {
        timestep_akma * 0.04888
    } else {
        // Try reading as f64 (NAMD style)
        if rec1.len() >= 48 {
            let ts_f64 = f64::from_le_bytes(rec1[40..48].try_into().unwrap());
            (ts_f64 as f32) * 0.04888
        } else {
            0.0
        }
    };

    // has_crystal flag at offset 44 (4 bytes)
    let has_crystal_flag = i32::from_le_bytes(rec1[44..48].try_into().unwrap());
    let has_crystal = has_crystal_flag != 0;

    // ── Record 2: title block (variable length) ──────────────────────────────
    let _title_rec = read_record(r)?; // skip title

    // ── Record 3: n_atoms (4 bytes) ──────────────────────────────────────────
    let natoms_rec = read_record(r)?;
    if natoms_rec.len() < 4 {
        anyhow::bail!("DCD n_atoms record too short: {} bytes", natoms_rec.len());
    }
    let n_atoms = u32::from_le_bytes(natoms_rec[0..4].try_into().unwrap());

    if n_atoms == 0 {
        anyhow::bail!("DCD header reports 0 atoms");
    }

    // Save current position — this is where frames start
    let frames_start = r.stream_position()?;

    Ok(DcdHeader {
        n_atoms,
        n_frames: nframes,
        timestep_ps,
        has_crystal,
        frames_start,
    })
}

pub fn read_dcd_frame<R: Read>(r: &mut R, header: &DcdHeader) -> anyhow::Result<Vec<Vec3>> {
    // Optionally skip crystal/unit-cell record (6 × f64 = 48 bytes)
    if header.has_crystal {
        let _crystal = read_record(r)?;
    }

    // Read X coords
    let x_rec = read_record(r)?;
    // Read Y coords
    let y_rec = read_record(r)?;
    // Read Z coords
    let z_rec = read_record(r)?;

    let n = header.n_atoms as usize;
    let expected = n * 4; // n_atoms × sizeof(f32)

    if x_rec.len() < expected || y_rec.len() < expected || z_rec.len() < expected {
        anyhow::bail!(
            "DCD frame coord records too short: got x={} y={} z={}, expected {}",
            x_rec.len(),
            y_rec.len(),
            z_rec.len(),
            expected
        );
    }

    let mut coords = Vec::with_capacity(n);
    for i in 0..n {
        let x = f32::from_le_bytes(x_rec[i * 4..i * 4 + 4].try_into().unwrap());
        let y = f32::from_le_bytes(y_rec[i * 4..i * 4 + 4].try_into().unwrap());
        let z = f32::from_le_bytes(z_rec[i * 4..i * 4 + 4].try_into().unwrap());
        coords.push(Vec3::new(x, y, z));
    }

    Ok(coords)
}

pub fn parse_dcd_file(
    path: &std::path::Path,
) -> anyhow::Result<(DcdHeader, Vec<Vec<Vec3>>)> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open DCD file {:?}: {}", path, e))?;

    let mut reader = std::io::BufReader::new(file);

    let header = read_dcd_header(&mut reader)?;

    log::info!(
        "DCD header: n_atoms={} n_frames={} timestep={:.4} ps has_crystal={}",
        header.n_atoms,
        header.n_frames,
        header.timestep_ps,
        header.has_crystal
    );

    let mut frames = Vec::new();

    loop {
        match read_dcd_frame(&mut reader, &header) {
            Ok(coords) => {
                frames.push(coords);
            }
            Err(e) => {
                // Check if we hit a clean EOF vs a real error
                let msg = e.to_string();
                if msg.contains("EOF") || msg.contains("UnexpectedEof") || msg.contains("eof") {
                    // Expected EOF — finished reading all frames
                    break;
                }
                // Check the underlying I/O error
                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == std::io::ErrorKind::UnexpectedEof {
                        break;
                    }
                }
                log::warn!("Error reading DCD frame {}: {} — stopping", frames.len(), e);
                break;
            }
        }
    }

    log::info!("Loaded {} frames from {:?}", frames.len(), path);

    Ok((header, frames))
}
