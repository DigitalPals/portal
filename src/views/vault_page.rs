use iced::widget::{
    Column, Space, button, column, container, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::app::VaultUiState;
use crate::config::{AuthMethod, HostsConfig};
use crate::hub::vault::{HubVaultConfig, VaultKey};
use crate::icons::{self, icon_with_color};
use crate::message::{Message, VaultMessage};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, ScaledFonts, Theme};
use crate::views::components::{BadgeTone, status_badge};

pub struct VaultPageContext<'a> {
    pub vault: &'a HubVaultConfig,
    pub hosts: &'a HostsConfig,
    pub state: &'a VaultUiState,
    pub portal_hub_configured: bool,
    pub portal_hub_vault_enabled: bool,
    pub theme: Theme,
    pub fonts: ScaledFonts,
}

pub fn vault_page_view(ctx: VaultPageContext<'_>) -> Element<'static, Message> {
    let sync_label = if ctx.portal_hub_configured && ctx.portal_hub_vault_enabled {
        "Hub sync enabled"
    } else if ctx.portal_hub_configured {
        "Hub sync disabled"
    } else {
        "Local vault"
    };
    let sync_tone = if ctx.portal_hub_configured && ctx.portal_hub_vault_enabled {
        BadgeTone::Success
    } else {
        BadgeTone::Neutral
    };

    let header = row![
        column![
            row![
                text("Vault")
                    .size(ctx.fonts.page_title)
                    .color(ctx.theme.text_primary),
                status_badge(sync_label, sync_tone, ctx.theme, ctx.fonts),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            text("Encrypted SSH keys stored locally and synced as client-side encrypted blobs when Portal Hub vault sync is enabled.")
                .size(ctx.fonts.label)
                .color(ctx.theme.text_muted),
        ]
        .spacing(5),
        Space::new().width(Fill),
        button(
            row![
                icon_with_color(icons::ui::PLUS, 14, ctx.theme.text_primary),
                text("Add Key")
                    .size(ctx.fonts.label)
                    .color(ctx.theme.text_primary),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .on_press(Message::Vault(VaultMessage::AddKeyOpen)),
    ]
    .align_y(Alignment::Center);

    let search = text_input("Search vault keys...", &ctx.state.search_query)
        .on_input(|value| Message::Vault(VaultMessage::SearchChanged(value)))
        .padding([10, 14])
        .width(Fill);

    let mut content = Column::new()
        .spacing(16)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0))
        .push(header)
        .push(search);

    if let Some(error) = &ctx.state.operation_error {
        content = content.push(error_banner(error.clone(), ctx.theme, ctx.fonts));
    }

    let keys = filtered_keys(ctx.vault, &ctx.state.search_query);
    if keys.is_empty() {
        content = content.push(empty_state(ctx.vault.keys.is_empty(), ctx.theme, ctx.fonts));
    } else {
        for key in keys {
            content = content.push(key_card(key, ctx.hosts, ctx.theme, ctx.fonts));
        }
    }

    scrollable(container(content).width(Fill))
        .height(Fill)
        .into()
}

pub fn vault_add_key_dialog_view<'a>(
    state: &'a VaultUiState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let import_label = state
        .imported_path
        .as_ref()
        .map(|path| format!("Loaded {}", path.display()))
        .unwrap_or_else(|| "Paste a private key or import one from disk.".to_string());

    let mut fields: Vec<Element<'a, Message>> = vec![
        text("Add Key")
            .size(fonts.dialog_title)
            .color(theme.text_primary)
            .into(),
        text(import_label)
            .size(fonts.label)
            .color(theme.text_muted)
            .into(),
        text_input("Name", &state.key_name)
            .on_input(|value| Message::Vault(VaultMessage::AddKeyNameChanged(value)))
            .padding([8, 10])
            .width(Fill)
            .into(),
        labeled_editor(
            "Private key",
            &state.private_key,
            |action| Message::Vault(VaultMessage::AddKeyPrivateKeyChanged(action)),
            theme,
            fonts,
            150.0,
        ),
        row![
            small_button("Import file", theme, fonts)
                .on_press(Message::Vault(VaultMessage::AddKeyImportFileRequested)),
            small_button("Use default private key", theme, fonts)
                .on_press(Message::Vault(VaultMessage::AddKeyImportDefaultRequested)),
        ]
        .spacing(8)
        .into(),
    ];

    if let Some(error) = &state.add_key_error {
        fields.push(
            text(error.clone())
                .size(fonts.label)
                .color(iced::Color::from_rgb8(220, 80, 80))
                .into(),
        );
    }

    fields.push(
        row![
            Space::new().width(Fill),
            dialog_button("Cancel", theme, fonts, false)
                .on_press(Message::Vault(VaultMessage::AddKeyCancel)),
            dialog_button("Save", theme, fonts, true)
                .on_press(Message::Vault(VaultMessage::AddKeySubmit)),
        ]
        .spacing(8)
        .into(),
    );

    vault_dialog_backdrop(
        scrollable(
            container(Column::with_children(fields).spacing(10))
                .padding(24)
                .width(Length::Fixed(560.0)),
        )
        .height(Length::Fixed(600.0)),
        theme,
    )
}

