//! SFTP file browser view - Dual-pane implementation

use std::collections::HashSet;
use std::path::PathBuf;

use iced::widget::{button, column, container, pick_list, row, scrollable, stack, text, text_input, Column, Space};
use iced::{Alignment, Element, Fill, Length, Padding, Point};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::Message;
use crate::sftp::{format_size, FileEntry, FileIcon, SortOrder};
use crate::theme::Theme;
use crate::widgets::mouse_area;

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
    pub selected_indices: HashSet<usize>,
    pub last_selected_index: Option<usize>, // For shift-click range selection
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
            selected_indices: HashSet::new(),
            last_selected_index: None,
            sort_order: SortOrder::default(),
            loading: true,
            error: None,
        }
    }


    pub fn set_entries(&mut self, mut entries: Vec<FileEntry>) {
        self.sort_order.sort(&mut entries);
        self.entries = entries;
        self.selected_indices.clear();
        self.last_selected_index = None;
        self.loading = false;
        self.error = None;
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// Select a single item (clear other selections)
    pub fn select(&mut self, index: usize) {
        self.selected_indices.clear();
        self.selected_indices.insert(index);
        self.last_selected_index = Some(index);
    }


    /// Check if an index is selected
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected_indices.contains(&index)
    }

    /// Get all selected entries
    pub fn selected_entries(&self) -> Vec<&FileEntry> {
        self.selected_indices
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .collect()
    }


}

/// Context menu action types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    Open,
    OpenWith,
    CopyToTarget,
    Rename,
    Delete,
    Refresh,
    NewFolder,
    EditPermissions,
}

/// State for the context menu
#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub visible: bool,
    pub position: Point,
    pub target_pane: PaneId,
}

impl Default for ContextMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            position: Point::ORIGIN,
            target_pane: PaneId::Left,
        }
    }
}

/// Type of SFTP dialog currently open
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SftpDialogType {
    NewFolder,
    Rename { original_name: String },
    Delete { entries: Vec<(String, PathBuf, bool)> }, // (name, path, is_dir)
    EditPermissions {
        name: String,
        path: PathBuf,
        permissions: PermissionBits,
    },
    OpenWith {
        name: String,
        path: PathBuf,
        is_remote: bool,
    },
}

/// Unix permission bits for a file or directory
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionBits {
    pub owner_read: bool,
    pub owner_write: bool,
    pub owner_execute: bool,
    pub group_read: bool,
    pub group_write: bool,
    pub group_execute: bool,
    pub other_read: bool,
    pub other_write: bool,
    pub other_execute: bool,
}

impl PermissionBits {
    /// Create from a Unix mode (e.g., 0o755)
    pub fn from_mode(mode: u32) -> Self {
        Self {
            owner_read: mode & 0o400 != 0,
            owner_write: mode & 0o200 != 0,
            owner_execute: mode & 0o100 != 0,
            group_read: mode & 0o040 != 0,
            group_write: mode & 0o020 != 0,
            group_execute: mode & 0o010 != 0,
            other_read: mode & 0o004 != 0,
            other_write: mode & 0o002 != 0,
            other_execute: mode & 0o001 != 0,
        }
    }

    /// Convert to Unix mode
    pub fn to_mode(&self) -> u32 {
        let mut mode = 0u32;
        if self.owner_read { mode |= 0o400; }
        if self.owner_write { mode |= 0o200; }
        if self.owner_execute { mode |= 0o100; }
        if self.group_read { mode |= 0o040; }
        if self.group_write { mode |= 0o020; }
        if self.group_execute { mode |= 0o010; }
        if self.other_read { mode |= 0o004; }
        if self.other_write { mode |= 0o002; }
        if self.other_execute { mode |= 0o001; }
        mode
    }

    /// Format as octal string (e.g., "755")
    pub fn as_octal_string(&self) -> String {
        format!("{:03o}", self.to_mode())
    }
}

