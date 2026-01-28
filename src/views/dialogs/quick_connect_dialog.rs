use std::collections::HashMap;

use iced::widget::{Space, button, column, pick_list, row, text, text_input};
use iced::{Alignment, Element, Length};

use crate::message::{DialogMessage, Message, QuickConnectField};
use crate::theme::Theme;
use crate::validation::{validate_hostname, validate_port, validate_username};

use super::common::{
    ERROR_COLOR, dialog_backdrop, dialog_input_style_with_error, dialog_pick_list_menu_style,
    dialog_pick_list_style, primary_button_style, secondary_button_style,
};
use super::host_dialog::AuthMethodChoice;

/// State for the quick connect dialog
#[derive(Debug, Clone)]
pub struct QuickConnectDialogState {
    pub hostname: String,
    pub port: String,
    pub username: String,
    pub auth_method: AuthMethodChoice,
    pub validation_errors: HashMap<String, String>,
}

impl Default for QuickConnectDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickConnectDialogState {
    /// Create a new quick connect dialog with defaults
    pub fn new() -> Self {
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_default();

        Self {
            hostname: String::new(),
            port: "22".to_string(),
            username,
            auth_method: AuthMethodChoice::Agent,
            validation_errors: HashMap::new(),
        }
    }

    /// Validate all fields and return whether validation passed.
    /// Updates self.validation_errors with results.
    pub fn validate(&mut self) -> bool {
        self.validation_errors.clear();

        // Validate hostname (required)
        if let Err(e) = validate_hostname(&self.hostname) {
            self.validation_errors
                .insert("hostname".to_string(), e.message);
        }

        // Validate port
        if let Err(e) = validate_port(&self.port) {
            self.validation_errors.insert("port".to_string(), e.message);
        }

        // Validate username (empty is allowed - will default to current user)
        if let Err(e) = validate_username(&self.username) {
            self.validation_errors
                .insert("username".to_string(), e.message);
        }

        self.validation_errors.is_empty()
    }

    /// Check if the form has minimum required fields filled
    pub fn is_valid(&self) -> bool {
        !self.hostname.trim().is_empty() && self.validation_errors.is_empty()
    }

    /// Get validation error for a specific field
    pub fn get_error(&self, field: &str) -> Option<&String> {
        self.validation_errors.get(field)
    }

    /// Get the port as u16, defaulting to 22 if invalid
    pub fn port_u16(&self) -> u16 {
        self.port.parse().unwrap_or(22)
    }

    /// Get the username, defaulting to current user if empty
    pub fn effective_username(&self) -> String {
        if self.username.trim().is_empty() {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        } else {
            self.username.trim().to_string()
        }
    }
}

/// Build the quick connect dialog view
pub fn quick_connect_dialog_view(
    state: &QuickConnectDialogState,
    theme: Theme,
) -> Element<'static, Message> {
    // Clone values to make them owned
    let hostname_value = state.hostname.clone();
    let port_value = state.port.clone();
    let username_value = state.username.clone();
    let auth_method = state.auth_method;
    let is_valid = state.is_valid();
    let username_placeholder = std::env::var("USER").unwrap_or_default();

    // Get validation errors
    let hostname_error = state.get_error("hostname").cloned();
    let port_error = state.get_error("port").cloned();
    let username_error = state.get_error("username").cloned();

    // Hostname input
    let hostname_input = {
        let has_error = hostname_error.is_some();
        let mut col = column![
            text("Hostname / IP").size(12).color(theme.text_secondary),
            text_input("192.168.1.100", &hostname_value)
                .on_input(|s| Message::Dialog(DialogMessage::QuickConnectFieldChanged(
                    QuickConnectField::Hostname,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::QuickConnectSubmit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = hostname_error {
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    // Port input
    let port_input = {
        let has_error = port_error.is_some();
        let mut col = column![
            text("Port").size(12).color(theme.text_secondary),
            text_input("22", &port_value)
                .on_input(|s| Message::Dialog(DialogMessage::QuickConnectFieldChanged(
                    QuickConnectField::Port,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::QuickConnectSubmit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = port_error {
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    // Username input
    let username_input = {
        let has_error = username_error.is_some();
        let mut col = column![
            text("Username").size(12).color(theme.text_secondary),
            text_input(&username_placeholder, &username_value)
                .on_input(|s| Message::Dialog(DialogMessage::QuickConnectFieldChanged(
                    QuickConnectField::Username,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::QuickConnectSubmit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = username_error {
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    // Auth method picker
    let auth_picker = column![
        text("Authentication").size(12).color(theme.text_secondary),
        pick_list(
            AuthMethodChoice::ALL.as_slice(),
            Some(auth_method),
            |choice| Message::Dialog(DialogMessage::QuickConnectFieldChanged(
                QuickConnectField::AuthMethod,
                format!("{:?}", choice)
            ))
        )
        .width(Length::Fill)
        .padding(8)
        .style(dialog_pick_list_style(theme))
        .menu_style(dialog_pick_list_menu_style(theme))
    ]
    .spacing(4);

    // Buttons
    let cancel_button = button(text("Cancel").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::Close));

    let connect_button = button(text("Connect").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press_maybe(if is_valid {
            Some(Message::Dialog(DialogMessage::QuickConnectSubmit))
        } else {
            None
        });

    let button_row = row![
        Space::new().width(Length::Fill),
        cancel_button,
        connect_button,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Form layout
    let form = column![
        text("Quick Connect").size(20).color(theme.text_primary),
        Space::new().height(16),
        row![
            column![hostname_input].width(Length::FillPortion(3)),
            column![port_input].width(Length::FillPortion(1)),
        ]
        .spacing(12),
        username_input,
        auth_picker,
        Space::new().height(16),
        button_row,
    ]
    .spacing(12)
    .padding(24)
    .width(Length::Fixed(400.0));

    dialog_backdrop(form, theme)
}
