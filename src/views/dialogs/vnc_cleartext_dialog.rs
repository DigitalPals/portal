//! Warning dialog shown before connecting VNC to a non-private target
//! without an SSH tunnel: VNC traffic (including keystrokes and the remote
//! screen) is transmitted unencrypted.

use iced::widget::{Space, button, checkbox, column, container, row, text};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, Message};
use crate::theme::{BORDER_RADIUS, ScaledFonts, Theme};

use super::common::{dialog_backdrop, primary_button_style, secondary_button_style};

/// State for the unencrypted-VNC warning dialog
#[derive(Debug, Clone)]
pub struct VncCleartextDialogState {
    /// The host being connected to
    pub host_id: Uuid,
    /// Display name of the host
    pub host_name: String,
    /// The VNC target ("hostname:port")
    pub target: String,
    /// "Don't warn again for this host" checkbox state
    pub dont_warn_again: bool,
}

impl VncCleartextDialogState {
    pub fn new(host_id: Uuid, host_name: String, target: String) -> Self {
        Self {
            host_id,
            host_name,
            target,
            dont_warn_again: false,
        }
    }
}

/// Build the unencrypted-VNC warning dialog view
pub fn vnc_cleartext_dialog_view(
    state: &VncCleartextDialogState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let warning_color = iced::Color::from_rgb8(230, 160, 30);
    let warning_icon = icon_with_color(icons::ui::ALERT_TRIANGLE, 28, warning_color);

    let title = text("Unencrypted VNC connection")
        .size(fonts.heading)
        .color(theme.text_primary);

    let host_info = text(format!("{} ({})", state.host_name, state.target))
        .size(fonts.body)
        .color(theme.text_secondary);

    let body = text(
        "This VNC target is not on a private network, and VNC traffic is not \
         encrypted: everything you type (including passwords) and everything \
         shown on the remote screen can be read by anyone on the network path.",
    )
    .size(fonts.body)
    .color(theme.text_secondary);

    let recommendation = container(
        text(
            "Recommendation: set an SSH tunnel for this host (Edit Host \u{2192} \
             VNC \u{2192} SSH tunnel) so the connection is carried over an \
             encrypted SSH channel.",
        )
        .size(fonts.small)
        .color(theme.text_primary),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(theme.background.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: BORDER_RADIUS.into(),
        },
        ..Default::default()
    });

    let dont_warn_checkbox = checkbox(state.dont_warn_again)
        .label("Don't warn again for this host")
        .on_toggle(|value| Message::Dialog(DialogMessage::VncCleartextDontWarnToggled(value)))
        .spacing(8);

    let cancel_button = button(
        text("Cancel")
            .size(fonts.button_small)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .style(secondary_button_style(theme))
    .on_press(Message::Dialog(DialogMessage::VncCleartextCancel));

    let connect_button = button(text("Connect Anyway").size(fonts.button_small))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::VncCleartextConnectAnyway));

    let button_row = row![
        Space::new().width(Length::Fill),
        cancel_button,
        connect_button,
    ]
    .spacing(8);

    let content = column![
        row![warning_icon, title]
            .spacing(12)
            .align_y(Alignment::Center),
        Space::new().height(8),
        host_info,
        Space::new().height(12),
        body,
        Space::new().height(12),
        recommendation,
        Space::new().height(12),
        dont_warn_checkbox,
        Space::new().height(24),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(480.0));

    dialog_backdrop(content, theme)
}
