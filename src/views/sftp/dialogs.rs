//! SFTP dialog rendering
//!
//! This module contains the rendering functions for SFTP-related dialogs
//! (New Folder, Rename, Delete, Permissions).

use iced::widget::{Column, Space, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Fill, Length, Padding};
use std::path::PathBuf;

use crate::icons::{self, icon_with_color};
use crate::message::{Message, SessionId, SftpMessage};
use crate::theme::{ScaledFonts, Theme};

use super::state::{DualPaneSftpState, SftpDialogState};
use super::types::{PermissionBit, PermissionBits, SftpDialogType};

/// Build the SFTP dialog overlay (New Folder, Rename, or Delete)
pub fn sftp_dialog_view(state: &DualPaneSftpState, theme: Theme, fonts: ScaledFonts) -> Element<'_, Message> {
    let Some(ref dialog) = state.dialog else {
        return Space::new().into();
    };

    let tab_id = state.tab_id;

    // Build dialog content based on type
    let dialog_content: Element<'_, Message> = match &dialog.dialog_type {
        SftpDialogType::Delete { entries } => {
            build_delete_dialog(tab_id, entries, dialog.error.as_deref(), theme, fonts)
        }
        SftpDialogType::EditPermissions {
            name, permissions, ..
        } => build_permissions_dialog(tab_id, name, permissions, dialog.error.as_deref(), theme, fonts),
        _ => build_input_dialog(tab_id, dialog, theme, fonts),
    };

    // Dialog box with styling
    let dialog_box = container(dialog_content).style(move |_| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba8(0, 0, 0, 0.5),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..Default::default()
    });

    // Backdrop
    let backdrop = container(
        container(dialog_box)
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .height(Fill)
    .style(move |_| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.5).into()),
        ..Default::default()
    });

    backdrop.into()
}

/// Build input dialog for New Folder or Rename
fn build_input_dialog(
    tab_id: SessionId,
    dialog: &SftpDialogState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'_, Message> {
    // Handle unexpected dialog types by returning an error element early
    if matches!(
        &dialog.dialog_type,
        SftpDialogType::Delete { .. } | SftpDialogType::EditPermissions { .. }
    ) {
        // These dialog types should be handled by build_delete_dialog and
        // build_permissions_dialog respectively. If we reach here, it's a bug.
        tracing::error!(
            "build_input_dialog called for Delete/EditPermissions dialog type - this is a bug"
        );
        return column![
            text("Internal Error")
                .size(fonts.heading)
                .color(iced::Color::from_rgb8(220, 80, 80)),
            text("Unexpected dialog type. Please report this issue.")
                .size(fonts.body)
                .color(theme.text_secondary),
        ]
        .padding(24)
        .width(Length::Fixed(300.0))
        .into();
    }

    let (title, placeholder, submit_label, subtitle): (
        &'static str,
        &'static str,
        &'static str,
        Option<String>,
    ) = match &dialog.dialog_type {
        SftpDialogType::NewFolder => ("New Folder", "Folder name", "Create", None),
        SftpDialogType::Rename { .. } => ("Rename", "New name", "Rename", None),
        // Already handled above with early return
        SftpDialogType::Delete { .. } | SftpDialogType::EditPermissions { .. } => {
            ("Error", "", "Close", None)
        }
    };

    let title_text = text(title)
        .size(fonts.heading)
        .color(theme.text_primary);

    let input_value = dialog.input_value.clone();
    let input = text_input(placeholder, &input_value)
        .on_input(move |value| Message::Sftp(SftpMessage::DialogInputChanged(tab_id, value)))
        .on_submit(Message::Sftp(SftpMessage::DialogSubmit(tab_id)))
        .padding([10, 12])
        .size(fonts.body)
        .style(move |_theme, _status| text_input::Style {
            background: theme.background.into(),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            icon: theme.text_muted,
            placeholder: theme.text_muted,
            value: theme.text_primary,
            selection: theme.accent,
        });

    // Error message if any
    let error_text: Element<'_, Message> = if let Some(ref error) = dialog.error {
        text(error)
            .size(fonts.label)
            .color(iced::Color::from_rgb8(220, 80, 80))
            .into()
    } else {
        Space::new().into()
    };

    let cancel_btn = dialog_cancel_button(tab_id, theme, fonts);

    let is_valid = dialog.is_valid();
    let submit_btn = dialog_submit_button(tab_id, submit_label, is_valid, false, theme, fonts);

    let button_row = row![Space::new().width(Fill), cancel_btn, submit_btn].spacing(8);

    // Build subtitle element if present
    let subtitle_element: Element<'_, Message> = if let Some(subtitle) = subtitle {
        text(subtitle)
            .size(fonts.button_small)
            .color(theme.text_muted)
            .into()
    } else {
        Space::new().into()
    };

    column![
        title_text,
        subtitle_element,
        Space::new().height(12),
        input,
        error_text,
        Space::new().height(16),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(380.0))
    .into()
}

