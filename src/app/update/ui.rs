//! UI state message handlers

use iced::Task;
use iced::keyboard::{self, Key};

use crate::app::ActiveDialog;
use crate::app::{FocusSection, Portal, SIDEBAR_AUTO_COLLAPSE_THRESHOLD, View};
use crate::app::services;
use crate::keybindings::AppAction;
use crate::message::{
    DialogMessage, HistoryMessage, HostMessage, Message, SessionMessage, SftpMessage,
    SidebarMenuItem, TabMessage, UiMessage, VncMessage,
};
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::dialogs::host_dialog::host_dialog_field_id;
use crate::views::sftp::PaneId;
use crate::views::toast::Toast;

/// Handle UI state messages
pub fn handle_ui(portal: &mut Portal, msg: UiMessage) -> Task<Message> {
    match msg {
        UiMessage::SearchChanged(query) => {
            portal.ui.search_query = query;
            Task::none()
        }
        UiMessage::FolderToggle(id) => {
            if let Some(group) = portal.config.hosts.find_group_mut(id) {
                group.collapsed = !group.collapsed;
                if let Err(e) = portal.config.hosts.save() {
                    tracing::error!("Failed to save config: {}", e);
                }
            }
            Task::none()
        }
        UiMessage::SidebarItemSelect(item) => {
            // Auto-close pristine SFTP tab when navigating away (not when staying on SFTP)
            if item != SidebarMenuItem::Sftp {
                if let View::DualSftp(tab_id) = portal.ui.active_view {
                    if let Some(state) = portal.sftp.get_tab(tab_id) {
                        if state.is_pristine() {
                            portal.close_tab(tab_id);
                        }
                    }
                }
            }

            // Release keyboard capture when navigating via sidebar
            portal.ui.terminal_captured = false;

            portal.ui.sidebar_selection = item;
            tracing::info!("Sidebar item selected");
            match item {
                SidebarMenuItem::Hosts => {
                    portal.restore_sidebar_after_session();
                    portal.enter_host_grid();
                    return iced::widget::operation::focus(crate::app::search_input_id());
                }
                SidebarMenuItem::History => {
                    portal.restore_sidebar_after_session();
                    portal.enter_host_grid();
                }
                SidebarMenuItem::Sftp => {
                    if let Some(tab_id) = portal.sftp.first_tab_id() {
                        portal.set_active_tab(tab_id);
                    } else {
                        return portal.update(Message::Sftp(SftpMessage::Open));
                    }
                }
                SidebarMenuItem::Settings => {
                    // Open settings page view instead of dialog
                    portal.ui.active_view = View::Settings;
                }
                SidebarMenuItem::Snippets => {
                    // Navigate to snippets page
                    portal.ui.active_view = View::Snippets;
                    portal.snippets.editing = None;
                    portal.restore_sidebar_after_session();
                }
                SidebarMenuItem::About => {
                    portal.dialogs.open_about();
                }
            }
            Task::none()
        }
        UiMessage::SidebarToggleCollapse => {
            portal.ui.sidebar_state = portal.ui.sidebar_state.next();
            portal.ui.sidebar_manually_set = true;
            tracing::info!("Sidebar state updated (manual)");
            Task::none()
        }
        UiMessage::ThemeChange(theme_id) => {
            portal.prefs.theme_id = theme_id;
            portal.save_settings();
            Task::none()
        }
        UiMessage::FontChange(font) => {
            tracing::info!("Font changed");
            portal.prefs.terminal_font = font;
            portal.save_settings();
            Task::none()
        }
        UiMessage::FontSizeChange(size) => {
            portal.prefs.terminal_font_size = size;
            portal.save_settings();
            Task::none()
        }
        UiMessage::UiScaleChange(scale) => {
            // Clamp scale to valid range (0.8 to 1.5)
            let clamped_scale = scale.clamp(0.8, 1.5);
            portal.prefs.ui_scale_override = Some(clamped_scale);
            portal.save_settings();
            Task::none()
        }
        UiMessage::UiScaleReset => {
            portal.prefs.ui_scale_override = None;
            portal.save_settings();
            Task::none()
        }
        UiMessage::SnippetHistoryEnabled(enabled) => {
            portal.config.snippet_history.enabled = enabled;
            portal.save_snippet_history();
            Task::none()
        }
        UiMessage::SnippetHistoryStoreCommand(store_command) => {
            portal.config.snippet_history.store_command = store_command;
            portal.save_snippet_history();
            Task::none()
        }
        UiMessage::SnippetHistoryStoreOutput(store_output) => {
            portal.config.snippet_history.store_output = store_output;
            portal.save_snippet_history();
            Task::none()
        }
        UiMessage::SnippetHistoryRedactOutput(redact_output) => {
            portal.config.snippet_history.redact_output = redact_output;
            portal.save_snippet_history();
            Task::none()
        }
        UiMessage::SessionLoggingEnabled(enabled) => {
            portal.prefs.session_logging_enabled = enabled;
            portal.save_settings();
            Task::none()
        }
        UiMessage::CredentialTimeoutChange(timeout_seconds) => {
            let clamped = timeout_seconds.min(3600);
            portal.prefs.credential_timeout = clamped;
            portal.save_settings();
            // Apply to the global in-memory cache for future entries.
            services::connection::init_passphrase_cache(clamped);
            Task::none()
        }
        UiMessage::WindowResized(size) => {
            portal.ui.window_size = size;
            if !portal.ui.sidebar_manually_set {
                use crate::app::SidebarState;
                portal.ui.sidebar_state = if size.width < SIDEBAR_AUTO_COLLAPSE_THRESHOLD {
                    SidebarState::IconsOnly
                } else {
                    SidebarState::Expanded
                };
            }
            if portal.prefs.vnc_settings.remote_resize {
                if let View::VncViewer(session_id) = portal.ui.active_view {
                    if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                        if let Some((w, h)) = portal.vnc_target_size() {
                            vnc.session.try_request_desktop_size(w, h);
                        }
                    }
                }
            }
            Task::none()
        }
        UiMessage::ToastDismiss(id) => {
            portal.toast_manager.dismiss(id);
            Task::none()
        }
        UiMessage::ToastTick => {
            portal.toast_manager.cleanup_expired();
            Task::none()
        }
        UiMessage::KeyboardEvent(key, modifiers) => handle_keyboard_event(portal, key, modifiers),
        UiMessage::KeyReleased(key, modifiers) => {
            // Forward key release to VNC session if active
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
                } else {
                    // Apply the same Shift mapping as key press so we release
                    // the correct keysym (avoids phantom key stuck on server).
                    if modifiers.shift() {
                        if let Key::Character(c) = &key {
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
                            key
                        }
                    } else {
                        key
                    }
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
    }
}

