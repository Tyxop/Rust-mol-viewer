use crate::structures::*;
use glam::Vec3;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Failed to open file: {0}")]
    FileError(#[from] std::io::Error),

    #[error("Failed to parse line {line}: {message}")]
    ParseLineError { line: usize, message: String },
}

pub fn parse_pdb_file<P: AsRef<Path>>(path: P) -> Result<Protein, ParseError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut protein = Protein::new();
    let mut current_chain_id: Option<char> = None;
    let mut residue_map = rustc_hash::FxHashMap::default();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;

        if line.len() < 6 {
            continue;
        }

        let record_type = &line[0..6];

        match record_type {
            "ATOM  " | "HETATM" => {
                match parse_atom_line(&line) {
                    Ok(atom) => {
                        // Track chain changes
                        if Some(atom.residue.chain_id) != current_chain_id {
                            current_chain_id = Some(atom.residue.chain_id);
                        }

                        // Track residues for chain building
                        let res_key = (
                            atom.residue.chain_id,
                            atom.residue.seq_num,
                            atom.residue.insertion_code,
                        );

                        let atom_idx = protein.atoms.len();
                        protein.atoms.push(atom);

                        residue_map
                            .entry(res_key)
                            .or_insert_with(Vec::new)
                            .push(atom_idx);
                    }
                    Err(e) => {
                        log::warn!("Failed to parse ATOM at line {}: {}", line_num + 1, e);
                    }
                }
            }
            "HELIX " => {
                if let Ok(helix) = parse_helix_line(&line) {
                    protein.secondary_structure.helices.push(helix);
                }
            }
            "SHEET " => {
                if let Ok(sheet) = parse_sheet_line(&line) {
                    protein.secondary_structure.sheets.push(sheet);
                }
            }
            _ => {}
        }
    }

    // Build chains from residue map
    build_chains(&mut protein, residue_map);

    log::info!(
        "Parsed PDB file: {} atoms, {} chains, {} helices, {} sheets",
        protein.atoms.len(),
        protein.chains.len(),
        protein.secondary_structure.helices.len(),
        protein.secondary_structure.sheets.len()
    );

    // Build octree for spatial queries (used for atom selection)
    if !protein.atoms.is_empty() {
        let octree = crate::spatial::Octree::new(&protein, 6, 50);
        protein.octree = Some(octree);
    }

    Ok(protein)
}

/// Parse PDB file with potential multi-model support (for trajectories/animations)
/// Returns a Trajectory with topology + frames
pub fn parse_pdb_trajectory<P: AsRef<Path>>(path: P) -> Result<Trajectory, ParseError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut topology: Option<Protein> = None;
    let mut frames: Vec<Frame> = Vec::new();

    let mut in_model = false;
    let mut current_model = 0;
    let mut current_coords: Vec<Vec3> = Vec::new();

    // For first model, we build the full topology
    let mut protein = Protein::new();
    let mut current_chain_id: Option<char> = None;
    let mut residue_map = rustc_hash::FxHashMap::default();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;

        if line.len() < 6 {
            continue;
        }

        let record_type = &line[0..6];

        match record_type {
            "MODEL " => {
                // Parse model number
                current_model = line[10..14].trim().parse().unwrap_or(current_model + 1);
                in_model = true;
                current_coords.clear();

                log::debug!("Parsing MODEL {}", current_model);
            }
            "ENDMDL" => {
                if in_model {
                    // Save frame
                    let time = if topology.is_some() {
                        (current_model - 1) as f32
                    } else {
                        0.0
                    };
                    frames.push(Frame::new(current_coords.clone(), time));

                    // First model: save topology
                    if topology.is_none() {
                        build_chains(&mut protein, residue_map.clone());
                        topology = Some(protein);
                        protein = Protein::new(); // Create new for next iterations (won't be used)
                    }

                    in_model = false;
                }
            }
            "ATOM  " | "HETATM" => {
                match parse_atom_line(&line) {
                    Ok(atom) => {
                        current_coords.push(atom.position);

                        // Only build topology for first model
                        if topology.is_none() {
                            if Some(atom.residue.chain_id) != current_chain_id {
                                current_chain_id = Some(atom.residue.chain_id);
                            }

                            let res_key = (
                                atom.residue.chain_id,
                                atom.residue.seq_num,
                                atom.residue.insertion_code,
                            );

                            let atom_idx = protein.atoms.len();
                            protein.atoms.push(atom);

                            residue_map
                                .entry(res_key)
                                .or_insert_with(Vec::new)
                                .push(atom_idx);
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to parse ATOM at line {}: {}", line_num + 1, e);
                    }
                }
            }
            "HELIX " => {
                if topology.is_none() {
                    if let Ok(helix) = parse_helix_line(&line) {
                        protein.secondary_structure.helices.push(helix);
                    }
                }
            }
            "SHEET " => {
                if topology.is_none() {
                    if let Ok(sheet) = parse_sheet_line(&line) {
                        protein.secondary_structure.sheets.push(sheet);
                    }
                }
            }
            _ => {}
        }
    }

    // Handle case where file has no MODEL/ENDMDL (single structure)
    if topology.is_none() {
        build_chains(&mut protein, residue_map);

        log::info!(
            "Parsed single-model PDB: {} atoms, {} chains",
            protein.atoms.len(),
            protein.chains.len()
        );

        // Build octree for spatial queries
        if !protein.atoms.is_empty() {
            let octree = crate::spatial::Octree::new(&protein, 6, 50);
            protein.octree = Some(octree);
        }

        return Ok(Trajectory::from_protein(protein));
    }

    // Multi-model file
    let mut topology = topology.unwrap();

    log::info!(
        "Parsed multi-model PDB: {} atoms, {} chains, {} frames",
        topology.atoms.len(),
        topology.chains.len(),
        frames.len()
    );

    // Build octree for spatial queries
    if !topology.atoms.is_empty() {
        let octree = crate::spatial::Octree::new(&topology, 6, 50);
        topology.octree = Some(octree);
    }

    Ok(Trajectory {
        topology,
        frames,
    })
}