pub fn vault_edit_key_dialog_view<'a>(
    state: &'a VaultUiState,
    vault: &'a HubVaultConfig,
    hosts: &'a HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let key = state.edit_key_id.and_then(|id| vault.find_key(id));
    let Some(key) = key else {
        return vault_dialog_backdrop(
            container(text("Vault key not found").color(theme.text_primary)).padding(24),
            theme,
        );
    };

    let referenced = referenced_host_names(hosts, key.id);
    let usage = if referenced.is_empty() {
        "Not used by any host".to_string()
    } else {
        format!("Used by {}", referenced.join(", "))
    };
    let fingerprint = key
        .fingerprint
        .clone()
        .unwrap_or_else(|| "Fingerprint unavailable".to_string());
    let algorithm = key
        .algorithm
        .clone()
        .unwrap_or_else(|| "Unknown key".to_string());

    let mut fields: Vec<Element<'a, Message>> = vec![
        text("Edit Key")
            .size(fonts.dialog_title)
            .color(theme.text_primary)
            .into(),
        row![
            status_badge(algorithm, BadgeTone::Info, theme, fonts),
            text(fingerprint)
                .size(fonts.label)
                .color(theme.text_secondary),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        text(usage).size(fonts.label).color(theme.text_muted).into(),
        text_input("Name", &state.edit_name)
            .on_input(|value| Message::Vault(VaultMessage::EditNameChanged(value)))
            .on_submit(Message::Vault(VaultMessage::EditSave))
            .padding([8, 10])
            .width(Fill)
            .into(),
    ];

    if let Some(public_key) = &key.public_key {
        fields.push(
            container(
                text(truncate_middle(public_key, 120))
                    .size(fonts.small)
                    .color(theme.text_muted),
            )
            .padding(10)
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(theme.background.into()),
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            })
            .into(),
        );
    }

    if state.edit_delete_requested {
        let warning = if referenced.is_empty() {
            "Delete this key from the vault?".to_string()
        } else {
            format!(
                "Delete this key? These hosts currently reference it: {}",
                referenced.join(", ")
            )
        };
        fields.push(
            container(text(warning).size(fonts.label).color(theme.text_secondary))
                .padding(10)
                .width(Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Color::from_rgba8(220, 80, 80, 0.10).into()),
                    border: iced::Border {
                        color: iced::Color::from_rgb8(220, 80, 80),
                        width: 1.0,
                        radius: BORDER_RADIUS.into(),
                    },
                    ..Default::default()
                })
                .into(),
        );
    }

    let mut secondary_actions = row![
        small_button("Copy public key", theme, fonts)
            .on_press(Message::Vault(VaultMessage::CopyPublicKey(key.id))),
        small_button("Copy fingerprint", theme, fonts)
            .on_press(Message::Vault(VaultMessage::CopyFingerprint(key.id))),
    ]
    .spacing(8);

    if state.edit_delete_requested {
        secondary_actions = secondary_actions.push(
            small_button("Confirm delete", theme, fonts)
                .on_press(Message::Vault(VaultMessage::EditDeleteConfirm)),
        );
    } else {
        secondary_actions = secondary_actions.push(
            small_button("Delete", theme, fonts)
                .on_press(Message::Vault(VaultMessage::EditDeleteRequested)),
        );
    }
    fields.push(secondary_actions.into());

    fields.push(
        row![
            Space::new().width(Fill),
            dialog_button("Cancel", theme, fonts, false)
                .on_press(Message::Vault(VaultMessage::EditCancel)),
            dialog_button("Save", theme, fonts, true)
                .on_press(Message::Vault(VaultMessage::EditSave)),
        ]
        .spacing(8)
        .into(),
    );

    vault_dialog_backdrop(
        container(Column::with_children(fields).spacing(10))
            .padding(24)
            .width(Length::Fixed(520.0)),
        theme,
    )
}

fn filtered_keys<'a>(vault: &'a HubVaultConfig, query: &str) -> Vec<&'a VaultKey> {
    let query = query.trim().to_lowercase();
    vault
        .keys
        .iter()
        .filter(|key| {
            query.is_empty()
                || key.name.to_lowercase().contains(&query)
                || key
                    .fingerprint
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
                || key
                    .algorithm
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
        })
        .collect()
}