/// Build delete confirmation dialog
fn build_delete_dialog<'a>(
    tab_id: SessionId,
    entries: &'a [(String, PathBuf, bool)],
    error: Option<&'a str>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let title_text = text("Delete")
        .size(fonts.heading)
        .color(theme.text_primary);

    // Build the confirmation message
    let count = entries.len();
    let has_folders = entries.iter().any(|(_, _, is_dir)| *is_dir);

    let warning_msg = if count == 1 {
        let (name, _, is_dir) = &entries[0];
        if *is_dir {
            format!("Delete folder \"{}\" and all its contents?", name)
        } else {
            format!("Delete \"{}\"?", name)
        }
    } else if has_folders {
        format!(
            "Delete {} items? Folders will be deleted with all their contents.",
            count
        )
    } else {
        format!("Delete {} items?", count)
    };

    let warning_text = text(warning_msg)
        .size(fonts.body)
        .color(theme.text_secondary);

    // List the items to be deleted (show up to 5)
    let items_list: Element<'_, Message> = if count <= 5 {
        let items: Vec<Element<'_, Message>> = entries
            .iter()
            .map(|(name, _, is_dir)| {
                let icon_data = if *is_dir {
                    icons::files::FOLDER
                } else {
                    icons::files::FILE
                };
                let icon = icon_with_color(icon_data, 14, theme.text_muted);
                row![
                    icon,
                    text(name)
                        .size(fonts.button_small)
                        .color(theme.text_secondary)
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            })
            .collect();

        Column::with_children(items)
            .spacing(4)
            .padding(Padding::from([8, 12]))
            .into()
    } else {
        // Show first 3 items + "and X more"
        let mut items: Vec<Element<'_, Message>> = entries
            .iter()
            .take(3)
            .map(|(name, _, is_dir)| {
                let icon_data = if *is_dir {
                    icons::files::FOLDER
                } else {
                    icons::files::FILE
                };
                let icon = icon_with_color(icon_data, 14, theme.text_muted);
                row![
                    icon,
                    text(name)
                        .size(fonts.button_small)
                        .color(theme.text_secondary)
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            })
            .collect();

        items.push(
            text(format!("... and {} more", count - 3))
                .size(fonts.button_small)
                .color(theme.text_muted)
                .into(),
        );

        Column::with_children(items)
            .spacing(4)
            .padding(Padding::from([8, 12]))
            .into()
    };

    // Items container with background
    let items_container = container(items_list)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(theme.background.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

    // Warning about permanent deletion
    let permanent_warning = row![
        icon_with_color(
            icons::ui::ALERT_TRIANGLE,
            16,
            iced::Color::from_rgb8(220, 160, 60)
        ),
        text("This action cannot be undone.")
            .size(fonts.label)
            .color(iced::Color::from_rgb8(220, 160, 60))
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Error message if any
    let error_text: Element<'_, Message> = if let Some(error) = error {
        text(error)
            .size(fonts.label)
            .color(iced::Color::from_rgb8(220, 80, 80))
            .into()
    } else {
        Space::new().into()
    };

    let cancel_btn = dialog_cancel_button(tab_id, theme, fonts);
    let delete_btn = dialog_submit_button(tab_id, "Delete", true, true, theme, fonts);

    let button_row = row![Space::new().width(Fill), cancel_btn, delete_btn].spacing(8);

    column![
        title_text,
        Space::new().height(12),
        warning_text,
        Space::new().height(12),
        items_container,
        Space::new().height(12),
        permanent_warning,
        error_text,
        Space::new().height(16),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(400.0))
    .into()
}

/// Build the permissions dialog
fn build_permissions_dialog<'a>(
    tab_id: SessionId,
    name: &'a str,
    permissions: &'a PermissionBits,
    error: Option<&'a str>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let title_text = text("Edit Permissions")
        .size(fonts.heading)
        .color(theme.text_primary);

    // File name display
    let file_info = row![
        icon_with_color(icons::files::FILE, 16, theme.text_muted),
        text(name).size(fonts.body).color(theme.text_secondary)
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Current mode display
    let mode_text = text(format!("Mode: {}", permissions.as_octal_string()))
        .size(fonts.button_small)
        .color(theme.text_muted);

    // Permission grid headers
    let header_row = row![
        Space::new().width(Length::Fixed(80.0)),
        text("Read")
            .size(fonts.label)
            .color(theme.text_muted)
            .width(Length::Fixed(60.0)),
        text("Write")
            .size(fonts.label)
            .color(theme.text_muted)
            .width(Length::Fixed(60.0)),
        text("Execute")
            .size(fonts.label)
            .color(theme.text_muted)
            .width(Length::Fixed(60.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Owner row
    let owner_row = permission_row(
        tab_id,
        "Owner",
        permissions.owner_read,
        permissions.owner_write,
        permissions.owner_execute,
        PermissionBit::OwnerRead,
        PermissionBit::OwnerWrite,
        PermissionBit::OwnerExecute,
        theme,
        fonts,
    );

    // Group row
    let group_row = permission_row(
        tab_id,
        "Group",
        permissions.group_read,
        permissions.group_write,
        permissions.group_execute,
        PermissionBit::GroupRead,
        PermissionBit::GroupWrite,
        PermissionBit::GroupExecute,
        theme,
        fonts,
    );

    // Other row
    let other_row = permission_row(
        tab_id,
        "Other",
        permissions.other_read,
        permissions.other_write,
        permissions.other_execute,
        PermissionBit::OtherRead,
        PermissionBit::OtherWrite,
        PermissionBit::OtherExecute,
        theme,
        fonts,
    );

    // Permission grid
    let permission_grid =
        container(column![header_row, owner_row, group_row, other_row].spacing(8))
            .padding(12)
            .width(Fill)
            .style(move |_| container::Style {
                background: Some(theme.background.into()),
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

    // Error message if any
    let error_text: Element<'_, Message> = if let Some(error) = error {
        text(error)
            .size(fonts.label)
            .color(iced::Color::from_rgb8(220, 80, 80))
            .into()
    } else {
        Space::new().into()
    };

    let cancel_btn = dialog_cancel_button(tab_id, theme, fonts);
    let apply_btn = dialog_submit_button(tab_id, "Apply", true, false, theme, fonts);

    let button_row = row![Space::new().width(Fill), cancel_btn, apply_btn].spacing(8);

    column![
        title_text,
        Space::new().height(12),
        file_info,
        mode_text,
        Space::new().height(12),
        permission_grid,
        error_text,
        Space::new().height(16),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(350.0))
    .into()
}

/// Create a row of permission checkboxes for owner/group/other
#[allow(clippy::too_many_arguments)]
fn permission_row<'a>(
    tab_id: SessionId,
    label: &'a str,
    read: bool,
    write: bool,
    execute: bool,
    read_bit: PermissionBit,
    write_bit: PermissionBit,
    execute_bit: PermissionBit,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(fonts.button_small)
            .color(theme.text_primary)
            .width(Length::Fixed(80.0)),
        permission_checkbox(tab_id, read, read_bit, theme),
        permission_checkbox(tab_id, write, write_bit, theme),
        permission_checkbox(tab_id, execute, execute_bit, theme),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// Create a styled permission checkbox
fn permission_checkbox(
    tab_id: SessionId,
    checked: bool,
    bit: PermissionBit,
    theme: Theme,
) -> iced::widget::Button<'static, Message> {
    let bg_color = if checked {
        theme.accent
    } else {
        theme.background
    };
    let icon_color = if checked {
        theme.background
    } else {
        theme.text_muted
    };

    let icon_content: Element<'static, Message> = if checked {
        icon_with_color(icons::ui::CHECK, 14, icon_color).into()
    } else {
        Space::new().width(14).height(14).into()
    };

    button(
        container(icon_content)
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .width(Length::Fixed(60.0))
    .style(move |_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => {
                if checked {
                    iced::Color::from_rgb8(0, 100, 180)
                } else {
                    theme.hover
                }
            }
            _ => bg_color,
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            text_color: icon_color,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    })
    .on_press(Message::Sftp(SftpMessage::PermissionToggle(
        tab_id, bit, !checked,
    )))
}

/// Create a cancel button for dialogs
fn dialog_cancel_button(tab_id: SessionId, theme: Theme, fonts: ScaledFonts) -> iced::widget::Button<'static, Message> {
    button(
        text("Cancel")
            .size(fonts.button_small)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .style(move |_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => theme.hover,
            _ => theme.surface,
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    })
    .on_press(Message::Sftp(SftpMessage::DialogCancel(tab_id)))
}

/// Create a submit button for dialogs
fn dialog_submit_button(
    tab_id: SessionId,
    label: &str,
    is_valid: bool,
    is_destructive: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> iced::widget::Button<'static, Message> {
    let (normal_color, hover_color) = if is_destructive {
        (
            iced::Color::from_rgb8(180, 60, 60),
            iced::Color::from_rgb8(200, 70, 70),
        )
    } else {
        (theme.accent, iced::Color::from_rgb8(0, 100, 180))
    };

    let btn = button(
        text(label.to_string())
            .size(fonts.button_small)
            .color(if is_valid {
                theme.background
            } else {
                theme.text_muted
            }),
    )
    .padding([8, 16])
    .style(move |_theme, status| {
        let bg = if is_valid {
            match status {
                iced::widget::button::Status::Hovered => hover_color,
                _ => normal_color,
            }
        } else {
            theme.surface
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            text_color: if is_valid {
                theme.background
            } else {
                theme.text_muted
            },
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    if is_valid {
        btn.on_press(Message::Sftp(SftpMessage::DialogSubmit(tab_id)))
    } else {
        btn
    }
}