fn parse_atom_line(line: &str) -> Result<Atom, String> {
    if line.len() < 54 {
        return Err("Line too short".to_string());
    }

    // PDB format specification:
    // COLUMNS        DATA  TYPE    FIELD        DEFINITION
    // --------------------------------------------------------------------
    //  1 -  6        Record name   "ATOM  " or "HETATM"
    //  7 - 11        Integer       serial       Atom serial number.
    // 13 - 16        Atom          name         Atom name.
    // 17             Character     altLoc       Alternate location indicator.
    // 18 - 20        Residue name  resName      Residue name.
    // 22             Character     chainID      Chain identifier.
    // 23 - 26        Integer       resSeq       Residue sequence number.
    // 27             AChar         iCode        Code for insertion of residues.
    // 31 - 38        Real(8.3)     x            Orthogonal coordinates for X in Angstroms.
    // 39 - 46        Real(8.3)     y            Orthogonal coordinates for Y in Angstroms.
    // 47 - 54        Real(8.3)     z            Orthogonal coordinates for Z in Angstroms.
    // 55 - 60        Real(6.2)     occupancy    Occupancy.
    // 61 - 66        Real(6.2)     tempFactor   Temperature factor.
    // 77 - 78        LString(2)    element      Element symbol, right-justified.
    // 79 - 80        LString(2)    charge       Charge on the atom.

    let serial: u32 = line[6..11]
        .trim()
        .parse()
        .map_err(|_| "Invalid serial number")?;

    let atom_name = line[12..16].trim().to_string();

    let alt_loc = match line.chars().nth(16) {
        Some(' ') | None => None,
        Some(c) => Some(c),
    };

    let res_name = line[17..20].trim().to_string();

    let chain_id = line.chars().nth(21).unwrap_or(' ');

    let res_seq: i32 = line[22..26]
        .trim()
        .parse()
        .map_err(|_| "Invalid residue sequence number")?;

    let insertion_code = match line.chars().nth(26) {
        Some(' ') | None => None,
        Some(c) => Some(c),
    };

    let x: f32 = line[30..38]
        .trim()
        .parse()
        .map_err(|_| "Invalid X coordinate")?;

    let y: f32 = line[38..46]
        .trim()
        .parse()
        .map_err(|_| "Invalid Y coordinate")?;

    let z: f32 = line[46..54]
        .trim()
        .parse()
        .map_err(|_| "Invalid Z coordinate")?;

    let occupancy: f32 = if line.len() >= 60 {
        line[54..60].trim().parse().unwrap_or(1.0)
    } else {
        1.0
    };

    let temp_factor: f32 = if line.len() >= 66 {
        line[60..66].trim().parse().unwrap_or(0.0)
    } else {
        0.0
    };

    // Element symbol (columns 77-78)
    let element = if line.len() >= 78 {
        Element::from_str(&line[76..78])
    } else {
        // Try to infer from atom name
        infer_element(&atom_name)
    };

    let charge = None; // Parse if needed

    Ok(Atom {
        serial,
        name: atom_name,
        alt_loc,
        residue: ResidueRef {
            name: res_name,
            chain_id,
            seq_num: res_seq,
            insertion_code,
        },
        position: Vec3::new(x, y, z),
        occupancy,
        temp_factor,
        element,
        charge,
    })
}

