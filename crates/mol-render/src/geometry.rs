use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

impl Vertex {
    pub fn new(position: Vec3, normal: Vec3) -> Self {
        Self {
            position: position.into(),
            normal: normal.into(),
        }
    }

    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// Generate an icosphere (subdivided icosahedron)
pub fn create_icosphere(subdivisions: u32) -> Mesh {
    // Golden ratio
    let t = (1.0 + 5.0_f32.sqrt()) / 2.0;

    // Initial 12 vertices of icosahedron
    let mut vertices = vec![
        Vec3::new(-1.0, t, 0.0).normalize(),
        Vec3::new(1.0, t, 0.0).normalize(),
        Vec3::new(-1.0, -t, 0.0).normalize(),
        Vec3::new(1.0, -t, 0.0).normalize(),
        Vec3::new(0.0, -1.0, t).normalize(),
        Vec3::new(0.0, 1.0, t).normalize(),
        Vec3::new(0.0, -1.0, -t).normalize(),
        Vec3::new(0.0, 1.0, -t).normalize(),
        Vec3::new(t, 0.0, -1.0).normalize(),
        Vec3::new(t, 0.0, 1.0).normalize(),
        Vec3::new(-t, 0.0, -1.0).normalize(),
        Vec3::new(-t, 0.0, 1.0).normalize(),
    ];

    // Initial 20 triangular faces
    let mut indices = vec![
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11,
        1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7, 1, 8,
        3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9,
        4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9, 8, 1,
    ];

    // Subdivide
    for _ in 0..subdivisions {
        let mut new_indices = Vec::new();
        let mut midpoint_cache = std::collections::HashMap::new();

        for chunk in indices.chunks(3) {
            let v0 = chunk[0];
            let v1 = chunk[1];
            let v2 = chunk[2];

            // Get midpoints (or create and cache them)
            let m0 = get_midpoint(v0, v1, &mut vertices, &mut midpoint_cache);
            let m1 = get_midpoint(v1, v2, &mut vertices, &mut midpoint_cache);
            let m2 = get_midpoint(v2, v0, &mut vertices, &mut midpoint_cache);

            // Create 4 triangles from 1
            new_indices.extend_from_slice(&[v0, m0, m2]);
            new_indices.extend_from_slice(&[v1, m1, m0]);
            new_indices.extend_from_slice(&[v2, m2, m1]);
            new_indices.extend_from_slice(&[m0, m1, m2]);
        }

        indices = new_indices;
    }

    // Convert to Vertex format (for unit sphere, position == normal)
    let vertex_buffer: Vec<Vertex> = vertices
        .iter()
        .map(|&pos| Vertex::new(pos, pos))
        .collect();

    Mesh {
        vertices: vertex_buffer,
        indices,
    }
}

fn get_midpoint(
    v0: u32,
    v1: u32,
    vertices: &mut Vec<Vec3>,
    cache: &mut std::collections::HashMap<(u32, u32), u32>,
) -> u32 {
    let key = if v0 < v1 { (v0, v1) } else { (v1, v0) };

    if let Some(&index) = cache.get(&key) {
        return index;
    }

    let p0 = vertices[v0 as usize];
    let p1 = vertices[v1 as usize];
    let middle = ((p0 + p1) * 0.5).normalize();

    let index = vertices.len() as u32;
    vertices.push(middle);
    cache.insert(key, index);

    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icosphere_generation() {
        let mesh = create_icosphere(0);
        assert_eq!(mesh.vertices.len(), 12); // Base icosahedron
        assert_eq!(mesh.indices.len(), 60);  // 20 triangles * 3 vertices

        let mesh1 = create_icosphere(1);
        assert!(mesh1.vertices.len() > 12);
        assert!(mesh1.indices.len() > 60);
    }

    #[test]
    fn test_vertex_normalized() {
        let mesh = create_icosphere(2);
        for vertex in &mesh.vertices {
            let pos = Vec3::from(vertex.position);
            let length = pos.length();
            assert!((length - 1.0).abs() < 0.001, "Vertex not normalized: {}", length);
        }
    }
}
