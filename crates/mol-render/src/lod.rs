use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodLevel {
    High = 0,      // Close: icosphere subdivision 3 (512 tris)
    Medium = 1,    // Medium: icosphere subdivision 2 (128 tris)
    Low = 2,       // Far: icosphere subdivision 1 (32 tris)
    VeryLow = 3,   // Very far: octahedron (8 tris)
    Impostor = 4,  // Extremely far: billboard (2 tris)
}

impl LodLevel {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => LodLevel::High,
            1 => LodLevel::Medium,
            2 => LodLevel::Low,
            3 => LodLevel::VeryLow,
            4 => LodLevel::Impostor,
            _ => LodLevel::Impostor,
        }
    }

    pub fn subdivision_level(&self) -> u32 {
        match self {
            LodLevel::High => 3,
            LodLevel::Medium => 2,
            LodLevel::Low => 1,
            LodLevel::VeryLow => 0,
            LodLevel::Impostor => 0, // Not used for spheres
        }
    }
}

pub struct LodConfig {
    pub distance_high: f32,      // 0-50 Angstroms
    pub distance_medium: f32,    // 50-150 Angstroms
    pub distance_low: f32,       // 150-500 Angstroms
    pub distance_very_low: f32,  // 500-1000 Angstroms
    pub distance_impostor: f32,  // >1000 Angstroms: use billboards
    pub hysteresis: f32,         // Overlap factor (0.1 = 10% overlap bands)
}

impl Default for LodConfig {
    fn default() -> Self {
        Self {
            distance_high: 50.0,
            distance_medium: 150.0,
            distance_low: 500.0,
            distance_very_low: 1000.0,
            distance_impostor: f32::INFINITY,
            hysteresis: 0.1, // 10% overlap to prevent popping
        }
    }
}

pub struct LodSystem {
    config: LodConfig,
    pub stats: LodStats,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LodStats {
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub very_low_count: usize,
    pub impostor_count: usize,
    pub total_count: usize,
}

impl LodSystem {
    pub fn new(config: LodConfig) -> Self {
        Self {
            config,
            stats: LodStats::default(),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(LodConfig::default())
    }

    pub fn compute_lod(&self, distance: f32) -> LodLevel {
        if distance < self.config.distance_high {
            LodLevel::High
        } else if distance < self.config.distance_medium {
            LodLevel::Medium
        } else if distance < self.config.distance_low {
            LodLevel::Low
        } else if distance < self.config.distance_very_low {
            LodLevel::VeryLow
        } else {
            LodLevel::Impostor
        }
    }

    /// Compute LOD with hysteresis to prevent visual popping
    pub fn compute_lod_with_previous(&self, distance: f32, previous: Option<LodLevel>) -> LodLevel {
        if let Some(prev) = previous {
            let h = self.config.hysteresis;

            // Apply hysteresis bands to prevent thrashing
            match prev {
                LodLevel::High if distance < self.config.distance_high * (1.0 + h) => {
                    return LodLevel::High;
                }
                LodLevel::Medium if distance >= self.config.distance_high * (1.0 - h)
                                 && distance < self.config.distance_medium * (1.0 + h) => {
                    return LodLevel::Medium;
                }
                LodLevel::Low if distance >= self.config.distance_medium * (1.0 - h)
                              && distance < self.config.distance_low * (1.0 + h) => {
                    return LodLevel::Low;
                }
                LodLevel::VeryLow if distance >= self.config.distance_low * (1.0 - h)
                                  && distance < self.config.distance_very_low * (1.0 + h) => {
                    return LodLevel::VeryLow;
                }
                LodLevel::Impostor if distance >= self.config.distance_very_low * (1.0 - h) => {
                    return LodLevel::Impostor;
                }
                _ => {}
            }
        }

        // Standard LOD selection if no previous or outside hysteresis bands
        self.compute_lod(distance)
    }

    pub fn compute_lod_for_position(&self, position: Vec3, camera_pos: Vec3) -> LodLevel {
        let distance = position.distance(camera_pos);
        self.compute_lod(distance)
    }

    pub fn assign_lods<T, F>(
        &mut self,
        items: &[T],
        camera_pos: Vec3,
        get_position: F,
    ) -> Vec<(usize, LodLevel)>
    where
        F: Fn(&T) -> Vec3,
    {
        // Reset stats
        self.stats = LodStats::default();
        self.stats.total_count = items.len();

        let mut result = Vec::with_capacity(items.len());

        for (i, item) in items.iter().enumerate() {
            let position = get_position(item);
            let lod = self.compute_lod_for_position(position, camera_pos);

            match lod {
                LodLevel::High => self.stats.high_count += 1,
                LodLevel::Medium => self.stats.medium_count += 1,
                LodLevel::Low => self.stats.low_count += 1,
                LodLevel::VeryLow => self.stats.very_low_count += 1,
                LodLevel::Impostor => self.stats.impostor_count += 1,
            }

            result.push((i, lod));
        }

        result
    }

    pub fn update_config(&mut self, config: LodConfig) {
        self.config = config;
    }

    pub fn get_config(&self) -> &LodConfig {
        &self.config
    }

    pub fn get_stats(&self) -> &LodStats {
        &self.stats
    }
}

/// LOD groups for efficient rendering
#[derive(Debug, Clone)]
pub struct LodGroups {
    pub high: Vec<usize>,
    pub medium: Vec<usize>,
    pub low: Vec<usize>,
    pub very_low: Vec<usize>,
    pub impostors: Vec<usize>,
}

impl LodGroups {
    pub fn new() -> Self {
        Self {
            high: Vec::new(),
            medium: Vec::new(),
            low: Vec::new(),
            very_low: Vec::new(),
            impostors: Vec::new(),
        }
    }

