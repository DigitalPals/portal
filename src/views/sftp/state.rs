//! SFTP state management
//!
//! This module contains state structs for the SFTP dual-pane browser.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::Id;

use crate::message::SessionId;
use crate::sftp::{FileEntry, SortOrder, is_safe_sftp_entry_name};

use super::types::{
    ColumnWidths, ContextMenuState, PaneId, PaneSource, PermissionBit, PermissionBits, SftpColumn,
    SftpDialogType,
};

const SFTP_VISIBLE_REBUILD_LOG_ENTRY_THRESHOLD: usize = 1_000;
const SFTP_VISIBLE_REBUILD_LOG_DURATION_THRESHOLD: Duration = Duration::from_millis(8);

/// State for a single file browser pane (can be local or remote)
#[derive(Debug, Clone)]
pub struct FilePaneState {
    pub source: PaneSource,
    pub current_path: PathBuf,
    pub entries: Vec<FileEntry>,
    visible_entry_indices: Vec<usize>,
    pub selected_indices: HashSet<usize>,
    pub last_selected_index: Option<usize>, // For shift-click range selection
    pub sort_order: SortOrder,
    pub loading: bool,
    pub error: Option<String>,
    pub show_hidden: bool,
    pub filter_text: String,
    pub scrollable_id: Id,
    pub actions_menu_open: bool,
    pub column_widths: ColumnWidths,
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
            visible_entry_indices: Vec::new(),
            selected_indices: HashSet::new(),
            last_selected_index: None,
            sort_order: SortOrder::default(),
            loading: true,
            error: None,
            show_hidden: false,
            filter_text: String::new(),
            scrollable_id: Id::unique(),
            actions_menu_open: false,
            column_widths: ColumnWidths::default(),
        }
    }

    pub fn new_local_with_column_widths(column_widths: ColumnWidths) -> Self {
        let home_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            source: PaneSource::Local,
            current_path: home_dir,
            entries: Vec::new(),
            visible_entry_indices: Vec::new(),
            selected_indices: HashSet::new(),
            last_selected_index: None,
            sort_order: SortOrder::default(),
            loading: true,
            error: None,
            show_hidden: false,
            filter_text: String::new(),
            scrollable_id: Id::unique(),
            actions_menu_open: false,
            column_widths,
        }
    }

    pub fn set_entries(&mut self, mut entries: Vec<FileEntry>) {
        self.sort_order.sort(&mut entries);
        self.entries = entries;
        self.rebuild_visible_entries();
        self.clear_selection();
        self.loading = false;
        self.error = None;
    }

    pub fn clear_entries(&mut self) {
        self.entries.clear();
        self.visible_entry_indices.clear();
        self.clear_selection();
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.clear_entries();
        self.loading = false;
    }

    /// Select a single item (clear other selections)
    pub fn select(&mut self, index: usize) {
        self.selected_indices.clear();
        if index < self.entries.len() {
            self.selected_indices.insert(index);
            self.last_selected_index = Some(index);
        } else {
            self.last_selected_index = None;
        }
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

    pub fn set_filter_text(&mut self, filter_text: String) {
        if self.filter_text == filter_text {
            return;
        }
        self.filter_text = filter_text;
        self.rebuild_visible_entries();
        self.clear_selection();
    }

    pub fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.rebuild_visible_entries();
        self.clear_selection();
    }

    pub fn sort_by_column(&mut self, column: SftpColumn) {
        self.sort_order = self.sort_order.for_column_next(column);
        self.sort_order.sort(&mut self.entries);
        self.rebuild_visible_entries();
        self.clear_selection();
    }

    pub fn visible_entry_count(&self) -> usize {
        self.visible_entry_indices.len()
    }

    pub fn visible_entries_iter(&self) -> impl Iterator<Item = (usize, &FileEntry)> + '_ {
        self.visible_entry_indices
            .iter()
            .filter_map(|&index| self.entries.get(index).map(|entry| (index, entry)))
    }

    /// Get cached filtered entries with their original indices.
    pub fn visible_entries(&self) -> Vec<(usize, &FileEntry)> {
        self.visible_entries_iter().collect()
    }

    fn rebuild_visible_entries(&mut self) {
        let started = Instant::now();
        let filter_text = self.filter_text.trim().to_lowercase();
        let show_hidden = self.show_hidden;
        self.visible_entry_indices.clear();
        self.visible_entry_indices
            .extend(
                self.entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        if Self::entry_is_visible(entry, show_hidden, &filter_text) {
                            Some(index)
                        } else {
                            None
                        }
                    }),
            );
        let elapsed = started.elapsed();
        if self.entries.len() >= SFTP_VISIBLE_REBUILD_LOG_ENTRY_THRESHOLD
            || elapsed >= SFTP_VISIBLE_REBUILD_LOG_DURATION_THRESHOLD
        {
            tracing::debug!(
                entries = self.entries.len(),
                visible = self.visible_entry_indices.len(),
                show_hidden,
                filter_len = filter_text.len(),
                elapsed_ms = elapsed.as_millis(),
                "rebuilt SFTP visible-entry cache"
            );
        }
    }

    fn entry_is_visible(entry: &FileEntry, show_hidden: bool, filter_text: &str) -> bool {
        if entry.is_parent() {
            return true;
        }
        if !show_hidden && entry.name.starts_with('.') {
            return false;
        }
        filter_text.is_empty() || entry.name.to_lowercase().contains(filter_text)
    }

    fn clear_selection(&mut self) {
        self.selected_indices.clear();
        self.last_selected_index = None;
    }
}

