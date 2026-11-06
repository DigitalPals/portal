//! Keyboard handling for UI state messages.

use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::{Focusable, Outcome, TextInput};
use iced::keyboard::{self, Key};
use iced::widget::Id;
use iced::{Rectangle, Task};

use crate::app::ActiveDialog;
use crate::app::{FocusSection, Portal, VaultModal, View};
use crate::keybindings::{AppAction, is_command_palette_shortcut};
use crate::message::{
    DialogMessage, HistoryMessage, HostMessage, Message, SearchMessage, SessionMessage,
    SftpMessage, SidebarMenuItem, TabMessage, UiMessage, VncMessage,
};
use crate::sftp::FileEntry;
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::command_palette::{
    available_commands, command_list_id, filter_commands, move_selection, selected_matching_action,
};
use crate::views::dialogs::host_dialog::host_dialog_field_id;
use crate::views::dialogs::portal_hub_dialogs::{
    PortalHubOnboardingField, portal_hub_onboarding_field_from_id,
};
use crate::views::sftp::PaneId;
use crate::views::toast::Toast;

pub(super) fn handle_key_released(
    portal: &mut Portal,
    key: Key,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    if portal.ui.command_palette_open {
        return Task::none();
    }
    if let View::VncViewer(session_id) = portal.ui.active_view {
        let passthrough = portal
            .vnc_sessions
            .get(&session_id)
            .map(|v| v.keyboard_passthrough)
            .unwrap_or(false);
        let effective_key = if passthrough {
            if let Key::Character(c) = &key {
                Key::Character(c.to_lowercase().into())
            } else {
                key
            }
        } else if modifiers.shift() {
            shift_modified_key(&key)
        } else {
            key
        };
        if let Some(keysym) = crate::vnc::keysym::key_to_keysym(&effective_key) {
            tracing::debug!(
                "VNC key release: key={:?} keysym=0x{:04X}",
                effective_key,
                keysym
            );
            return Task::done(Message::Vnc(VncMessage::KeyEvent {
                session_id,
                keysym,
                pressed: false,
            }));
        }
    }
    Task::none()
}

