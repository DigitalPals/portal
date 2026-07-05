//! Keyboard-interactive authentication prompt dialog.
//!
//! Shows the server-provided name/instructions and one input per prompt
//! (masked when the prompt's echo flag is false). Submit sends the responses
//! back to the waiting auth task; Cancel aborts the connection cleanly.

use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::oneshot;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, Message};
use crate::ssh::auth_prompt::{AuthPrompt, AuthPromptRequest, AuthPromptResponse};
use crate::theme::{BORDER_RADIUS, ScaledFonts, Theme};

use super::common::{
    dialog_backdrop, dialog_input_style, primary_button_style, secondary_button_style,
};

/// State for the keyboard-interactive auth prompt dialog
pub struct AuthPromptDialogState {
    /// Host being authenticated
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Server-provided round name (may be empty)
    pub name: String,
    /// Server-provided instructions (may be empty)
    pub instructions: String,
    /// The prompts for this round
    pub prompts: Vec<AuthPrompt>,
    /// User responses, one per prompt (sensitive — cleared on close)
    pub responses: Vec<SecretString>,
    /// Responder back to the waiting auth task
    responder: Option<oneshot::Sender<AuthPromptResponse>>,
}

impl AuthPromptDialogState {
    pub fn from_request(request: AuthPromptRequest) -> Self {
        let responses = request
            .prompts
            .iter()
            .map(|_| SecretString::from(String::new()))
            .collect();
        Self {
            host: request.host,
            port: request.port,
            username: request.username,
            name: request.name,
            instructions: request.instructions,
            prompts: request.prompts,
            responses,
            responder: Some(request.responder),
        }
    }

    /// Update a single response value
    pub fn set_response(&mut self, index: usize, value: SecretString) {
        if let Some(slot) = self.responses.get_mut(index) {
            *slot = value;
        }
    }

    /// Send the collected responses and consume the responder
    pub fn submit(&mut self) {
        let responses = std::mem::take(&mut self.responses);
        if let Some(responder) = self.responder.take() {
            let _ = responder.send(AuthPromptResponse::Submit(responses));
        }
    }

    /// Cancel authentication (aborts the connection) and clear responses
    pub fn cancel(&mut self) {
        self.clear_responses();
        if let Some(responder) = self.responder.take() {
            let _ = responder.send(AuthPromptResponse::Cancel);
        }
    }

    /// Clear response values (for security)
    pub fn clear_responses(&mut self) {
        for slot in &mut self.responses {
            *slot = SecretString::from(String::new());
        }
    }
}

/// Build the keyboard-interactive auth prompt dialog view
pub fn auth_prompt_dialog_view(
    state: &AuthPromptDialogState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let key_icon = icon_with_color(icons::ui::SERVER, 28, theme.accent);

    let title_text = if state.name.trim().is_empty() {
        "Authentication Required".to_string()
    } else {
        state.name.clone()
    };
    let title = text(title_text)
        .size(fonts.heading)
        .color(theme.text_primary);

    let connection_info = text(format!(
        "{}@{}:{}",
        state.username, state.host, state.port
    ))
    .size(fonts.body)
    .color(theme.text_secondary);

    let mut content_items: Vec<Element<'static, Message>> = vec![
        row![key_icon, title]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
        Space::new().height(8).into(),
        connection_info.into(),
        Space::new().height(8).into(),
    ];

    if !state.instructions.trim().is_empty() {
        let instructions = container(
            text(state.instructions.trim().to_string())
                .size(fonts.small)
                .color(theme.text_primary),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        });
        content_items.push(instructions.into());
        content_items.push(Space::new().height(8).into());
    }

    for (index, prompt) in state.prompts.iter().enumerate() {
        let label = text(prompt.prompt.trim_end().to_string())
            .size(fonts.label)
            .color(theme.text_muted);

        let value = state
            .responses
            .get(index)
            .map(|secret| secret.expose_secret().to_string())
            .unwrap_or_default();

        let mut input = text_input("", &value)
            .size(fonts.body)
            .padding(10)
            .width(Length::Fill)
            .secure(!prompt.echo)
            .style(dialog_input_style(theme))
            .on_input(move |s| {
                Message::Dialog(DialogMessage::AuthPromptInputChanged(
                    index,
                    SecretString::from(s),
                ))
            });
        if index + 1 == state.prompts.len() {
            input = input.on_submit(Message::Dialog(DialogMessage::AuthPromptSubmit));
        }

        content_items.push(label.into());
        content_items.push(Space::new().height(4).into());
        content_items.push(input.into());
        content_items.push(Space::new().height(8).into());
    }

    let cancel_button = button(
        text("Cancel")
            .size(fonts.button_small)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .style(secondary_button_style(theme))
    .on_press(Message::Dialog(DialogMessage::AuthPromptCancel));

    let submit_button = button(text("Submit").size(fonts.button_small))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::AuthPromptSubmit));

    let button_row = row![
        Space::new().width(Length::Fill),
        cancel_button,
        submit_button,
    ]
    .spacing(8);

    content_items.extend([Space::new().height(16).into(), button_row.into()]);

    let content = column(content_items)
        .spacing(4)
        .padding(24)
        .width(Length::Fixed(420.0));

    dialog_backdrop(content, theme)
}