impl Default for PermissionBits {
    fn default() -> Self {
        // Default to 644 (rw-r--r--)
        Self::from_mode(0o644)
    }
}

/// State for SFTP dialogs (New Folder, Rename, etc.)
#[derive(Debug, Clone)]
pub struct SftpDialogState {
    pub dialog_type: SftpDialogType,
    pub target_pane: PaneId,
    pub input_value: String,
    pub error: Option<String>,
}

impl SftpDialogState {
    pub fn new_folder(pane_id: PaneId) -> Self {
        Self {
            dialog_type: SftpDialogType::NewFolder,
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
        }
    }

    pub fn rename(pane_id: PaneId, original_name: String) -> Self {
        Self {
            dialog_type: SftpDialogType::Rename { original_name: original_name.clone() },
            target_pane: pane_id,
            input_value: original_name,
            error: None,
        }
    }

    pub fn delete(pane_id: PaneId, entries: Vec<(String, PathBuf, bool)>) -> Self {
        Self {
            dialog_type: SftpDialogType::Delete { entries },
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
        }
    }

    pub fn edit_permissions(pane_id: PaneId, name: String, path: PathBuf, permissions: PermissionBits) -> Self {
        Self {
            dialog_type: SftpDialogType::EditPermissions { name, path, permissions },
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
        }
    }

    pub fn open_with(pane_id: PaneId, name: String, path: PathBuf, is_remote: bool) -> Self {
        Self {
            dialog_type: SftpDialogType::OpenWith { name, path, is_remote },
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        match &self.dialog_type {
            SftpDialogType::Delete { entries } => !entries.is_empty(),
            SftpDialogType::EditPermissions { .. } => true, // Always valid
            SftpDialogType::OpenWith { .. } => !self.input_value.trim().is_empty(), // Need a command
            _ => {
                let name = self.input_value.trim();
                !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
            }
        }
    }

    /// Update a permission bit (for EditPermissions dialog)
    pub fn set_permission(&mut self, bit: PermissionBit, value: bool) {
        if let SftpDialogType::EditPermissions { permissions, .. } = &mut self.dialog_type {
            match bit {
                PermissionBit::OwnerRead => permissions.owner_read = value,
                PermissionBit::OwnerWrite => permissions.owner_write = value,
                PermissionBit::OwnerExecute => permissions.owner_execute = value,
                PermissionBit::GroupRead => permissions.group_read = value,
                PermissionBit::GroupWrite => permissions.group_write = value,
                PermissionBit::GroupExecute => permissions.group_execute = value,
                PermissionBit::OtherRead => permissions.other_read = value,
                PermissionBit::OtherWrite => permissions.other_write = value,
                PermissionBit::OtherExecute => permissions.other_execute = value,
            }
        }
    }
}

/// Individual permission bit identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionBit {
    OwnerRead,
    OwnerWrite,
    OwnerExecute,
    GroupRead,
    GroupWrite,
    GroupExecute,
    OtherRead,
    OtherWrite,
    OtherExecute,
}

/// State for the dual-pane SFTP browser
#[derive(Debug, Clone)]
pub struct DualPaneSftpState {
    pub tab_id: SessionId,
    pub left_pane: FilePaneState,
    pub right_pane: FilePaneState,
    pub active_pane: PaneId,
    pub context_menu: ContextMenuState,
    pub dialog: Option<SftpDialogState>,
}

impl DualPaneSftpState {
    pub fn new(tab_id: SessionId) -> Self {
        Self {
            tab_id,
            left_pane: FilePaneState::new_local(),
            right_pane: FilePaneState::new_local(),
            active_pane: PaneId::Left,
            context_menu: ContextMenuState::default(),
            dialog: None,
        }
    }

    pub fn show_context_menu(&mut self, pane_id: PaneId, x: f32, y: f32) {
        self.context_menu.visible = true;
        self.context_menu.position = Point::new(x, y);
        self.context_menu.target_pane = pane_id;
        self.active_pane = pane_id;
    }

