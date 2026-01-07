use portal::app::Portal;
use portal::fonts;

use iced::Size;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting Portal SSH Client");

    iced::application(Portal::new, Portal::update, Portal::view)
        .title("Portal")
        .theme(Portal::theme)
        .subscription(Portal::subscription)
        .window_size(Size::new(1200.0, 800.0))
        .font(fonts::JETBRAINS_MONO_NERD_BYTES)
        .font(fonts::HACK_NERD_BYTES)
        .run()
}
