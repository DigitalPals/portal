//! Dialog manager for application dialogs
//!
//! Consolidates dialog state into a single enum, ensuring only one dialog
//! can be open at a time and simplifying state management.

use crate::views::dialogs::about_dialog::AboutDialogState;
use crate::views::dialogs::connecting_dialog::ConnectingDialogState;
use crate::views::dialogs::host_dialog::HostDialogState;
use crate::views::dialogs::host_key_dialog::HostKeyDialogState;
use crate::views::dialogs::passphrase_dialog::PassphraseDialogState;
use crate::views::dialogs::password_dialog::PasswordDialogState;
use crate::views::dialogs::quick_connect_dialog::QuickConnectDialogState;

/// The currently active dialog, if any
#[derive(Default)]
pub enum ActiveDialog {
    /// No dialog is open
    #[default]
    None,
    /// Host add/edit dialog
    Host(HostDialogState),
    /// SSH host key verification dialog
    HostKey(HostKeyDialogState),
    /// About dialog
    About(AboutDialogState),
    /// Password prompt dialog for SSH/SFTP password authentication
    PasswordPrompt(PasswordDialogState),
    /// Passphrase prompt dialog for SSH key authentication
    PassphrasePrompt(PassphraseDialogState),
    /// Quick connect dialog for ad-hoc connections
    QuickConnect(QuickConnectDialogState),
    /// Connecting progress dialog
    Connecting(ConnectingDialogState),
}

/// Manages the active dialog state
#[derive(Default)]
pub struct DialogManager {
    active: ActiveDialog,
    /// Current focused field index in host dialog (for Tab navigation)
    pub host_dialog_focus: usize,
}

impl DialogManager {
    /// Create a new dialog manager with no active dialog
    pub fn new() -> Self {
        Self {
            active: ActiveDialog::None,
            host_dialog_focus: 0,
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
        self.host_dialog_focus = 0; // Reset focus to first field
    }

    /// Get mutable host dialog state if it's the active dialog
    pub fn host_mut(&mut self) -> Option<&mut HostDialogState> {
        match &mut self.active {
            ActiveDialog::Host(state) => Some(state),
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

    // ---- About dialog operations ----

    /// Open the About dialog
    pub fn open_about(&mut self) {
        self.active = ActiveDialog::About(AboutDialogState::new());
    }

    // ---- Password dialog operations ----

    /// Open the password dialog with the given state
    pub fn open_password(&mut self, state: PasswordDialogState) {
        self.active = ActiveDialog::PasswordPrompt(state);
    }

    /// Get password dialog state if it's the active dialog
    pub fn password(&self) -> Option<&PasswordDialogState> {
        match &self.active {
            ActiveDialog::PasswordPrompt(state) => Some(state),
            _ => None,
        }
    }

    /// Get mutable password dialog state if it's the active dialog
    pub fn password_mut(&mut self) -> Option<&mut PasswordDialogState> {
        match &mut self.active {
            ActiveDialog::PasswordPrompt(state) => Some(state),
            _ => None,
        }
    }

    // ---- Passphrase dialog operations ----

    /// Open the passphrase dialog with the given state
    pub fn open_passphrase(&mut self, state: PassphraseDialogState) {
        self.active = ActiveDialog::PassphrasePrompt(state);
    }

    /// Get passphrase dialog state if it's the active dialog
    pub fn passphrase(&self) -> Option<&PassphraseDialogState> {
        match &self.active {
            ActiveDialog::PassphrasePrompt(state) => Some(state),
            _ => None,
        }
    }

    /// Get mutable passphrase dialog state if it's the active dialog
    pub fn passphrase_mut(&mut self) -> Option<&mut PassphraseDialogState> {
        match &mut self.active {
            ActiveDialog::PassphrasePrompt(state) => Some(state),
            _ => None,
        }
    }

    // ---- Quick connect dialog operations ----

    /// Open the quick connect dialog
    pub fn open_quick_connect(&mut self) {
        self.active = ActiveDialog::QuickConnect(QuickConnectDialogState::new());
    }

    // ---- Connecting dialog operations ----

    /// Open the connecting dialog
    pub fn open_connecting(&mut self, host_name: String, protocol: &str) {
        self.active = ActiveDialog::Connecting(ConnectingDialogState::new(host_name, protocol));
    }

    /// Close the dialog only if it's the connecting dialog
    pub fn close_connecting(&mut self) {
        if matches!(self.active, ActiveDialog::Connecting(_)) {
            self.active = ActiveDialog::None;
        }
    }

    /// Get mutable quick connect dialog state if it's the active dialog
    pub fn quick_connect_mut(&mut self) -> Option<&mut QuickConnectDialogState> {
        match &mut self.active {
            ActiveDialog::QuickConnect(state) => Some(state),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_has_no_dialog() {
        let manager = DialogManager::new();
        assert!(!manager.is_open());
        assert!(matches!(manager.active(), ActiveDialog::None));
    }

    #[test]
    fn test_open_about_dialog() {
        let mut manager = DialogManager::new();
        manager.open_about();
        assert!(manager.is_open());
        assert!(matches!(manager.active(), ActiveDialog::About(_)));
    }

    #[test]
    fn test_close_clears_dialog() {
        let mut manager = DialogManager::new();
        manager.open_about();
        assert!(manager.is_open());

        manager.close();
        assert!(!manager.is_open());
        assert!(matches!(manager.active(), ActiveDialog::None));
    }

    #[test]
    fn test_single_dialog_constraint() {
        let mut manager = DialogManager::new();

        // Open about dialog
        manager.open_about();
        assert!(matches!(manager.active(), ActiveDialog::About(_)));

        // Opening another dialog replaces the first
        manager.open_about();
        assert!(matches!(manager.active(), ActiveDialog::About(_)));

        // Close and verify
        manager.close();
        assert!(!manager.is_open());
    }
}
