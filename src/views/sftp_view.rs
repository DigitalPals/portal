//! SFTP file browser view - Dual-pane implementation

use std::path::PathBuf;

use iced::widget::{button, column, container, pick_list, row, scrollable, text, Column, Space};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::Message;
use crate::sftp::{format_size, FileEntry, FileIcon, SortOrder};
use crate::theme::THEME;

// ============================================================================
// Dual-Pane SFTP Types
// ============================================================================

/// Identifies which pane an action targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    Left,
    Right,
}

/// Session ID type alias for clarity
pub type SessionId = Uuid;

/// Source of files for a pane - either local filesystem or remote SFTP
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneSource {
    Local,
    Remote {
        session_id: SessionId,
        host_name: String,
    },
}

impl PaneSource {
    pub fn display_name(&self) -> &str {
        match self {
            PaneSource::Local => "Local",
            PaneSource::Remote { host_name, .. } => host_name,
        }
    }
}

/// State for a single file browser pane (can be local or remote)
#[derive(Debug, Clone)]
pub struct FilePaneState {
    pub source: PaneSource,
    pub current_path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected_index: Option<usize>,
    pub sort_order: SortOrder,
    pub loading: bool,
    pub error: Option<String>,
}

impl FilePaneState {
    pub fn new_local() -> Self {
        let home_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            source: PaneSource::Local,
            current_path: home_dir,
            entries: Vec::new(),
            selected_index: None,
            sort_order: SortOrder::default(),
            loading: true,
            error: None,
        }
    }

    pub fn new_remote(session_id: SessionId, host_name: String, home_dir: PathBuf) -> Self {
        Self {
            source: PaneSource::Remote { session_id, host_name },
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

/// Available source option for pane dropdown
#[derive(Debug, Clone)]
pub struct SourceOption {
    pub source: PaneSource,
    pub display_name: String,
}

/// State for the dual-pane SFTP browser
#[derive(Debug, Clone)]
pub struct DualPaneSftpState {
    pub tab_id: SessionId,
    pub left_pane: FilePaneState,
    pub right_pane: FilePaneState,
    pub active_pane: PaneId,
}

impl DualPaneSftpState {
    pub fn new(tab_id: SessionId) -> Self {
        Self {
            tab_id,
            left_pane: FilePaneState::new_local(),
            right_pane: FilePaneState::new_local(),
            active_pane: PaneId::Left,
        }
    }

    pub fn pane_mut(&mut self, pane_id: PaneId) -> &mut FilePaneState {
        match pane_id {
            PaneId::Left => &mut self.left_pane,
            PaneId::Right => &mut self.right_pane,
        }
    }

    pub fn pane(&self, pane_id: PaneId) -> &FilePaneState {
        match pane_id {
            PaneId::Left => &self.left_pane,
            PaneId::Right => &self.right_pane,
        }
    }
}

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

// ============================================================================
// Dual-Pane SFTP View
// ============================================================================

/// Build the dual-pane SFTP browser view
pub fn dual_pane_sftp_view(
    state: &DualPaneSftpState,
    available_hosts: Vec<(Uuid, String)>,
) -> Element<'_, Message> {
    let left_pane = single_pane_view(
        &state.left_pane,
        PaneId::Left,
        state.tab_id,
        available_hosts.clone(),
        state.active_pane == PaneId::Left,
    );

    let right_pane = single_pane_view(
        &state.right_pane,
        PaneId::Right,
        state.tab_id,
        available_hosts,
        state.active_pane == PaneId::Right,
    );

    // Vertical divider between panes
    let divider = container(Space::with_width(0))
        .width(Length::Fixed(1.0))
        .height(Fill)
        .style(|_| container::Style {
            background: Some(THEME.border.into()),
            ..Default::default()
        });

    let content = row![left_pane, divider, right_pane];

    container(content)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}

/// Build a single pane view for the dual-pane browser
fn single_pane_view(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    available_hosts: Vec<(Uuid, String)>,
    is_active: bool,
) -> Element<'_, Message> {
    let header = pane_header(state, pane_id, tab_id, available_hosts);
    let file_list = pane_file_list(state, pane_id, tab_id);
    let footer = pane_footer(state, pane_id, tab_id);

    let border_color = if is_active { THEME.accent } else { THEME.background };

    let content = column![header, file_list, footer].spacing(0);

    // Wrap in a button to handle focus
    button(
        container(content)
            .width(Length::FillPortion(1))
            .height(Fill)
            .style(move |_theme| container::Style {
                border: iced::Border {
                    color: border_color,
                    width: if is_active { 2.0 } else { 0.0 },
                    radius: 0.0.into(),
                },
                ..Default::default()
            }),
    )
    .style(|_theme, _status| iced::widget::button::Style {
        background: None,
        text_color: THEME.text_primary,
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
    })
    .padding(0)
    .width(Length::FillPortion(1))
    .on_press(Message::DualSftpPaneFocus(tab_id, pane_id))
    .into()
}

