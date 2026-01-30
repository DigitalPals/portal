//! VNC viewer view
//!
//! Renders the VNC framebuffer with a simple toolbar.

use iced::widget::{column, container, row, text};
use iced::{Element, Fill};

use crate::app::managers::session_manager::VncActiveSession;
use crate::message::{Message, SessionId};
use crate::theme::{ScaledFonts, Theme};
use crate::vnc::widget::vnc_framebuffer;

/// Build the VNC viewer view with toolbar and framebuffer display
pub fn vnc_viewer_view<'a>(
    _session_id: SessionId,
    vnc: &'a VncActiveSession,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let fb = vnc.session.framebuffer.lock();
    let resolution_text = format!("{}x{}", fb.width, fb.height);
    drop(fb);

    // Status bar
    let status_bar = container(
        row![
            text("VNC").size(fonts.label).color(theme.text_secondary),
            text(" | ").size(fonts.label).color(theme.text_muted),
            text(&vnc.host_name)
                .size(fonts.label)
                .color(theme.text_primary),
            text(" | ").size(fonts.label).color(theme.text_muted),
            text(resolution_text)
                .size(fonts.label)
                .color(theme.text_secondary),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .padding([4, 12])
    .width(Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    });

    // Framebuffer — custom shader widget for flicker-free rendering
    let fb_content: Element<'a, Message> = vnc_framebuffer(&vnc.session.framebuffer);

    let framebuffer = container(fb_content)
        .width(Fill)
        .height(Fill)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(move |_| iced::widget::container::Style {
            background: Some(iced::Color::BLACK.into()),
            ..Default::default()
        });

    column![status_bar, framebuffer]
        .width(Fill)
        .height(Fill)
        .into()
}
