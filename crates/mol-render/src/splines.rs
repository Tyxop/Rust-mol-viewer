use glam::Vec3;

/// Catmull-Rom spline for smooth interpolation between control points
pub struct CatmullRomSpline {
    pub control_points: Vec<Vec3>,
}

impl CatmullRomSpline {
    pub fn new(control_points: Vec<Vec3>) -> Self {
        Self { control_points }
    }

    /// Evaluate position at parameter t (0.0 to 1.0) for the given segment
    /// segment_idx refers to the segment between control_points[segment_idx] and control_points[segment_idx+1]
    pub fn evaluate(&self, segment_idx: usize, t: f32) -> Vec3 {
        if self.control_points.len() < 4 {
            // Fallback for short chains
            return self.control_points[segment_idx.min(self.control_points.len() - 1)];
        }

        // Get the four control points for Catmull-Rom
        let p0 = self.get_point(segment_idx.saturating_sub(1));
        let p1 = self.get_point(segment_idx);
        let p2 = self.get_point(segment_idx + 1);
        let p3 = self.get_point(segment_idx + 2);

        // Catmull-Rom formula:
        // P(t) = 0.5 * [(2*P1) + (-P0 + P2)*t + (2*P0 - 5*P1 + 4*P2 - P3)*t² + (-P0 + 3*P1 - 3*P2 + P3)*t³]
        let t2 = t * t;
        let t3 = t2 * t;

        0.5 * (2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
    }

    /// Evaluate tangent (derivative) at parameter t for the given segment
    pub fn tangent(&self, segment_idx: usize, t: f32) -> Vec3 {
        if self.control_points.len() < 4 {
            // Fallback: simple forward difference
            if segment_idx + 1 < self.control_points.len() {
                return (self.control_points[segment_idx + 1] - self.control_points[segment_idx])
                    .normalize();
            }
            return Vec3::Z; // Default forward direction
        }

        let p0 = self.get_point(segment_idx.saturating_sub(1));
        let p1 = self.get_point(segment_idx);
        let p2 = self.get_point(segment_idx + 1);
        let p3 = self.get_point(segment_idx + 2);

        // Derivative of Catmull-Rom:
        // P'(t) = 0.5 * [(-P0 + P2) + 2*(2*P0 - 5*P1 + 4*P2 - P3)*t + 3*(-P0 + 3*P1 - 3*P2 + P3)*t²]
        let t2 = t * t;

        let tangent = 0.5 * ((-p0 + p2)
            + 2.0 * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t
            + 3.0 * (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t2);

        tangent.normalize()
    }

    /// Subdivide the spline into a series of points
    /// segments_per_interval: number of subdivisions between each control point pair
    pub fn subdivide(&self, segments_per_interval: usize) -> Vec<Vec3> {
        if self.control_points.len() < 2 {
            return self.control_points.clone();
        }

        let mut result = Vec::new();

        // For each segment between control points
        for i in 0..(self.control_points.len() - 1) {
            for j in 0..segments_per_interval {
                let t = j as f32 / segments_per_interval as f32;
                result.push(self.evaluate(i, t));
            }
        }

        // Add the last point
        result.push(*self.control_points.last().unwrap());

        result
    }

    /// Get control point with boundary handling
    fn get_point(&self, idx: usize) -> Vec3 {
        if idx >= self.control_points.len() {
            *self.control_points.last().unwrap()
        } else {
            self.control_points[idx]
        }
    }

    /// Get subdivided positions and tangents
    pub fn subdivide_with_tangents(
        &self,
        segments_per_interval: usize,
    ) -> (Vec<Vec3>, Vec<Vec3>) {
        if self.control_points.len() < 2 {
            return (
                self.control_points.clone(),
                vec![Vec3::Z; self.control_points.len()],
            );
        }

        let mut positions = Vec::new();
        let mut tangents = Vec::new();

        for i in 0..(self.control_points.len() - 1) {
            for j in 0..segments_per_interval {
                let t = j as f32 / segments_per_interval as f32;
                positions.push(self.evaluate(i, t));
                tangents.push(self.tangent(i, t));
            }
        }

        // Add the last point
        positions.push(*self.control_points.last().unwrap());
        tangents.push(self.tangent(self.control_points.len() - 2, 1.0));

        (positions, tangents)
    }
}

/// Compute normals using parallel transport to avoid twisting
/// This creates a smooth normal frame that follows the curve without unnecessary rotation
pub fn compute_parallel_transport_normals(
    positions: &[Vec3],
    tangents: &[Vec3],
    initial_normal: Vec3,
) -> Vec<Vec3> {
    if positions.is_empty() {
        return Vec::new();
    }

    let mut normals = Vec::with_capacity(positions.len());

    // First normal
    let mut normal = initial_normal.normalize();
    // Ensure the first normal is perpendicular to the first tangent
    normal = (normal - tangents[0] * normal.dot(tangents[0])).normalize();
    normals.push(normal);

    // Parallel transport along the curve
    for i in 1..positions.len() {
        let prev_tangent = tangents[i - 1];
        let curr_tangent = tangents[i];

        // Compute the rotation axis (perpendicular to both tangents)
        let rotation_axis = prev_tangent.cross(curr_tangent);

        if rotation_axis.length_squared() < 1e-6 {
            // Tangents are parallel, no rotation needed
            normals.push(normals[i - 1]);
        } else {
            // Rotate the normal to follow the tangent change
            let rotation_axis = rotation_axis.normalize();
            let rotation_angle = prev_tangent.dot(curr_tangent).clamp(-1.0, 1.0).acos();

            // Rodrigues' rotation formula
            let prev_normal = normals[i - 1];
            let rotated_normal = prev_normal * rotation_angle.cos()
                + rotation_axis.cross(prev_normal) * rotation_angle.sin()
                + rotation_axis * rotation_axis.dot(prev_normal) * (1.0 - rotation_angle.cos());

            // Ensure perpendicularity
            let final_normal = (rotated_normal - curr_tangent * rotated_normal.dot(curr_tangent))
                .normalize();

            normals.push(final_normal);
        }
    }

    normals
}

/// Compute binormals (perpendicular to both tangent and normal)
pub fn compute_binormals(tangents: &[Vec3], normals: &[Vec3]) -> Vec<Vec3> {
    tangents
        .iter()
        .zip(normals.iter())
        .map(|(t, n)| t.cross(*n).normalize())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catmull_rom_basic() {
        let points = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
        ];

        let spline = CatmullRomSpline::new(points);

        // At t=0, should be close to control point 1
        let p0 = spline.evaluate(0, 0.0);
        assert!((p0 - Vec3::new(1.0, 0.0, 0.0)).length() < 0.01);

        // At t=1, should be close to control point 2
        let p1 = spline.evaluate(0, 1.0);
        assert!((p1 - Vec3::new(2.0, 1.0, 0.0)).length() < 0.01);
    }

    #[test]
    fn test_subdivide() {
        let points = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        ];

        let spline = CatmullRomSpline::new(points);
        let subdivided = spline.subdivide(4);

        // Should have 4 subdivisions per segment + 1 final point
        // 2 segments × 4 = 8, + 1 = 9 points
        assert_eq!(subdivided.len(), 9);
    }

    #[test]
    fn test_parallel_transport() {
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        ];

        let tangents = vec![Vec3::X, Vec3::X, Vec3::X];

        let normals = compute_parallel_transport_normals(&positions, &tangents, Vec3::Y);

        // All normals should be Y since tangent is constant
        assert_eq!(normals.len(), 3);
        for normal in normals {
            assert!((normal - Vec3::Y).length() < 0.01);
        }
    }
}
