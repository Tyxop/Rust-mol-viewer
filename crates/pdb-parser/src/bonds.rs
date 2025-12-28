use crate::structures::*;
use rayon::prelude::*;

/// Infer bonds based on atomic distances
pub fn infer_bonds(protein: &Protein) -> Vec<Bond> {
    let mut bonds = Vec::new();

    // For efficiency, we'll use a simple distance-based approach
    // In a full implementation, you'd use a spatial index (octree)

    log::info!("Inferring bonds for {} atoms...", protein.atoms.len());

    // Parallel bond detection using rayon
    let all_bonds: Vec<Vec<Bond>> = protein
        .atoms
        .par_iter()
        .enumerate()
        .map(|(i, atom1)| {
            let mut local_bonds = Vec::new();

            for (j, atom2) in protein.atoms.iter().enumerate().skip(i + 1) {
                // Skip if atoms are too far apart (optimization)
                let distance = atom1.position.distance(atom2.position);

                // Maximum bond distance = sum of covalent radii + tolerance
                let max_bond_distance = atom1.element.covalent_radius()
                    + atom2.element.covalent_radius()
                    + 0.4; // 0.4 Angstrom tolerance

                if distance < max_bond_distance {
                    local_bonds.push(Bond {
                        atom1: i,
                        atom2: j,
                        order: infer_bond_order(&atom1.element, &atom2.element, distance),
                    });
                }
            }

            local_bonds
        })
        .collect();

    // Flatten all bonds
    for local_bonds in all_bonds {
        bonds.extend(local_bonds);
    }

    log::info!("Inferred {} bonds", bonds.len());

    bonds
}

/// Simple bond order inference based on distance
fn infer_bond_order(elem1: &Element, elem2: &Element, distance: f32) -> BondOrder {
    // Typical single bond distance
    let single_bond_dist = elem1.covalent_radius() + elem2.covalent_radius();

    // If distance is significantly shorter, might be double or triple
    // This is a simplification; proper implementation would consider
    // chemical context
    let ratio = distance / single_bond_dist;

    if ratio < 0.85 {
        BondOrder::Double // Shorter distance suggests multiple bond
    } else {
        BondOrder::Single
    }
}

/// Optimized bond inference using spatial partitioning
/// This is a simplified version; full implementation would use octree
pub fn infer_bonds_optimized(protein: &Protein, max_search_radius: f32) -> Vec<Bond> {
    let mut bonds = Vec::new();

    // Build a simple grid-based spatial index
    let (min, max) = protein.bounding_box();
    let grid_size = max_search_radius * 2.0;

    // Create grid cells
    let grid_dims = ((max - min) / grid_size).ceil();
    let nx = grid_dims.x.max(1.0) as usize;
    let ny = grid_dims.y.max(1.0) as usize;
    let nz = grid_dims.z.max(1.0) as usize;

    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); nx * ny * nz];

    // Assign atoms to grid cells
    for (idx, atom) in protein.atoms.iter().enumerate() {
        let pos = (atom.position - min) / grid_size;
        let ix = (pos.x as usize).min(nx - 1);
        let iy = (pos.y as usize).min(ny - 1);
        let iz = (pos.z as usize).min(nz - 1);
        let cell_idx = ix + iy * nx + iz * nx * ny;
        grid[cell_idx].push(idx);
    }

    // Check bonds only within neighboring cells
    for (i, atom1) in protein.atoms.iter().enumerate() {
        let pos = (atom1.position - min) / grid_size;
        let ix = (pos.x as usize).min(nx - 1);
        let iy = (pos.y as usize).min(ny - 1);
        let iz = (pos.z as usize).min(nz - 1);

        // Check current cell and neighbors
        for dx in -1..=1_i32 {
            for dy in -1..=1_i32 {
                for dz in -1..=1_i32 {
                    let nx_i = ix as i32 + dx;
                    let ny_i = iy as i32 + dy;
                    let nz_i = iz as i32 + dz;

                    if nx_i < 0 || ny_i < 0 || nz_i < 0 {
                        continue;
                    }
                    if nx_i >= nx as i32 || ny_i >= ny as i32 || nz_i >= nz as i32 {
                        continue;
                    }

                    let cell_idx = nx_i as usize
                        + ny_i as usize * nx
                        + nz_i as usize * nx * ny;

                    for &j in &grid[cell_idx] {
                        if j <= i {
                            continue; // Avoid duplicates
                        }

                        let atom2 = &protein.atoms[j];
                        let distance = atom1.position.distance(atom2.position);

                        let max_bond_distance = atom1.element.covalent_radius()
                            + atom2.element.covalent_radius()
                            + 0.4;

                        if distance < max_bond_distance {
                            bonds.push(Bond {
                                atom1: i,
                                atom2: j,
                                order: infer_bond_order(&atom1.element, &atom2.element, distance),
                            });
                        }
                    }
                }
            }
        }
    }

    log::info!("Inferred {} bonds (optimized)", bonds.len());

    bonds
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn test_bond_inference() {
        let mut protein = Protein::new();

        // Create two carbon atoms close enough to bond
        protein.atoms.push(Atom {
            serial: 1,
            name: "C1".to_string(),
            alt_loc: None,
            residue: ResidueRef {
                name: "TST".to_string(),
                chain_id: 'A',
                seq_num: 1,
                insertion_code: None,
            },
            position: Vec3::new(0.0, 0.0, 0.0),
            occupancy: 1.0,
            temp_factor: 0.0,
            element: Element::C,
            charge: None,
        });

        protein.atoms.push(Atom {
            serial: 2,
            name: "C2".to_string(),
            alt_loc: None,
            residue: ResidueRef {
                name: "TST".to_string(),
                chain_id: 'A',
                seq_num: 1,
                insertion_code: None,
            },
            position: Vec3::new(1.5, 0.0, 0.0), // ~1.5 Angstrom apart (typical C-C bond)
            occupancy: 1.0,
            temp_factor: 0.0,
            element: Element::C,
            charge: None,
        });

        let bonds = infer_bonds(&protein);

        assert_eq!(bonds.len(), 1);
        assert_eq!(bonds[0].atom1, 0);
        assert_eq!(bonds[0].atom2, 1);
    }
}