    pub fn hide_context_menu(&mut self) {
        self.context_menu.visible = false;
    }

    pub fn show_new_folder_dialog(&mut self) {
        self.dialog = Some(SftpDialogState::new_folder(self.active_pane));
        self.hide_context_menu();
    }

    pub fn show_rename_dialog(&mut self, original_name: String) {
        self.dialog = Some(SftpDialogState::rename(self.active_pane, original_name));
        self.hide_context_menu();
    }

    pub fn show_delete_dialog(&mut self, entries: Vec<(String, PathBuf, bool)>) {
        self.dialog = Some(SftpDialogState::delete(self.active_pane, entries));
        self.hide_context_menu();
    }

    pub fn show_permissions_dialog(&mut self, name: String, path: PathBuf, permissions: PermissionBits) {
        self.dialog = Some(SftpDialogState::edit_permissions(self.active_pane, name, path, permissions));
        self.hide_context_menu();
    }

    pub fn show_open_with_dialog(&mut self, name: String, path: PathBuf, is_remote: bool) {
        self.dialog = Some(SftpDialogState::open_with(self.active_pane, name, path, is_remote));
        self.hide_context_menu();
    }

    pub fn close_dialog(&mut self) {
        self.dialog = None;
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
    theme: Theme,
) -> Element<'_, Message> {
    let left_pane = single_pane_view(
        &state.left_pane,
        PaneId::Left,
        state.tab_id,
        available_hosts.clone(),
        state.active_pane == PaneId::Left,
        theme,
    );

    let right_pane = single_pane_view(
        &state.right_pane,
        PaneId::Right,
        state.tab_id,
        available_hosts,
        state.active_pane == PaneId::Right,
        theme,
    );

    // Vertical divider between panes
    let divider = container(Space::with_width(0))
        .width(Length::Fixed(1.0))
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(theme.border.into()),
            ..Default::default()
        });

    let content = row![left_pane, divider, right_pane];

    let main = container(content)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        });

    // Overlay dialog if open (context menu is rendered at app level for correct positioning)
    if state.dialog.is_some() {
        stack![main, sftp_dialog_view(state, theme)].into()
    } else {
        main.into()
    }
}

/// Build the context menu overlay - should be rendered at app level for correct window positioning
pub fn sftp_context_menu_overlay(state: &DualPaneSftpState, theme: Theme) -> Element<'_, Message> {
    context_menu_view(state, theme)
}

/// Build a single pane view for the dual-pane browser
fn single_pane_view(
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
fn pane_header(
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
fn pane_file_list<'a>(
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
fn pane_file_entry_row(
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
fn pane_footer<'a>(
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

// ============================================================================
// Context Menu
// ============================================================================

/// Build a context menu item button
fn context_menu_item<'a>(
    label: &'static str,
    action: ContextMenuAction,
    tab_id: SessionId,
    enabled: bool,
    theme: Theme,
) -> Element<'a, Message> {
    let text_color = if enabled {
        theme.text_primary
    } else {
        theme.text_muted
    };

    let btn = button(
        text(label).size(13).color(text_color),
    )
    .padding([6, 16])
    .width(Length::Fixed(200.0))
    .style(move |_theme, status| {
        let bg = if enabled {
            match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => None,
            }
        } else {
            None
        };
        iced::widget::button::Style {
            background: bg,
            text_color,
            border: iced::Border::default(),
            ..Default::default()
        }
    });

    if enabled {
        btn.on_press(Message::DualSftpContextMenuAction(tab_id, action)).into()
    } else {
        btn.into()
    }
}

/// Build a divider for context menu
fn context_menu_divider<'a>(theme: Theme) -> Element<'a, Message> {
    container(Space::with_height(1))
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(theme.border.into()),
            ..Default::default()
        })
        .padding([4, 8])
        .into()
}