/// Header with source dropdown, navigation buttons, and path bar
fn pane_header(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    available_hosts: Vec<(Uuid, String)>,
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
    let up_btn = button(icon_with_color(icons::ui::CHEVRON_LEFT, 14, THEME.text_primary))
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
        .on_press(Message::DualSftpPaneNavigateUp(tab_id, pane_id));

    let refresh_btn = button(icon_with_color(icons::ui::REFRESH, 14, THEME.text_primary))
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
        .on_press(Message::DualSftpPaneRefresh(tab_id, pane_id));

    // Path bar
    let path_bar = container(text(path_text).size(12).color(THEME.text_primary))
        .padding(Padding::new(6.0).left(8.0).right(8.0))
        .width(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.surface.into()),
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

    container(
        row![source_picker, up_btn, refresh_btn, path_bar]
            .spacing(6)
            .padding(6)
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

/// File list for a single pane
fn pane_file_list<'a>(
    state: &'a FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
) -> Element<'a, Message> {
    if state.loading {
        return container(text("Loading...").size(14).color(THEME.text_muted))
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
            column![
                icon_with_color(icons::files::FOLDER, 32, THEME.text_muted),
                text("Empty directory").size(14).color(THEME.text_muted),
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
                .size(11)
                .color(THEME.text_muted)
                .width(Length::FillPortion(4)),
            text("Size")
                .size(11)
                .color(THEME.text_muted)
                .width(Length::FillPortion(1)),
        ]
        .spacing(8)
        .padding(Padding::new(4.0).left(8.0).right(8.0)),
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
            pane_file_entry_row(entry, index, state.selected_index == Some(index), tab_id, pane_id)
        })
        .collect();

    let file_list = scrollable(Column::with_children(entries).spacing(0))
        .height(Fill)
        .width(Fill);

    column![headers, file_list].spacing(0).into()
}

/// Single file entry row for a pane
fn pane_file_entry_row(
    entry: &FileEntry,
    index: usize,
    is_selected: bool,
    tab_id: SessionId,
    pane_id: PaneId,
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
        THEME.accent
    } else {
        THEME.background
    };

    let text_color = if is_selected {
        THEME.background
    } else {
        THEME.text_primary
    };

    let icon_color = if is_selected {
        THEME.background
    } else if entry.is_dir {
        THEME.accent
    } else {
        THEME.text_secondary
    };

    let path = entry.path.clone();
    let is_dir = entry.is_dir;

    let name_row = row![
        icon_with_color(icon_data, 14, icon_color),
        text(name).size(12).color(text_color),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let content = row![
        container(name_row).width(Length::FillPortion(4)),
        text(size)
            .size(11)
            .color(if is_selected {
                text_color
            } else {
                THEME.text_secondary
            })
            .width(Length::FillPortion(1)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(
        container(content)
            .padding(Padding::new(4.0).left(8.0).right(8.0))
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
        Message::DualSftpPaneNavigate(tab_id, pane_id, path)
    } else {
        Message::DualSftpPaneSelect(tab_id, pane_id, index)
    })
    .into()
}

/// Footer with status for a pane
fn pane_footer<'a>(
    state: &'a FilePaneState,
    _pane_id: PaneId,
    _tab_id: SessionId,
) -> Element<'a, Message> {
    let item_count = state.entries.len();
    let status = if state.loading {
        "Loading...".to_string()
    } else {
        format!("{} items", item_count)
    };

    container(
        row![text(status).size(11).color(THEME.text_muted),]
            .spacing(8)
            .padding(6)
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
