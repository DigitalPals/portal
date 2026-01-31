//! Adaptive quality tracking for VNC connections
//!
//! Monitors frame timing to estimate connection quality and adjust refresh rate.

use std::collections::VecDeque;
use std::time::Instant;

use crate::message::QualityLevel;

/// Tracks connection quality based on frame delivery timing.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ConnectionQuality {
    /// Rolling window of frame delivery times (duration between refresh request and frame receipt)
    frame_times: VecDeque<f64>,
    /// Rolling window of frame byte sizes for throughput estimation
    frame_bytes: VecDeque<usize>,
    /// Timestamps corresponding to frame_bytes entries
    frame_timestamps: VecDeque<Instant>,
    /// Maximum entries in rolling windows
    window_size: usize,
    /// Current quality level
    pub level: QualityLevel,
    /// Last time quality was recalculated
    last_check: Instant,
}

impl Default for ConnectionQuality {
    fn default() -> Self {
        Self::new(30)
    }
}

#[allow(dead_code)]
impl ConnectionQuality {
    /// Create a new quality tracker with the given rolling window size.
    pub fn new(window_size: usize) -> Self {
        Self {
            frame_times: VecDeque::with_capacity(window_size),
            frame_bytes: VecDeque::with_capacity(window_size),
            frame_timestamps: VecDeque::with_capacity(window_size),
            window_size,
            level: QualityLevel::High,
            last_check: Instant::now(),
        }
    }

    /// Record a frame delivery time in milliseconds.
    pub fn record_frame_time(&mut self, ms: f64) {
        if self.frame_times.len() >= self.window_size {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(ms);
    }

    /// Record frame data received (bytes).
    pub fn record_frame_bytes(&mut self, bytes: usize) {
        let now = Instant::now();
        if self.frame_bytes.len() >= self.window_size {
            self.frame_bytes.pop_front();
            self.frame_timestamps.pop_front();
        }
        self.frame_bytes.push_back(bytes);
        self.frame_timestamps.push_back(now);
    }

    /// Average frame delivery time in ms.
    pub fn avg_frame_time_ms(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64
    }

    /// Recalculate quality level. Returns Some if quality changed.
    pub fn recalculate(&mut self) -> Option<QualityLevel> {
        let now = Instant::now();
        if now.duration_since(self.last_check).as_secs_f32() < 2.0 {
            return None;
        }
        self.last_check = now;

        if self.frame_times.len() < 5 {
            return None;
        }

        let avg = self.avg_frame_time_ms();
        let new_level = if avg < 50.0 {
            QualityLevel::High
        } else if avg < 150.0 {
            QualityLevel::Medium
        } else {
            QualityLevel::Low
        };

        if new_level != self.level {
            self.level = new_level;
            Some(new_level)
        } else {
            None
        }
    }

    /// Get the recommended refresh FPS based on current quality.
    pub fn recommended_fps(&self, base_fps: u32) -> u32 {
        match self.level {
            QualityLevel::High => base_fps,
            QualityLevel::Medium => (base_fps * 2 / 3).max(1),
            QualityLevel::Low => (base_fps / 2).max(1),
        }
    }
}