/// Build the context menu overlay
fn context_menu_view(
    state: &DualPaneSftpState,
    theme: Theme,
) -> Element<'_, Message> {
    if !state.context_menu.visible {
        return Space::new(0, 0).into();
    }

    let pane = state.pane(state.context_menu.target_pane);
    let selection_count = pane.selected_indices.len();
    let has_selection = selection_count > 0;
    let is_single = selection_count == 1;

    // Check if any selected item is a directory or parent
    let has_dir = pane.selected_entries().iter().any(|e| e.is_dir);
    let has_parent = pane.selected_entries().iter().any(|e| e.is_parent());
    let is_file_selected = has_selection && !has_dir && !has_parent;

    let tab_id = state.tab_id;

    // Build menu items based on selection context
    let mut items: Vec<Element<'_, Message>> = vec![];

    // Open / Open With (only for single file selection)
    if is_single && is_file_selected {
        items.push(context_menu_item("Open", ContextMenuAction::Open, tab_id, true, theme));
        items.push(context_menu_item("Open With...", ContextMenuAction::OpenWith, tab_id, true, theme));
        items.push(context_menu_divider(theme));
    }

    // Copy to target (for any selection except parent directory)
    if has_selection && !has_parent {
        items.push(context_menu_item("Copy to Target", ContextMenuAction::CopyToTarget, tab_id, true, theme));
    }

    // Rename (only for single non-parent selection)
    if is_single && !has_parent {
        items.push(context_menu_item("Rename", ContextMenuAction::Rename, tab_id, true, theme));
    }

    // Delete (for any selection except parent directory)
    if has_selection && !has_parent {
        items.push(context_menu_item("Delete", ContextMenuAction::Delete, tab_id, true, theme));
    }

    if has_selection && !has_parent {
        items.push(context_menu_divider(theme));
    }

    // Always available actions
    items.push(context_menu_item("Refresh", ContextMenuAction::Refresh, tab_id, true, theme));
    items.push(context_menu_item("New Folder", ContextMenuAction::NewFolder, tab_id, true, theme));

    // Edit Permissions (only for single file/folder selection, not parent)
    if is_single && !has_parent {
        items.push(context_menu_divider(theme));
        items.push(context_menu_item("Edit Permissions", ContextMenuAction::EditPermissions, tab_id, true, theme));
    }

    let menu = container(Column::with_children(items).spacing(2))
        .padding(4)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 6.0.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.3),
                offset: iced::Vector::new(2.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        });

    // Position the menu at the click location
    // Using container with absolute positioning
    let pos = state.context_menu.position;

    // Wrap in a clickable background to dismiss when clicking outside
    let background = mouse_area(
        container(Space::new(Fill, Fill))
            .width(Fill)
            .height(Fill)
    )
    .on_press(Message::DualSftpHideContextMenu(tab_id));

    // Position the menu using margins
    let positioned_menu = container(menu)
        .width(Fill)
        .height(Fill)
        .padding(Padding::new(0.0).top(pos.y).left(pos.x));

    stack![background, positioned_menu].into()
}

// ============================================================================
// SFTP Dialog (New Folder, Rename)
// ============================================================================