fn infer_element(atom_name: &str) -> Element {
    let first_char = atom_name.chars().next().unwrap_or(' ');
    let name_upper = atom_name.to_uppercase();

    // Try two-letter elements first
    if name_upper.starts_with("CA") && atom_name != "CA" { // "CA" is alpha carbon, not calcium
        return Element::Ca;
    }
    if name_upper.starts_with("CL") {
        return Element::Cl;
    }
    if name_upper.starts_with("BR") {
        return Element::Br;
    }
    if name_upper.starts_with("MG") {
        return Element::Mg;
    }
    if name_upper.starts_with("FE") {
        return Element::Fe;
    }
    if name_upper.starts_with("ZN") {
        return Element::Zn;
    }
    if name_upper.starts_with("CU") {
        return Element::Cu;
    }
    if name_upper.starts_with("MN") {
        return Element::Mn;
    }
    if name_upper.starts_with("NA") {
        return Element::Na;
    }

    // Single letter elements
    match first_char {
        'C' => Element::C,
        'N' => Element::N,
        'O' => Element::O,
        'H' => Element::H,
        'S' => Element::S,
        'P' => Element::P,
        'F' => Element::F,
        'I' => Element::I,
        'K' => Element::K,
        _ => Element::Unknown,
    }
}

fn parse_helix_line(line: &str) -> Result<Helix, String> {
    if line.len() < 38 {
        return Err("HELIX line too short".to_string());
    }

    let chain_id = line.chars().nth(19).unwrap_or(' ');
    let start_residue: i32 = line[21..25].trim().parse().map_err(|_| "Invalid start residue")?;
    let end_residue: i32 = line[33..37].trim().parse().map_err(|_| "Invalid end residue")?;
    let helix_class: u8 = line[38..40].trim().parse().unwrap_or(1);

    Ok(Helix {
        chain_id,
        start_residue,
        end_residue,
        helix_class,
    })
}

fn parse_sheet_line(line: &str) -> Result<Sheet, String> {
    if line.len() < 38 {
        return Err("SHEET line too short".to_string());
    }

    let strand_id = line[11..14].trim().to_string();
    let chain_id = line.chars().nth(21).unwrap_or(' ');
    let start_residue: i32 = line[22..26].trim().parse().map_err(|_| "Invalid start residue")?;
    let end_residue: i32 = line[33..37].trim().parse().map_err(|_| "Invalid end residue")?;

    Ok(Sheet {
        strand_id,
        chain_id,
        start_residue,
        end_residue,
    })
}

fn build_chains(
    protein: &mut Protein,
    residue_map: rustc_hash::FxHashMap<(char, i32, Option<char>), Vec<usize>>,
) {
    use rustc_hash::FxHashMap;

    // Group residues by chain
    let mut chain_residues: FxHashMap<char, Vec<(i32, Option<char>, Vec<usize>)>> =
        FxHashMap::default();

    for ((chain_id, seq_num, ins_code), atom_indices) in residue_map {
        chain_residues
            .entry(chain_id)
            .or_insert_with(Vec::new)
            .push((seq_num, ins_code, atom_indices));
    }

    // Create chains
    for (chain_id, mut residues) in chain_residues {
        // Sort residues by sequence number
        residues.sort_by_key(|(seq_num, ins_code, _)| (*seq_num, *ins_code));

        let chain = Chain {
            id: chain_id,
            residues: residues
                .into_iter()
                .map(|(seq_num, ins_code, atoms)| {
                    let first_atom = &protein.atoms[atoms[0]];
                    Residue {
                        ref_info: ResidueRef {
                            name: first_atom.residue.name.clone(),
                            chain_id,
                            seq_num,
                            insertion_code: ins_code,
                        },
                        atoms,
                    }
                })
                .collect(),
        };

        protein.chains.push(chain);
    }

    // Sort chains by ID
    protein.chains.sort_by_key(|c| c.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_atom_line() {
        let line = "ATOM      1  N   GLN J  20     241.707 293.929 247.047  1.00177.60           N  ";
        let atom = parse_atom_line(line).unwrap();

        assert_eq!(atom.serial, 1);
        assert_eq!(atom.name, "N");
        assert_eq!(atom.residue.name, "GLN");
        assert_eq!(atom.residue.chain_id, 'J');
        assert_eq!(atom.residue.seq_num, 20);
        assert_eq!(atom.element, Element::N);
        assert!((atom.position.x - 241.707).abs() < 0.001);
        assert!((atom.position.y - 293.929).abs() < 0.001);
        assert!((atom.position.z - 247.047).abs() < 0.001);
    }

    #[test]
    fn test_element_inference() {
        assert_eq!(infer_element("CA"), Element::C);  // Alpha carbon
        assert_eq!(infer_element("C"), Element::C);
        assert_eq!(infer_element("N"), Element::N);
        assert_eq!(infer_element("O"), Element::O);
        assert_eq!(infer_element("ZN"), Element::Zn);
    }
}