/// Handle keyboard shortcuts
pub(super) fn handle_keyboard_event(
    portal: &mut Portal,
    key: Key,
    modifiers: keyboard::Modifiers,
    shortcut_key: Option<char>,
) -> Task<Message> {
    if portal.ui.command_palette_open {
        match &key {
            Key::Named(keyboard::key::Named::Escape) => {
                return Task::done(Message::Ui(UiMessage::CommandPaletteClose));
            }
            Key::Named(
                named @ (keyboard::key::Named::ArrowUp | keyboard::key::Named::ArrowDown),
            ) => {
                let commands = available_commands(
                    &portal.config.hosts,
                    &portal.config.snippets,
                    portal.prefs.portal_hub.sync_configured(),
                );
                let count = filter_commands(&commands, &portal.ui.command_palette_query).len();
                let selected = move_selection(
                    portal.ui.command_palette_selected,
                    count,
                    *named == keyboard::key::Named::ArrowDown,
                );
                portal.ui.command_palette_selected = selected;
                // Keep keyboard selection visible, including when wrapping to either end.
                return iced::widget::operation::snap_to(
                    command_list_id(),
                    iced::widget::operation::RelativeOffset {
                        x: 0.0,
                        y: selected as f32 / count.saturating_sub(1).max(1) as f32,
                    },
                );
            }
            Key::Named(keyboard::key::Named::Enter) => {
                let commands = available_commands(
                    &portal.config.hosts,
                    &portal.config.snippets,
                    portal.prefs.portal_hub.sync_configured(),
                );
                if let Some(action) = selected_matching_action(
                    &commands,
                    &portal.ui.command_palette_query,
                    portal.ui.command_palette_selected,
                ) {
                    return Task::done(Message::Ui(UiMessage::CommandPaletteRun(action)));
                }
                return Task::none();
            }
            _ => return Task::none(),
        }
    }

    // Priority 1: Dialog open - handle dialog-specific keyboard navigation
    if portal.dialogs.is_open() {
        if let Some(task) =
            handle_portal_hub_onboarding_clipboard(portal, &key, &modifiers, shortcut_key)
        {
            return task;
        }
        return handle_dialog_keyboard(portal, &key, &modifiers);
    }

    if portal.vault_ui.modal != VaultModal::None {
        if let Key::Named(keyboard::key::Named::Escape) = key {
            portal.vault_ui.modal = VaultModal::None;
        }
        return Task::none();
    }

    if let Some(tab_id) = portal
        .tabs
        .iter()
        .find(|tab| tab.rename_value.is_some())
        .map(|tab| tab.id)
    {
        if let Key::Named(keyboard::key::Named::Escape) = key {
            return Task::done(Message::Tab(TabMessage::RenameCancel(tab_id)));
        }
        // The text input handles editing and Enter. Do not let an uncaptured
        // key also trigger application or terminal shortcuts while renaming.
        return Task::none();
    }

    // Priority 2: VNC viewer — forward all keys to remote (except Ctrl+Shift combos for UI)
    if let View::VncViewer(session_id) = portal.ui.active_view {
        let passthrough = portal
            .vnc_sessions
            .get(&session_id)
            .map(|v| v.keyboard_passthrough)
            .unwrap_or(false);

        // Ctrl+Shift+Escape: escape hatch to toggle keyboard passthrough off
        if passthrough
            && modifiers.control()
            && modifiers.shift()
            && matches!(&key, Key::Named(keyboard::key::Named::Escape))
        {
            return Task::done(Message::Vnc(VncMessage::ToggleKeyboardPassthrough));
        }

        // In passthrough mode, forward ALL keys to VNC (no local shortcuts).
        // Shift/Ctrl/etc are forwarded as separate key events, so send the
        // base (lowercase) keysym for characters — the server applies modifiers.
        if passthrough {
            let passthrough_key = if let Key::Character(c) = &key {
                let lower: String = c.to_lowercase();
                Key::Character(lower.into())
            } else {
                key.clone()
            };
            if let Some(keysym) = crate::vnc::keysym::key_to_keysym(&passthrough_key) {
                tracing::debug!(
                    "VNC passthrough key press: original={:?} mapped={:?} keysym=0x{:04X}",
                    key,
                    passthrough_key,
                    keysym
                );
                return Task::done(Message::Vnc(VncMessage::KeyEvent {
                    session_id,
                    keysym,
                    pressed: true,
                }));
            }
            return Task::none();
        }

        if let Some(task) = handle_configured_actions(
            portal,
            &key,
            &modifiers,
            &[
                AppAction::ToggleFullscreen,
                AppAction::NewWindow,
                AppAction::NewConnection,
                AppAction::CloseSession,
                AppAction::NewTab,
                AppAction::NextSession,
                AppAction::PreviousSession,
            ],
        ) {
            return task;
        }

        // Ctrl+Shift shortcuts for VNC-specific actions
        if modifiers.control()
            && modifiers.shift()
            && let Key::Character(c) = &key
        {
            match c.as_str() {
                // Ctrl+Shift+S: Screenshot
                "s" | "S" => {
                    return Task::done(Message::Vnc(VncMessage::CaptureScreenshot(session_id)));
                }
                // Ctrl+Shift+V: Paste clipboard to VNC
                "v" | "V" => {
                    return iced::clipboard::read().map(move |contents| {
                        if let Some(text) = contents {
                            Message::Vnc(VncMessage::ClipboardSend(session_id, text))
                        } else {
                            Message::Noop
                        }
                    });
                }
                _ => {} // Fall through to global shortcuts
            }
        }

        // Forward modifier key presses (Shift, Ctrl, Alt) to VNC so the remote
        // sees the correct modifier state even in non-passthrough mode.
        if let Key::Named(named) = &key {
            let modifier_keysym = match named {
                keyboard::key::Named::Shift => Some(0xFFE1), // XK_Shift_L
                keyboard::key::Named::Control => Some(0xFFE3), // XK_Control_L
                keyboard::key::Named::Alt => Some(0xFFE9),   // XK_Alt_L
                keyboard::key::Named::Super => Some(0xFFEB), // XK_Super_L
                _ => None,
            };
            if let Some(keysym) = modifier_keysym {
                return Task::done(Message::Vnc(VncMessage::KeyEvent {
                    session_id,
                    keysym,
                    pressed: true,
                }));
            }
        }

        // Forward remaining keys to VNC (except Ctrl+Shift combos for tab management etc.)
        // When Shift is held, Iced may still report lowercase characters on some
        // platforms. Uppercase single ASCII letters so the correct keysym is sent.
        // NOTE: The Shift+key mapping assumes US QWERTY layout. Passthrough mode
        // is recommended for non-US layouts since it delegates layout handling to the server.
        if !(modifiers.control() && modifiers.shift()) {
            let effective_key = if modifiers.shift() {
                shift_modified_key(&key)
            } else {
                key.clone()
            };
            if let Some(keysym) = crate::vnc::keysym::key_to_keysym(&effective_key) {
                tracing::debug!(
                    "VNC non-passthrough key press: key={:?} effective={:?} modifiers={:?} keysym=0x{:04X}",
                    key,
                    effective_key,
                    modifiers,
                    keysym
                );
                return Task::done(Message::Vnc(VncMessage::KeyEvent {
                    session_id,
                    keysym,
                    pressed: true,
                }));
            }
        }
    }

    // Terminal scrollback search: the configured binding opens/refocuses the
    // bar; while it is open, Enter/Shift+Enter navigate and Escape closes.
    if let View::Terminal(session_id) = portal.ui.active_view
        && portal.sessions.contains(session_id)
    {
        if portal
            .prefs
            .keybindings
            .matches_action(AppAction::TerminalSearch, &key, &modifiers)
        {
            return portal.update(Message::Session(SessionMessage::Search(
                SearchMessage::Open(session_id),
            )));
        }

        let search_open = portal
            .sessions
            .get(session_id)
            .is_some_and(|session| session.search.open);
        if search_open {
            return match &key {
                Key::Named(keyboard::key::Named::Escape) => portal.update(Message::Session(
                    SessionMessage::Search(SearchMessage::Close(session_id)),
                )),
                Key::Named(keyboard::key::Named::Enter) => {
                    let msg = if modifiers.shift() {
                        SearchMessage::PreviousMatch(session_id)
                    } else {
                        SearchMessage::NextMatch(session_id)
                    };
                    portal.update(Message::Session(SessionMessage::Search(msg)))
                }
                // Everything else belongs to the search input (or is ignored);
                // no global shortcuts while the search bar owns the keyboard.
                _ => Task::none(),
            };
        }
    }

    // App shortcuts take priority over terminal capture. VNC passthrough and
    // modal/search input have already handled their keys above.
    if let Some(action) = portal.prefs.keybindings.app_action(&key, &modifiers) {
        return portal.handle_keybinding_action(action);
    }
    if is_command_palette_shortcut(&key, &modifiers) {
        return Task::done(Message::Ui(UiMessage::CommandPaletteToggle));
    }

    if let View::Terminal(session_id) = portal.ui.active_view
        && modifiers.is_empty()
        && matches!(key, Key::Named(keyboard::key::Named::Enter))
        && portal
            .sessions
            .get(session_id)
            .is_some_and(|session| session.disconnected_reason.is_some())
    {
        return portal.update(Message::Session(SessionMessage::ReconnectNow(session_id)));
    }

    // Terminal captured: leave ordinary shell keys to the terminal widget.
    if portal.ui.terminal_captured {
        // Ctrl+Escape exits captured mode
        if let Key::Named(keyboard::key::Named::Escape) = &key
            && modifiers.control()
        {
            portal.ui.terminal_captured = false;
            portal.ui.focus_section = FocusSection::Content;
            return Task::none();
        }
        // Ctrl+Shift+K installs SSH key and Ctrl+Shift+U jumps to unread agent
        // notifications - allow these through to global shortcuts.
        if let Key::Character(c) = &key {
            if modifiers.control()
                && modifiers.shift()
                && matches!(c.as_str(), "k" | "K" | "u" | "U")
            {
                // Fall through to global shortcuts below
            } else {
                return Task::none();
            }
        } else {
            return Task::none();
        }
    }

    // Global navigation shortcuts (unless terminal captured)
    match (&key, modifiers.control(), modifiers.shift()) {
        // F1 - Focus Sidebar
        (Key::Named(keyboard::key::Named::F1), false, false) => {
            portal.ui.focus_section = FocusSection::Sidebar;
            return Task::none();
        }
        // F2 - Focus Tab Bar
        (Key::Named(keyboard::key::Named::F2), false, false) => {
            portal.ui.focus_section = FocusSection::TabBar;
            return Task::none();
        }
        // F3 - Focus Content
        (Key::Named(keyboard::key::Named::F3), false, false) => {
            portal.ui.focus_section = FocusSection::Content;
            return Task::none();
        }
        // Escape - close context menus, or exit terminal capture indication
        (Key::Named(keyboard::key::Named::Escape), _, _) => {
            // Close any open SFTP context menu or dialog
            for tab_state in portal.sftp.tab_values_mut() {
                tab_state.hide_context_menu();
                tab_state.close_dialog();
            }
            portal.ui.tab_context_menu.hide();
            return Task::none();
        }
        // Ctrl+Shift+K - Install SSH key on remote server
        (Key::Character(c), true, true) if c.as_str() == "k" || c.as_str() == "K" => {
            if let View::Terminal(session_id) = portal.ui.active_view
                && portal.sessions.contains(session_id)
            {
                return portal.update(Message::Session(SessionMessage::InstallKey(session_id)));
            }
            return Task::none();
        }
        _ => {}
    }

    // Priority 4: Section-specific navigation
    match portal.ui.focus_section {
        FocusSection::Sidebar => handle_sidebar_keyboard(portal, &key, &modifiers),
        FocusSection::TabBar => handle_tabbar_keyboard(portal, &key, &modifiers),
        FocusSection::Content => handle_content_keyboard(portal, &key, &modifiers),
    }
}