/// Build the SFTP dialog overlay (New Folder, Rename, or Delete)
fn sftp_dialog_view(state: &DualPaneSftpState, theme: Theme) -> Element<'_, Message> {
    let Some(ref dialog) = state.dialog else {
        return Space::new(0, 0).into();
    };

    let tab_id = state.tab_id;

    // Build dialog content based on type
    let dialog_content: Element<'_, Message> = match &dialog.dialog_type {
        SftpDialogType::Delete { entries } => {
            build_delete_dialog(tab_id, entries, dialog.error.as_deref(), theme)
        }
        SftpDialogType::EditPermissions { name, permissions, .. } => {
            build_permissions_dialog(tab_id, name, permissions, dialog.error.as_deref(), theme)
        }
        _ => {
            build_input_dialog(tab_id, dialog, theme)
        }
    };

    // Dialog box with styling
    let dialog_box = container(dialog_content)
        .style(move |_| container::Style {
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

/// Build input dialog for New Folder, Rename, or Open With
fn build_input_dialog(
    tab_id: SessionId,
    dialog: &SftpDialogState,
    theme: Theme,
) -> Element<'_, Message> {
    let (title, placeholder, submit_label, subtitle) = match &dialog.dialog_type {
        SftpDialogType::NewFolder => ("New Folder", "Folder name", "Create", None),
        SftpDialogType::Rename { .. } => ("Rename", "New name", "Rename", None),
        SftpDialogType::OpenWith { name, .. } => (
            "Open With",
            "Command (e.g., vim, code, nano)",
            "Open",
            Some(format!("Opening: {}", name)),
        ),
        SftpDialogType::Delete { .. } | SftpDialogType::EditPermissions { .. } => unreachable!(),
    };

    let title_text = text(title).size(18).color(theme.text_primary);

    let input_value = dialog.input_value.clone();
    let input = text_input(placeholder, &input_value)
        .on_input(move |value| Message::DualSftpDialogInputChanged(tab_id, value))
        .on_submit(Message::DualSftpDialogSubmit(tab_id))
        .padding([10, 12])
        .size(14)
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
            .size(12)
            .color(iced::Color::from_rgb8(220, 80, 80))
            .into()
    } else {
        Space::new(0, 0).into()
    };

    let cancel_btn = dialog_cancel_button(tab_id, theme);

    let is_valid = dialog.is_valid();
    let submit_btn = dialog_submit_button(tab_id, submit_label, is_valid, false, theme);

    let button_row = row![Space::with_width(Fill), cancel_btn, submit_btn].spacing(8);

    // Build subtitle element if present
    let subtitle_element: Element<'_, Message> = if let Some(subtitle) = subtitle {
        text(subtitle)
            .size(13)
            .color(theme.text_muted)
            .into()
    } else {
        Space::new(0, 0).into()
    };

    column![
        title_text,
        subtitle_element,
        Space::with_height(12),
        input,
        error_text,
        Space::with_height(16),
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
) -> Element<'a, Message> {
    let title_text = text("Delete").size(18).color(theme.text_primary);

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
        format!("Delete {} items? Folders will be deleted with all their contents.", count)
    } else {
        format!("Delete {} items?", count)
    };

    let warning_text = text(warning_msg)
        .size(14)
        .color(theme.text_secondary);

    // List the items to be deleted (show up to 5)
    let items_list: Element<'_, Message> = if count <= 5 {
        let items: Vec<Element<'_, Message>> = entries
            .iter()
            .map(|(name, _, is_dir)| {
                let icon_data = if *is_dir {
                    crate::icons::files::FOLDER
                } else {
                    crate::icons::files::FILE
                };
                let icon = icon_with_color(icon_data, 14, theme.text_muted);
                row![icon, text(name).size(13).color(theme.text_secondary)]
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
                    crate::icons::files::FOLDER
                } else {
                    crate::icons::files::FILE
                };
                let icon = icon_with_color(icon_data, 14, theme.text_muted);
                row![icon, text(name).size(13).color(theme.text_secondary)]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .into()
            })
            .collect();

        items.push(
            text(format!("... and {} more", count - 3))
                .size(13)
                .color(theme.text_muted)
                .into()
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
        icon_with_color(crate::icons::ui::ALERT_TRIANGLE, 16, iced::Color::from_rgb8(220, 160, 60)),
        text("This action cannot be undone.")
            .size(12)
            .color(iced::Color::from_rgb8(220, 160, 60))
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Error message if any
    let error_text: Element<'_, Message> = if let Some(error) = error {
        text(error)
            .size(12)
            .color(iced::Color::from_rgb8(220, 80, 80))
            .into()
    } else {
        Space::new(0, 0).into()
    };

    let cancel_btn = dialog_cancel_button(tab_id, theme);
    let delete_btn = dialog_submit_button(tab_id, "Delete", true, true, theme);

    let button_row = row![Space::with_width(Fill), cancel_btn, delete_btn].spacing(8);

    column![
        title_text,
        Space::with_height(12),
        warning_text,
        Space::with_height(12),
        items_container,
        Space::with_height(12),
        permanent_warning,
        error_text,
        Space::with_height(16),
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
) -> Element<'a, Message> {
    let title_text = text("Edit Permissions").size(18).color(theme.text_primary);

    // File name display
    let file_info = row![
        icon_with_color(crate::icons::files::FILE, 16, theme.text_muted),
        text(name).size(14).color(theme.text_secondary)
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Current mode display
    let mode_text = text(format!("Mode: {}", permissions.as_octal_string()))
        .size(13)
        .color(theme.text_muted);

    // Permission grid headers
    let header_row = row![
        Space::with_width(Length::Fixed(80.0)),
        text("Read").size(12).color(theme.text_muted).width(Length::Fixed(60.0)),
        text("Write").size(12).color(theme.text_muted).width(Length::Fixed(60.0)),
        text("Execute").size(12).color(theme.text_muted).width(Length::Fixed(60.0)),
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
    );

    // Permission grid
    let permission_grid = container(
        column![header_row, owner_row, group_row, other_row].spacing(8)
    )
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
            .size(12)
            .color(iced::Color::from_rgb8(220, 80, 80))
            .into()
    } else {
        Space::new(0, 0).into()
    };

    let cancel_btn = dialog_cancel_button(tab_id, theme);
    let apply_btn = dialog_submit_button(tab_id, "Apply", true, false, theme);

    let button_row = row![Space::with_width(Fill), cancel_btn, apply_btn].spacing(8);

    column![
        title_text,
        Space::with_height(12),
        file_info,
        mode_text,
        Space::with_height(12),
        permission_grid,
        error_text,
        Space::with_height(16),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(350.0))
    .into()
}

