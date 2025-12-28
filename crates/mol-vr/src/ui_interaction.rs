//! VR UI Interaction
//!
//! This module provides ray-quad intersection testing and event injection
//! for interacting with 3D UI panels in VR.

use glam::{Mat4, Vec2, Vec3};
use mol_core::Ray;

use crate::ui_panel::VrUiPanel;

/// Result of a ray-quad intersection test
#[derive(Debug, Clone, Copy)]
pub struct QuadIntersection {
    /// UV coordinates on the quad (0-1 range)
    pub uv: Vec2,
    /// Distance along the ray to the intersection point
    pub distance: f32,
}

/// Test if a ray intersects with a UI panel quad
///
/// # Arguments
/// * `ray` - The picking ray in world space
/// * `panel` - The UI panel to test against
///
/// # Returns
/// Some(QuadIntersection) if the ray hits the quad, None otherwise
///
/// # Algorithm
/// 1. Transform ray to panel's local space
/// 2. Test intersection with XY plane (Z=0 in local space)
/// 3. Check if hit point is within quad bounds [-0.5, 0.5]
/// 4. Convert to UV coordinates [0, 1]
pub fn ray_quad_intersection(ray: &Ray, panel: &VrUiPanel) -> Option<QuadIntersection> {
    // Compute aspect ratio and build transform matrix
    let aspect = panel.width as f32 / panel.height as f32;

    // Build model matrix: Translation * Rotation * Scale
    let model_matrix = Mat4::from_scale_rotation_translation(
        Vec3::new(panel.scale * aspect, panel.scale, 1.0),
        panel.rotation,
        panel.position,
    );

    // Inverse transform to go from world space to quad local space
    let inv_transform = model_matrix.inverse();

    // Transform ray to local space
    let local_ray_origin = inv_transform.transform_point3(ray.origin);
    let local_ray_dir = inv_transform.transform_vector3(ray.direction).normalize();

    // In local space, the quad is in the XY plane (Z=0)
    // Check if ray is parallel to the quad
    if local_ray_dir.z.abs() < 0.0001 {
        return None; // Ray is parallel, no intersection
    }

    // Compute intersection distance t where ray hits Z=0 plane
    let t = -local_ray_origin.z / local_ray_dir.z;

    // Check if intersection is behind the ray origin
    if t < 0.0 {
        return None;
    }

    // Compute hit point in local space
    let hit_point = local_ray_origin + local_ray_dir * t;

    // Check if hit point is within quad bounds [-0.5, 0.5]
    if hit_point.x < -0.5 || hit_point.x > 0.5 || hit_point.y < -0.5 || hit_point.y > 0.5 {
        return None;
    }

    // Convert to UV coordinates [0, 1]
    // X: -0.5 -> 0, +0.5 -> 1
    // Y: +0.5 -> 0, -0.5 -> 1 (flip Y for texture coordinates)
    let uv = Vec2::new(hit_point.x + 0.5, 0.5 - hit_point.y);

    Some(QuadIntersection { uv, distance: t })
}

/// Convert UV coordinates to egui screen position
///
/// # Arguments
/// * `uv` - UV coordinates in [0, 1] range
/// * `panel_width` - Width of the UI panel in pixels
/// * `panel_height` - Height of the UI panel in pixels
///
/// # Returns
/// egui::Pos2 in screen pixel coordinates
pub fn uv_to_screen_pos(uv: Vec2, panel_width: u32, panel_height: u32) -> egui::Pos2 {
    egui::pos2(uv.x * panel_width as f32, uv.y * panel_height as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_quad_intersection_center_hit() {
        // Create a simple panel at origin, facing -Z
        let panel = create_test_panel(Vec3::ZERO, Quat::IDENTITY, 1.0);

        // Ray from (0, 0, 2) pointing toward panel (0, 0, -1)
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 2.0),
            direction: Vec3::new(0.0, 0.0, -1.0),
        };

        let result = ray_quad_intersection(&ray, &panel);
        assert!(result.is_some());

        let intersection = result.unwrap();
        // Center of quad should be UV (0.5, 0.5)
        assert!((intersection.uv.x - 0.5).abs() < 0.01);
        assert!((intersection.uv.y - 0.5).abs() < 0.01);
        assert!((intersection.distance - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_ray_quad_intersection_miss() {
        let panel = create_test_panel(Vec3::ZERO, Quat::IDENTITY, 1.0);

        // Ray pointing away from panel
        let ray = Ray {
            origin: Vec3::new(0.0, 0.0, 2.0),
            direction: Vec3::new(0.0, 0.0, 1.0), // Pointing away
        };

        let result = ray_quad_intersection(&ray, &panel);
        assert!(result.is_none());
    }

    #[test]
    fn test_ray_quad_intersection_outside_bounds() {
        let panel = create_test_panel(Vec3::ZERO, Quat::IDENTITY, 1.0);

        // Ray hits plane but outside quad bounds
        let ray = Ray {
            origin: Vec3::new(2.0, 2.0, 2.0), // Far to the side
            direction: Vec3::new(0.0, 0.0, -1.0),
        };

        let result = ray_quad_intersection(&ray, &panel);
        assert!(result.is_none());
    }

    // Helper to create a test panel (mocked)
    fn create_test_panel(position: Vec3, rotation: Quat, scale: f32) -> VrUiPanel {
        // This is a mock - in real tests we'd need to create actual wgpu resources
        // For now, this won't compile without proper initialization
        // But it shows the test structure
        unimplemented!("Test helper needs proper wgpu device initialization")
    }
}