    pub fn from_assignments(assignments: &[(usize, LodLevel)]) -> Self {
        let mut groups = Self::new();

        for &(index, lod) in assignments {
            match lod {
                LodLevel::High => groups.high.push(index),
                LodLevel::Medium => groups.medium.push(index),
                LodLevel::Low => groups.low.push(index),
                LodLevel::VeryLow => groups.very_low.push(index),
                LodLevel::Impostor => groups.impostors.push(index),
            }
        }

        groups
    }

    pub fn total_count(&self) -> usize {
        self.high.len()
            + self.medium.len()
            + self.low.len()
            + self.very_low.len()
            + self.impostors.len()
    }
}

impl Default for LodGroups {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lod_assignment() {
        let lod = LodSystem::with_default_config();
        let camera_pos = Vec3::ZERO;

        assert_eq!(lod.compute_lod_for_position(Vec3::new(0.0, 0.0, 10.0), camera_pos), LodLevel::High);
        assert_eq!(lod.compute_lod_for_position(Vec3::new(0.0, 0.0, 100.0), camera_pos), LodLevel::Medium);
        assert_eq!(lod.compute_lod_for_position(Vec3::new(0.0, 0.0, 300.0), camera_pos), LodLevel::Low);
        assert_eq!(lod.compute_lod_for_position(Vec3::new(0.0, 0.0, 750.0), camera_pos), LodLevel::VeryLow);
        assert_eq!(lod.compute_lod_for_position(Vec3::new(0.0, 0.0, 2000.0), camera_pos), LodLevel::Impostor);
    }

    #[test]
    fn test_lod_groups() {
        let assignments = vec![
            (0, LodLevel::High),
            (1, LodLevel::Medium),
            (2, LodLevel::High),
            (3, LodLevel::Impostor),
        ];

        let groups = LodGroups::from_assignments(&assignments);

        assert_eq!(groups.high.len(), 2);
        assert_eq!(groups.medium.len(), 1);
        assert_eq!(groups.impostors.len(), 1);
        assert_eq!(groups.total_count(), 4);
    }
}