/// Create a row of permission checkboxes for owner/group/other
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
) -> Element<'a, Message> {
    row![
        text(label).size(13).color(theme.text_primary).width(Length::Fixed(80.0)),
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
    let icon = if checked { "✓" } else { "" };
    let bg_color = if checked { theme.accent } else { theme.background };
    let text_color = if checked { theme.background } else { theme.text_muted };

    button(
        container(text(icon).size(12).color(text_color))
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
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
            text_color,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    })
    .on_press(Message::DualSftpPermissionToggle(tab_id, bit, !checked))
}

/// Create a cancel button for dialogs
fn dialog_cancel_button(tab_id: SessionId, theme: Theme) -> iced::widget::Button<'static, Message> {
    button(text("Cancel").size(13).color(theme.text_primary))
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
        .on_press(Message::DualSftpDialogCancel(tab_id))
}

/// Create a submit button for dialogs
fn dialog_submit_button(
    tab_id: SessionId,
    label: &str,
    is_valid: bool,
    is_destructive: bool,
    theme: Theme,
) -> iced::widget::Button<'static, Message> {
    let (normal_color, hover_color) = if is_destructive {
        (
            iced::Color::from_rgb8(180, 60, 60),
            iced::Color::from_rgb8(200, 70, 70),
        )
    } else {
        (theme.accent, iced::Color::from_rgb8(0, 100, 180))
    };

    let btn = button(text(label.to_string()).size(13).color(if is_valid { theme.background } else { theme.text_muted }))
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
                text_color: if is_valid { theme.background } else { theme.text_muted },
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

    if is_valid {
        btn.on_press(Message::DualSftpDialogSubmit(tab_id))
    } else {
        btn
    }
}
