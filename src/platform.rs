//! Platform-specific utilities
//!
//! This module provides platform-specific functionality such as detecting
//! system UI scaling preferences.

/// Detect the system UI scale factor.
///
/// Returns a scale factor (typically 1.0-2.0) based on system preferences:
/// - Linux (GNOME): Reads `text-scaling-factor` from GSettings
/// - Linux (fallback): Checks `GDK_DPI_SCALE` or `QT_SCALE_FACTOR` env vars
/// - macOS: Returns 1.0 (macOS handles scaling at OS level)
///
/// Returns 1.0 if detection fails or on unsupported platforms.
pub fn detect_system_ui_scale() -> f32 {
    #[cfg(target_os = "linux")]
    {
        detect_linux_scale()
    }

    #[cfg(target_os = "macos")]
    {
        // macOS handles DPI scaling at the OS level, no text-scaling-factor equivalent
        1.0
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        1.0
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_scale() -> f32 {
    // Try GNOME GSettings first
    if let Some(scale) = detect_gnome_scale() {
        tracing::info!("Detected GNOME text-scaling-factor: {}", scale);
        return scale;
    }

    // Fallback to environment variables
    if let Some(scale) = detect_env_scale() {
        tracing::info!("Detected scale from environment variable: {}", scale);
        return scale;
    }

    tracing::debug!("No system UI scale detected, using default 1.0");
    1.0
}

#[cfg(target_os = "linux")]
fn detect_gnome_scale() -> Option<f32> {
    use gio::prelude::*;

    // Try to get the GNOME desktop interface settings
    let settings = gio::Settings::new("org.gnome.desktop.interface");

    // Read text-scaling-factor (typically 1.0-2.0)
    let scale: f64 = settings.get("text-scaling-factor");

    // Validate the scale is reasonable (0.5 to 3.0)
    if (0.5..=3.0).contains(&scale) {
        Some(scale as f32)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_env_scale() -> Option<f32> {
    // Check GDK_DPI_SCALE (GTK apps)
    if let Ok(val) = std::env::var("GDK_DPI_SCALE") {
        if let Ok(scale) = val.parse::<f32>() {
            if (0.5..=3.0).contains(&scale) {
                return Some(scale);
            }
        }
    }

    // Check QT_SCALE_FACTOR (Qt/KDE apps)
    if let Ok(val) = std::env::var("QT_SCALE_FACTOR") {
        if let Ok(scale) = val.parse::<f32>() {
            if (0.5..=3.0).contains(&scale) {
                return Some(scale);
            }
        }
    }

    None
}