/// Handle keyboard shortcuts
fn handle_keyboard_event(
    portal: &mut Portal,
    key: Key,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    // Priority 1: Dialog open - handle dialog-specific keyboard navigation
    if portal.dialogs.is_open() {
        return handle_dialog_keyboard(portal, &key, &modifiers);
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
        if modifiers.control() && modifiers.shift() {
            if let Key::Character(c) = &key {
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
                if let Key::Character(c) = &key {
                    match c.chars().next() {
                        Some(ch) if ch.is_ascii_lowercase() => {
                            Key::Character(ch.to_ascii_uppercase().to_string().into())
                        }
                        Some(ch) => {
                            // Map US-layout shifted characters (e.g. '2' -> '@')
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

    // Priority 3: Terminal captured - only Ctrl+Escape exits
    if portal.ui.terminal_captured {
        // Ctrl+Escape exits captured mode
        if let Key::Named(keyboard::key::Named::Escape) = &key {
            if modifiers.control() {
                portal.ui.terminal_captured = false;
                portal.ui.focus_section = FocusSection::Content;
                return Task::none();
            }
        }
        // Ctrl+Shift+K installs SSH key - allow this through to global shortcuts
        if let Key::Character(c) = &key {
            if modifiers.control() && modifiers.shift() && (c.as_str() == "k" || c.as_str() == "K")
            {
                // Fall through to global shortcuts below
            } else {
                return Task::none();
            }
        } else {
            return Task::none();
        }
    }

    // Priority 3: Global shortcuts (always work unless terminal captured)
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
            if let View::Terminal(session_id) = portal.ui.active_view {
                if portal.sessions.contains(session_id) {
                    return portal.update(Message::Session(SessionMessage::InstallKey(session_id)));
                }
            }
            return Task::none();
        }
        _ => {}
    }

    if let Some(task) = handle_configured_actions(
        portal,
        &key,
        &modifiers,
        &[
            AppAction::NewWindow,
            AppAction::NewConnection,
            AppAction::CloseSession,
            AppAction::NewTab,
            AppAction::NextSession,
            AppAction::PreviousSession,
            AppAction::ToggleFullscreen,
        ],
    ) {
        return task;
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
        _ => {
            // Other dialogs: just handle Escape
            if let Key::Named(keyboard::key::Named::Escape) = key {
                portal.dialogs.close();
            }
            Task::none()
        }
    }
}

/// Number of sidebar menu items
const SIDEBAR_MENU_COUNT: usize = 6;

/// Handle keyboard navigation in sidebar
fn handle_sidebar_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            portal.ui.sidebar_focus_index = portal.ui.sidebar_focus_index.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            portal.ui.sidebar_focus_index =
                (portal.ui.sidebar_focus_index + 1).min(SIDEBAR_MENU_COUNT - 1);
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.ui.sidebar_focus_index = 0;
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.ui.sidebar_focus_index = SIDEBAR_MENU_COUNT - 1;
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            portal.ui.focus_section = FocusSection::Content;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            let item = match portal.ui.sidebar_focus_index {
                0 => SidebarMenuItem::Hosts,
                1 => SidebarMenuItem::Sftp,
                2 => SidebarMenuItem::Snippets,
                3 => SidebarMenuItem::History,
                4 => SidebarMenuItem::Settings,
                5 => SidebarMenuItem::About,
                _ => return Task::none(),
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
            | SidebarMenuItem::Snippets
            | SidebarMenuItem::Settings
            | SidebarMenuItem::About => handle_host_grid_keyboard(portal, key, modifiers),
            SidebarMenuItem::History => handle_history_keyboard(portal, key, modifiers),
        },
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
        if let Key::Character(c) = key {
            if c.as_str() == "/" {
                portal.ui.host_grid_focus_index = None; // Clear grid focus when focusing search
                return iced::widget::operation::focus(crate::views::host_grid::search_input_id());
            }
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
            if let Some(idx) = portal.ui.history_focus_index {
                if let Some(entry) = portal.config.history.entries.get(idx) {
                    let host_id = entry.host_id;
                    return portal.update(Message::History(HistoryMessage::Reconnect(host_id)));
                }
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
        Key::Named(keyboard::key::Named::ArrowUp) => {
            if visible_count > 0 {
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
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            if visible_count > 0 {
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
        }
        Key::Named(keyboard::key::Named::Home) => {
            if visible_count > 0 {
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
        }
        Key::Named(keyboard::key::Named::End) => {
            if visible_count > 0 {
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
            if let Some(idx) = pane_state.last_selected_index {
                if let Some(entry) = pane_state.entries.get(idx) {
                    if entry.is_dir {
                        let path = entry.path.clone();
                        return portal.update(Message::Sftp(SftpMessage::PaneNavigate(
                            tab_id,
                            active_pane,
                            path,
                        )));
                    }
                }
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
