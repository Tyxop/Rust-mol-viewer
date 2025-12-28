use std::time::{Duration, Instant};

pub struct FpsCounter {
    frames: Vec<Instant>,
    last_update: Instant,
    current_fps: f32,
    frame_time_ms: f32,
    update_interval: Duration,
}

impl FpsCounter {
    pub fn new() -> Self {
        Self {
            frames: Vec::with_capacity(120),
            last_update: Instant::now(),
            current_fps: 0.0,
            frame_time_ms: 0.0,
            update_interval: Duration::from_millis(500), // Update every 500ms
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.frames.push(now);

        // Remove frames older than 1 second
        let one_second_ago = now - Duration::from_secs(1);
        self.frames.retain(|&t| t > one_second_ago);

        // Update FPS if enough time has passed
        if now.duration_since(self.last_update) >= self.update_interval {
            self.current_fps = self.frames.len() as f32;

            // Calculate average frame time
            if self.frames.len() > 1 {
                let total_time: Duration = self.frames
                    .windows(2)
                    .map(|w| w[1].duration_since(w[0]))
                    .sum();

                let avg_frame_time = total_time / (self.frames.len() as u32 - 1);
                self.frame_time_ms = avg_frame_time.as_secs_f32() * 1000.0;
            }

            self.last_update = now;
        }
    }

    pub fn fps(&self) -> f32 {
        self.current_fps
    }

    pub fn frame_time_ms(&self) -> f32 {
        self.frame_time_ms
    }

    pub fn reset(&mut self) {
        self.frames.clear();
        self.current_fps = 0.0;
        self.frame_time_ms = 0.0;
        self.last_update = Instant::now();
    }
}

impl Default for FpsCounter {
    fn default() -> Self {
        Self::new()
    }
}
