//! Portal Hub onboarding and sync conflict dialogs.

use iced::widget::Id;
use iced::widget::{
    Column, Space, button, checkbox, column, container, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::app::PortalHubWizardState;
use crate::config::HostsConfig;
use crate::config::settings::PortalHubSettings;
use crate::hub::sync::{ConflictChoice, PortalHubSyncService, SyncConflict};
use crate::message::{DialogMessage, Message, UiMessage};
use crate::proxy::ProxyStatus;
use crate::theme::{ScaledFonts, Theme};
use crate::views::components::{BadgeTone, status_badge};

use super::common::{
    alert_dialog, dialog_backdrop, dialog_input_style, primary_button_style, secondary_button_style,
};

#[allow(clippy::too_many_arguments)]
pub fn portal_hub_onboarding_dialog_view(
    settings: &PortalHubSettings,
    status: Option<ProxyStatus>,
    status_error: Option<&str>,
    status_loading: bool,
    auth_user: Option<&str>,
    auth_error: Option<&str>,
    auth_loading: bool,
    wizard: &PortalHubWizardState,
    hosts: &HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let hub_url_value = wizard.hub_url_input.clone();
    let has_hub_url = !hub_url_value.trim().is_empty();
    let connection_ready = has_hub_url && status.is_some() && !wizard.connection_dirty;
    let signed_in = auth_user.is_some() && !wizard.connection_dirty;
    let checked_url = settings.effective_web_url();

    let (setup_badge, setup_tone, setup_detail) = if wizard.connection_dirty {
        if has_hub_url {
            (
                "Not checked",
                BadgeTone::Neutral,
                "Press Enter or check setup to apply this address.".to_string(),
            )
        } else {
            (
                "Address needed",
                BadgeTone::Neutral,
                "Enter the URL or address of your Portal Hub.".to_string(),
            )
        }
    } else if status_loading {
        (
            "Checking",
            BadgeTone::Info,
            "Connecting to Portal Hub...".to_string(),
        )
    } else if let Some(status) = status.as_ref() {
        (
            "Ready",
            BadgeTone::Success,
            format!(
                "Portal Hub v{} · API {} · proxy {} · sync {}",
                status.version,
                status.api_version,
                capability_label(status.web_proxy),
                capability_label(status.sync_v2)
            ),
        )
    } else if let Some(error) = status_error {
        ("Needs attention", BadgeTone::Danger, error.to_string())
    } else {
        (
            "Not checked",
            BadgeTone::Neutral,
            "Check the connection before signing in.".to_string(),
        )
    };

    let (auth_badge, auth_tone, auth_detail) = if wizard.connection_dirty {
        (
            "Waiting",
            BadgeTone::Neutral,
            "Check the new address to continue.".to_string(),
        )
    } else if auth_loading {
        (
            "Browser open",
            BadgeTone::Info,
            "Finish signing in through your browser.".to_string(),
        )
    } else if let Some(user) = auth_user {
        ("Connected", BadgeTone::Success, user.to_string())
    } else if let Some(error) = auth_error {
        ("Sign-in failed", BadgeTone::Danger, error.to_string())
    } else if connection_ready {
        (
            "Ready",
            BadgeTone::Info,
            "Connection verified. Sign in to link this device.".to_string(),
        )
    } else {
        (
            "Locked",
            BadgeTone::Neutral,
            "Available after the connection check passes.".to_string(),
        )
    };

    let check_button = button(
        text(if status_loading {
            "Checking"
        } else if connection_ready {
            "Check again"
        } else {
            "Check setup"
        })
        .size(fonts.body),
    )
    .padding([8, 16])
    .style(secondary_button_style(theme))
    .on_press_maybe(
        (has_hub_url && !status_loading && !auth_loading)
            .then_some(Message::Ui(UiMessage::PortalHubCheckStatus)),
    );

    let auth_button = button(
        text(if signed_in {
            "Signed in"
        } else if auth_loading {
            "Waiting"
        } else {
            "Sign in"
        })
        .size(fonts.body),
    )
    .padding([8, 16])
    .style(primary_button_style(theme))
    .on_press_maybe(
        (!signed_in && connection_ready && !auth_loading && !status_loading)
            .then_some(Message::Ui(UiMessage::PortalHubAuthenticate)),
    );

    // Defaults step: explicit consent for routing, vault preference, and
    // sync — nothing switches on silently. Includes a bulk host review.
    let finish_section = if signed_in {
        let eligible: Vec<_> = hosts
            .hosts
            .iter()
            .filter(|host| host.hub_eligible())
            .collect();
        let selected_count = eligible
            .iter()
            .filter(|host| !wizard.excluded_hosts.contains(&host.id))
            .count();

        let mut host_rows = Column::new().spacing(4);
        for host in &eligible {
            let host_id = host.id;
            let selected = !wizard.excluded_hosts.contains(&host_id);
            let auth_label = match &host.auth {
                crate::config::AuthMethod::Agent => "SSH Agent",
                crate::config::AuthMethod::PublicKey { .. } => "Public Key",
                crate::config::AuthMethod::Password => "Password",
                crate::config::AuthMethod::KeyboardInteractive => "Keyboard Interactive",
            };
            host_rows = host_rows.push(
                row![
                    checkbox(selected)
                        .label(host.name.clone())
                        .on_toggle(move |_| Message::Ui(UiMessage::PortalHubWizardToggleHost(
                            host_id
                        )))
                        .spacing(8),
                    Space::new().width(Length::Fill),
                    text(auth_label)
                        .size(fonts.small)
                        .color(theme.text_tertiary),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }

        let host_list: Element<'static, Message> = if eligible.is_empty() {
            text("No eligible hosts yet — SSH hosts using Agent or Public Key auth appear here.")
                .size(fonts.label)
                .color(theme.text_tertiary)
                .into()
        } else {
            container(scrollable(host_rows).height(Length::Fixed(160.0)))
                .padding(10)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(theme.background.into()),
                    border: iced::Border {
                        color: theme.border,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        };

        let apply_label = if wizard.route_default && selected_count > 0 {
            format!(
                "Enable Hub on {} host{}",
                selected_count,
                if selected_count == 1 { "" } else { "s" }
            )
        } else {
            "Save defaults".to_string()
        };

        column![
            section_title("3. Choose defaults", theme, fonts),
            checkbox(wizard.route_default)
                .label("Route eligible hosts via Portal Hub — sessions survive crashes and sleep")
                .on_toggle(|value| Message::Ui(UiMessage::PortalHubWizardRouteDefault(value)))
                .spacing(8),
            checkbox(wizard.prefer_vault)
                .label("Prefer Vault keys for new public-key hosts")
                .on_toggle(|value| Message::Ui(UiMessage::PortalHubWizardPreferVault(value)))
                .spacing(8),
            checkbox(wizard.sync_all)
                .label("Sync hosts, settings, snippets, and key vault between devices")
                .on_toggle(|value| Message::Ui(UiMessage::PortalHubWizardSyncAll(value)))
                .spacing(8),
            host_list,
            text(format!(
                "{} of {} eligible hosts selected — unchecked hosts stay Direct",
                selected_count,
                eligible.len()
            ))
            .size(fonts.small)
            .color(theme.text_tertiary),
            row![
                button(text("Skip for now").size(fonts.body))
                    .padding([8, 18])
                    .style(secondary_button_style(theme))
                    .on_press(Message::Ui(UiMessage::PortalHubWizardSkip)),
                Space::new().width(Length::Fill),
                button(text(apply_label).size(fonts.body))
                    .padding([8, 18])
                    .style(primary_button_style(theme))
                    .on_press(Message::Ui(UiMessage::PortalHubWizardApply)),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(10)
    } else {
        column![]
    };

    let connection_caption = if wizard.connection_dirty {
        "Changes are kept as a draft until you check setup.".to_string()
    } else if checked_url.is_empty() {
        "Enter a Hub address to get started.".to_string()
    } else {
        format!("Portal will connect to {checked_url}")
    };

    let content = column![
        row![
            text("Set Up Portal Hub")
                .size(fonts.dialog_title)
                .color(theme.text_primary),
            Space::new().width(Length::Fill),
            button(text("Close").size(fonts.label))
                .padding([6, 12])
                .style(secondary_button_style(theme))
                .on_press(Message::Dialog(DialogMessage::Close)),
        ]
        .align_y(Alignment::Center),
        text("Portal Hub keeps SSH sessions alive through a private proxy, stores encrypted key vault items, and syncs hosts, settings, and snippets between devices.")
            .size(fonts.body)
            .color(theme.text_secondary),
        Space::new().height(8),
        section_title("1. Connect", theme, fonts),
        hub_url_input(
            hub_url_value,
            !status_loading && !auth_loading,
            has_hub_url && !status_loading && !auth_loading,
            theme,
            fonts
        ),
        text("Paste a full URL or enter a host. The port is optional; add :port only when your Hub uses a custom web port.")
            .size(fonts.small)
            .color(theme.text_tertiary),
        text(connection_caption)
            .size(fonts.label)
            .color(theme.text_muted),
        row![
            status_badge(setup_badge, setup_tone, theme, fonts),
            text(setup_detail)
                .size(fonts.label)
                .color(theme.text_secondary)
                .width(Length::Fill),
            check_button
        ]
            .spacing(10)
            .align_y(Alignment::Center),
        Space::new().height(6),
        section_title("2. Sign in", theme, fonts),
        row![
            status_badge(auth_badge, auth_tone, theme, fonts),
            text(auth_detail)
                .size(fonts.label)
                .color(theme.text_secondary)
                .width(Length::Fill),
            auth_button
        ]
            .spacing(10)
            .align_y(Alignment::Center),
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

fn capability_label(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalHubOnboardingField {
    WebUrl,
}

pub fn portal_hub_onboarding_field_id(field: PortalHubOnboardingField) -> Id {
    match field {
        PortalHubOnboardingField::WebUrl => Id::new("portal-hub-onboarding-web-url"),
    }
}

pub fn portal_hub_onboarding_field_from_id(id: &Id) -> Option<PortalHubOnboardingField> {
    if portal_hub_onboarding_field_id(PortalHubOnboardingField::WebUrl) == *id {
        Some(PortalHubOnboardingField::WebUrl)
    } else {
        None
    }
}

fn hub_url_input(
    value: String,
    can_edit: bool,
    can_submit: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    column![
        text("Hub URL or address")
            .size(fonts.label)
            .color(theme.text_muted),
        text_input("https://portal-hub.example.ts.net", &value)
            .id(portal_hub_onboarding_field_id(
                PortalHubOnboardingField::WebUrl
            ))
            .on_input_maybe(
                can_edit.then_some(|value| Message::Ui(UiMessage::PortalHubWebUrlChanged(value)))
            )
            .on_submit_maybe(can_submit.then_some(Message::Ui(UiMessage::PortalHubCheckStatus)))
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
            text_color: if selected {
                theme.text_on(background)
            } else {
                theme.text_primary
            },
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
        let field = PortalHubOnboardingField::WebUrl;
        assert_eq!(
            portal_hub_onboarding_field_from_id(&portal_hub_onboarding_field_id(field)),
            Some(field)
        );
    }
}
