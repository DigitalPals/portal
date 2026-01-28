//! Snippet edit form view
//!
//! Full-page edit form for creating and editing snippets,
//! including host selection for multi-host execution.

use iced::widget::{
    Column, Row, Space, button, column, container, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Fill, Length};
use uuid::Uuid;

use crate::app::SnippetEditState;
use crate::config::DetectedOs;
use crate::icons::{self, icon_with_color};
use crate::message::{Message, SnippetField, SnippetMessage};
use crate::theme::{BORDER_RADIUS, ScaledFonts, Theme};
use crate::views::host_grid::os_icon_data;

/// Build the snippet edit form (full-page, replacing grid)
pub fn snippet_edit_view(
    state: &SnippetEditState,
    hosts: &[(Uuid, String, Option<DetectedOs>)],
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let title = if state.snippet_id.is_some() {
        "Edit Snippet"
    } else {
        "New Snippet"
    };

    let title_row = row![
        icon_with_color(icons::ui::CODE, 24, theme.accent),
        text(title)
            .size(fonts.dialog_title + 4.0)
            .color(theme.text_primary),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    // Name input
    let name_label = text("Name")
        .size(fonts.body)
        .color(theme.text_secondary);
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
    let command_label = text("Command")
        .size(fonts.body)
        .color(theme.text_secondary);
    let command_value = state.command.clone();
    let command_input = text_input(
        "e.g., sudo apt update && sudo apt upgrade -y",
        &command_value,
    )
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
        .size(fonts.body)
        .color(theme.text_secondary);
    let description_value = state.description.clone();
    let description_input = text_input(
        "Optional description of what this command does",
        &description_value,
    )
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
    let hosts_label = text("Target Hosts")
        .size(fonts.body)
        .color(theme.text_secondary);
    let hosts_help = text("Select which hosts to run this command on")
        .size(fonts.label)
        .color(theme.text_muted);

    let selected_hosts = state.selected_hosts.clone();
    let host_pills: Vec<Element<'static, Message>> = hosts
        .iter()
        .map(|(host_id, host_name, detected_os)| {
            let is_selected = selected_hosts.contains(host_id);
            let hid = *host_id;
            let name = host_name.clone();

            // Get OS icon and color
            let os_icon_bytes = os_icon_data(detected_os);
            let os_color = match detected_os {
                Some(os) => {
                    let (r, g, b) = os.icon_color();
                    iced::Color::from_rgb8(r, g, b)
                }
                None => iced::Color::from_rgb8(0x70, 0x70, 0x70),
            };

            // Build row content: [OS icon] [Name] [spacer] [checkmark if selected]
            let os_icon = icon_with_color(os_icon_bytes, 16, os_color);

            let checkmark: Element<'static, Message> = if is_selected {
                icon_with_color(icons::ui::CHECK, 14, theme.accent).into()
            } else {
                Space::new().width(14).into()
            };

            let row_content = row![
                os_icon,
                Space::new().width(10),
                text(name).size(fonts.body).color(theme.text_primary),
                Space::new().width(Fill),
                checkmark,
            ]
            .align_y(Alignment::Center);

            button(container(row_content).padding([8, 12]).width(Fill))
                .style(move |_theme, status| {
                    let (bg, border_color) = match (status, is_selected) {
                        (_, true) => (theme.accent.scale_alpha(0.15), theme.accent),
                        (button::Status::Hovered, false) => (theme.hover, theme.border),
                        _ => (iced::Color::TRANSPARENT, theme.border),
                    };
                    button::Style {
                        background: Some(bg.into()),
                        text_color: theme.text_primary,
                        border: iced::Border {
                            color: border_color,
                            width: 1.0,
                            radius: 20.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .padding(0)
                .width(Fill)
                .on_press(Message::Snippet(SnippetMessage::ToggleHost(
                    hid,
                    !is_selected,
                )))
                .into()
        })
        .collect();

    let hosts_list: Element<'static, Message> = if host_pills.is_empty() {
        container(
            column![
                text("No hosts configured")
                    .size(fonts.body)
                    .color(theme.text_muted),
                text("Add hosts from the Hosts page first")
                    .size(fonts.label)
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
        // Arrange pills in two columns
        let mut rows: Vec<Element<'static, Message>> = Vec::new();
        let mut iter = host_pills.into_iter();

        while let Some(first) = iter.next() {
            let row_content: Row<'static, Message> = if let Some(second) = iter.next() {
                Row::with_children(vec![first, second])
                    .spacing(8)
                    .width(Fill)
            } else {
                // Odd number of items - last row has one item taking half width
                Row::with_children(vec![
                    container(first).width(Fill).into(),
                    Space::new().width(Fill).into(),
                ])
                .spacing(8)
                .width(Fill)
            };
            rows.push(row_content.into());
        }

        container(
            scrollable(
                row![
                    Column::with_children(rows).spacing(8).width(Fill),
                    Space::new().width(8), // Right margin for scrollbar
                ]
                .width(Fill),
            )
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default()
                    .width(4)
                    .scroller_width(4)
                    .anchor(scrollable::Anchor::Start),
            ))
            .style(move |_theme, status| {
                let scroller_color = match status {
                    scrollable::Status::Active { .. } => iced::Color::TRANSPARENT,
                    scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. } => {
                        theme.border
                    }
                };
                scrollable::Style {
                    container: container::Style::default(),
                    vertical_rail: scrollable::Rail {
                        background: None,
                        border: iced::Border::default(),
                        scroller: scrollable::Scroller {
                            background: scroller_color.into(),
                            border: iced::Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                        },
                    },
                    horizontal_rail: scrollable::Rail {
                        background: None,
                        border: iced::Border::default(),
                        scroller: scrollable::Scroller {
                            background: iced::Color::TRANSPARENT.into(),
                            border: iced::Border::default(),
                        },
                    },
                    gap: None,
                    auto_scroll: scrollable::AutoScroll {
                        background: iced::Color::TRANSPARENT.into(),
                        border: iced::Border::default(),
                        shadow: iced::Shadow::default(),
                        icon: iced::Color::TRANSPARENT,
                    },
                }
            })
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

    let cancel_btn = button(
        text("Cancel")
            .size(fonts.body)
            .color(theme.text_primary),
    )
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
        button(text("Save").size(fonts.body).color(iced::Color::WHITE))
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
        button(text("Save").size(fonts.body).color(theme.text_muted))
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
        button(
            text("Delete")
                .size(fonts.body)
                .color(iced::Color::from_rgb8(0xd2, 0x0f, 0x39)),
        )
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