fn handle_configured_actions(
    portal: &mut Portal,
    key: &Key,
    modifiers: &keyboard::Modifiers,
    actions: &[AppAction],
) -> Option<Task<Message>> {
    for action in actions {
        if portal
            .prefs
            .keybindings
            .matches_action(*action, key, modifiers)
        {
            return Some(portal.handle_keybinding_action(*action));
        }
    }
    None
}

#[derive(Debug, Clone)]
struct FocusedPortalHubInput {
    field: PortalHubOnboardingField,
    text: String,
}

#[derive(Default)]
struct FocusedPortalHubInputOperation {
    focused_field: Option<PortalHubOnboardingField>,
    inputs: Vec<FocusedPortalHubInput>,
}

impl Operation<Option<FocusedPortalHubInput>> for FocusedPortalHubInputOperation {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn Operation<Option<FocusedPortalHubInput>>),
    ) {
        operate(self);
    }

    fn text_input(&mut self, id: Option<&Id>, _bounds: Rectangle, state: &mut dyn TextInput) {
        let Some(field) = id.and_then(portal_hub_onboarding_field_from_id) else {
            return;
        };

        self.inputs.push(FocusedPortalHubInput {
            field,
            text: state.text().to_string(),
        });
    }

    fn focusable(&mut self, id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        let Some(field) = id.and_then(portal_hub_onboarding_field_from_id) else {
            return;
        };

        if state.is_focused() {
            self.focused_field = Some(field);
        }
    }

    fn finish(&self) -> Outcome<Option<FocusedPortalHubInput>> {
        let focused = self.focused_field.and_then(|field| {
            self.inputs
                .iter()
                .find(|input| input.field == field)
                .cloned()
        });
        Outcome::Some(focused)
    }
}

fn handle_portal_hub_onboarding_clipboard(
    portal: &Portal,
    key: &Key,
    modifiers: &keyboard::Modifiers,
    shortcut_key: Option<char>,
) -> Option<Task<Message>> {
    if !matches!(portal.dialogs.active(), ActiveDialog::PortalHubOnboarding) {
        return None;
    }
    if !is_portal_hub_clipboard_modifier(*modifiers) {
        return None;
    }

    let shortcut = shortcut_key.or_else(|| character_shortcut_key(key))?;

    match shortcut.to_ascii_lowercase() {
        'c' => Some(
            iced::advanced::widget::operate(FocusedPortalHubInputOperation::default())
                .and_then(|input| iced::clipboard::write::<Message>(input.text)),
        ),
        'v' => Some(
            iced::advanced::widget::operate(FocusedPortalHubInputOperation::default()).then(
                |focused| {
                    let Some(focused) = focused else {
                        return Task::none();
                    };
                    iced::clipboard::read().map(move |contents| {
                        let Some(contents) = contents else {
                            return Message::Noop;
                        };
                        Message::Ui(match focused.field {
                            PortalHubOnboardingField::WebUrl => {
                                UiMessage::PortalHubWebUrlChanged(contents)
                            }
                        })
                    })
                },
            ),
        ),
        _ => None,
    }
}

