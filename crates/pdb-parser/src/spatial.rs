use crate::structures::*;
use glam::Vec3;

#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl BoundingBox {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn contains_point(&self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        let closest = Vec3::new(
            center.x.clamp(self.min.x, self.max.x),
            center.y.clamp(self.min.y, self.max.y),
            center.z.clamp(self.min.z, self.max.z),
        );

        center.distance(closest) <= radius
    }
}

#[derive(Debug, Clone)]
pub struct OctreeNode {
    pub bounds: BoundingBox,
    pub atoms: Vec<usize>, // Atom indices if leaf
    pub children: Option<Box<[OctreeNode; 8]>>,
}

#[derive(Debug, Clone)]
pub struct Octree {
    pub root: OctreeNode,
    pub max_depth: u32,
    pub max_atoms_per_leaf: usize,
}

impl Octree {
    pub fn new(protein: &Protein, max_depth: u32, max_atoms_per_leaf: usize) -> Self {
        let (min, max) = protein.bounding_box();

        // Expand bounds slightly to ensure all atoms are inside
        let expansion = 0.1;
        let size = max - min;
        let min = min - size * expansion;
        let max = max + size * expansion;

        let bounds = BoundingBox::new(min, max);

        let atom_indices: Vec<usize> = (0..protein.atoms.len()).collect();

        let root = Self::build_node(&protein.atoms, &atom_indices, &bounds, 0, max_depth, max_atoms_per_leaf);

        log::info!(
            "Built octree: max_depth={}, max_atoms_per_leaf={}, total_atoms={}",
            max_depth,
            max_atoms_per_leaf,
            protein.atoms.len()
        );

        Self {
            root,
            max_depth,
            max_atoms_per_leaf,
        }
    }

    fn build_node(
        atoms: &[Atom],
        atom_indices: &[usize],
        bounds: &BoundingBox,
        depth: u32,
        max_depth: u32,
        max_atoms: usize,
    ) -> OctreeNode {
        // If we've reached max depth or have few atoms, create a leaf
        if depth >= max_depth || atom_indices.len() <= max_atoms {
            return OctreeNode {
                bounds: bounds.clone(),
                atoms: atom_indices.to_vec(),
                children: None,
            };
        }

        // Subdivide into 8 children
        let center = bounds.center();
        let mut child_bounds = Vec::with_capacity(8);

        for i in 0..8 {
            let x_bit = (i & 1) != 0;
            let y_bit = (i & 2) != 0;
            let z_bit = (i & 4) != 0;

            let child_min = Vec3::new(
                if x_bit { center.x } else { bounds.min.x },
                if y_bit { center.y } else { bounds.min.y },
                if z_bit { center.z } else { bounds.min.z },
            );

            let child_max = Vec3::new(
                if x_bit { bounds.max.x } else { center.x },
                if y_bit { bounds.max.y } else { center.y },
                if z_bit { bounds.max.z } else { center.z },
            );

            child_bounds.push(BoundingBox::new(child_min, child_max));
        }

        // Distribute atoms to children
        let mut child_atoms: Vec<Vec<usize>> = vec![Vec::new(); 8];

        for &atom_idx in atom_indices {
            let atom_pos = atoms[atom_idx].position;

            for (i, child_bound) in child_bounds.iter().enumerate() {
                if child_bound.contains_point(atom_pos) {
                    child_atoms[i].push(atom_idx);
                    break;
                }
            }
        }

        // Recursively build children
        let children: Vec<OctreeNode> = (0..8)
            .map(|i| {
                Self::build_node(
                    atoms,
                    &child_atoms[i],
                    &child_bounds[i],
                    depth + 1,
                    max_depth,
                    max_atoms,
                )
            })
            .collect();

        // Convert to array
        let children_array: [OctreeNode; 8] = children.try_into().unwrap();

        OctreeNode {
            bounds: bounds.clone(),
            atoms: Vec::new(), // Interior node has no atoms
            children: Some(Box::new(children_array)),
        }
    }

    /// Query atoms within a sphere
    pub fn query_sphere(&self, center: Vec3, radius: f32) -> Vec<usize> {
        let mut result = Vec::new();
        Self::query_sphere_recursive(&self.root, center, radius, &mut result);
        result
    }

    fn query_sphere_recursive(
        node: &OctreeNode,
        center: Vec3,
        radius: f32,
        result: &mut Vec<usize>,
    ) {
        // Check if sphere intersects node bounds
        if !node.bounds.intersects_sphere(center, radius) {
            return;
        }

        // If leaf, check all atoms
        if node.children.is_none() {
            result.extend_from_slice(&node.atoms);
            return;
        }

        // Recurse into children
        if let Some(ref children) = node.children {
            for child in children.iter() {
                Self::query_sphere_recursive(child, center, radius, result);
            }
        }
    }

    /// Query atoms within a bounding box
    pub fn query_box(&self, query_box: &BoundingBox) -> Vec<usize> {
        let mut result = Vec::new();
        Self::query_box_recursive(&self.root, query_box, &mut result);
        result
    }

    fn query_box_recursive(
        node: &OctreeNode,
        query_box: &BoundingBox,
        result: &mut Vec<usize>,
    ) {
        // Check if boxes intersect
        if !boxes_intersect(&node.bounds, query_box) {
            return;
        }

        // If leaf, add all atoms
        if node.children.is_none() {
            result.extend_from_slice(&node.atoms);
            return;
        }

        // Recurse into children
        if let Some(ref children) = node.children {
            for child in children.iter() {
                Self::query_box_recursive(child, query_box, result);
            }
        }
    }
}

fn boxes_intersect(a: &BoundingBox, b: &BoundingBox) -> bool {
    a.min.x <= b.max.x
        && a.max.x >= b.min.x
        && a.min.y <= b.max.y
        && a.max.y >= b.min.y
        && a.min.z <= b.max.z
        && a.max.z >= b.min.z
}

impl Protein {
    pub fn build_octree(&mut self, max_depth: u32, max_atoms_per_leaf: usize) {
        let _octree = Octree::new(self, max_depth, max_atoms_per_leaf);
        // Store octree in protein if needed (would require adding field to Protein struct)
        // For now, octree is built on-demand
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box_contains() {
        let bbox = BoundingBox::new(Vec3::ZERO, Vec3::ONE);
        assert!(bbox.contains_point(Vec3::new(0.5, 0.5, 0.5)));
        assert!(!bbox.contains_point(Vec3::new(2.0, 0.5, 0.5)));
    }

    #[test]
    fn test_bounding_box_intersects_sphere() {
        let bbox = BoundingBox::new(Vec3::ZERO, Vec3::ONE);
        assert!(bbox.intersects_sphere(Vec3::new(0.5, 0.5, 0.5), 0.1));
        assert!(bbox.intersects_sphere(Vec3::new(1.5, 0.5, 0.5), 0.6));
        assert!(!bbox.intersects_sphere(Vec3::new(5.0, 0.5, 0.5), 1.0));
    }
}
