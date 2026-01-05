//! SFTP file browser view

use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, text, Column, Row};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::message::Message;
use crate::sftp::{format_size, FileEntry, SortOrder};
use crate::theme::THEME;

/// State for the SFTP browser
#[derive(Debug, Clone)]
pub struct SftpBrowserState {
    pub session_id: Uuid,
    pub host_name: String,
    pub current_path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected_index: Option<usize>,
    pub sort_order: SortOrder,
    pub loading: bool,
    pub error: Option<String>,
}

impl SftpBrowserState {
    pub fn new(session_id: Uuid, host_name: String, home_dir: PathBuf) -> Self {
        Self {
            session_id,
            host_name,
            current_path: home_dir,
            entries: Vec::new(),
            selected_index: None,
            sort_order: SortOrder::default(),
            loading: true,
            error: None,
        }
    }

    pub fn set_entries(&mut self, mut entries: Vec<FileEntry>) {
        self.sort_order.sort(&mut entries);
        self.entries = entries;
        self.selected_index = if self.entries.is_empty() {
            None
        } else {
            Some(0)
        };
        self.loading = false;
        self.error = None;
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.selected_index.and_then(|i| self.entries.get(i))
    }
}

/// Build the SFTP browser view
pub fn sftp_browser_view(state: &SftpBrowserState) -> Element<'_, Message> {
    let header = browser_header(state);
    let file_list = file_list_view(state);
    let footer = browser_footer(state);

    let content = column![header, file_list, footer].spacing(0);

    container(content)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}

/// Header with path bar and controls
fn browser_header(state: &SftpBrowserState) -> Element<'_, Message> {
    let path_text = state.current_path.to_string_lossy().to_string();

    // Navigation buttons
    let up_btn = button(text("⬆").size(14))
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(THEME.hover.into()),
                _ => Some(THEME.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: THEME.text_primary,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([4, 8])
        .on_press(Message::SftpNavigateUp(state.session_id));

    let refresh_btn = button(text("⟳").size(14))
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(THEME.hover.into()),
                _ => Some(THEME.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: THEME.text_primary,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([4, 8])
        .on_press(Message::SftpRefresh(state.session_id));

    // Path bar
    let path_bar = container(
        text(path_text)
            .size(13)
            .color(THEME.text_primary),
    )
    .padding(Padding::new(6.0).left(12.0).right(12.0))
    .style(|_theme| container::Style {
        background: Some(THEME.surface.into()),
        border: iced::Border {
            color: THEME.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    // Host name
    let host_label = text(format!("📡 {}", state.host_name))
        .size(12)
        .color(THEME.accent);

    container(
        row![
            up_btn,
            refresh_btn,
            path_bar,
            container(text("")).width(Fill),
            host_label,
        ]
        .spacing(8)
        .padding(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(|_theme| container::Style {
        background: Some(THEME.surface.into()),
        border: iced::Border {
            color: THEME.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// File list with scrolling
fn file_list_view(state: &SftpBrowserState) -> Element<'_, Message> {
    if state.loading {
        return container(
            text("Loading...")
                .size(14)
                .color(THEME.text_muted),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    if let Some(ref error) = state.error {
        return container(
            column![
                text("Error").size(16).color(THEME.text_primary),
                text(error).size(12).color(THEME.text_muted),
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
            text("Empty directory")
                .size(14)
                .color(THEME.text_muted),
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
            text("Name").size(11).color(THEME.text_muted).width(Length::FillPortion(4)),
            text("Size").size(11).color(THEME.text_muted).width(Length::FillPortion(1)),
            text("Modified").size(11).color(THEME.text_muted).width(Length::FillPortion(2)),
        ]
        .spacing(8)
        .padding(Padding::new(4.0).left(12.0).right(12.0)),
    )
    .style(|_theme| container::Style {
        border: iced::Border {
            color: THEME.border,
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
            file_entry_row(entry, index, state.selected_index == Some(index), state.session_id)
        })
        .collect();

    let file_list = scrollable(
        Column::with_children(entries).spacing(0),
    )
    .height(Fill)
    .width(Fill);

    column![headers, file_list].spacing(0).into()
}

/// Single file entry row
fn file_entry_row(
    entry: &FileEntry,
    index: usize,
    is_selected: bool,
    session_id: Uuid,
) -> Element<'static, Message> {
    let icon = entry.icon();
    let name = entry.name.clone();
    let size = if entry.is_dir {
        "—".to_string()
    } else {
        format_size(entry.size)
    };
    let modified = entry
        .modified
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".to_string());

    let bg_color = if is_selected {
        THEME.accent
    } else {
        THEME.background
    };

    let text_color = if is_selected {
        THEME.background
    } else {
        THEME.text_primary
    };

    let path = entry.path.clone();
    let is_dir = entry.is_dir;

    let content = row![
        text(format!("{} {}", icon, name))
            .size(13)
            .color(text_color)
            .width(Length::FillPortion(4)),
        text(size)
            .size(12)
            .color(if is_selected { text_color } else { THEME.text_secondary })
            .width(Length::FillPortion(1)),
        text(modified)
            .size(12)
            .color(if is_selected { text_color } else { THEME.text_muted })
            .width(Length::FillPortion(2)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(
        container(content)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .width(Fill),
    )
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered if !is_selected => THEME.hover,
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
        Message::SftpNavigate(session_id, path)
    } else {
        Message::SftpSelect(session_id, index)
    })
    .into()
}

/// Footer with status and actions
fn browser_footer(state: &SftpBrowserState) -> Element<'_, Message> {
    let item_count = state.entries.len();
    let status = if state.loading {
        "Loading...".to_string()
    } else {
        format!("{} items", item_count)
    };

    // Action buttons
    let download_btn = button(text("Download").size(12))
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(THEME.accent.into()),
                _ => Some(THEME.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: THEME.text_primary,
                border: iced::Border {
                    color: THEME.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .padding([4, 12])
        .on_press_maybe(
            state
                .selected_entry()
                .filter(|e| !e.is_parent())
                .map(|e| Message::SftpDownload(state.session_id, e.path.clone())),
        );

    let upload_btn = button(text("Upload").size(12))
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(THEME.accent.into()),
                _ => Some(THEME.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: THEME.text_primary,
                border: iced::Border {
                    color: THEME.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .padding([4, 12])
        .on_press(Message::SftpUpload(state.session_id));

    let mkdir_btn = button(text("New Folder").size(12))
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(THEME.hover.into()),
                _ => Some(THEME.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: THEME.text_primary,
                border: iced::Border {
                    color: THEME.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .padding([4, 12])
        .on_press(Message::SftpMkdir(state.session_id));

    let delete_btn = button(text("Delete").size(12))
        .style(|_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(iced::Color::from_rgb8(200, 60, 60).into()),
                _ => Some(THEME.surface.into()),
            };
            iced::widget::button::Style {
                background: bg,
                text_color: THEME.text_primary,
                border: iced::Border {
                    color: THEME.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        })
        .padding([4, 12])
        .on_press_maybe(
            state
                .selected_entry()
                .filter(|e| !e.is_parent())
                .map(|e| Message::SftpDelete(state.session_id, e.path.clone())),
        );

    container(
        row![
            text(status).size(12).color(THEME.text_muted),
            container(text("")).width(Fill),
            upload_btn,
            mkdir_btn,
            download_btn,
            delete_btn,
        ]
        .spacing(8)
        .padding(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(|_theme| container::Style {
        background: Some(THEME.surface.into()),
        border: iced::Border {
            color: THEME.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}
