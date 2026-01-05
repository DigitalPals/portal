//! Snippets manager dialog

use iced::widget::{button, column, container, row, scrollable, text, text_input, Column, Space};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::config::Snippet;
use crate::message::{Message, SnippetField};
use crate::theme::{Theme, BORDER_RADIUS};

/// State for the snippets dialog
#[derive(Debug, Clone)]
pub struct SnippetsDialogState {
    pub snippets: Vec<Snippet>,
    pub selected_id: Option<Uuid>,
    pub editing: bool,
    // Edit form fields
    pub edit_name: String,
    pub edit_command: String,
    pub edit_description: String,
}

impl SnippetsDialogState {
    pub fn new(snippets: Vec<Snippet>) -> Self {
        Self {
            snippets,
            selected_id: None,
            editing: false,
            edit_name: String::new(),
            edit_command: String::new(),
            edit_description: String::new(),
        }
    }

    pub fn start_new(&mut self) {
        self.selected_id = None;
        self.editing = true;
        self.edit_name = String::new();
        self.edit_command = String::new();
        self.edit_description = String::new();
    }

    pub fn start_edit(&mut self, snippet: &Snippet) {
        self.selected_id = Some(snippet.id);
        self.editing = true;
        self.edit_name = snippet.name.clone();
        self.edit_command = snippet.command.clone();
        self.edit_description = snippet.description.clone().unwrap_or_default();
    }

    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.edit_name.clear();
        self.edit_command.clear();
        self.edit_description.clear();
    }

    pub fn is_form_valid(&self) -> bool {
        !self.edit_name.trim().is_empty() && !self.edit_command.trim().is_empty()
    }
}

/// Build the snippets dialog view
pub fn snippets_dialog_view(state: &SnippetsDialogState, theme: Theme) -> Element<'static, Message> {
    let title = text("Snippets").size(20).color(theme.text_primary);

    let content = if state.editing {
        snippet_edit_form(state, theme)
    } else {
        snippet_list_view(state, theme)
    };

    let form = column![title, Space::with_height(16), content,]
        .spacing(0)
        .padding(24)
        .width(Length::Fixed(500.0));

    dialog_backdrop(form, theme)
}

fn snippet_list_view(state: &SnippetsDialogState, theme: Theme) -> Element<'static, Message> {
    // Snippet list
    let snippet_items: Vec<Element<'static, Message>> = state
        .snippets
        .iter()
        .map(|snippet| {
            let is_selected = state.selected_id == Some(snippet.id);
            let snippet_id = snippet.id;
            let name = snippet.name.clone();
            let command = snippet.command.clone();

            let content = column![
                text(name).size(14).color(theme.text_primary),
                text(command).size(12).color(theme.text_muted),
            ]
            .spacing(2);

            let bg_color = if is_selected {
                Some(theme.selected.into())
            } else {
                None
            };

            button(
                container(content)
                    .padding([8, 12])
                    .width(Length::Fill),
            )
            .style(move |_theme, status| {
                let background = match status {
                    button::Status::Hovered if !is_selected => Some(theme.hover.into()),
                    _ => bg_color,
                };
                button::Style {
                    background,
                    text_color: theme.text_primary,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            })
            .padding(0)
            .width(Length::Fill)
            .on_press(Message::SnippetSelect(snippet_id))
            .into()
        })
        .collect();

    let list: Element<'static, Message> = if snippet_items.is_empty() {
        container(
            text("No snippets yet. Click 'New' to create one.")
                .size(14)
                .color(theme.text_muted),
        )
        .padding(20)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .into()
    } else {
        scrollable(
            Column::with_children(snippet_items)
                .spacing(4)
                .width(Length::Fill),
        )
        .height(Length::Fixed(300.0))
        .into()
    };

    // Action buttons
    let new_btn = button(text("New").size(12).color(theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => theme.accent,
                _ => theme.surface,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .padding([6, 14])
        .on_press(Message::SnippetNew);

    let edit_btn = button(text("Edit").size(12).color(theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => theme.hover,
                button::Status::Disabled => theme.surface,
                _ => theme.surface,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .padding([6, 14])
        .on_press_maybe(state.selected_id.map(Message::SnippetEdit));

    let delete_btn = button(text("Delete").size(12).color(theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(200, 60, 60),
                button::Status::Disabled => theme.surface,
                _ => theme.surface,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .padding([6, 14])
        .on_press_maybe(state.selected_id.map(Message::SnippetDelete));

    let insert_btn = button(text("Insert").size(12).color(theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => theme.accent,
                button::Status::Disabled => theme.surface,
                _ => theme.accent,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([6, 14])
        .on_press_maybe(state.selected_id.map(Message::SnippetInsert));

    let close_btn = button(text("Close").size(12).color(theme.text_primary))
        .style(move |_theme, _status| button::Style {
            background: Some(theme.surface.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .padding([6, 14])
        .on_press(Message::DialogClose);

    let action_row = row![new_btn, edit_btn, delete_btn, Space::with_width(Length::Fill), insert_btn, close_btn,]
        .spacing(8)
        .align_y(Alignment::Center);

    column![list, Space::with_height(16), action_row,].spacing(0).into()
}

fn snippet_edit_form(state: &SnippetsDialogState, theme: Theme) -> Element<'static, Message> {
    let title = if state.selected_id.is_some() {
        "Edit Snippet"
    } else {
        "New Snippet"
    };

    let title_text = text(title).size(14).color(theme.text_muted);

    let name_input = column![
        text("Name").size(12).color(theme.text_secondary),
        text_input("e.g., List Files", &state.edit_name)
            .on_input(|s| Message::SnippetFieldChanged(SnippetField::Name, s))
            .padding(8)
            .width(Length::Fill),
    ]
    .spacing(4);

    let command_input = column![
        text("Command").size(12).color(theme.text_secondary),
        text_input("e.g., ls -la", &state.edit_command)
            .on_input(|s| Message::SnippetFieldChanged(SnippetField::Command, s))
            .padding(8)
            .width(Length::Fill),
    ]
    .spacing(4);

    let description_input = column![
        text("Description (optional)").size(12).color(theme.text_secondary),
        text_input("Optional description", &state.edit_description)
            .on_input(|s| Message::SnippetFieldChanged(SnippetField::Description, s))
            .padding(8)
            .width(Length::Fill),
    ]
    .spacing(4);

    let is_valid = state.is_form_valid();

    let cancel_btn = button(text("Cancel").size(12).color(theme.text_primary))
        .style(move |_theme, _status| button::Style {
            background: Some(theme.surface.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .padding([6, 14])
        .on_press(Message::SnippetEditCancel);

    let save_btn = button(text("Save").size(12).color(theme.text_primary))
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Disabled => theme.surface,
                _ => theme.accent,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([6, 14])
        .on_press_maybe(if is_valid {
            Some(Message::SnippetSave)
        } else {
            None
        });

    let button_row = row![Space::with_width(Length::Fill), cancel_btn, save_btn,].spacing(8);

    column![
        title_text,
        Space::with_height(16),
        name_input,
        command_input,
        description_input,
        Space::with_height(16),
        button_row,
    ]
    .spacing(12)
    .into()
}

/// Helper to wrap dialog content in a backdrop
fn dialog_backdrop(
    content: impl Into<Element<'static, Message>>,
    theme: Theme,
) -> Element<'static, Message> {
    let dialog_box = container(content)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: (BORDER_RADIUS * 2.0).into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.5),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });

    container(
        container(dialog_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.7).into()),
        ..Default::default()
    })
    .into()
}
