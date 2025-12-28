/// GPU Compute Benchmarking Module
///
/// Compares performance of CPU vs GPU culling + LOD assignment

use std::time::Instant;

#[derive(Debug, Clone, Copy, Default)]
pub struct BenchmarkStats {
    pub gpu_compute_time_us: f32,
    pub cpu_compute_time_us: f32,
    pub gpu_enabled: bool,
    pub atom_count: usize,
    pub frames_measured: u32,
}

impl BenchmarkStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn speedup_factor(&self) -> f32 {
        if self.gpu_compute_time_us > 0.0 {
            self.cpu_compute_time_us / self.gpu_compute_time_us
        } else {
            1.0
        }
    }

    pub fn is_gpu_faster(&self) -> bool {
        self.speedup_factor() > 1.0
    }
}

/// Simple timer for benchmarking
pub struct BenchmarkTimer {
    start: Instant,
}

impl BenchmarkTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_us(&self) -> f32 {
        self.start.elapsed().as_micros() as f32
    }

    pub fn elapsed_ms(&self) -> f32 {
        self.start.elapsed().as_secs_f32() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_timer() {
        let timer = BenchmarkTimer::start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.elapsed_ms();
        assert!(elapsed >= 10.0 && elapsed < 20.0);
    }

    #[test]
    fn test_speedup_calculation() {
        let stats = BenchmarkStats {
            gpu_compute_time_us: 100.0,
            cpu_compute_time_us: 400.0,
            ..Default::default()
        };
        assert_eq!(stats.speedup_factor(), 4.0);
        assert!(stats.is_gpu_faster());
    }
}
