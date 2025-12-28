//! VR Performance Monitoring
//!
//! This module provides frame timing monitoring specifically for VR,
//! where maintaining 90 FPS is critical to prevent motion sickness.

use std::collections::VecDeque;
use std::time::Instant;

/// Target frame time for 90 FPS (11.11 ms)
pub const TARGET_FRAME_TIME_MS: f32 = 11.11;

/// Target frame time for 72 FPS (13.89 ms) - Quest 1 fallback
pub const TARGET_FRAME_TIME_72FPS_MS: f32 = 13.89;

/// Performance statistics for VR rendering
#[derive(Debug, Clone)]
pub struct VrPerformanceStats {
    /// Current frame time in milliseconds
    pub frame_time_ms: f32,

    /// Average frame time over last N frames
    pub avg_frame_time_ms: f32,

    /// Maximum frame time in last N frames
    pub max_frame_time_ms: f32,

    /// Current FPS
    pub fps: f32,

    /// Number of frames that exceeded target time
    pub dropped_frames: usize,

    /// Total frames measured
    pub total_frames: usize,

    /// Percentage of frames meeting 90 FPS target
    pub performance_rating: f32,
}

impl Default for VrPerformanceStats {
    fn default() -> Self {
        Self {
            frame_time_ms: 0.0,
            avg_frame_time_ms: 0.0,
            max_frame_time_ms: 0.0,
            fps: 90.0,
            dropped_frames: 0,
            total_frames: 0,
            performance_rating: 100.0,
        }
    }
}

/// VR performance monitor with rolling average
pub struct VrPerformanceMonitor {
    /// Last frame timestamp
    last_frame: Option<Instant>,

    /// Rolling window of frame times (last 90 frames = 1 second at 90 FPS)
    frame_times: VecDeque<f32>,

    /// Maximum window size
    window_size: usize,

    /// Frame time target (default 90 FPS)
    target_frame_time_ms: f32,

    /// Statistics
    stats: VrPerformanceStats,
}

impl VrPerformanceMonitor {
    /// Create a new performance monitor
    pub fn new() -> Self {
        Self::with_target_fps(90.0)
    }

    /// Create a performance monitor with specific target FPS
    pub fn with_target_fps(target_fps: f32) -> Self {
        let target_frame_time_ms = 1000.0 / target_fps;
        let window_size = target_fps as usize; // 1 second of frames

        Self {
            last_frame: None,
            frame_times: VecDeque::with_capacity(window_size),
            window_size,
            target_frame_time_ms,
            stats: VrPerformanceStats::default(),
        }
    }

    /// Record a frame and update statistics
    pub fn tick(&mut self) {
        let now = Instant::now();

        if let Some(last) = self.last_frame {
            let frame_time = now.duration_since(last);
            let frame_time_ms = frame_time.as_secs_f32() * 1000.0;

            // Add to rolling window
            self.frame_times.push_back(frame_time_ms);
            if self.frame_times.len() > self.window_size {
                self.frame_times.pop_front();
            }

            // Update statistics
            self.stats.frame_time_ms = frame_time_ms;
            self.stats.total_frames += 1;

            // Check if frame exceeded target
            if frame_time_ms > self.target_frame_time_ms {
                self.stats.dropped_frames += 1;
            }

            // Compute rolling average
            if !self.frame_times.is_empty() {
                let sum: f32 = self.frame_times.iter().sum();
                self.stats.avg_frame_time_ms = sum / self.frame_times.len() as f32;

                // Compute max
                self.stats.max_frame_time_ms = self.frame_times.iter()
                    .copied()
                    .fold(0.0f32, f32::max);
            }

            // Compute FPS
            if self.stats.avg_frame_time_ms > 0.0 {
                self.stats.fps = 1000.0 / self.stats.avg_frame_time_ms;
            }

            // Compute performance rating
            if self.stats.total_frames > 0 {
                let good_frames = self.stats.total_frames - self.stats.dropped_frames;
                self.stats.performance_rating = (good_frames as f32 / self.stats.total_frames as f32) * 100.0;
            }
        }

        self.last_frame = Some(now);
    }

    /// Get current statistics
    pub fn stats(&self) -> &VrPerformanceStats {
        &self.stats
    }

    /// Check if performance is acceptable (>95% of frames meet target)
    pub fn is_performance_acceptable(&self) -> bool {
        self.stats.performance_rating > 95.0
    }

    /// Check if current frame exceeded target
    pub fn is_current_frame_slow(&self) -> bool {
        self.stats.frame_time_ms > self.target_frame_time_ms
    }

    /// Get warning message if performance is poor
    pub fn get_warning(&self) -> Option<String> {
        if self.stats.total_frames < 90 {
            // Not enough data yet
            return None;
        }

        if self.stats.performance_rating < 80.0 {
            Some(format!(
                "VR Performance Warning: Only {:.1}% of frames meeting 90 FPS target (avg: {:.2}ms, max: {:.2}ms)",
                self.stats.performance_rating,
                self.stats.avg_frame_time_ms,
                self.stats.max_frame_time_ms
            ))
        } else if self.stats.performance_rating < 95.0 {
            Some(format!(
                "VR Performance Notice: {:.1}% of frames meeting target (avg: {:.2}ms)",
                self.stats.performance_rating,
                self.stats.avg_frame_time_ms
            ))
        } else {
            None
        }
    }

    /// Reset statistics
    pub fn reset(&mut self) {
        self.frame_times.clear();
        self.stats = VrPerformanceStats::default();
        self.last_frame = None;
    }
}

impl Default for VrPerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_performance_monitor_basic() {
        let mut monitor = VrPerformanceMonitor::new();

        // Simulate a few frames
        for _ in 0..10 {
            monitor.tick();
            sleep(Duration::from_millis(10)); // ~100 FPS
        }

        let stats = monitor.stats();
        assert!(stats.fps > 80.0 && stats.fps < 120.0); // Should be around 100 FPS
        assert_eq!(stats.total_frames, 10);
    }

    #[test]
    fn test_dropped_frames() {
        let mut monitor = VrPerformanceMonitor::new();

        // First frame: good (10ms)
        monitor.tick();
        sleep(Duration::from_millis(10));

        // Second frame: bad (20ms > 11.11ms target)
        monitor.tick();
        sleep(Duration::from_millis(20));

        monitor.tick();

        let stats = monitor.stats();
        assert_eq!(stats.dropped_frames, 1); // One frame exceeded target
    }
}
