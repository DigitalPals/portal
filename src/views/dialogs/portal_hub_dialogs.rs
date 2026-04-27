//! Portal Hub onboarding and sync conflict dialogs.

use iced::widget::Id;
use iced::widget::{Column, Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use crate::config::settings::PortalHubSettings;
use crate::hub::sync::{ConflictChoice, PortalHubSyncService, SyncConflict};
use crate::message::{DialogMessage, Message, UiMessage};
use crate::theme::{STATUS_FAILURE, ScaledFonts, Theme};

use super::common::{
    alert_dialog, dialog_backdrop, dialog_input_style, primary_button_style, secondary_button_style,
};

pub fn portal_hub_onboarding_dialog_view(
    settings: &PortalHubSettings,
    auth_user: Option<&str>,
    auth_error: Option<&str>,
    auth_loading: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let host = settings.host.clone();
    let web_port = settings.web_port.to_string();
    let web_url = settings.effective_web_url();

    let auth_status = if auth_loading {
        text("Waiting for browser sign-in...")
            .size(fonts.label)
            .color(theme.text_muted)
    } else if let Some(user) = auth_user {
        text(format!("Signed in as {}", user))
            .size(fonts.label)
            .color(theme.text_secondary)
    } else if let Some(error) = auth_error {
        text(error.to_string())
            .size(fonts.label)
            .color(STATUS_FAILURE)
    } else {
        text("Not signed in")
            .size(fonts.label)
            .color(theme.text_muted)
    };

    let auth_button = button(
        text(if auth_loading {
            "Waiting"
        } else {
            "Authenticate"
        })
        .size(fonts.body),
    )
    .padding([8, 16])
    .style(primary_button_style(theme))
    .on_press_maybe((!auth_loading).then_some(Message::Ui(UiMessage::PortalHubAuthenticate)));

    let finish_section = if auth_user.is_some() {
        column![
            text("All Portal Hub sync services are enabled automatically.")
                .size(fonts.label)
                .color(theme.text_muted),
            row![
                button(text("Finish").size(fonts.body))
                    .padding([8, 18])
                    .style(primary_button_style(theme))
                    .on_press(Message::Ui(UiMessage::PortalHubSyncNow)),
                button(text("Close").size(fonts.body))
                    .padding([8, 18])
                    .style(secondary_button_style(theme))
                    .on_press(Message::Dialog(DialogMessage::Close)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(10)
    } else {
        column![]
    };

    let content = column![
        text("Set Up Portal Hub")
            .size(fonts.dialog_title)
            .color(theme.text_primary),
        text("Portal Hub keeps SSH sessions alive through a private proxy, stores encrypted key vault items, and syncs hosts, settings, and snippets between devices.")
            .size(fonts.body)
            .color(theme.text_secondary),
        Space::new().height(8),
        section_title("Connection", theme, fonts),
        labeled_input(
            PortalHubOnboardingField::Host,
            "Host / IP",
            host,
            UiMessage::PortalHubHostChanged,
            theme,
            fonts
        ),
        labeled_input(
            PortalHubOnboardingField::WebPort,
            "Web port",
            web_port,
            UiMessage::PortalHubWebPortChanged,
            theme,
            fonts
        ),
        labeled_input(
            PortalHubOnboardingField::WebUrl,
            "Web URL",
            web_url.clone(),
            UiMessage::PortalHubWebUrlChanged,
            theme,
            fonts
        ),
        text(format!("Portal will authenticate through {}", web_url))
            .size(fonts.label)
            .color(theme.text_muted),
        row![auth_status, Space::new().width(Length::Fill), auth_button].align_y(Alignment::Center),
        Space::new().height(6),
        finish_section,
    ]
    .spacing(10)
    .padding(24)
    .width(Length::Fixed(620.0));

    dialog_backdrop(scrollable(content).height(Length::Shrink), theme)
}

pub fn portal_hub_disable_sync_dialog_view(
    service: PortalHubSyncService,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let actions = row![
        button(text("Cancel").size(fonts.body))
            .padding([8, 18])
            .style(secondary_button_style(theme))
            .on_press(Message::Dialog(DialogMessage::Close)),
        Space::new().width(Length::Fill),
        button(text("Keep data").size(fonts.body))
            .padding([8, 18])
            .style(secondary_button_style(theme))
            .on_press(Message::Ui(UiMessage::PortalHubDisableSyncKeepData(
                service
            ))),
        button(text("Delete data").size(fonts.body))
            .padding([8, 18])
            .style(primary_button_style(theme))
            .on_press(Message::Ui(UiMessage::PortalHubDisableSyncDeleteData(
                service
            ))),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    alert_dialog(
        format!("Disable {}?", service.label()),
        format!(
            "Do you want to delete {} already stored in Portal Hub, or only stop syncing it from this device?",
            service.stored_data_label()
        ),
        actions,
        theme,
        fonts,
    )
}

pub fn portal_hub_conflict_dialog_view(
    conflicts: &[SyncConflict],
    choices: &[ConflictChoice],
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let rows: Vec<Element<'static, Message>> = conflicts
        .iter()
        .enumerate()
        .map(|(index, conflict)| {
            let choice = choices.get(index).copied().unwrap_or(ConflictChoice::Local);
            container(
                column![
                    text(conflict.service.to_uppercase())
                        .size(fonts.body)
                        .color(theme.text_primary),
                    text("This service changed locally and on Portal Hub.")
                        .size(fonts.label)
                        .color(theme.text_muted),
                    row![
                        button(text("Keep local").size(fonts.label))
                            .padding([6, 12])
                            .style(conflict_button_style(
                                theme,
                                choice == ConflictChoice::Local
                            ))
                            .on_press(Message::Ui(UiMessage::PortalHubConflictChoiceChanged(
                                index,
                                ConflictChoice::Local,
                            ))),
                        button(text("Use Hub").size(fonts.label))
                            .padding([6, 12])
                            .style(conflict_button_style(theme, choice == ConflictChoice::Hub))
                            .on_press(Message::Ui(UiMessage::PortalHubConflictChoiceChanged(
                                index,
                                ConflictChoice::Hub,
                            ))),
                    ]
                    .spacing(8)
                ]
                .spacing(8),
            )
            .padding(12)
            .style(move |_theme| iced::widget::container::Style {
                background: Some(theme.surface.into()),
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .into()
        })
        .collect();

    let content = column![
        text("Resolve Portal Hub Conflicts")
            .size(fonts.dialog_title)
            .color(theme.text_primary),
        text("Choose which version should win for each service, then apply the resolution.")
            .size(fonts.body)
            .color(theme.text_secondary),
        scrollable(Column::with_children(rows).spacing(10)).height(Length::Fixed(260.0)),
        row![
            button(text("Cancel").size(fonts.body))
                .padding([8, 18])
                .style(secondary_button_style(theme))
                .on_press(Message::Dialog(DialogMessage::Close)),
            Space::new().width(Length::Fill),
            button(text("Apply").size(fonts.body))
                .padding([8, 18])
                .style(primary_button_style(theme))
                .on_press(Message::Ui(UiMessage::PortalHubResolveConflicts)),
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(14)
    .padding(24)
    .width(Length::Fixed(560.0));

    dialog_backdrop(content, theme)
}

fn section_title(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    text(label)
        .size(fonts.section)
        .color(theme.text_primary)
        .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalHubOnboardingField {
    Host,
    WebPort,
    WebUrl,
}

pub fn portal_hub_onboarding_field_id(field: PortalHubOnboardingField) -> Id {
    match field {
        PortalHubOnboardingField::Host => Id::new("portal-hub-onboarding-host"),
        PortalHubOnboardingField::WebPort => Id::new("portal-hub-onboarding-web-port"),
        PortalHubOnboardingField::WebUrl => Id::new("portal-hub-onboarding-web-url"),
    }
}

pub fn portal_hub_onboarding_field_from_id(id: &Id) -> Option<PortalHubOnboardingField> {
    [
        PortalHubOnboardingField::Host,
        PortalHubOnboardingField::WebPort,
        PortalHubOnboardingField::WebUrl,
    ]
    .into_iter()
    .find(|field| portal_hub_onboarding_field_id(*field) == *id)
}

fn labeled_input(
    field: PortalHubOnboardingField,
    label: &'static str,
    value: String,
    message: fn(String) -> UiMessage,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    column![
        text(label).size(fonts.label).color(theme.text_muted),
        text_input(label, &value)
            .id(portal_hub_onboarding_field_id(field))
            .on_input(move |value| Message::Ui(message(value)))
            .padding([8, 10])
            .style(dialog_input_style(theme))
    ]
    .spacing(4)
    .into()
}

fn conflict_button_style(
    theme: Theme,
    selected: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_iced_theme, status| {
        let background = if selected {
            theme.accent
        } else if matches!(status, iced::widget::button::Status::Hovered) {
            theme.hover
        } else {
            theme.surface
        };
        iced::widget::button::Style {
            background: Some(background.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_hub_onboarding_field_ids_roundtrip() {
        for field in [
            PortalHubOnboardingField::Host,
            PortalHubOnboardingField::WebPort,
            PortalHubOnboardingField::WebUrl,
        ] {
            assert_eq!(
                portal_hub_onboarding_field_from_id(&portal_hub_onboarding_field_id(field)),
                Some(field)
            );
        }
    }
}
