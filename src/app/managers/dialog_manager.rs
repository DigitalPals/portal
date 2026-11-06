//! Dialog manager for application dialogs
//!
//! Consolidates dialog state into a single enum, ensuring only one dialog
//! can be open at a time and simplifying state management.

use std::collections::VecDeque;
use uuid::Uuid;

use crate::views::dialogs::about_dialog::AboutDialogState;
use crate::views::dialogs::auth_prompt_dialog::AuthPromptDialogState;
use crate::views::dialogs::connecting_dialog::ConnectingDialogState;
use crate::views::dialogs::host_dialog::HostDialogState;
use crate::views::dialogs::host_key_dialog::HostKeyDialogState;
use crate::views::dialogs::passphrase_dialog::PassphraseDialogState;
use crate::views::dialogs::password_dialog::PasswordDialogState;
use crate::views::dialogs::quick_connect_dialog::QuickConnectDialogState;
use crate::views::dialogs::session_choice_dialog::SessionChoiceDialogState;
use crate::views::dialogs::vnc_cleartext_dialog::VncCleartextDialogState;

/// The currently active dialog, if any
#[derive(Default)]
#[allow(clippy::large_enum_variant)]
pub enum ActiveDialog {
    /// No dialog is open
    #[default]
    None,
    /// Host add/edit dialog
    Host(HostDialogState),
    /// SSH host key verification dialog
    HostKey(HostKeyDialogState),
    /// Keyboard-interactive authentication prompt dialog
    AuthPrompt(AuthPromptDialogState),
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
    /// Existing-session picker for duplicate host clicks
    SessionChoice(SessionChoiceDialogState),
    /// Warning before connecting VNC to a non-private target without a tunnel
    VncCleartextWarning(VncCleartextDialogState),
    /// Confirmation before closing a file with unsaved edits
    UnsavedFile(Uuid),
    /// Portal Hub onboarding wizard
    PortalHubOnboarding,
    /// Portal Hub sync conflict resolver
    PortalHubConflicts,
    /// Confirmation before disabling a Portal Hub sync service
    PortalHubDisableSync(crate::hub::sync::PortalHubSyncService),
}

/// Manages the active dialog state
#[derive(Default)]
pub struct DialogManager {
    active: ActiveDialog,
    owner: Option<Uuid>,
    pending: VecDeque<(ActiveDialog, Option<Uuid>)>,
    /// Current focused field index in host dialog (for Tab navigation)
    pub host_dialog_focus: usize,
}

impl DialogManager {
    /// Create a new dialog manager with no active dialog
    pub fn new() -> Self {
        Self {
            active: ActiveDialog::None,
            host_dialog_focus: 0,
            ..Self::default()
        }
    }

    /// Check if any dialog is open
    pub fn is_open(&self) -> bool {
        !matches!(self.active, ActiveDialog::None)
    }

    /// Close any open dialog
    pub fn close(&mut self) {
        let (dialog, owner) = self.pending.pop_front().unwrap_or_default();
        self.active = dialog;
        self.owner = owner;
    }

    fn is_prompt(dialog: &ActiveDialog) -> bool {
        matches!(
            dialog,
            ActiveDialog::AuthPrompt(_)
                | ActiveDialog::HostKey(_)
                | ActiveDialog::PasswordPrompt(_)
                | ActiveDialog::PassphrasePrompt(_)
                | ActiveDialog::VncCleartextWarning(_)
                | ActiveDialog::SessionChoice(_)
                | ActiveDialog::UnsavedFile(_)
        )
    }

    fn enqueue(&mut self, dialog: ActiveDialog, owner: Option<Uuid>) {
        // Prompts are FIFO ahead of suspended screens; responders remain alive.
        let index = if Self::is_prompt(&dialog) {
            self.pending
                .iter()
                .position(|(d, _)| !Self::is_prompt(d))
                .unwrap_or(self.pending.len())
        } else {
            self.pending.len()
        };
        if matches!(dialog, ActiveDialog::PortalHubOnboarding)
            && self
                .pending
                .iter()
                .any(|(d, _)| matches!(d, ActiveDialog::PortalHubOnboarding))
        {
            return;
        }
        self.pending.insert(index, (dialog, owner));
    }

    fn present(&mut self, dialog: ActiveDialog, owner: Option<Uuid>) {
        if Self::is_prompt(&self.active) {
            self.enqueue(dialog, owner);
            return;
        }
        if Self::is_prompt(&dialog)
            && !matches!(
                self.active,
                ActiveDialog::None | ActiveDialog::Connecting(_)
            )
        {
            let previous = std::mem::take(&mut self.active);
            let previous_owner = self.owner.take();
            self.enqueue(previous, previous_owner);
        }
        self.active = dialog;
        self.owner = owner;
    }

    pub fn open_management_auth(&mut self, operation: Uuid, state: AuthPromptDialogState) {
        self.present(ActiveDialog::AuthPrompt(state), Some(operation));
    }

    pub fn open_management_host_key(&mut self, operation: Uuid, state: HostKeyDialogState) {
        self.present(ActiveDialog::HostKey(state), Some(operation));
    }

    /// Completion/cancellation retires only this operation's prompts.
    pub fn finish_management(&mut self, operation: Uuid) {
        self.pending.retain(|(_, owner)| *owner != Some(operation));
        if self.owner == Some(operation) {
            self.close();
        }
    }

    pub fn close_onboarding(&mut self, operation: Uuid) {
        self.pending
            .retain(|(dialog, _)| !matches!(dialog, ActiveDialog::PortalHubOnboarding));
        self.finish_management(operation);
        if matches!(self.active, ActiveDialog::PortalHubOnboarding) {
            self.close();
        }
    }

