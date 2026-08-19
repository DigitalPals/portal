use portal::app::Portal;
use portal::fonts;

use iced::Size;

#[cfg(target_os = "linux")]
const PORTAL_APPLICATION_ID: &str = "portal";

fn portal_window_settings() -> iced::window::Settings {
    let settings = iced::window::Settings {
        size: Size::new(1200.0, 800.0),
        ..iced::window::Settings::default()
    };

    #[cfg(target_os = "linux")]
    {
        let mut settings = settings;
        settings.platform_specific.application_id = PORTAL_APPLICATION_ID.to_owned();
        settings
    }

    #[cfg(not(target_os = "linux"))]
    settings
}

fn main() -> iced::Result {
    // Initialize logging with file output.
    let log_dir = portal::config::paths::ensure_log_dir().ok();
    let _guard = portal::logging::init_logging(log_dir);

    tracing::info!("Starting Portal SSH Client");
    if let Some(dir) = portal::config::paths::log_dir() {
        tracing::info!("Logging to {}", dir.display());
    }

    iced::application(Portal::new, Portal::update, Portal::view)
        .title("Portal")
        .theme(Portal::theme)
        .subscription(Portal::subscription)
        .window(portal_window_settings())
        .default_font(fonts::INTER)
        .font(fonts::INTER_BYTES)
        .font(fonts::JETBRAINS_MONO_NERD_BYTES)
        .font(fonts::HACK_NERD_BYTES)
        .run()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn window_application_id_matches_desktop_file() {
        assert_eq!(
            portal_window_settings().platform_specific.application_id,
            PORTAL_APPLICATION_ID
        );
    }
}
