use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Fill, Length};

use crate::config::{HostsConfig, Protocol, SnippetsConfig};
use crate::icons::{self, icon_with_color};
use crate::message::{CommandAction, Message, UiMessage};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, ScaledFonts, Theme};

pub fn command_input_id() -> iced::widget::Id {
    iced::widget::Id::new("command_palette_input")
}

#[derive(Debug, Clone)]
pub struct CommandItem {
    pub title: String,
    pub subtitle: String,
    pub action: CommandAction,
    pub icon: &'static [u8],
}

pub fn available_commands(
    hosts: &HostsConfig,
    snippets: &SnippetsConfig,
    portal_hub_configured: bool,
) -> Vec<CommandItem> {
    let mut commands = vec![
        command(
            "Hosts",
            "Open host grid",
            CommandAction::Hosts,
            icons::ui::SERVER,
        ),
        command(
            "SFTP",
            "Open file browser",
            CommandAction::Sftp,
            icons::ui::HARD_DRIVE,
        ),
        command(
            "Snippets",
            "Open command snippets",
            CommandAction::Snippets,
            icons::ui::CODE,
        ),
        command(
            "Vault",
            "Open key vault",
            CommandAction::Vault,
            icons::ui::KEY,
        ),
        command(
            "History",
            "Open connection history",
            CommandAction::History,
            icons::ui::HISTORY,
        ),
        command(
            "Settings",
            "Open preferences",
            CommandAction::Settings,
            icons::ui::SETTINGS,
        ),
        command(
            "Quick Connect",
            "Connect to a new endpoint",
            CommandAction::QuickConnect,
            icons::ui::ZAP,
        ),
        command(
            "New Host",
            "Create a host profile",
            CommandAction::NewHost,
            icons::ui::PLUS,
        ),
        command(
            "Local Terminal",
            "Open a shell on this machine",
            CommandAction::LocalTerminal,
            icons::ui::TERMINAL,
        ),
    ];

    if portal_hub_configured {
        commands.push(command(
            "Sync Portal Hub",
            "Push and pull configured profile data",
            CommandAction::PortalHubSync,
            icons::ui::REFRESH,
        ));
    }

    commands.extend(hosts.hosts.iter().map(|host| {
        let protocol = match host.protocol {
            Protocol::Ssh => "SSH",
            Protocol::Vnc => "VNC",
        };
        command(
            format!("Connect {}", host.name),
            format!(
                "{} {}@{}",
                protocol,
                host.effective_username(),
                host.hostname
            ),
            CommandAction::ConnectHost(host.id),
            icons::ui::ZAP,
        )
    }));

    commands.extend(snippets.snippets.iter().map(|snippet| {
        command(
            format!("Run {}", snippet.name),
            snippet.command.clone(),
            CommandAction::RunSnippet(snippet.id),
            icons::ui::CODE,
        )
    }));

    commands
}

pub fn first_matching_action(commands: &[CommandItem], query: &str) -> Option<CommandAction> {
    filter_commands(commands, query)
        .first()
        .map(|item| item.action.clone())
}

pub fn command_palette_view(
    query: &str,
    commands: &[CommandItem],
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let filtered = filter_commands(commands, query);

    let input = text_input("Search commands, hosts, and snippets...", query)
        .id(command_input_id())
        .on_input(|value| Message::Ui(UiMessage::CommandPaletteChanged(value)))
        .padding([12, 14])
        .size(fonts.body)
        .style(move |_theme, status| {
            let border_color = match status {
                text_input::Status::Focused { .. } => theme.accent,
                _ => theme.border,
            };
            text_input::Style {
                background: theme.background.into(),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                icon: theme.text_muted,
                placeholder: theme.text_muted,
                value: theme.text_primary,
                selection: theme.selected,
            }
        });

    let list: Element<'static, Message> = if filtered.is_empty() {
        container(
            text("No matching commands")
                .size(fonts.body)
                .color(theme.text_muted),
        )
        .padding(20)
        .width(Fill)
        .align_x(Alignment::Center)
        .into()
    } else {
        let rows: Vec<Element<'static, Message>> = filtered
            .into_iter()
            .take(10)
            .enumerate()
            .map(|(index, item)| command_row(index == 0, item, theme, fonts))
            .collect();
        scrollable(Column::with_children(rows).spacing(4))
            .height(Length::Fixed(420.0))
            .into()
    };

    let panel = container(column![input, list].spacing(10))
        .width(Length::Fixed(680.0))
        .padding(12)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: CARD_BORDER_RADIUS.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.45,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 8.0),
                blur_radius: 28.0,
            },
            ..Default::default()
        });

    button(
        container(
            container(panel)
                .width(Fill)
                .height(Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Start)
                .padding([96, 24]),
        )
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..Default::default()
        }),
    )
    .padding(0)
    .width(Fill)
    .height(Fill)
    .style(|_, _| button::Style {
        background: None,
        ..Default::default()
    })
    .on_press(Message::Ui(UiMessage::CommandPaletteClose))
    .into()
}

fn command(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    action: CommandAction,
    icon: &'static [u8],
) -> CommandItem {
    CommandItem {
        title: title.into(),
        subtitle: subtitle.into(),
        action,
        icon,
    }
}

fn command_row(
    selected: bool,
    item: CommandItem,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let background = if selected {
        theme.selected
    } else {
        theme.surface
    };
    let icon_color = if selected {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    button(
        row![
            container(icon_with_color(item.icon, 17, icon_color))
                .width(32)
                .height(32)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(move |_| container::Style {
                    background: Some(theme.background.into()),
                    border: iced::Border {
                        color: theme.border,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }),
            column![
                text(item.title.clone())
                    .size(fonts.body)
                    .color(theme.text_primary),
                text(item.subtitle.clone())
                    .size(fonts.label)
                    .color(theme.text_muted)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(2)
            .width(Fill),
            if selected {
                text("Enter").size(fonts.label).color(theme.text_secondary)
            } else {
                text("").size(fonts.label)
            },
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([9, 10])
    .width(Fill)
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: if selected {
                    theme.accent
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if selected { 1.0 } else { 0.0 },
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        }
    })
    .on_press(Message::Ui(UiMessage::CommandPaletteRun(item.action)))
    .into()
}

fn filter_commands(commands: &[CommandItem], query: &str) -> Vec<CommandItem> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return commands.iter().take(10).cloned().collect();
    }

    commands
        .iter()
        .filter(|item| {
            item.title.to_lowercase().contains(&query)
                || item.subtitle.to_lowercase().contains(&query)
        })
        .cloned()
        .collect()
}