    /// Get a reference to the active dialog
    pub fn active(&self) -> &ActiveDialog {
        &self.active
    }

    pub fn open_unsaved_file(&mut self, viewer_id: Uuid) {
        if matches!(self.active, ActiveDialog::UnsavedFile(id) if id == viewer_id)
            || self.pending.iter().any(
                |(dialog, _)| matches!(dialog, ActiveDialog::UnsavedFile(id) if *id == viewer_id),
            )
        {
            return;
        }
        self.present(ActiveDialog::UnsavedFile(viewer_id), None);
    }

    pub fn close_unsaved_file(&mut self, viewer_id: Uuid) {
        self.pending.retain(
            |(dialog, _)| !matches!(dialog, ActiveDialog::UnsavedFile(id) if *id == viewer_id),
        );
        if matches!(self.active, ActiveDialog::UnsavedFile(id) if id == viewer_id) {
            self.close();
        }
    }

    // ---- Host dialog operations ----

    /// Open the host dialog with the given state
    pub fn open_host(&mut self, state: HostDialogState) {
        self.present(ActiveDialog::Host(state), None);
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
        self.present(ActiveDialog::HostKey(state), None);
    }

    /// Get mutable host key dialog state if it's the active dialog
    pub fn host_key_mut(&mut self) -> Option<&mut HostKeyDialogState> {
        match &mut self.active {
            ActiveDialog::HostKey(state) => Some(state),
            _ => None,
        }
    }

    // ---- Auth prompt dialog operations ----

    /// Open the keyboard-interactive auth prompt dialog
    pub fn open_auth_prompt(&mut self, state: AuthPromptDialogState) {
        self.present(ActiveDialog::AuthPrompt(state), None);
    }

    /// Get mutable auth prompt dialog state if it's the active dialog
    pub fn auth_prompt_mut(&mut self) -> Option<&mut AuthPromptDialogState> {
        match &mut self.active {
            ActiveDialog::AuthPrompt(state) => Some(state),
            _ => None,
        }
    }

    // ---- About dialog operations ----

    /// Open the About dialog
    pub fn open_about(&mut self) {
        self.present(ActiveDialog::About(AboutDialogState::new()), None);
    }

    // ---- Password dialog operations ----

    /// Open the password dialog with the given state
    pub fn open_password(&mut self, state: PasswordDialogState) {
        self.present(ActiveDialog::PasswordPrompt(state), None);
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
        self.present(ActiveDialog::PassphrasePrompt(state), None);
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
        self.present(
            ActiveDialog::QuickConnect(QuickConnectDialogState::new()),
            None,
        );
    }

    // ---- Connecting dialog operations ----

    /// Open the connecting dialog
    pub fn open_connecting(&mut self, host_name: String, protocol: &str) {
        self.present(
            ActiveDialog::Connecting(ConnectingDialogState::new(host_name, protocol)),
            None,
        );
    }

    /// Close the dialog only if it's the connecting dialog
    pub fn close_connecting(&mut self) {
        self.pending
            .retain(|(dialog, _)| !matches!(dialog, ActiveDialog::Connecting(_)));
        if matches!(self.active, ActiveDialog::Connecting(_)) {
            self.close();
        }
    }

    /// Get mutable quick connect dialog state if it's the active dialog
    pub fn quick_connect_mut(&mut self) -> Option<&mut QuickConnectDialogState> {
        match &mut self.active {
            ActiveDialog::QuickConnect(state) => Some(state),
            _ => None,
        }
    }

    /// Open the duplicate host session choice dialog.
    pub fn open_session_choice(&mut self, state: SessionChoiceDialogState) {
        self.present(ActiveDialog::SessionChoice(state), None);
    }

    // ---- Unencrypted VNC warning dialog operations ----

    /// Open the unencrypted-VNC warning dialog
    pub fn open_vnc_cleartext(&mut self, state: VncCleartextDialogState) {
        self.present(ActiveDialog::VncCleartextWarning(state), None);
    }

    /// Get mutable unencrypted-VNC warning dialog state if it is active
    pub fn vnc_cleartext_mut(&mut self) -> Option<&mut VncCleartextDialogState> {
        match &mut self.active {
            ActiveDialog::VncCleartextWarning(state) => Some(state),
            _ => None,
        }
    }

    /// Open the Portal Hub onboarding wizard.
    pub fn open_portal_hub_onboarding(&mut self) {
        self.present(ActiveDialog::PortalHubOnboarding, None);
    }

    /// Open the Portal Hub conflict resolver.
    pub fn open_portal_hub_conflicts(&mut self) {
        self.present(ActiveDialog::PortalHubConflicts, None);
    }

    pub fn close_portal_hub_conflicts(&mut self) {
        self.pending
            .retain(|(dialog, _)| !matches!(dialog, ActiveDialog::PortalHubConflicts));
        if matches!(self.active, ActiveDialog::PortalHubConflicts) {
            self.close();
        }
    }

    /// Open confirmation before disabling a Portal Hub sync service.
    pub fn open_portal_hub_disable_sync(
        &mut self,
        service: crate::hub::sync::PortalHubSyncService,
    ) {
        self.present(ActiveDialog::PortalHubDisableSync(service), None);
    }

    /// Get mutable session choice dialog state if it is active.
    pub fn session_choice_mut(&mut self) -> Option<&mut SessionChoiceDialogState> {
        match &mut self.active {
            ActiveDialog::SessionChoice(state) => Some(state),
            _ => None,
        }
    }

    /// Get session choice dialog state if it is active.
    pub fn session_choice(&self) -> Option<&SessionChoiceDialogState> {
        match &self.active {
            ActiveDialog::SessionChoice(state) => Some(state),
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
