//! Portal Hub onboarding and sync conflict dialogs.

use iced::widget::Id;
use iced::widget::{
    Column, Space, button, checkbox, column, container, pick_list, row, scrollable, text,
    text_input,
};
use iced::{Alignment, Element, Length};

use crate::app::PortalHubWizardState;
use crate::config::HostsConfig;
use crate::config::settings::PortalHubSettings;
use crate::hub::deployment::{
    DeploymentAction as Action, ManagedHubState, ManagementMessage as M, SetupChoice,
};
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
    cleanup_warnings: &[String],
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
                "Check the address to continue. Your current Hub stays connected until you finish."
                    .to_string(),
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
        let recovery_hosts = wizard.managed.recovery_hosts(&hosts.hosts);
        let eligible: Vec<_> = hosts
            .hosts
            .iter()
            .filter(|host| host.hub_eligible() && !recovery_hosts.contains(&host.id))
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
            text(if recovery_hosts.is_empty() {
                ""
            } else {
                "Management servers and their jump hosts stay Direct for recovery."
            })
            .size(fonts.small)
            .color(theme.text_secondary),
            text(format!(
                "{} of {} eligible hosts selected — unchecked hosts stay Direct",
                selected_count,
                eligible.len()
            ))
            .size(fonts.small)
            .color(theme.text_tertiary),
            row![
                button(text("Cancel setup").size(fonts.body))
                    .padding([8, 18])
                    .style(secondary_button_style(theme))
                    .on_press(Message::Ui(UiMessage::PortalHubWizardSkip)),
                Space::new().width(Length::Fill),
                button(text(apply_label).size(fonts.body))
                    .padding([8, 18])
                    .style(primary_button_style(theme))
                    .on_press_maybe(
                        wizard
                            .can_finish()
                            .then_some(Message::Ui(UiMessage::PortalHubWizardApply))
                    ),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
        ]
        .spacing(10)
    } else {
        column![]
    };

    let connection_caption = if wizard.connection_dirty {
        "Changes stay private to this setup until you save your choices.".to_string()
    } else if checked_url.is_empty() {
        "Enter a Hub address to get started.".to_string()
    } else {
        format!("Portal will connect to {checked_url}")
    };

    // Server setup connects and signs in by itself: its steps appear only once
    // it hands the new Hub over, to show progress and offer retries.
    let server = wizard.managed.choice == SetupChoice::Server;
    let connect_steps: Element<'static, Message> = if !server || wizard.auto_finish.is_some() {
        column![
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
        .into()
    } else {
        column![].into()
    };
    let choice_style = move |selected: bool| {
        move |iced_theme: &iced::Theme, status| {
            if selected {
                primary_button_style(theme)(iced_theme, status)
            } else {
                secondary_button_style(theme)(iced_theme, status)
            }
        }
    };

    let content = column![
        row![
            text("Set Up or Manage Portal Hub")
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
        row![
            button(text("Connect an existing Hub").size(fonts.body)).padding([8, 12])
                .style(choice_style(!server)).on_press(management(M::Choose(SetupChoice::Existing))),
            button(text("Set up on a server").size(fonts.body)).padding([8, 12])
                .style(choice_style(server)).on_press(management(M::Choose(SetupChoice::Server))),
        ].spacing(10),
        if server {
            let offer_connect = wizard.auto_finish.is_none()
                && wizard.managed.ready_url().is_some_and(|url| wizard.signed_in_url.as_deref() != Some(url.as_str()));
            managed_hub_view(&wizard.managed, wizard.auth_loading, offer_connect, hosts, theme, fonts)
        } else { column![].into() },
        column(cleanup_warnings.iter().map(|warning| text(warning.clone()).size(fonts.label).color(super::common::ERROR_COLOR).into())),
        connect_steps,
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
    use crate::hub::deployment::DeploymentResponse;
    use crate::theme::{ThemeId, get_theme};

    fn onboarding(wizard: &PortalHubWizardState) -> Element<'static, Message> {
        portal_hub_onboarding_dialog_view(
            &wizard.draft,
            None,
            None,
            false,
            None,
            None,
            false,
            wizard,
            &[],
            &HostsConfig::default(),
            get_theme(ThemeId::default()),
            ScaledFonts::new(1.0),
        )
    }

    fn shows(view: Element<'static, Message>, label: &str) -> bool {
        iced_test::simulator(view).find(label).is_ok()
    }

    #[test]
    fn server_setup_hides_the_existing_hub_steps_until_it_hands_over() {
        let mut wizard = PortalHubWizardState::begin(Default::default());
        assert!(shows(onboarding(&wizard), "1. Connect"));
        wizard.managed.choice = SetupChoice::Server;
        assert!(shows(onboarding(&wizard), "Deploy Hub"));
        assert!(!shows(onboarding(&wizard), "1. Connect"));
        assert!(!shows(onboarding(&wizard), "2. Sign in"));
        // The deployment connects this device: its progress and retries show.
        wizard.auto_finish = Some(Default::default());
        assert!(shows(onboarding(&wizard), "1. Connect"));
        assert!(shows(onboarding(&wizard), "2. Sign in"));
    }

    fn installed(image: &str, version: &str) -> DeploymentResponse {
        DeploymentResponse::parse(
            &serde_json::json!({"protocol_version":1,"ok":true,"phase":"ready","message":"Hub is ready",
                "data":{"hub_image":image,"hub_version":version,"owner_exists":true,"owner_ready":true,"active_sessions":0}})
            .to_string(),
        )
        .unwrap()
    }

    fn maintenance(state: &ManagedHubState, response: &DeploymentResponse) -> Vec<String> {
        let mut ui = iced_test::simulator(maintenance_view(
            state,
            response,
            get_theme(ThemeId::default()),
            ScaledFonts::new(1.0),
        ));
        let offer = format!("Update Hub to {}", env!("CARGO_PKG_VERSION"));
        ["Update Hub", "Back up Hub", offer.as_str()]
            .into_iter()
            .filter(|label| ui.find(*label).is_ok())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn maintenance_offers_an_update_only_when_there_is_one() {
        let current = crate::hub::deployment::DEFAULT_IMAGE;
        let mut state = ManagedHubState::default();
        let up_to_date = installed(current, env!("CARGO_PKG_VERSION"));
        assert_eq!(maintenance(&state, &up_to_date), ["Back up Hub"]);

        let old = "ghcr.io/digitalpals/portal-hub:0.6.0";
        state.image = old.into();
        let behind = installed(old, "0.6.0");
        let offer = format!("Update Hub to {}", env!("CARGO_PKG_VERSION"));
        assert_eq!(maintenance(&state, &behind), ["Back up Hub", &offer]);

        // An image chosen in Advanced options is the update target instead.
        state.image = "ghcr.io/digitalpals/portal-hub:9.9.9".into();
        assert_eq!(maintenance(&state, &behind), ["Update Hub", "Back up Hub"]);
        state.image = current.into();
        state.archive = Some("/tmp/portal-hub.tar".into());
        assert_eq!(
            maintenance(&state, &up_to_date),
            ["Update Hub", "Back up Hub"]
        );
    }

    #[test]
    fn advanced_options_hold_overrides_and_diagnostics_only() {
        let mut wizard = PortalHubWizardState::begin(Default::default());
        wizard.managed.choice = SetupChoice::Server;
        wizard.managed.show_advanced = true;
        for label in ["Choose local image tar…", "Clear"] {
            assert!(shows(onboarding(&wizard), label), "{label}");
        }
        // Readiness checks and logs belong to an installed server.
        assert!(!shows(onboarding(&wizard), "Check readiness"));
        wizard.managed.selected = Some(crate::hub::deployment::DeploymentRecord {
            deployment_id: uuid::Uuid::new_v4(),
            ssh_host_id: uuid::Uuid::new_v4(),
            hostname: "portal-hub".into(),
            hub_image: crate::hub::deployment::DEFAULT_IMAGE.into(),
            public_url: None,
            owner_username: None,
        });
        for label in ["Refresh status", "Check readiness", "View diagnostics"] {
            assert!(shows(onboarding(&wizard), label), "{label}");
        }
        // Setup steps run only through Deploy Hub / Resume setup.
        for label in [
            "Check server",
            "Install Hub",
            "Enable private HTTPS",
            "Create owner passkey",
            "Open Tailscale in browser",
            "Verify Hub connection",
        ] {
            assert!(!shows(onboarding(&wizard), label), "{label}");
        }
    }

    #[test]
    fn a_ready_hub_offers_to_connect_this_device_unless_it_is_signed_in() {
        let url = "https://portal-hub.example-tail.ts.net";
        let mut wizard = PortalHubWizardState::begin(Default::default());
        wizard.managed.choice = SetupChoice::Server;
        wizard.managed.response = Some(
            DeploymentResponse::parse(
                &serde_json::json!({"protocol_version":1,"ok":true,"phase":"ready","message":"Hub is ready",
                    "data":{"owner_exists":true,"owner_ready":true,"public_url":url}})
                .to_string(),
            )
            .unwrap(),
        );
        assert!(shows(onboarding(&wizard), "Connect this device"));
        assert!(!shows(onboarding(&wizard), "Deploy Hub"));
        wizard.signed_in_url = Some(url.into());
        assert!(!shows(onboarding(&wizard), "Connect this device"));
    }

    #[test]
    fn portal_hub_onboarding_field_ids_roundtrip() {
        let field = PortalHubOnboardingField::WebUrl;
        assert_eq!(
            portal_hub_onboarding_field_from_id(&portal_hub_onboarding_field_id(field)),
            Some(field)
        );
    }
}

fn management(value: M) -> Message {
    Message::Ui(UiMessage::PortalHubManagement(value))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagementHost {
    id: uuid::Uuid,
    label: String,
}
impl std::fmt::Display for ManagementHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn managed_hub_view(
    state: &ManagedHubState,
    // The browser passkey step is running; a new setup link would replace it.
    signing_in: bool,
    // The selected Hub is ready, and this device is neither signed in to it
    // nor being connected to it.
    offer_connect: bool,
    hosts: &HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let choices: Vec<_> = hosts
        .hosts
        .iter()
        .filter(|h| h.protocol == crate::config::Protocol::Ssh)
        .map(|h| ManagementHost {
            id: h.id,
            label: format!("{} — {}@{}", h.name, h.effective_username(), h.hostname),
        })
        .collect();
    let selected = choices
        .iter()
        .find(|h| Some(h.id) == state.host_id)
        .cloned();
    let mut content = column![
        section_title("Your server", theme, fonts),
        text("Manage this Hub through SSH, including its jump hosts. Keep this SSH connection available for recovery.").size(fonts.label).color(theme.text_secondary),
    ].spacing(10);
    if !state.records.is_empty() {
        content = content.push(
            row![
                pick_list(state.records.clone(), state.selected.clone(), |record| {
                    management(M::Resume(record))
                })
                .placeholder("Resume or manage a saved server")
                .width(Length::Fill),
                button(text("New server").size(fonts.label))
                    .on_press_maybe((!state.busy).then_some(management(M::NewDeployment)))
                    .style(secondary_button_style(theme))
                    .padding([8, 12]),
            ]
            .spacing(8),
        );
    }
    if state.selected.is_none() {
        content = content.push(
            pick_list(choices, selected, |host| management(M::SelectHost(host.id)))
                .placeholder("Choose the server to install Hub on")
                .width(Length::Fill),
        );
    } else if let Some(host) = selected {
        content = content.push(text(host.label).size(fonts.body));
    } else {
        content = content.push(
            text("The saved SSH host is missing. Restore it to resume this deployment.")
                .color(super::common::ERROR_COLOR)
                .size(fonts.label),
        );
    }
    let ready = state
        .response
        .as_ref()
        .is_some_and(|r| r.ok && r.phase == "ready");
    if !ready && !signing_in {
        content = content.push(
            text("Portal installs Docker if needed and runs Hub in two containers with its own private Tailscale address. No public ports are opened. Your browser opens to sign in to Tailscale, then to create the passkey you sign in to your Hub with.")
                .size(fonts.small)
                .color(theme.text_secondary),
        );
        let label = match (state.selected.is_some(), state.tailnet_override) {
            (false, false) => "Deploy Hub",
            (false, true) => "Deploy anyway",
            (true, false) => "Resume setup",
            (true, true) => "Resume anyway",
        };
        content = content.push(
            button(text(label).size(fonts.body))
                .padding([8, 16])
                .style(primary_button_style(theme))
                .on_press_maybe(
                    (!state.busy && state.host_id.is_some()).then_some(management(M::Deploy)),
                ),
        );
    }
    if ready && offer_connect && !state.busy && !signing_in {
        content = content.push(
            button(text("Connect this device").size(fonts.body))
                .padding([8, 16])
                .style(primary_button_style(theme))
                .on_press(management(M::ConnectDevice)),
        );
    }
    if state.busy && state.browser_url.is_some() {
        content = content.push(
            button(text("Open Tailscale page again").size(fonts.label))
                .padding([6, 12])
                .style(secondary_button_style(theme))
                .on_press(management(M::ReopenBrowser)),
        );
    }
    if !state.progress.is_empty() {
        content = content.push(
            text(state.progress.clone())
                .size(fonts.body)
                .color(theme.text_secondary),
        );
    }
    if state.busy {
        content = content.push(
            button(
                text(if state.automated {
                    "Stop setup"
                } else {
                    "Cancel operation"
                })
                .size(fonts.label),
            )
            .on_press(management(M::Cancel))
            .padding([8, 12])
            .style(secondary_button_style(theme)),
        );
    }
    if let Some(error) = &state.error {
        content = content.push(
            text(error.clone())
                .size(fonts.label)
                .color(super::common::ERROR_COLOR),
        );
        if let Some(action) = state
            .last_action
            .filter(|action| !state.automated && *action != Action::OwnerSetup)
        {
            content = content.push(
                button(text(action.label()).size(fonts.label))
                    .padding([8, 12])
                    .style(secondary_button_style(theme))
                    .on_press_maybe((!state.busy).then_some(management(M::Run(action)))),
            );
        }
    }
    if let Some(owner) = state
        .selected
        .as_ref()
        .and_then(|record| record.owner_username.clone())
    {
        content = content.push(
            text(format!(
                "Hub owner: {owner}. Signs in with a passkey; no password is stored."
            ))
            .size(fonts.label)
            .color(theme.text_secondary),
        );
    }
    content = content.push(
        button(
            text(if state.show_advanced {
                "Hide advanced options"
            } else {
                "Advanced options"
            })
            .size(fonts.label),
        )
        .padding([6, 10])
        .style(secondary_button_style(theme))
        .on_press(management(M::ToggleAdvanced)),
    );
    if state.show_advanced {
        content = content.push(advanced_hub_view(state, theme, fonts));
    }
    if let Some(response) = state
        .response
        .as_ref()
        .filter(|r| state.selected.is_some() && r.string("hub_version").is_some())
    {
        content = content.push(maintenance_view(state, response, theme, fonts));
    }
    if let Some(diagnostics) = &state.diagnostics {
        content = content.push(
            container(scrollable(text(diagnostics.clone()).size(fonts.small)))
                .height(Length::Fixed(160.0)),
        );
    }
    content.into()
}

/// Overrides for an unusual setup, plus readiness checks and logs. Setup and
/// recovery themselves always run through Deploy Hub / Resume setup.
fn advanced_hub_view(
    state: &ManagedHubState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let mut content = column![].spacing(10);
    if state.selected.is_none() {
        content = content.push(
            text_input("Server name in Tailscale", &state.hostname)
                .on_input_maybe((!state.busy).then_some(|v| management(M::Hostname(v))))
                .padding(8)
                .style(dialog_input_style(theme)),
        );
    }
    let owner_exists = state
        .response
        .as_ref()
        .is_some_and(|r| r.bool("owner_exists"));
    if !owner_exists {
        let default = state
            .selected
            .as_ref()
            .and_then(|record| record.owner_username.clone())
            .unwrap_or_else(crate::hub::deployment::default_owner_username);
        content = content.push(
            text_input(
                &format!("Owner account name (default: {default})"),
                &state.owner_name,
            )
            .on_input_maybe((!state.busy).then_some(|v| management(M::OwnerName(v))))
            .padding(8)
            .style(dialog_input_style(theme)),
        );
    }
    content = content.push(
        text_input("Hub image and version", &state.image)
            .on_input_maybe((!state.busy).then_some(|v| management(M::Image(v))))
            .padding(8)
            .style(dialog_input_style(theme)),
    );
    content = content.push(
        row![
            button(text("Choose local image tar…").size(fonts.label))
                .on_press_maybe((!state.busy).then_some(management(M::ChooseArchive)))
                .padding([6, 10])
                .style(secondary_button_style(theme)),
            button(text("Clear").size(fonts.label))
                .on_press_maybe(
                    (!state.busy && state.archive.is_some()).then_some(management(M::ClearArchive))
                )
                .padding([6, 10])
                .style(secondary_button_style(theme)),
        ]
        .spacing(8),
    );
    content = content.push(
        text(
            state
                .archive
                .as_ref()
                .map(|p| format!("Local image: {}", p.display()))
                .unwrap_or_else(|| {
                    "Optional: use a Docker image tar for an unpublished or offline Hub build."
                        .into()
                }),
        )
        .size(fonts.small)
        .color(theme.text_secondary),
    );
    if state.selected.is_some() {
        let action_button = |action: Action| {
            button(text(action.label()).size(fonts.label))
                .padding([8, 12])
                .style(secondary_button_style(theme))
                .on_press_maybe((!state.busy).then_some(management(M::Run(action))))
        };
        content = content.push(
            row![
                action_button(Action::Status),
                action_button(Action::Doctor),
                action_button(Action::Logs)
            ]
            .spacing(8),
        );
    }
    content.into()
}

/// Update, backup and removal of an installed Hub.
fn maintenance_view(
    state: &ManagedHubState,
    response: &crate::hub::deployment::DeploymentResponse,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let action_button = |action: Action, enabled: bool| {
        button(text(action.label()).size(fonts.label))
            .padding([8, 12])
            .style(secondary_button_style(theme))
            .on_press_maybe((enabled && !state.busy).then_some(management(M::Run(action))))
    };
    use crate::hub::deployment::OwnerSetupMode;
    let passkey_button = |label: &'static str, mode: OwnerSetupMode, enabled: bool| {
        button(text(label).size(fonts.label))
            .padding([8, 12])
            .style(secondary_button_style(theme))
            .on_press_maybe((enabled && !state.busy).then_some(management(M::SetUpPasskey(mode))))
    };
    // One update offer, shown only when there is something to update to: an
    // image chosen in Advanced options, else the version of this Portal.
    let installed = response.string("hub_image");
    let update_offer =
        if state.archive.is_some() || installed.is_some_and(|installed| installed != state.image) {
            Some((
                format!(
                    "Advanced options select {}{}.",
                    state.image,
                    if state.archive.is_some() {
                        " from a local image tar"
                    } else {
                        ""
                    }
                ),
                "Update Hub".to_string(),
                M::Run(Action::Update),
            ))
        } else {
            response
                .string("hub_version")
                .filter(|version| crate::hub::deployment::hub_is_older_than_portal(version))
                .map(|version| {
                    (
                        format!(
                            "This Hub runs version {version}. This Portal deploys version {}.",
                            env!("CARGO_PKG_VERSION")
                        ),
                        format!("Update Hub to {}", env!("CARGO_PKG_VERSION")),
                        M::UpdateToPortalVersion,
                    )
                })
        }
        .map(|(detail, label, action)| {
            column![
                text(detail).size(fonts.label),
                button(text(label).size(fonts.label))
                    .padding([8, 12])
                    .style(primary_button_style(theme))
                    .on_press_maybe((!state.busy).then_some(management(action))),
            ]
            .spacing(8)
        });
    let mut content = column![
        section_title("Maintain this Hub", theme, fonts),
        text(match response.sessions() {
            Some(count) => format!("{count} active sessions. Updating, backing up or uninstalling can interrupt sessions."),
            None => "Session count is unavailable. Maintenance checks the server again before making changes.".into(),
        })
        .size(fonts.label),
        checkbox(state.allow_interruptions)
            .label("Allow this operation to interrupt active sessions")
            .on_toggle(|v| management(M::AllowInterruptions(v))),
    ]
    .spacing(10);
    if let Some(offer) = update_offer {
        content = content.push(offer);
    }
    content
        .push(action_button(Action::Backup, true))
        .push(
            checkbox(state.uninstall_confirmed)
                .label("Uninstall this Hub's services and retain its data")
                .on_toggle(|v| management(M::ConfirmUninstall(v))),
        )
        .push(action_button(Action::Uninstall, state.uninstall_confirmed))
        .push(section_title("Owner passkeys", theme, fonts))
        .push(
            text("Your Hub has no password. Add a passkey on another authenticator, or recover access by replacing all passkeys through this server's SSH connection.")
                .size(fonts.label)
                .color(theme.text_secondary),
        )
        .push(passkey_button(
            "Add a passkey",
            OwnerSetupMode::Add,
            response.bool("owner_ready"),
        ))
        .push(
            checkbox(state.reset_confirmed)
                .label("Replace every passkey and sign out every device")
                .on_toggle(|v| management(M::ConfirmReset(v))),
        )
        .push(passkey_button(
            "Reset owner passkey",
            OwnerSetupMode::Reset,
            response.bool("owner_exists") && state.reset_confirmed,
        ))
        .into()
}
