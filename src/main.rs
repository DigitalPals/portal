use portal::app::Portal;
use portal::fonts;

use iced::Size;

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
        .window_size(Size::new(1200.0, 800.0))
        .default_font(fonts::INTER)
        .font(fonts::INTER_BYTES)
        .font(fonts::JETBRAINS_MONO_NERD_BYTES)
        .font(fonts::HACK_NERD_BYTES)
        .run()
}
