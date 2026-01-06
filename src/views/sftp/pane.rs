//! SFTP pane rendering
//!
//! This module contains the rendering functions for individual panes
//! in the dual-pane SFTP browser.

use iced::widget::{button, column, container, pick_list, row, scrollable, text, Column};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{Message, SessionId};
use crate::sftp::{format_size, FileEntry, FileIcon};
use crate::theme::Theme;
use crate::widgets::mouse_area;

use super::state::FilePaneState;
use super::types::{PaneId, PaneSource};

/// Get SVG icon data for a file icon type
fn file_icon_data(icon_type: FileIcon) -> &'static [u8] {
    match icon_type {
        FileIcon::ParentDir => icons::ui::CHEVRON_LEFT,
        FileIcon::Folder => icons::files::FOLDER,
        FileIcon::Symlink => icons::files::FILE,
        FileIcon::Code => icons::files::FILE_CODE,
        FileIcon::Text => icons::files::FILE_TEXT,
        FileIcon::Image => icons::files::IMAGE,
        FileIcon::Audio => icons::files::MUSIC,
        FileIcon::Video => icons::files::VIDEO,
        FileIcon::Archive => icons::files::ARCHIVE,
        FileIcon::Config => icons::files::FILE_JSON,
        FileIcon::Executable => icons::files::FILE_COG,
        FileIcon::File => icons::files::FILE,
    }
}

/// Build a single pane view for the dual-pane browser
pub fn single_pane_view(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    available_hosts: Vec<(Uuid, String)>,
    is_active: bool,
    theme: Theme,
) -> Element<'_, Message> {
    let header = pane_header(state, pane_id, tab_id, available_hosts, is_active, theme);
    let file_list = pane_file_list(state, pane_id, tab_id, theme);
    let footer = pane_footer(state, pane_id, tab_id, theme);

    let content = column![header, file_list, footer].spacing(0);

    // Simple container - focus is set via file clicks and context menu
    container(content)
        .width(Length::FillPortion(1))
        .height(Fill)
        .into()
}

