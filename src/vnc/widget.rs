//! VNC framebuffer widget for Iced
//!
//! Renders the VNC framebuffer as an image and captures mouse/keyboard input.

use iced::widget::{image, Image};
use iced::{Element, Length};

use crate::message::Message;
use crate::vnc::VncSession;

/// Create an Iced image element from the VNC session's framebuffer.
///
/// The framebuffer is read from the shared mutex and converted to an
/// iced image handle. Mouse and keyboard input is handled at the view level.
pub fn vnc_framebuffer_image<'a>(
    session: &VncSession,
) -> Element<'a, Message> {
    let fb = session.framebuffer.lock();
    let width = fb.width;
    let height = fb.height;

    if width == 0 || height == 0 {
        return iced::widget::text("Waiting for framebuffer...").into();
    }

    // Convert BGRA to RGBA for iced's image widget
    let mut rgba = fb.pixels.clone();
    for chunk in rgba.chunks_exact_mut(4) {
        chunk.swap(0, 2); // swap B and R
    }

    let handle = image::Handle::from_rgba(width, height, rgba);

    Image::new(handle)
        .width(Length::Fill)
        .height(Length::Fill)
        .content_fit(iced::ContentFit::Contain)
        .into()
}