fn is_portal_hub_clipboard_modifier(modifiers: keyboard::Modifiers) -> bool {
    modifiers.command() || modifiers.logo()
}

fn character_shortcut_key(key: &Key) -> Option<char> {
    let Key::Character(character) = key else {
        return None;
    };

    let mut chars = character.chars();
    let shortcut = chars.next()?;

    chars.next().is_none().then_some(shortcut)
}

fn shift_modified_key(key: &Key) -> Key {
    if let Key::Character(c) = key {
        match c.chars().next() {
            Some(ch) if ch.is_ascii_lowercase() => {
                Key::Character(ch.to_ascii_uppercase().to_string().into())
            }
            Some(ch) => {
                let shifted = match ch {
                    '1' => '!',
                    '2' => '@',
                    '3' => '#',
                    '4' => '$',
                    '5' => '%',
                    '6' => '^',
                    '7' => '&',
                    '8' => '*',
                    '9' => '(',
                    '0' => ')',
                    '-' => '_',
                    '=' => '+',
                    '[' => '{',
                    ']' => '}',
                    '\\' => '|',
                    ';' => ':',
                    '\'' => '"',
                    ',' => '<',
                    '.' => '>',
                    '/' => '?',
                    '`' => '~',
                    _ => ch,
                };
                if shifted != ch {
                    Key::Character(shifted.to_string().into())
                } else {
                    key.clone()
                }
            }
            _ => key.clone(),
        }
    } else {
        key.clone()
    }
}

/// Handle keyboard navigation in dialogs
fn handle_dialog_keyboard(
    portal: &mut Portal,
    key: &Key,
    modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    match portal.dialogs.active() {
        ActiveDialog::Host(state) => {
            let is_valid = state.is_valid();
            let has_key_path = state.auth_method
                == crate::views::dialogs::host_dialog::AuthMethodChoice::PublicKey;

            // Build list of focusable fields based on current state
            let focusable: Vec<usize> = if has_key_path {
                vec![0, 1, 2, 3, 5, 6, 7] // Include key path field
            } else {
                vec![0, 1, 2, 3, 6, 7] // Skip key path field
            };

            match key {
                Key::Named(keyboard::key::Named::Escape) => {
                    portal.dialogs.close();
                    Task::none()
                }
                Key::Named(keyboard::key::Named::Tab) => {
                    // Find current position in focusable list
                    let current = portal.dialogs.host_dialog_focus;
                    let current_pos = focusable.iter().position(|&f| f == current).unwrap_or(0);

                    let next_pos = if modifiers.shift() {
                        // Shift+Tab: go backwards
                        if current_pos == 0 {
                            focusable.len() - 1
                        } else {
                            current_pos - 1
                        }
                    } else {
                        // Tab: go forwards
                        (current_pos + 1) % focusable.len()
                    };

                    let next_field = focusable[next_pos];
                    portal.dialogs.host_dialog_focus = next_field;
                    iced::widget::operation::focus(host_dialog_field_id(next_field))
                }
                Key::Named(keyboard::key::Named::Enter) => {
                    // Submit form if valid
                    if is_valid {
                        portal.update(Message::Dialog(DialogMessage::Submit))
                    } else {
                        Task::none()
                    }
                }
                _ => Task::none(),
            }
        }
        ActiveDialog::HostKey(_) => {
            // Host key dialog: Escape to reject
            if let Key::Named(keyboard::key::Named::Escape) = key {
                if let Some(dialog) = portal.dialogs.host_key_mut() {
                    dialog.respond(HostKeyVerificationResponse::Reject);
                    portal
                        .toast_manager
                        .push(Toast::warning("Connection cancelled"));
                }
                portal.dialogs.close();
            }
            Task::none()
        }
        ActiveDialog::Connecting(_) => {
            if let Key::Named(keyboard::key::Named::Escape) = key {
                portal.cancel_pending_connect();
            }
            Task::none()
        }
        _ => {
            // Route cancellation through the owner so draft tasks are invalidated.
            if let Key::Named(keyboard::key::Named::Escape) = key {
                return portal.update(Message::Dialog(DialogMessage::Close));
            }
            Task::none()
        }
    }
}

fn visible_sidebar_items(show_sessions: bool) -> Vec<SidebarMenuItem> {
    let mut items = vec![SidebarMenuItem::Hosts, SidebarMenuItem::Sftp];
    if show_sessions {
        items.push(SidebarMenuItem::Sessions);
    }
    items.extend([
        SidebarMenuItem::Vault,
        SidebarMenuItem::Snippets,
        SidebarMenuItem::History,
        SidebarMenuItem::Settings,
        SidebarMenuItem::About,
    ]);
    items
}

