//! SFTP-specific dialogs (mkdir, delete confirmation)

use std::path::PathBuf;

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::message::Message;
use crate::theme::{BORDER_RADIUS, THEME};

/// State for the mkdir dialog
#[derive(Debug, Clone)]
pub struct MkdirDialogState {
    pub session_id: Uuid,
    pub parent_path: PathBuf,
    pub folder_name: String,
}

impl MkdirDialogState {
    pub fn new(session_id: Uuid, parent_path: PathBuf) -> Self {
        Self {
            session_id,
            parent_path,
            folder_name: String::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        let name = self.folder_name.trim();
        !name.is_empty() && !name.contains('/') && !name.contains('\\')
    }

    pub fn full_path(&self) -> PathBuf {
        self.parent_path.join(self.folder_name.trim())
    }
}

/// State for the delete confirmation dialog
#[derive(Debug, Clone)]
pub struct DeleteConfirmDialogState {
    pub session_id: Uuid,
    pub path: PathBuf,
    pub is_directory: bool,
    pub name: String,
}

impl DeleteConfirmDialogState {
    pub fn new(session_id: Uuid, path: PathBuf, is_directory: bool) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        Self {
            session_id,
            path,
            is_directory,
            name,
        }
    }
}

/// Build the mkdir dialog view
pub fn mkdir_dialog_view(state: &MkdirDialogState) -> Element<'static, Message> {
    let folder_name_value = state.folder_name.clone();
    let is_valid = state.is_valid();
    let parent_display = state.parent_path.to_string_lossy().to_string();

    let title = text("New Folder").size(20).color(THEME.text_primary);

    let path_hint = text(format!("Creating in: {}", parent_display))
        .size(12)
        .color(THEME.text_muted);

    let name_input = column![
        text("Folder Name").size(12).color(THEME.text_secondary),
        text_input("new-folder", &folder_name_value)
            .on_input(Message::SftpMkdirNameChanged)
            .on_submit(Message::SftpMkdirSubmit)
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    let cancel_button = button(text("Cancel").size(14).color(THEME.text_primary))
        .padding([8, 16])
        .style(|_theme, _status| button::Style {
            background: Some(THEME.surface.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .on_press(Message::DialogClose);

    let create_button = button(text("Create").size(14).color(THEME.text_primary))
        .padding([8, 16])
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => THEME.accent,
                button::Status::Disabled => THEME.surface,
                _ => THEME.accent,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: THEME.text_primary,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .on_press_maybe(if is_valid {
            Some(Message::SftpMkdirSubmit)
        } else {
            None
        });

    let button_row = row![Space::with_width(Length::Fill), cancel_button, create_button,].spacing(8);

    let form = column![
        title,
        Space::with_height(8),
        path_hint,
        Space::with_height(16),
        name_input,
        Space::with_height(16),
        button_row,
    ]
    .spacing(0)
    .padding(24)
    .width(Length::Fixed(400.0));

    dialog_backdrop(form)
}

/// Build the delete confirmation dialog view
pub fn delete_confirm_dialog_view(state: &DeleteConfirmDialogState) -> Element<'static, Message> {
    let item_type = if state.is_directory { "folder" } else { "file" };
    let name = state.name.clone();

    let title = text("Confirm Delete").size(20).color(THEME.text_primary);

    let warning_icon = text("⚠").size(32).color(iced::Color::from_rgb8(255, 180, 0));

    let message = text(format!(
        "Are you sure you want to delete the {} \"{}\"?",
        item_type, name
    ))
    .size(14)
    .color(THEME.text_secondary);

    let subdirectory_warning: Element<'static, Message> = if state.is_directory {
        text("This will delete all contents recursively.")
            .size(12)
            .color(THEME.text_muted)
            .into()
    } else {
        Space::with_height(0).into()
    };

    let cancel_button = button(text("Cancel").size(14).color(THEME.text_primary))
        .padding([8, 16])
        .style(|_theme, _status| button::Style {
            background: Some(THEME.surface.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .on_press(Message::DialogClose);

    let delete_button = button(text("Delete").size(14).color(THEME.text_primary))
        .padding([8, 16])
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(220, 50, 50),
                _ => iced::Color::from_rgb8(180, 40, 40),
            };
            button::Style {
                background: Some(bg.into()),
                text_color: THEME.text_primary,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .on_press(Message::SftpDeleteConfirm);

    let button_row = row![Space::with_width(Length::Fill), cancel_button, delete_button,].spacing(8);

    let content = column![
        row![warning_icon, title].spacing(12).align_y(Alignment::Center),
        Space::with_height(16),
        message,
        subdirectory_warning,
        Space::with_height(24),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(420.0));

    dialog_backdrop(content)
}

/// Helper to wrap dialog content in a backdrop
fn dialog_backdrop(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    let dialog_box = container(content)
        .style(|_theme| container::Style {
            background: Some(THEME.surface.into()),
            border: iced::Border {
                color: THEME.border,
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

    container(
        container(dialog_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.7).into()),
        ..Default::default()
    })
    .into()
}
