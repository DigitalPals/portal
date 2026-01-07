//! Snippet edit form view
//!
//! Full-page edit form for creating and editing snippets,
//! including host selection for multi-host execution.

use iced::widget::{
    Checkbox, Column, Space, button, column, container, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Fill, Length};
use uuid::Uuid;

use crate::app::SnippetEditState;
use crate::icons::{self, icon_with_color};
use crate::message::{Message, SnippetField, SnippetMessage};
use crate::theme::{BORDER_RADIUS, Theme};

/// Build the snippet edit form (full-page, replacing grid)
pub fn snippet_edit_view(
    state: &SnippetEditState,
    hosts: &[(Uuid, String)],
    theme: Theme,
) -> Element<'static, Message> {
    let title = if state.snippet_id.is_some() {
        "Edit Snippet"
    } else {
        "New Snippet"
    };

    let title_row = row![
        icon_with_color(icons::ui::CODE, 24, theme.accent),
        text(title).size(24).color(theme.text_primary),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    // Name input
    let name_label = text("Name").size(14).color(theme.text_secondary);
    let name_value = state.name.clone();
    let name_input = text_input("e.g., Update System", &name_value)
        .on_input(|s| Message::Snippet(SnippetMessage::FieldChanged(SnippetField::Name, s)))
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme, status| {
            use iced::widget::text_input::{Status, Style};
            let border_color = match status {
                Status::Focused { .. } => theme.accent,
                _ => theme.border,
            };
            Style {
                background: theme.surface.into(),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                icon: theme.text_muted,
                placeholder: theme.text_muted,
                value: theme.text_primary,
                selection: theme.selected,
            }
        });

    // Command input
    let command_label = text("Command").size(14).color(theme.text_secondary);
    let command_value = state.command.clone();
    let command_input = text_input("e.g., sudo apt update && sudo apt upgrade -y", &command_value)
        .on_input(|s| Message::Snippet(SnippetMessage::FieldChanged(SnippetField::Command, s)))
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme, status| {
            use iced::widget::text_input::{Status, Style};
            let border_color = match status {
                Status::Focused { .. } => theme.accent,
                _ => theme.border,
            };
            Style {
                background: theme.surface.into(),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                icon: theme.text_muted,
                placeholder: theme.text_muted,
                value: theme.text_primary,
                selection: theme.selected,
            }
        });

    // Description input (optional)
    let description_label = text("Description (optional)")
        .size(14)
        .color(theme.text_secondary);
    let description_value = state.description.clone();
    let description_input = text_input("Optional description of what this command does", &description_value)
        .on_input(|s| Message::Snippet(SnippetMessage::FieldChanged(SnippetField::Description, s)))
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme, status| {
            use iced::widget::text_input::{Status, Style};
            let border_color = match status {
                Status::Focused { .. } => theme.accent,
                _ => theme.border,
            };
            Style {
                background: theme.surface.into(),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                icon: theme.text_muted,
                placeholder: theme.text_muted,
                value: theme.text_primary,
                selection: theme.selected,
            }
        });

    // Host selection section
    let hosts_label = text("Target Hosts").size(14).color(theme.text_secondary);
    let hosts_help = text("Select which hosts to run this command on")
        .size(12)
        .color(theme.text_muted);

    let selected_hosts = state.selected_hosts.clone();
    let host_checkboxes: Vec<Element<'static, Message>> = hosts
        .iter()
        .map(|(host_id, host_name)| {
            let is_selected = selected_hosts.contains(host_id);
            let hid = *host_id;

            Checkbox::new(is_selected)
                .label(host_name.clone())
                .on_toggle(move |checked| Message::Snippet(SnippetMessage::ToggleHost(hid, checked)))
                .text_size(14)
                .size(18)
                .spacing(10)
                .style(move |_theme, status| {
                    use iced::widget::checkbox::{Status, Style};
                    let (bg, border_color, icon_color) = match status {
                        Status::Active { is_checked } | Status::Hovered { is_checked } => {
                            if is_checked {
                                (theme.accent, theme.accent, iced::Color::WHITE)
                            } else {
                                (iced::Color::TRANSPARENT, theme.border, theme.text_primary)
                            }
                        }
                        Status::Disabled { .. } => {
                            (iced::Color::TRANSPARENT, theme.text_muted, theme.text_muted)
                        }
                    };
                    Style {
                        background: bg.into(),
                        icon_color,
                        border: iced::Border {
                            color: border_color,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        text_color: Some(theme.text_primary),
                    }
                })
                .into()
        })
        .collect();

    let hosts_list: Element<'static, Message> = if host_checkboxes.is_empty() {
        container(
            column![
                text("No hosts configured")
                    .size(14)
                    .color(theme.text_muted),
                text("Add hosts from the Hosts page first")
                    .size(12)
                    .color(theme.text_muted),
            ]
            .spacing(4),
        )
        .padding(16)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
    } else {
        container(
            scrollable(Column::with_children(host_checkboxes).spacing(12).padding(4))
                .height(Length::Fixed(200.0)),
        )
        .padding(12)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
    };

    // Action buttons
    let is_valid = state.is_valid();

    let cancel_btn = button(text("Cancel").size(14).color(theme.text_primary))
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
        .padding([10, 20])
        .on_press(Message::Snippet(SnippetMessage::EditCancel));

    let save_btn = if is_valid {
        button(text("Save").size(14).color(iced::Color::WHITE))
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
                    _ => theme.accent,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        radius: BORDER_RADIUS.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .padding([10, 20])
            .on_press(Message::Snippet(SnippetMessage::Save))
    } else {
        button(text("Save").size(14).color(theme.text_muted))
            .style(move |_theme, _status| button::Style {
                background: Some(theme.surface.into()),
                text_color: theme.text_muted,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            })
            .padding([10, 20])
    };

    // Delete button (only for existing snippets)
    let delete_btn: Element<'static, Message> = if let Some(sid) = state.snippet_id {
        button(text("Delete").size(14).color(iced::Color::from_rgb8(0xd2, 0x0f, 0x39)))
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => iced::Color::from_rgba8(0xd2, 0x0f, 0x39, 0.13),
                    _ => iced::Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: iced::Color::from_rgb8(0xd2, 0x0f, 0x39),
                    border: iced::Border {
                        color: iced::Color::from_rgb8(0xd2, 0x0f, 0x39),
                        width: 1.0,
                        radius: BORDER_RADIUS.into(),
                    },
                    ..Default::default()
                }
            })
            .padding([10, 20])
            .on_press(Message::Snippet(SnippetMessage::Delete(sid)))
            .into()
    } else {
        Space::new().into()
    };

    let button_row = row![
        delete_btn,
        Space::new().width(Length::Fill),
        cancel_btn,
        Space::new().width(8),
        save_btn,
    ]
    .align_y(Alignment::Center);

    // Layout
    let form_content = column![
        title_row,
        Space::new().height(24),
        name_label,
        Space::new().height(4),
        name_input,
        Space::new().height(16),
        command_label,
        Space::new().height(4),
        command_input,
        Space::new().height(16),
        description_label,
        Space::new().height(4),
        description_input,
        Space::new().height(24),
        hosts_label,
        hosts_help,
        Space::new().height(8),
        hosts_list,
        Space::new().height(24),
        button_row,
    ]
    .padding(32)
    .max_width(600);

    let scrollable_form = scrollable(
        container(form_content)
            .width(Fill)
            .align_x(Alignment::Center),
    )
    .height(Fill);

    container(scrollable_form)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}