/// State for SFTP dialogs (New Folder, Rename, etc.)
#[derive(Debug, Clone)]
pub struct SftpDialogState {
    pub dialog_type: SftpDialogType,
    pub target_pane: PaneId,
    pub input_value: String,
    pub error: Option<String>,
    pub delete_hold_started: Option<Instant>,
}

impl SftpDialogState {
    pub fn new_folder(pane_id: PaneId) -> Self {
        Self {
            dialog_type: SftpDialogType::NewFolder,
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
            delete_hold_started: None,
        }
    }

    pub fn rename(pane_id: PaneId, original_name: String) -> Self {
        Self {
            dialog_type: SftpDialogType::Rename {
                original_name: original_name.clone(),
            },
            target_pane: pane_id,
            input_value: original_name,
            error: None,
            delete_hold_started: None,
        }
    }

    pub fn delete(pane_id: PaneId, entries: Vec<(String, PathBuf, bool)>) -> Self {
        Self {
            dialog_type: SftpDialogType::Delete { entries },
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
            delete_hold_started: None,
        }
    }

    pub fn edit_permissions(
        pane_id: PaneId,
        name: String,
        path: PathBuf,
        permissions: PermissionBits,
    ) -> Self {
        Self {
            dialog_type: SftpDialogType::EditPermissions {
                name,
                path,
                permissions,
            },
            target_pane: pane_id,
            input_value: String::new(),
            error: None,
            delete_hold_started: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        match &self.dialog_type {
            SftpDialogType::Delete { entries } => !entries.is_empty(),
            SftpDialogType::EditPermissions { .. } => true, // Always valid
            _ => {
                let name = self.input_value.trim();
                is_safe_sftp_entry_name(name)
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

/// State for active column resize drag operation
#[derive(Debug, Clone)]
pub struct ColumnResizeDrag {
    /// Which pane is being resized
    pub pane_id: PaneId,
    /// Which column's right edge is being dragged
    pub column: SftpColumn,
    /// Starting X position when drag began
    pub start_x: f32,
    /// Original column widths when drag started
    pub original_widths: ColumnWidths,
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
    pub column_resize_drag: Option<ColumnResizeDrag>,
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
            column_resize_drag: None,
        }
    }

    pub fn new_with_column_widths(tab_id: SessionId, column_widths: ColumnWidths) -> Self {
        Self {
            tab_id,
            left_pane: FilePaneState::new_local_with_column_widths(column_widths.clone()),
            right_pane: FilePaneState::new_local_with_column_widths(column_widths),
            active_pane: PaneId::Left,
            context_menu: ContextMenuState::default(),
            dialog: None,
            column_resize_drag: None,
        }
    }

    pub fn show_context_menu(&mut self, pane_id: PaneId, x: f32, y: f32) {
        self.context_menu.visible = true;
        self.context_menu.position = iced::Point::new(x, y);
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

    pub fn show_permissions_dialog(
        &mut self,
        name: String,
        path: PathBuf,
        permissions: PermissionBits,
    ) {
        self.dialog = Some(SftpDialogState::edit_permissions(
            self.active_pane,
            name,
            path,
            permissions,
        ));
        self.hide_context_menu();
    }

    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }

    /// Close actions menus in both panes
    pub fn close_actions_menus(&mut self) {
        self.left_pane.actions_menu_open = false;
        self.right_pane.actions_menu_open = false;
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

    /// Check if the tab is pristine (no remote connection, no navigation from home)
    pub fn is_pristine(&self) -> bool {
        Self::pane_is_pristine(&self.left_pane) && Self::pane_is_pristine(&self.right_pane)
    }

    fn pane_is_pristine(pane: &FilePaneState) -> bool {
        // Must be local (no remote connection)
        if !matches!(pane.source, PaneSource::Local) {
            return false;
        }
        // Must be at home directory (no navigation)
        let home_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        pane.current_path == home_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir: false,
            is_symlink: false,
            size: 0,
            modified: Some(Utc::now()),
        }
    }

    #[test]
    fn visible_entries_keeps_parent_and_filters_hidden() {
        let mut state = FilePaneState::new_local();
        state.set_entries(vec![entry(".."), entry(".secret"), entry("notes.txt")]);

        let visible: Vec<_> = state
            .visible_entries()
            .into_iter()
            .map(|(_, e)| e.name.clone())
            .collect();

        assert_eq!(visible, vec!["..", "notes.txt"]);
    }

    #[test]
    fn visible_entries_filters_by_text_case_insensitive() {
        let mut state = FilePaneState::new_local();
        state.set_entries(vec![
            entry("alpha.txt"),
            entry("Beta.md"),
            entry("gamma.log"),
        ]);
        state.set_filter_text(" BE ".to_string());

        let visible: Vec<_> = state
            .visible_entries()
            .into_iter()
            .map(|(_, e)| e.name.clone())
            .collect();

        assert_eq!(visible, vec!["Beta.md"]);
    }

    #[test]
    fn visible_entry_cache_updates_when_hidden_toggle_changes() {
        let mut state = FilePaneState::new_local();
        state.set_entries(vec![entry(".secret"), entry("notes.txt")]);

        assert_eq!(state.visible_entry_count(), 1);

        state.toggle_show_hidden();

        assert_eq!(state.visible_entry_count(), 2);
    }

    #[test]
    fn set_error_clears_stale_entries_and_selection() {
        let mut state = FilePaneState::new_local();
        state.set_entries(vec![entry("one"), entry("two")]);
        state.select(1);

        state.set_error("failed".to_string());

        assert!(state.entries.is_empty());
        assert_eq!(state.visible_entry_count(), 0);
        assert!(state.selected_indices.is_empty());
        assert_eq!(state.last_selected_index, None);
        assert!(!state.loading);
    }

    #[test]
    fn select_replaces_previous_selection() {
        let mut state = FilePaneState::new_local();
        state.set_entries(vec![entry("one"), entry("two"), entry("three")]);
        state.select(0);
        state.select(2);

        assert!(state.is_selected(2));
        assert!(!state.is_selected(0));
        assert_eq!(state.last_selected_index, Some(2));
    }

    #[test]
    fn select_ignores_out_of_range_index() {
        let mut state = FilePaneState::new_local();
        state.set_entries(vec![entry("one")]);

        state.select(9);

        assert!(state.selected_indices.is_empty());
        assert_eq!(state.last_selected_index, None);
    }

    #[test]
    fn dialog_is_valid_for_rename_rules() {
        let mut dialog = SftpDialogState::rename(PaneId::Left, "old".to_string());
        assert!(dialog.is_valid());

        dialog.input_value = "".to_string();
        assert!(!dialog.is_valid());

        dialog.input_value = "bad/name".to_string();
        assert!(!dialog.is_valid());

        dialog.input_value = r"bad\name".to_string();
        assert!(!dialog.is_valid());

        dialog.input_value = ".".to_string();
        assert!(!dialog.is_valid());

        dialog.input_value = "..".to_string();
        assert!(!dialog.is_valid());

        dialog.input_value = "bad\0name".to_string();
        assert!(!dialog.is_valid());
    }

    #[test]
    fn dialog_is_valid_for_delete() {
        let empty_delete = SftpDialogState::delete(PaneId::Left, Vec::new());
        assert!(!empty_delete.is_valid());

        let delete = SftpDialogState::delete(
            PaneId::Left,
            vec![("notes.txt".to_string(), PathBuf::from("notes.txt"), false)],
        );
        assert!(delete.is_valid());
    }
}
