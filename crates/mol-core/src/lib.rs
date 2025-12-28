//! Molecular core data structures and algorithms
//!
//! This crate provides shared utilities for molecular visualization,
//! including ray picking and geometric operations.

use glam::Vec3;

/// A ray in 3D space, used for ray casting and picking operations
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    /// Origin point of the ray
    pub origin: Vec3,
    /// Direction vector (should be normalized)
    pub direction: Vec3,
}

impl Ray {
    /// Create a new ray with the given origin and direction
    ///
    /// Note: The direction vector should be normalized for correct intersection tests
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self {
            origin,
            direction: direction.normalize(),
        }
    }

    /// Get a point along the ray at distance t from the origin
    pub fn point_at(&self, t: f32) -> Vec3 {
        self.origin + t * self.direction
    }
}

/// Test intersection between a ray and a sphere
///
/// Returns the distance along the ray to the closest intersection point,
/// or None if there is no intersection.
///
/// # Arguments
/// * `ray` - The ray to test
/// * `center` - Center point of the sphere
/// * `radius` - Radius of the sphere
///
/// # Algorithm
/// Solves the quadratic equation for ray-sphere intersection:
/// ||ray.origin + t*ray.direction - center||^2 = radius^2
///
/// This gives: a*t^2 + b*t + c = 0 where:
/// - a = direction · direction (should be 1 if normalized)
/// - b = 2 * direction · (origin - center)
/// - c = (origin - center) · (origin - center) - radius^2
pub fn ray_sphere_intersection(ray: &Ray, center: Vec3, radius: f32) -> Option<f32> {
    let oc = ray.origin - center;

    // Coefficients for quadratic equation at^2 + bt + c = 0
    let a = ray.direction.dot(ray.direction);
    let b = 2.0 * oc.dot(ray.direction);
    let c = oc.dot(oc) - radius * radius;

    let discriminant = b * b - 4.0 * a * c;

    if discriminant < 0.0 {
        // No intersection
        None
    } else {
        // Calculate the closest intersection point (smallest positive t)
        let sqrt_disc = discriminant.sqrt();
        let t1 = (-b - sqrt_disc) / (2.0 * a);
        let t2 = (-b + sqrt_disc) / (2.0 * a);

        // Return the closest positive intersection
        if t1 > 0.0 {
            Some(t1)
        } else if t2 > 0.0 {
            Some(t2)
        } else {
            // Both intersections are behind the ray origin
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_creation() {
        let ray = Ray::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(ray.origin, Vec3::ZERO);
        assert_eq!(ray.direction, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn test_ray_point_at() {
        let ray = Ray::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(ray.point_at(5.0), Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn test_ray_sphere_hit() {
        let ray = Ray::new(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let sphere_center = Vec3::ZERO;
        let sphere_radius = 1.0;

        let hit = ray_sphere_intersection(&ray, sphere_center, sphere_radius);
        assert!(hit.is_some());

        let t = hit.unwrap();
        assert!((t - 4.0).abs() < 0.001); // Should hit at t=4.0 (-5 + 4 = -1, edge of sphere)
    }

    #[test]
    fn test_ray_sphere_miss() {
        let ray = Ray::new(Vec3::new(-5.0, 5.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let sphere_center = Vec3::ZERO;
        let sphere_radius = 1.0;

        let hit = ray_sphere_intersection(&ray, sphere_center, sphere_radius);
        assert!(hit.is_none());
    }

    #[test]
    fn test_ray_sphere_inside() {
        // Ray starting inside the sphere
        let ray = Ray::new(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let sphere_center = Vec3::ZERO;
        let sphere_radius = 5.0;

        let hit = ray_sphere_intersection(&ray, sphere_center, sphere_radius);
        assert!(hit.is_some());

        let t = hit.unwrap();
        assert!(t > 0.0); // Should exit at positive t
    }
}
