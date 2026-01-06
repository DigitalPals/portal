//! Dialog manager for application dialogs
//!
//! Consolidates dialog state into a single enum, ensuring only one dialog
//! can be open at a time and simplifying state management.

use crate::views::dialogs::host_dialog::HostDialogState;
use crate::views::dialogs::host_key_dialog::HostKeyDialogState;
use crate::views::dialogs::snippets_dialog::SnippetsDialogState;

/// The currently active dialog, if any
pub enum ActiveDialog {
    /// No dialog is open
    None,
    /// Host add/edit dialog
    Host(HostDialogState),
    /// Snippets management dialog
    Snippets(SnippetsDialogState),
    /// SSH host key verification dialog
    HostKey(HostKeyDialogState),
}

impl Default for ActiveDialog {
    fn default() -> Self {
        ActiveDialog::None
    }
}

/// Manages the active dialog state
#[derive(Default)]
pub struct DialogManager {
    active: ActiveDialog,
}

impl DialogManager {
    /// Create a new dialog manager with no active dialog
    pub fn new() -> Self {
        Self {
            active: ActiveDialog::None,
        }
    }

    /// Check if any dialog is open
    pub fn is_open(&self) -> bool {
        !matches!(self.active, ActiveDialog::None)
    }

    /// Close any open dialog
    pub fn close(&mut self) {
        self.active = ActiveDialog::None;
    }

    /// Get a reference to the active dialog
    pub fn active(&self) -> &ActiveDialog {
        &self.active
    }

    // ---- Host dialog operations ----

    /// Open the host dialog with the given state
    pub fn open_host(&mut self, state: HostDialogState) {
        self.active = ActiveDialog::Host(state);
    }

    /// Get host dialog state if it's the active dialog
    pub fn host(&self) -> Option<&HostDialogState> {
        match &self.active {
            ActiveDialog::Host(state) => Some(state),
            _ => None,
        }
    }

    /// Get mutable host dialog state if it's the active dialog
    pub fn host_mut(&mut self) -> Option<&mut HostDialogState> {
        match &mut self.active {
            ActiveDialog::Host(state) => Some(state),
            _ => None,
        }
    }

    // ---- Snippets dialog operations ----

    /// Open the snippets dialog with the given state
    pub fn open_snippets(&mut self, state: SnippetsDialogState) {
        self.active = ActiveDialog::Snippets(state);
    }

    /// Get mutable snippets dialog state if it's the active dialog
    pub fn snippets_mut(&mut self) -> Option<&mut SnippetsDialogState> {
        match &mut self.active {
            ActiveDialog::Snippets(state) => Some(state),
            _ => None,
        }
    }

    // ---- Host key dialog operations ----

    /// Open the host key dialog with the given state
    pub fn open_host_key(&mut self, state: HostKeyDialogState) {
        self.active = ActiveDialog::HostKey(state);
    }

    /// Get mutable host key dialog state if it's the active dialog
    pub fn host_key_mut(&mut self) -> Option<&mut HostKeyDialogState> {
        match &mut self.active {
            ActiveDialog::HostKey(state) => Some(state),
            _ => None,
        }
    }
}