fn key_card(
    key: &VaultKey,
    hosts: &HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let key_id = key.id;
    let referenced = referenced_host_names(hosts, key_id);
    let algorithm = key
        .algorithm
        .clone()
        .unwrap_or_else(|| "Unknown key".to_string());
    let fingerprint = key
        .fingerprint
        .clone()
        .unwrap_or_else(|| "Fingerprint unavailable".to_string());
    let updated = key
        .updated_at
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    let host_text = if referenced.is_empty() {
        "Not used by any host".to_string()
    } else {
        format!("Used by {}", referenced.join(", "))
    };

    let card = column![
        row![
            row![
                icon_with_color(icons::ui::KEY, 18, theme.accent),
                text(key.name.clone())
                    .size(fonts.body)
                    .color(theme.text_primary),
                status_badge(algorithm, BadgeTone::Info, theme, fonts),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            Space::new().width(Fill),
            text(updated).size(fonts.label).color(theme.text_muted),
        ]
        .align_y(Alignment::Center),
        text(fingerprint)
            .size(fonts.label)
            .color(theme.text_secondary),
        text(host_text).size(fonts.label).color(theme.text_muted),
    ]
    .spacing(10);

    button(container(card).padding(16).width(Fill))
        .padding(0)
        .width(Fill)
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.surface.into()),
            };
            button::Style {
                background,
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: CARD_BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::Vault(VaultMessage::EditOpen(key_id)))
        .into()
}

fn labeled_editor<'a, F>(
    label: &'static str,
    content: &'a text_editor::Content,
    on_action: F,
    theme: Theme,
    fonts: ScaledFonts,
    height: f32,
) -> Element<'a, Message>
where
    F: Fn(text_editor::Action) -> Message + 'a,
{
    column![
        text(label).size(fonts.label).color(theme.text_secondary),
        text_editor(content)
            .on_action(on_action)
            .height(Length::Fixed(height))
            .padding(10)
            .style(move |_theme, _status| text_editor::Style {
                background: theme.background.into(),
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                placeholder: theme.text_muted,
                value: theme.text_primary,
                selection: theme.selected,
            }),
    ]
    .spacing(4)
    .into()
}

fn empty_state(empty_vault: bool, theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let message = if empty_vault {
        "No vault keys yet"
    } else {
        "No keys match your search"
    };
    container(text(message).size(fonts.body).color(theme.text_muted))
        .padding(24)
        .width(Fill)
        .into()
}

fn error_banner(message: String, theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    container(text(message).size(fonts.label).color(theme.text_secondary))
        .padding(12)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(iced::Color::from_rgba8(220, 80, 80, 0.10).into()),
            border: iced::Border {
                color: iced::Color::from_rgb8(220, 80, 80),
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn small_button<'a>(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(fonts.label).color(theme.text_primary))
        .padding([6, 10])
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.background.into()),
            };
            button::Style {
                background,
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
}

fn dialog_button<'a>(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
    primary: bool,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(fonts.label).color(theme.text_primary))
        .padding([8, 16])
        .style(move |_theme, status| {
            if primary {
                let bg = match status {
                    button::Status::Hovered => theme.focus_ring,
                    _ => theme.accent,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        color: theme.accent,
                        width: 1.0,
                        radius: BORDER_RADIUS.into(),
                    },
                    ..Default::default()
                }
            } else {
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
            }
        })
}

fn vault_dialog_backdrop<'a>(
    content: impl Into<Element<'a, Message>>,
    theme: Theme,
) -> Element<'a, Message> {
    let dialog_box = container(content).style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: (BORDER_RADIUS * 2.0).into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba8(0, 0, 0, 0.5),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..Default::default()
    });

    let backdrop = container(
        container(dialog_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.7).into()),
        ..Default::default()
    });

    iced::widget::mouse_area(backdrop)
        .on_press(Message::Noop)
        .on_release(Message::Noop)
        .into()
}

fn referenced_host_names(hosts: &HostsConfig, key_id: Uuid) -> Vec<String> {
    hosts
        .hosts
        .iter()
        .filter(|host| match &host.auth {
            AuthMethod::PublicKey { vault_key_id, .. } => *vault_key_id == Some(key_id),
            _ => false,
        })
        .map(|host| host.name.clone())
        .collect()
}

fn truncate_middle(value: &str, max_len: usize) -> String {
    if value.len() <= max_len || max_len < 8 {
        return value.to_string();
    }
    let keep = (max_len - 3) / 2;
    format!("{}...{}", &value[..keep], &value[value.len() - keep..])
}
