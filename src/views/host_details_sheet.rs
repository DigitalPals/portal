use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::config::{AuthMethod, Host, Protocol};
use crate::icons::{self, icon_with_color};
use crate::message::{HostMessage, Message};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, ScaledFonts, Theme};
use crate::views::components::{BadgeTone, status_badge};

pub fn host_details_sheet_view(
    host: &Host,
    group_name: Option<&str>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let host_id = host.id;
    let protocol = match host.protocol {
        Protocol::Ssh => "SSH",
        Protocol::Vnc => "VNC",
    };
    let endpoint = match host.protocol {
        Protocol::Ssh => format!("{}:{}", host.hostname, host.port),
        Protocol::Vnc => format!("{}:{}", host.hostname, host.effective_vnc_port()),
    };
    let auth = match &host.auth {
        AuthMethod::Password => "Password",
        AuthMethod::Agent => "SSH Agent",
        AuthMethod::PublicKey { vault_key_id, .. } if vault_key_id.is_some() => "Vault Key",
        AuthMethod::PublicKey { .. } => "Public Key",
    };
    let os = host
        .detected_os
        .as_ref()
        .map(|os| os.display_name().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let last_connected = host
        .last_connected
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Never".to_string());

    let close_button = button(icon_with_color(icons::ui::X, 16, theme.text_secondary))
        .padding(8)
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => Some(theme.hover.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .on_press(Message::Host(HostMessage::DetailsClose));

    let mut meta = column![
        detail_row("Endpoint", endpoint, theme, fonts),
        detail_row("Username", host.effective_username(), theme, fonts),
        detail_row("Authentication", auth, theme, fonts),
        detail_row("Operating system", os, theme, fonts),
        detail_row("Last connected", last_connected, theme, fonts),
        detail_row(
            "Group",
            group_name.unwrap_or("Ungrouped").to_string(),
            theme,
            fonts
        ),
    ]
    .spacing(12);

    if !host.tags.is_empty() {
        let tag_row = row(host
            .tags
            .iter()
            .map(|tag| status_badge(tag.clone(), BadgeTone::Neutral, theme, fonts))
            .collect::<Vec<_>>())
        .spacing(6);
        meta = meta.push(labeled_content("Tags", tag_row.into(), theme, fonts));
    }

    if let Some(notes) = &host.notes {
        if !notes.trim().is_empty() {
            meta = meta.push(labeled_content(
                "Notes",
                text(notes.clone())
                    .size(fonts.body)
                    .color(theme.text_secondary)
                    .into(),
                theme,
                fonts,
            ));
        }
    }

    let header = row![
        column![
            row![
                text(host.name.clone())
                    .size(fonts.heading)
                    .color(theme.text_primary),
                status_badge(protocol, BadgeTone::Info, theme, fonts),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(host.hostname.clone())
                .size(fonts.body)
                .color(theme.text_secondary),
        ]
        .spacing(4)
        .width(Fill),
        close_button,
    ]
    .align_y(Alignment::Start);

    let accent_text = theme.text_on_accent();
    let actions = row![
        button(
            row![
                icon_with_color(icons::ui::ZAP, 14, accent_text),
                text("Connect").size(fonts.button_small).color(accent_text),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([9, 14])
        .style(move |_theme, _status| button::Style {
            background: Some(theme.accent.into()),
            text_color: theme.text_on_accent(),
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press(Message::Host(HostMessage::Connect(host_id))),
        button(
            row![
                icon_with_color(icons::ui::PENCIL, 14, theme.text_primary),
                text("Edit")
                    .size(fonts.button_small)
                    .color(theme.text_primary),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([9, 14])
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => theme.hover,
                _ => theme.surface,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::Host(HostMessage::Edit(host_id))),
    ]
    .spacing(8);

    let sheet = container(scrollable(column![header, actions, meta].spacing(22)))
        .width(Length::Fixed(420.0))
        .height(Fill)
        .padding(24)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: CARD_BORDER_RADIUS.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.45,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(-8.0, 0.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });

    row![
        button(Space::new().width(Fill).height(Fill))
            .padding(0)
            .width(Fill)
            .height(Fill)
            .style(|_, _| button::Style {
                background: None,
                ..Default::default()
            })
            .on_press(Message::Host(HostMessage::DetailsClose)),
        sheet,
    ]
    .width(Fill)
    .height(Fill)
    .padding(Padding::from([10, 10]))
    .into()
}

fn detail_row(
    label: &'static str,
    value: impl Into<String>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    labeled_content(
        label,
        text(value.into())
            .size(fonts.body)
            .color(theme.text_primary)
            .into(),
        theme,
        fonts,
    )
}

fn labeled_content(
    label: &'static str,
    content: Element<'static, Message>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    column![
        text(label).size(fonts.label).color(theme.text_muted),
        content,
    ]
    .spacing(5)
    .into()
}
