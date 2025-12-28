use glam::{Mat4, Vec3, Vec4};

#[derive(Debug, Clone, Copy)]
pub struct Plane {
    pub normal: Vec3,
    pub distance: f32,
}

impl Plane {
    pub fn new(normal: Vec3, distance: f32) -> Self {
        Self { normal, distance }
    }

    pub fn from_point_normal(point: Vec3, normal: Vec3) -> Self {
        let normal = normal.normalize();
        let distance = -normal.dot(point);
        Self { normal, distance }
    }

    pub fn distance_to_point(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.distance
    }

    pub fn is_sphere_visible(&self, center: Vec3, radius: f32) -> bool {
        self.distance_to_point(center) >= -radius
    }
}

#[derive(Debug, Clone)]
pub struct Frustum {
    pub planes: [Plane; 6], // Left, Right, Top, Bottom, Near, Far
}

impl Frustum {
    pub fn from_view_projection(view_proj: Mat4) -> Self {
        let mut planes = [Plane::new(Vec3::ZERO, 0.0); 6];

        // Extract frustum planes from view-projection matrix
        // Plane equations: Ax + By + Cz + D = 0
        // Each plane is extracted from the matrix rows

        // Left plane: row4 + row1
        let row4 = view_proj.row(3);
        let row1 = view_proj.row(0);
        planes[0] = Self::normalize_plane(row4 + row1);

        // Right plane: row4 - row1
        planes[1] = Self::normalize_plane(row4 - row1);

        // Bottom plane: row4 + row2
        let row2 = view_proj.row(1);
        planes[2] = Self::normalize_plane(row4 + row2);

        // Top plane: row4 - row2
        planes[3] = Self::normalize_plane(row4 - row2);

        // Near plane: row4 + row3
        let row3 = view_proj.row(2);
        planes[4] = Self::normalize_plane(row4 + row3);

        // Far plane: row4 - row3
        planes[5] = Self::normalize_plane(row4 - row3);

        Self { planes }
    }

    fn normalize_plane(row: Vec4) -> Plane {
        let normal = Vec3::new(row.x, row.y, row.z);
        let length = normal.length();

        if length > 0.0001 {
            Plane {
                normal: normal / length,
                distance: row.w / length,
            }
        } else {
            Plane {
                normal: Vec3::ZERO,
                distance: 0.0,
            }
        }
    }

    pub fn is_sphere_visible(&self, center: Vec3, radius: f32) -> bool {
        // Sphere is visible if it's on the positive side of all planes
        for plane in &self.planes {
            if !plane.is_sphere_visible(center, radius) {
                return false;
            }
        }
        true
    }

    pub fn is_box_visible(&self, min: Vec3, max: Vec3) -> bool {
        // Check if bounding box is visible
        // Box is visible if any corner is inside frustum

        for plane in &self.planes {
            // Get positive vertex (farthest point in plane normal direction)
            let p_vertex = Vec3::new(
                if plane.normal.x >= 0.0 { max.x } else { min.x },
                if plane.normal.y >= 0.0 { max.y } else { min.y },
                if plane.normal.z >= 0.0 { max.z } else { min.z },
            );

            if plane.distance_to_point(p_vertex) < 0.0 {
                return false; // Box is completely outside this plane
            }
        }

        true
    }
}

pub struct CullingSystem {
    frustum: Frustum,
    pub culled_count: usize,
    pub visible_count: usize,
}

impl CullingSystem {
    pub fn new() -> Self {
        Self {
            frustum: Frustum {
                planes: [Plane::new(Vec3::ZERO, 0.0); 6],
            },
            culled_count: 0,
            visible_count: 0,
        }
    }

    pub fn update(&mut self, view_proj: Mat4) {
        self.frustum = Frustum::from_view_projection(view_proj);
    }

    pub fn is_sphere_visible(&self, center: Vec3, radius: f32) -> bool {
        self.frustum.is_sphere_visible(center, radius)
    }

    pub fn is_box_visible(&self, min: Vec3, max: Vec3) -> bool {
        self.frustum.is_box_visible(min, max)
    }

    pub fn cull_spheres<T, F>(&mut self, items: &[T], get_sphere: F) -> Vec<usize>
    where
        F: Fn(&T) -> (Vec3, f32),
    {
        let mut visible = Vec::new();

        for (i, item) in items.iter().enumerate() {
            let (center, radius) = get_sphere(item);
            if self.is_sphere_visible(center, radius) {
                visible.push(i);
            }
        }

        self.visible_count = visible.len();
        self.culled_count = items.len() - visible.len();

        visible
    }

    pub fn get_frustum(&self) -> &Frustum {
        &self.frustum
    }
}

impl Default for CullingSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plane_distance() {
        let plane = Plane::from_point_normal(Vec3::ZERO, Vec3::Y);
        assert!(plane.distance_to_point(Vec3::new(0.0, 1.0, 0.0)) > 0.0);
        assert!(plane.distance_to_point(Vec3::new(0.0, -1.0, 0.0)) < 0.0);
    }

    #[test]
    fn test_sphere_visibility() {
        let plane = Plane::from_point_normal(Vec3::ZERO, Vec3::Y);

        // Sphere above plane (visible)
        assert!(plane.is_sphere_visible(Vec3::new(0.0, 2.0, 0.0), 1.0));

        // Sphere below plane (not visible)
        assert!(!plane.is_sphere_visible(Vec3::new(0.0, -2.0, 0.0), 1.0));

        // Sphere intersecting plane (visible)
        assert!(plane.is_sphere_visible(Vec3::new(0.0, 0.5, 0.0), 1.0));
    }
}
