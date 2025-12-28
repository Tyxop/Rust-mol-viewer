use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    H, C, N, O, S, P, F, Cl, Br, I, // Common elements
    Ca, Mg, Fe, Zn, Cu, Mn, Na, K,  // Metal ions
    Unknown,
}

impl Element {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "H" => Element::H,
            "C" => Element::C,
            "N" => Element::N,
            "O" => Element::O,
            "S" => Element::S,
            "P" => Element::P,
            "F" => Element::F,
            "CL" => Element::Cl,
            "BR" => Element::Br,
            "I" => Element::I,
            "CA" => Element::Ca,
            "MG" => Element::Mg,
            "FE" => Element::Fe,
            "ZN" => Element::Zn,
            "CU" => Element::Cu,
            "MN" => Element::Mn,
            "NA" => Element::Na,
            "K" => Element::K,
            _ => Element::Unknown,
        }
    }

    /// Van der Waals radius in Angstroms
    pub fn vdw_radius(&self) -> f32 {
        match self {
            Element::H => 1.20,
            Element::C => 1.70,
            Element::N => 1.55,
            Element::O => 1.52,
            Element::S => 1.80,
            Element::P => 1.80,
            Element::F => 1.47,
            Element::Cl => 1.75,
            Element::Br => 1.85,
            Element::I => 1.98,
            Element::Ca => 2.31,
            Element::Mg => 1.73,
            Element::Fe => 2.00,
            Element::Zn => 1.39,
            Element::Cu => 1.40,
            Element::Mn => 2.00,
            Element::Na => 2.27,
            Element::K => 2.75,
            Element::Unknown => 1.70,
        }
    }

    /// Covalent radius in Angstroms
    pub fn covalent_radius(&self) -> f32 {
        match self {
            Element::H => 0.31,
            Element::C => 0.76,
            Element::N => 0.71,
            Element::O => 0.66,
            Element::S => 1.05,
            Element::P => 1.07,
            Element::F => 0.57,
            Element::Cl => 1.02,
            Element::Br => 1.20,
            Element::I => 1.39,
            Element::Ca => 1.76,
            Element::Mg => 1.41,
            Element::Fe => 1.32,
            Element::Zn => 1.22,
            Element::Cu => 1.32,
            Element::Mn => 1.39,
            Element::Na => 1.66,
            Element::K => 2.03,
            Element::Unknown => 0.70,
        }
    }

    /// CPK color in RGB (0-1 range)
    pub fn cpk_color(&self) -> [f32; 4] {
        match self {
            Element::H => [1.0, 1.0, 1.0, 1.0],    // White
            Element::C => [0.5, 0.5, 0.5, 1.0],    // Gray
            Element::N => [0.0, 0.0, 1.0, 1.0],    // Blue
            Element::O => [1.0, 0.0, 0.0, 1.0],    // Red
            Element::S => [1.0, 1.0, 0.0, 1.0],    // Yellow
            Element::P => [1.0, 0.5, 0.0, 1.0],    // Orange
            Element::F => [0.0, 1.0, 0.0, 1.0],    // Green
            Element::Cl => [0.0, 1.0, 0.0, 1.0],   // Green
            Element::Br => [0.5, 0.0, 0.0, 1.0],   // Dark red
            Element::I => [0.5, 0.0, 0.5, 1.0],    // Purple
            Element::Ca => [0.5, 0.5, 0.5, 1.0],   // Gray
            Element::Mg => [0.0, 1.0, 0.0, 1.0],   // Green
            Element::Fe => [1.0, 0.5, 0.0, 1.0],   // Orange
            Element::Zn => [0.5, 0.5, 0.5, 1.0],   // Gray
            Element::Cu => [1.0, 0.5, 0.0, 1.0],   // Orange
            Element::Mn => [0.5, 0.5, 0.5, 1.0],   // Gray
            Element::Na => [0.0, 0.0, 1.0, 1.0],   // Blue
            Element::K => [0.5, 0.0, 0.5, 1.0],    // Purple
            Element::Unknown => [1.0, 0.0, 1.0, 1.0], // Magenta
        }
    }
}