/// Handle keyboard navigation in sidebar
fn handle_sidebar_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    let items = visible_sidebar_items(portal.prefs.portal_hub.is_configured());
    let last_index = items.len().saturating_sub(1);
    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            portal.ui.sidebar_focus_index = portal.ui.sidebar_focus_index.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            portal.ui.sidebar_focus_index = (portal.ui.sidebar_focus_index + 1).min(last_index);
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.ui.sidebar_focus_index = 0;
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.ui.sidebar_focus_index = last_index;
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            portal.ui.focus_section = FocusSection::Content;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            let Some(item) = items.get(portal.ui.sidebar_focus_index).copied() else {
                return Task::none();
            };
            return portal.update(Message::Ui(UiMessage::SidebarItemSelect(item)));
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in tab bar
fn handle_tabbar_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    let tab_count = portal.tabs.len();
    if tab_count == 0 {
        // No tabs - switch to content
        portal.ui.focus_section = FocusSection::Content;
        return Task::none();
    }

    match key {
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            portal.ui.tab_focus_index = portal.ui.tab_focus_index.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            portal.ui.tab_focus_index =
                (portal.ui.tab_focus_index + 1).min(tab_count.saturating_sub(1));
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.ui.tab_focus_index = 0;
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.ui.tab_focus_index = tab_count.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            portal.ui.focus_section = FocusSection::Content;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            if let Some(tab) = portal.tabs.get(portal.ui.tab_focus_index) {
                let tab_id = tab.id;
                return portal.update(Message::Tab(TabMessage::Select(tab_id)));
            }
        }
        Key::Named(keyboard::key::Named::Delete) => {
            if let Some(tab) = portal.tabs.get(portal.ui.tab_focus_index) {
                let tab_id = tab.id;
                return portal.update(Message::Tab(TabMessage::Close(tab_id)));
            }
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in content area
fn handle_content_keyboard(
    portal: &mut Portal,
    key: &Key,
    modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    match &portal.ui.active_view {
        View::Settings => {
            // Settings page - arrow left goes back to sidebar
            if let Key::Named(keyboard::key::Named::ArrowLeft) = key {
                portal.ui.focus_section = FocusSection::Sidebar;
            }
            Task::none()
        }
        View::HostGrid => match portal.ui.sidebar_selection {
            SidebarMenuItem::Hosts
            | SidebarMenuItem::Sftp
            | SidebarMenuItem::Sessions
            | SidebarMenuItem::Vault
            | SidebarMenuItem::Snippets
            | SidebarMenuItem::Settings
            | SidebarMenuItem::About => handle_host_grid_keyboard(portal, key, modifiers),
            SidebarMenuItem::History => handle_history_keyboard(portal, key, modifiers),
        },
        View::ProxySessions => {
            if let Key::Named(keyboard::key::Named::ArrowLeft) = key {
                portal.ui.focus_section = FocusSection::Sidebar;
            }
            Task::none()
        }
        View::Vault => {
            if let Key::Named(keyboard::key::Named::ArrowLeft) = key {
                portal.ui.focus_section = FocusSection::Sidebar;
            }
            Task::none()
        }
        View::Terminal(_session_id) => {
            // When terminal_captured is false (after Ctrl+Escape), allow keyboard navigation
            match key {
                Key::Named(keyboard::key::Named::ArrowUp) => {
                    portal.ui.focus_section = FocusSection::TabBar;
                }
                Key::Named(keyboard::key::Named::ArrowLeft) => {
                    portal.ui.focus_section = FocusSection::Sidebar;
                }
                _ => {
                    // Re-capture terminal on any other key press
                    portal.ui.terminal_captured = true;
                }
            }
            Task::none()
        }
        View::DualSftp(tab_id) => handle_sftp_keyboard(portal, *tab_id, key, modifiers),
        View::FileViewer(_) => {
            // File viewer keyboard - arrow left goes back to sidebar
            if let Key::Named(keyboard::key::Named::ArrowLeft) = key {
                portal.ui.focus_section = FocusSection::Sidebar;
            }
            Task::none()
        }
        View::Snippets => {
            // Snippets page - arrow left goes back to sidebar
            if let Key::Named(keyboard::key::Named::ArrowLeft) = key {
                portal.ui.focus_section = FocusSection::Sidebar;
            }
            Task::none()
        }
        View::VncViewer(session_id) => {
            // Forward key press to VNC session
            if let Some(keysym) = crate::vnc::keysym::key_to_keysym(key) {
                return Task::done(Message::Vnc(VncMessage::KeyEvent {
                    session_id: *session_id,
                    keysym,
                    pressed: true,
                }));
            }
            Task::none()
        }
    }
}

/// Handle keyboard navigation in host grid
fn handle_host_grid_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    // Count total items (groups + hosts)
    let group_count = portal.config.hosts.groups.len();
    let host_count = portal.config.hosts.hosts.len();
    let total_items = group_count + host_count;

    if total_items == 0 {
        // "/" focuses search even when empty
        if let Key::Character(c) = key
            && c.as_str() == "/"
        {
            portal.ui.host_grid_focus_index = None; // Clear grid focus when focusing search
            return iced::widget::operation::focus(crate::views::host_grid::search_input_id());
        }
        return Task::none();
    }

    // Calculate column count for 2D navigation
    let columns = crate::views::host_grid::calculate_columns(
        portal.ui.window_size.width,
        portal.ui.sidebar_state,
    );

    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            if let Some(idx) = portal.ui.host_grid_focus_index {
                if idx >= columns {
                    portal.ui.host_grid_focus_index = Some(idx - columns);
                } else {
                    // At top row - move focus to tabs
                    portal.ui.focus_section = FocusSection::TabBar;
                }
            } else {
                portal.ui.host_grid_focus_index = Some(0);
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::operation::focus(iced::widget::Id::unique());
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            if let Some(idx) = portal.ui.host_grid_focus_index {
                let new_idx = idx + columns;
                if new_idx < total_items {
                    portal.ui.host_grid_focus_index = Some(new_idx);
                }
            } else {
                portal.ui.host_grid_focus_index = Some(0);
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::operation::focus(iced::widget::Id::unique());
        }
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            if let Some(idx) = portal.ui.host_grid_focus_index {
                if idx > 0 {
                    portal.ui.host_grid_focus_index = Some(idx - 1);
                } else {
                    portal.ui.focus_section = FocusSection::Sidebar;
                }
            } else {
                portal.ui.focus_section = FocusSection::Sidebar;
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::operation::focus(iced::widget::Id::unique());
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            if let Some(idx) = portal.ui.host_grid_focus_index {
                if idx + 1 < total_items {
                    portal.ui.host_grid_focus_index = Some(idx + 1);
                }
            } else {
                portal.ui.host_grid_focus_index = Some(0);
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::operation::focus(iced::widget::Id::unique());
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.ui.host_grid_focus_index = Some(0);
            return iced::widget::operation::focus(iced::widget::Id::unique());
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.ui.host_grid_focus_index = Some(total_items.saturating_sub(1));
            return iced::widget::operation::focus(iced::widget::Id::unique());
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            // Only activate if we have a focused card (not the search input)
            if let Some(idx) = portal.ui.host_grid_focus_index {
                // First come groups, then hosts
                if idx < group_count {
                    // Toggle group
                    if let Some(group) = portal.config.hosts.groups.get(idx) {
                        let group_id = group.id;
                        return portal.update(Message::Ui(UiMessage::FolderToggle(group_id)));
                    }
                } else {
                    // Connect to host
                    let host_idx = idx - group_count;
                    if let Some(host) = portal.config.hosts.hosts.get(host_idx) {
                        let host_id = host.id;
                        return portal.update(Message::Host(HostMessage::Connect(host_id)));
                    }
                }
            }
            // No card focused - don't handle Enter (let search input handle it)
        }
        Key::Character(c) if c.as_str() == "/" => {
            portal.ui.host_grid_focus_index = None; // Clear grid focus when focusing search
            return iced::widget::operation::focus(crate::views::host_grid::search_input_id());
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in history view
fn handle_history_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    let entry_count = portal.config.history.entries.len();
    if entry_count == 0 {
        return Task::none();
    }

    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            if let Some(idx) = portal.ui.history_focus_index {
                portal.ui.history_focus_index = Some(idx.saturating_sub(1));
            } else {
                portal.ui.history_focus_index = Some(0);
            }
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            if let Some(idx) = portal.ui.history_focus_index {
                portal.ui.history_focus_index = Some((idx + 1).min(entry_count - 1));
            } else {
                portal.ui.history_focus_index = Some(0);
            }
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.ui.history_focus_index = Some(0);
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.ui.history_focus_index = Some(entry_count.saturating_sub(1));
        }
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            portal.ui.focus_section = FocusSection::Sidebar;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            if let Some(idx) = portal.ui.history_focus_index
                && let Some(entry) = portal.config.history.entries.get(idx)
            {
                let host_id = entry.host_id;
                return portal.update(Message::History(HistoryMessage::Reconnect(host_id)));
            }
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in SFTP dual-pane view
fn handle_sftp_keyboard(
    portal: &mut Portal,
    tab_id: uuid::Uuid,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    use iced::widget::scrollable;

    let Some(state) = portal.sftp.get_tab_mut(tab_id) else {
        return Task::none();
    };

    let active_pane = state.active_pane;
    let pane_state = match active_pane {
        PaneId::Left => &mut state.left_pane,
        PaneId::Right => &mut state.right_pane,
    };

    // Get visible entries (respecting filter and show_hidden)
    let visible = pane_state.visible_entries();
    let visible_count = visible.len();

    // Height of each file row for scroll calculation
    const ROW_HEIGHT: f32 = 35.0;

    match key {
        // Tab switches between panes
        Key::Named(keyboard::key::Named::Tab) => {
            state.active_pane = match state.active_pane {
                PaneId::Left => PaneId::Right,
                PaneId::Right => PaneId::Left,
            };
        }
        Key::Named(keyboard::key::Named::ArrowUp) if visible_count > 0 => {
            // Find current position in visible entries
            let current_visible_pos = pane_state
                .last_selected_index
                .and_then(|idx| visible.iter().position(|(i, _)| *i == idx))
                .unwrap_or(0);
            let new_visible_pos = current_visible_pos.saturating_sub(1);
            let new_idx = visible[new_visible_pos].0;

            pane_state.selected_indices.clear();
            pane_state.selected_indices.insert(new_idx);
            pane_state.last_selected_index = Some(new_idx);

            // Scroll to keep selection visible
            let scroll_offset = new_visible_pos as f32 * ROW_HEIGHT;
            return iced::widget::operation::scroll_to(
                pane_state.scrollable_id.clone(),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: scroll_offset,
                },
            );
        }
        Key::Named(keyboard::key::Named::ArrowDown) if visible_count > 0 => {
            // Find current position in visible entries
            let current_visible_pos = pane_state
                .last_selected_index
                .and_then(|idx| visible.iter().position(|(i, _)| *i == idx))
                .unwrap_or(0);
            let new_visible_pos = (current_visible_pos + 1).min(visible_count - 1);
            let new_idx = visible[new_visible_pos].0;

            pane_state.selected_indices.clear();
            pane_state.selected_indices.insert(new_idx);
            pane_state.last_selected_index = Some(new_idx);

            // Scroll to keep selection visible
            let scroll_offset = new_visible_pos as f32 * ROW_HEIGHT;
            return iced::widget::operation::scroll_to(
                pane_state.scrollable_id.clone(),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: scroll_offset,
                },
            );
        }
        Key::Named(keyboard::key::Named::Home) if visible_count > 0 => {
            let new_idx = visible[0].0;
            pane_state.selected_indices.clear();
            pane_state.selected_indices.insert(new_idx);
            pane_state.last_selected_index = Some(new_idx);

            // Scroll to top
            return iced::widget::operation::scroll_to(
                pane_state.scrollable_id.clone(),
                scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            );
        }
        Key::Named(keyboard::key::Named::End) if visible_count > 0 => {
            let new_idx = visible[visible_count - 1].0;
            pane_state.selected_indices.clear();
            pane_state.selected_indices.insert(new_idx);
            pane_state.last_selected_index = Some(new_idx);

            // Scroll to bottom
            let scroll_offset = (visible_count - 1) as f32 * ROW_HEIGHT;
            return iced::widget::operation::scroll_to(
                pane_state.scrollable_id.clone(),
                scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: scroll_offset,
                },
            );
        }
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            // Need to re-borrow state since we're not inside the match
            let Some(state) = portal.sftp.get_tab_mut(tab_id) else {
                return Task::none();
            };
            if active_pane == PaneId::Left {
                portal.ui.focus_section = FocusSection::Sidebar;
            } else {
                state.active_pane = PaneId::Left;
            }
            return Task::none();
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            let Some(state) = portal.sftp.get_tab_mut(tab_id) else {
                return Task::none();
            };
            if active_pane != PaneId::Right {
                state.active_pane = PaneId::Right;
            }
            return Task::none();
        }
        Key::Named(keyboard::key::Named::Enter) => {
            // Navigate into directory or activate file
            if let Some(path) =
                sftp_enter_navigation_path(pane_state.last_selected_index, &pane_state.entries)
            {
                return portal.update(Message::Sftp(SftpMessage::PaneNavigate(
                    tab_id,
                    active_pane,
                    path,
                )));
            }
        }
        Key::Named(keyboard::key::Named::Backspace) => {
            // Navigate to parent
            return portal.update(Message::Sftp(SftpMessage::PaneNavigateUp(
                tab_id,
                active_pane,
            )));
        }
        Key::Named(keyboard::key::Named::F5) => {
            // Refresh
            return portal.update(Message::Sftp(SftpMessage::PaneRefresh(tab_id, active_pane)));
        }
        _ => {}
    }
    Task::none()
}

