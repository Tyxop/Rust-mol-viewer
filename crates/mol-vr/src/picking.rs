//! VR Ray Picking Utilities
//!
//! This module provides utilities for generating picking rays from VR controller poses,
//! enabling atom selection and interaction in VR space.

use glam::Vec3;
use mol_core::Ray;

use crate::input::Pose;

/// Generate a picking ray from a VR controller pose
///
/// # Arguments
/// * `pose` - The controller's pose (position and orientation)
///
/// # Returns
/// A Ray pointing forward from the controller in world space
///
/// # Notes
/// In OpenXR, the controller's forward direction is along the -Z axis after applying
/// the orientation quaternion. This creates a ray that extends from the controller's
/// position in the direction it's pointing.
pub fn controller_ray(pose: &Pose) -> Ray {
    let origin = pose.position;

    // In OpenXR, forward is -Z axis after rotation
    // Apply the controller's orientation to the -Z vector
    let direction = (pose.orientation * Vec3::new(0.0, 0.0, -1.0)).normalize();

    Ray { origin, direction }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn test_controller_ray_identity() {
        // Controller at origin, pointing forward (-Z)
        let pose = Pose {
            position: Vec3::ZERO,
            orientation: Quat::IDENTITY,
        };

        let ray = controller_ray(&pose);

        assert_eq!(ray.origin, Vec3::ZERO);
        assert!((ray.direction - Vec3::new(0.0, 0.0, -1.0)).length() < 0.001);
    }

    #[test]
    fn test_controller_ray_translated() {
        // Controller at (1, 2, 3), pointing forward (-Z)
        let pose = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::IDENTITY,
        };

        let ray = controller_ray(&pose);

        assert_eq!(ray.origin, Vec3::new(1.0, 2.0, 3.0));
        assert!((ray.direction - Vec3::new(0.0, 0.0, -1.0)).length() < 0.001);
    }

    #[test]
    fn test_controller_ray_rotated() {
        // Controller at origin, rotated 90 degrees around Y axis
        // Should point along +X axis
        let pose = Pose {
            position: Vec3::ZERO,
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };

        let ray = controller_ray(&pose);

        assert_eq!(ray.origin, Vec3::ZERO);
        // After 90° rotation around Y, -Z becomes +X
        assert!((ray.direction - Vec3::new(1.0, 0.0, 0.0)).length() < 0.001);
    }
}