#[derive(Debug, Clone)]
pub struct Atom {
    pub serial: u32,
    pub name: String,
    pub alt_loc: Option<char>,
    pub residue: ResidueRef,
    pub position: Vec3,
    pub occupancy: f32,
    pub temp_factor: f32,
    pub element: Element,
    pub charge: Option<i8>,
}

#[derive(Debug, Clone)]
pub struct ResidueRef {
    pub name: String,      // "GLN", "VAL", etc.
    pub chain_id: char,
    pub seq_num: i32,
    pub insertion_code: Option<char>,
}

#[derive(Debug, Clone)]
pub struct Chain {
    pub id: char,
    pub residues: Vec<Residue>,
}

#[derive(Debug, Clone)]
pub struct Residue {
    pub ref_info: ResidueRef,
    pub atoms: Vec<usize>,  // Indices into Protein::atoms
}

#[derive(Debug, Clone, Copy)]
pub struct Bond {
    pub atom1: usize,
    pub atom2: usize,
    pub order: BondOrder,
}

#[derive(Debug, Clone, Copy)]
pub enum BondOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

#[derive(Debug, Clone)]
pub struct SecondaryStructure {
    pub helices: Vec<Helix>,
    pub sheets: Vec<Sheet>,
}

#[derive(Debug, Clone)]
pub struct Helix {
    pub chain_id: char,
    pub start_residue: i32,
    pub end_residue: i32,
    pub helix_class: u8,  // 1=alpha, 3=pi, 5=310
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub strand_id: String,
    pub chain_id: char,
    pub start_residue: i32,
    pub end_residue: i32,
}

#[derive(Debug, Clone)]
pub struct Protein {
    pub atoms: Vec<Atom>,
    pub chains: Vec<Chain>,
    pub bonds: Vec<Bond>,
    pub secondary_structure: SecondaryStructure,
    /// Spatial partitioning structure for efficient spatial queries (e.g., atom picking)
    pub octree: Option<crate::spatial::Octree>,
}

impl Protein {
    pub fn new() -> Self {
        Self {
            atoms: Vec::new(),
            chains: Vec::new(),
            bonds: Vec::new(),
            secondary_structure: SecondaryStructure {
                helices: Vec::new(),
                sheets: Vec::new(),
            },
            octree: None,
        }
    }

    pub fn atom_count(&self) -> usize {
        self.atoms.len()
    }

    pub fn bounding_box(&self) -> (Vec3, Vec3) {
        if self.atoms.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }

        let first = self.atoms[0].position;
        let mut min = first;
        let mut max = first;

        for atom in &self.atoms {
            min = min.min(atom.position);
            max = max.max(atom.position);
        }

        (min, max)
    }

    pub fn center(&self) -> Vec3 {
        let (min, max) = self.bounding_box();
        (min + max) * 0.5
    }
}

impl Default for Protein {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Trajectory Support (for animations)
// ============================================================================

/// A single frame in a trajectory (only coordinates, topology is shared)
#[derive(Debug, Clone)]
pub struct Frame {
    /// Coordinates for all atoms (same order as Protein::atoms)
    pub coords: Vec<Vec3>,
    /// Time in picoseconds (optional, from MODEL record or calculated)
    pub time: f32,
}

impl Frame {
    pub fn new(coords: Vec<Vec3>, time: f32) -> Self {
        Self { coords, time }
    }
}

/// Loop mode for animation playback
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once and stop
    Once,
    /// Repeat from start
    Loop,
    /// Forward then backward (ping-pong)
    PingPong,
}

/// Molecular dynamics trajectory
#[derive(Debug)]
pub struct Trajectory {
    /// Topology (atoms, bonds, chains, etc.) - shared across all frames
    pub topology: Protein,
    /// Animation frames (only coordinates)
    pub frames: Vec<Frame>,
}

impl Trajectory {
    pub fn new(topology: Protein) -> Self {
        Self {
            topology,
            frames: Vec::new(),
        }
    }

    pub fn from_protein(protein: Protein) -> Self {
        // Convert single protein to trajectory with one frame
        let coords: Vec<Vec3> = protein.atoms.iter().map(|a| a.position).collect();
        let frame = Frame::new(coords, 0.0);

        Self {
            topology: protein,
            frames: vec![frame],
        }
    }

    pub fn add_frame(&mut self, coords: Vec<Vec3>, time: f32) {
        self.frames.push(Frame::new(coords, time));
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }
}