fn sftp_enter_navigation_path(
    selected_index: Option<usize>,
    entries: &[FileEntry],
) -> Option<std::path::PathBuf> {
    let entry = entries.get(selected_index?)?;
    entry.is_navigable_dir().then(|| entry.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybindings::Keybinding;
    use std::path::PathBuf;

    fn palette_key(portal: &mut Portal, key: keyboard::key::Named) -> Task<Message> {
        handle_keyboard_event(portal, Key::Named(key), keyboard::Modifiers::empty(), None)
    }

    fn task_messages(task: Task<Message>) -> Vec<Message> {
        use futures::StreamExt;
        let Some(stream) = iced_test::runtime::task::into_stream(task) else {
            return Vec::new();
        };
        futures::executor::block_on(
            stream
                .filter_map(|action| async move {
                    match action {
                        iced_test::runtime::Action::Output(message) => Some(message),
                        _ => None,
                    }
                })
                .collect(),
        )
    }

    #[test]
    fn command_palette_arrows_and_enter_use_the_selected_row() {
        let mut portal = Portal::test_state();
        portal.ui.command_palette_open = true;
        let _ = palette_key(&mut portal, keyboard::key::Named::ArrowDown);
        assert_eq!(portal.ui.command_palette_selected, 1);
        let messages = task_messages(palette_key(&mut portal, keyboard::key::Named::Enter));
        assert!(matches!(
            messages.as_slice(),
            [Message::Ui(UiMessage::CommandPaletteRun(
                crate::message::CommandAction::Sftp
            ))]
        ));

        let _ = palette_key(&mut portal, keyboard::key::Named::ArrowUp);
        let _ = palette_key(&mut portal, keyboard::key::Named::ArrowUp);
        assert_eq!(portal.ui.command_palette_selected, 8);
        let _ = palette_key(&mut portal, keyboard::key::Named::ArrowDown);
        assert_eq!(portal.ui.command_palette_selected, 0);
    }

    #[test]
    fn command_palette_empty_results_do_not_activate_or_navigate_underneath() {
        let mut portal = Portal::test_state();
        portal.ui.command_palette_open = true;
        portal.ui.command_palette_query = "unmatched".to_string();
        let _ = palette_key(&mut portal, keyboard::key::Named::ArrowUp);
        assert_eq!(portal.ui.command_palette_selected, 0);
        assert!(task_messages(palette_key(&mut portal, keyboard::key::Named::Enter)).is_empty());
        let focus = portal.ui.focus_section;
        let _ = palette_key(&mut portal, keyboard::key::Named::F1);
        assert_eq!(portal.ui.focus_section, focus);
    }

    fn captured_terminal_portal() -> Portal {
        let mut portal = Portal::test_state();
        portal.open_new_tab();
        portal.open_new_tab();
        portal.active_tab = Some(portal.tabs[0].id);
        portal.ui.active_view = View::Terminal(portal.tabs[0].id);
        portal.ui.terminal_captured = true;
        portal
    }

    #[test]
    fn captured_terminal_allows_next_and_previous_session_shortcuts() {
        for (modifiers, start, expected) in [
            (keyboard::Modifiers::CTRL, 0, 1),
            (keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT, 1, 0),
        ] {
            let mut portal = captured_terminal_portal();
            portal.active_tab = Some(portal.tabs[start].id);
            portal.ui.active_view = View::Terminal(portal.tabs[start].id);
            let expected_id = portal.tabs[expected].id;

            let _ = handle_keyboard_event(
                &mut portal,
                Key::Named(keyboard::key::Named::Tab),
                modifiers,
                None,
            );

            assert_eq!(portal.active_tab, Some(expected_id));
        }
    }

    #[test]
    fn captured_terminal_allows_new_and_close_tab_shortcuts() {
        for (key, expected_tabs) in [("t", 3), ("W", 1)] {
            let mut portal = captured_terminal_portal();

            let _ = handle_keyboard_event(
                &mut portal,
                Key::Character(key.into()),
                keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT,
                None,
            );

            assert_eq!(portal.tabs.len(), expected_tabs, "Ctrl+Shift+{key}");
        }
    }

    #[test]
    fn captured_terminal_allows_command_palette_shortcut() {
        let mut portal = captured_terminal_portal();
        let task = handle_keyboard_event(
            &mut portal,
            Key::Character("k".into()),
            keyboard::Modifiers::CTRL,
            Some('k'),
        );

        assert_eq!(task.units(), 1, "command palette toggle should be emitted");
    }

    #[test]
    fn captured_terminal_honors_remapped_shortcuts() {
        let mut portal = captured_terminal_portal();
        portal.prefs.keybindings.new_tab = vec![Keybinding::parse("Alt+N").unwrap()];

        let _ = handle_keyboard_event(
            &mut portal,
            Key::Character("t".into()),
            keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT,
            Some('t'),
        );
        assert_eq!(portal.tabs.len(), 2, "the old binding should be inactive");

        let _ = handle_keyboard_event(
            &mut portal,
            Key::Character("n".into()),
            keyboard::Modifiers::ALT,
            Some('n'),
        );
        assert_eq!(portal.tabs.len(), 3);
    }

    #[test]
    fn captured_terminal_leaves_shell_keys_and_focus_navigation_alone() {
        let mut portal = captured_terminal_portal();
        let active_tab = portal.active_tab;
        for (key, modifiers) in [
            (Key::Character("c".into()), keyboard::Modifiers::CTRL),
            (Key::Character("d".into()), keyboard::Modifiers::CTRL),
            (Key::Character("z".into()), keyboard::Modifiers::CTRL),
            (
                Key::Named(keyboard::key::Named::Tab),
                keyboard::Modifiers::empty(),
            ),
            (
                Key::Named(keyboard::key::Named::ArrowLeft),
                keyboard::Modifiers::empty(),
            ),
            (
                Key::Named(keyboard::key::Named::F1),
                keyboard::Modifiers::empty(),
            ),
        ] {
            let task = handle_keyboard_event(&mut portal, key, modifiers, None);

            assert_eq!(task.units(), 0);
            assert!(portal.ui.terminal_captured);
            assert_eq!(portal.ui.focus_section, FocusSection::Content);
            assert_eq!(portal.active_tab, active_tab);
            assert_eq!(portal.tabs.len(), 2);
        }
    }

    #[test]
    fn captured_terminal_keeps_ctrl_escape_exit() {
        let mut portal = captured_terminal_portal();
        let _ = handle_keyboard_event(
            &mut portal,
            Key::Named(keyboard::key::Named::Escape),
            keyboard::Modifiers::CTRL,
            None,
        );

        assert!(!portal.ui.terminal_captured);
        assert_eq!(portal.ui.focus_section, FocusSection::Content);
    }

    fn sftp_entry(name: &str, is_dir: bool, is_symlink: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink,
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn portal_hub_clipboard_modifier_accepts_platform_command() {
        assert!(is_portal_hub_clipboard_modifier(
            keyboard::Modifiers::COMMAND
        ));
    }

    #[test]
    fn portal_hub_clipboard_modifier_accepts_super_on_linux() {
        assert!(is_portal_hub_clipboard_modifier(keyboard::Modifiers::LOGO));
    }

    #[test]
    fn portal_hub_clipboard_modifier_rejects_shift_only() {
        assert!(!is_portal_hub_clipboard_modifier(
            keyboard::Modifiers::SHIFT
        ));
    }

    #[test]
    fn sftp_enter_navigation_path_allows_real_directory() {
        let entries = vec![sftp_entry("dir", true, false)];

        assert_eq!(
            sftp_enter_navigation_path(Some(0), &entries),
            Some(PathBuf::from("dir"))
        );
    }

    #[test]
    fn sftp_enter_navigation_path_rejects_symlinked_directory() {
        let entries = vec![sftp_entry("linked-dir", true, true)];

        assert_eq!(sftp_enter_navigation_path(Some(0), &entries), None);
    }

    #[test]
    fn sftp_enter_navigation_path_rejects_files_and_missing_selection() {
        let entries = vec![sftp_entry("file.txt", false, false)];

        assert_eq!(sftp_enter_navigation_path(Some(0), &entries), None);
        assert_eq!(sftp_enter_navigation_path(None, &entries), None);
        assert_eq!(sftp_enter_navigation_path(Some(1), &entries), None);
    }
}
