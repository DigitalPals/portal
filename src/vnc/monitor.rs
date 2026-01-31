//! Multi-monitor support for VNC
//!
//! Handles ExtendedDesktopSize pseudo-encoding for discovering remote monitors.

use crate::message::VncScreen;

/// Layout of all monitors on the remote desktop.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct MonitorLayout {
    pub screens: Vec<VncScreen>,
}

#[allow(dead_code)]
impl MonitorLayout {
    /// Total bounding box of all screens.
    pub fn total_bounds(&self) -> (u16, u16, u16, u16) {
        if self.screens.is_empty() {
            return (0, 0, 0, 0);
        }
        let min_x = self.screens.iter().map(|s| s.x).min().unwrap_or(0);
        let min_y = self.screens.iter().map(|s| s.y).min().unwrap_or(0);
        let max_x = self
            .screens
            .iter()
            .map(|s| s.x + s.width)
            .max()
            .unwrap_or(0);
        let max_y = self
            .screens
            .iter()
            .map(|s| s.y + s.height)
            .max()
            .unwrap_or(0);
        (min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Get UV coordinates for a specific monitor relative to the full framebuffer.
    /// Returns (u_min, v_min, u_max, v_max).
    pub fn monitor_uv(&self, index: usize, fb_width: u32, fb_height: u32) -> Option<(f32, f32, f32, f32)> {
        let screen = self.screens.get(index)?;
        if fb_width == 0 || fb_height == 0 {
            return None;
        }
        let u_min = screen.x as f32 / fb_width as f32;
        let v_min = screen.y as f32 / fb_height as f32;
        let u_max = (screen.x + screen.width) as f32 / fb_width as f32;
        let v_max = (screen.y + screen.height) as f32 / fb_height as f32;
        Some((u_min, v_min, u_max, v_max))
    }
}
