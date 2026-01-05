mod app;
mod config;
mod error;
mod icons;
mod message;
mod sftp;
mod ssh;
mod terminal;
mod theme;
mod views;

use app::Portal;
use iced::Size;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Starting Portal SSH Client");

    iced::application("Portal", Portal::update, Portal::view)
        .theme(Portal::theme)
        .subscription(Portal::subscription)
        .window_size(Size::new(1200.0, 800.0))
        .run_with(Portal::new)
}