/// Header with source dropdown, navigation buttons, and path bar
pub fn pane_header(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    available_hosts: Vec<(Uuid, String)>,
    is_active: bool,
    theme: Theme,
) -> Element<'_, Message> {
    let path_text = state.current_path.to_string_lossy().to_string();

    // Build source options: Local + all configured hosts
    let mut source_options: Vec<String> = vec!["Local".to_string()];
    for (_host_id, host_name) in &available_hosts {
        source_options.push(host_name.clone());
    }

    let current_source = state.source.display_name().to_string();

    // Clone hosts for the closure
    let hosts_for_closure = available_hosts.clone();

    // Source dropdown
    let source_picker = pick_list(
        source_options.clone(),
        Some(current_source.clone()),
        move |selected| {
            if selected == "Local" {
                Message::DualSftpPaneSourceChanged(tab_id, pane_id, PaneSource::Local)
            } else {
                // Find the host ID by name and trigger connection
                if let Some((host_id, _)) = hosts_for_closure.iter().find(|(_, name)| name == &selected) {
                    Message::DualSftpConnectHost(tab_id, pane_id, *host_id)
                } else {
                    Message::Noop
                }
            }
        },
    )
    .width(Length::Fixed(150.0))
    .text_size(12.0)
    .padding([4, 8]);

    // Navigation buttons
    let up_btn = button(icon_with_color(icons::ui::CHEVRON_LEFT, 14, theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: theme.text_primary,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([4, 8])
        .on_press(Message::DualSftpPaneNavigateUp(tab_id, pane_id));

    let refresh_btn = button(icon_with_color(icons::ui::REFRESH, 14, theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: theme.text_primary,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([4, 8])
        .on_press(Message::DualSftpPaneRefresh(tab_id, pane_id));

    // Path bar
    let path_bar = container(text(path_text).size(12).color(theme.text_primary))
        .padding(Padding::new(6.0).left(8.0).right(8.0))
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

    // Active pane indicator: colored top border
    let border_color = if is_active { theme.accent } else { theme.border };

    container(
        row![source_picker, up_btn, refresh_btn, path_bar]
            .spacing(8)
            .padding(8)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: border_color,
            width: if is_active { 2.0 } else { 1.0 },
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// File list for a single pane
pub fn pane_file_list<'a>(
    state: &'a FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    theme: Theme,
) -> Element<'a, Message> {
    if state.loading {
        return container(text("Loading...").size(14).color(theme.text_muted))
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();
    }

    if let Some(ref error) = state.error {
        return container(
            column![
                text("Error").size(16).color(theme.text_primary),
                text(error).size(12).color(theme.text_muted),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    if state.entries.is_empty() {
        return container(
            column![
                icon_with_color(icons::files::FOLDER, 32, theme.text_muted),
                text("Empty directory").size(14).color(theme.text_muted),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    // Column headers
    let headers = container(
        row![
            text("Name")
                .size(12)
                .color(theme.text_muted)
                .width(Length::FillPortion(4)),
            text("Date Modified")
                .size(12)
                .color(theme.text_muted)
                .width(Length::FillPortion(2)),
            text("Size")
                .size(12)
                .color(theme.text_muted)
                .width(Length::FillPortion(1)),
            text("Kind")
                .size(12)
                .color(theme.text_muted)
                .width(Length::FillPortion(2)),
        ]
        .spacing(8)
        .padding(Padding::new(8.0).left(12.0).right(12.0)),
    )
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    });

    // File entries
    let entries: Vec<Element<'_, Message>> = state
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            pane_file_entry_row(entry, index, state.is_selected(index), tab_id, pane_id, theme)
        })
        .collect();

    let file_list = scrollable(Column::with_children(entries).spacing(0))
        .height(Fill)
        .width(Fill);

    column![headers, file_list].spacing(0).into()
}

/// Single file entry row for a pane
pub fn pane_file_entry_row(
    entry: &FileEntry,
    index: usize,
    is_selected: bool,
    tab_id: SessionId,
    pane_id: PaneId,
    theme: Theme,
) -> Element<'static, Message> {
    let icon_type = entry.icon_type();
    let icon_data = file_icon_data(icon_type);
    let name = entry.name.clone();
    let size = if entry.is_dir {
        "—".to_string()
    } else {
        format_size(entry.size)
    };

    let bg_color = if is_selected {
        theme.accent
    } else {
        theme.background
    };

    let text_color = if is_selected {
        theme.background
    } else {
        theme.text_primary
    };

    let icon_color = if is_selected {
        theme.background
    } else if entry.is_dir {
        theme.accent
    } else {
        theme.text_secondary
    };

    let path = entry.path.clone();
    let is_dir = entry.is_dir;

    let modified = entry.formatted_modified();
    let kind = entry.kind_description();

    let name_row = row![
        icon_with_color(icon_data, 16, icon_color),
        text(name).size(13).color(text_color),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let secondary_color = if is_selected {
        text_color
    } else {
        theme.text_secondary
    };

    let content = row![
        container(name_row).width(Length::FillPortion(4)),
        text(modified)
            .size(12)
            .color(secondary_color)
            .width(Length::FillPortion(2)),
        text(size)
            .size(12)
            .color(secondary_color)
            .width(Length::FillPortion(1)),
        text(kind)
            .size(12)
            .color(secondary_color)
            .width(Length::FillPortion(2)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let btn = button(
        container(content)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .width(Fill),
    )
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered if !is_selected => theme.hover,
            _ => bg_color,
        };
        iced::widget::button::Style {
            background: Some(background.into()),
            text_color,
            border: iced::Border::default(),
            ..Default::default()
        }
    })
    .padding(0)
    .width(Fill)
    .on_press(if is_dir {
        Message::DualSftpPaneNavigate(tab_id, pane_id, path)
    } else {
        Message::DualSftpPaneSelect(tab_id, pane_id, index)
    });

    // Wrap in mouse_area to handle right-click
    mouse_area(btn)
        .on_right_press(move |x, y| {
            Message::DualSftpShowContextMenu(tab_id, pane_id, x, y, Some(index))
        })
        .into()
}

/// Footer with status for a pane
pub fn pane_footer<'a>(
    state: &'a FilePaneState,
    _pane_id: PaneId,
    _tab_id: SessionId,
    theme: Theme,
) -> Element<'a, Message> {
    let item_count = state.entries.len();
    let selected_count = state.selected_indices.len();
    let status = if state.loading {
        "Loading...".to_string()
    } else if selected_count > 0 {
        format!("{} of {} items selected", selected_count, item_count)
    } else {
        format!("{} items", item_count)
    };

    container(
        row![text(status).size(12).color(theme.text_muted),]
            .spacing(8)
            .padding(8)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}
