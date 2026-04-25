//! Settings page view (full page, not dialog)

use iced::widget::{
    Column, Row, Space, button, column, container, mouse_area, row, scrollable, slider, text,
    text_input,
};
use iced::{Alignment, Element, Fill, Length};

use crate::config::settings::{
    PortalProxySettings, TERMINAL_SCROLL_SPEED_BASE, TERMINAL_SCROLL_SPEED_MAX,
    TERMINAL_SCROLL_SPEED_MIN, VncEncodingPreference, VncQualityPreset, VncScalingMode,
    VncSettings,
};
use crate::fonts::TerminalFont;
use crate::message::{Message, SettingsTab, UiMessage};
use crate::proxy::ProxyStatus;
use crate::theme::{
    BORDER_RADIUS, CARD_BORDER_RADIUS, STATUS_FAILURE, ScaledFonts, Theme, ThemeId, get_theme,
};

pub struct SettingsPageContext {
    pub current_theme: ThemeId,
    pub active_tab: SettingsTab,
    pub terminal_font_size: f32,
    pub terminal_scroll_speed: f32,
    pub terminal_font: TerminalFont,
    pub vnc_settings: VncSettings,
    pub auto_reconnect: bool,
    pub reconnect_max_attempts: u32,
    pub reconnect_base_delay_ms: u64,
    pub reconnect_max_delay_ms: u64,
    pub allow_agent_forwarding: bool,
    pub snippet_history_enabled: bool,
    pub snippet_store_command: bool,
    pub snippet_store_output: bool,
    pub snippet_redact_output: bool,
    pub session_logging_enabled: bool,
    pub portal_proxy: PortalProxySettings,
    pub portal_proxy_status: Option<ProxyStatus>,
    pub portal_proxy_status_error: Option<String>,
    pub portal_proxy_status_loading: bool,
    /// Credential cache timeout in seconds (0 = disabled)
    pub credential_timeout: u64,
    pub security_audit_enabled: bool,
    /// Read-only display of the audit log file path.
    pub security_audit_log_location: String,
    /// Effective UI scale (user override or system default)
    pub ui_scale: f32,
    /// System-detected UI scale
    pub system_ui_scale: f32,
    /// Whether user has overridden the UI scale
    pub has_ui_scale_override: bool,
}

/// Build the settings page view
pub fn settings_page_view(
    context: SettingsPageContext,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let header = column![
        text("Settings")
            .size(fonts.page_title)
            .color(theme.text_primary),
        text(active_tab_description(context.active_tab))
            .size(fonts.body)
            .color(theme.text_secondary),
    ]
    .spacing(6);

    let tabs = settings_tabs(context.active_tab, theme, fonts);

    let mut content = column![
        header,
        Space::new().height(18),
        tabs,
        Space::new().height(18),
    ]
    .padding(32)
    .max_width(900)
    .spacing(0);

    for section in active_tab_sections(&context, theme, fonts) {
        content = content.push(section).push(Space::new().height(16));
    }

    let scrollable_content = scrollable(content).height(Fill).width(Fill);

    container(scrollable_content)
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}

fn settings_tabs(
    active_tab: SettingsTab,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let tabs = [
        (SettingsTab::UiUx, "UI & UX"),
        (SettingsTab::Terminal, "Terminal"),
        (SettingsTab::Connections, "Connections"),
        (SettingsTab::PortalProxy, "Portal Proxy"),
        (SettingsTab::SecurityLogs, "Security & Logs"),
        (SettingsTab::Snippets, "Snippets"),
    ];

    let mut row = Row::new().spacing(8);
    for (tab, label) in tabs {
        row = row.push(tab_button(tab, label, tab == active_tab, theme, fonts));
    }

    scrollable(row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Fill)
        .into()
}

fn tab_button(
    tab: SettingsTab,
    label: &'static str,
    selected: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let text_color = if selected {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    button(
        container(text(label).size(fonts.body).color(text_color))
            .padding([8, 14])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, status| {
        let background = match (selected, status) {
            (true, _) => Some(theme.selected.into()),
            (false, iced::widget::button::Status::Hovered) => Some(theme.hover.into()),
            (false, _) => Some(theme.surface.into()),
        };
        iced::widget::button::Style {
            background,
            text_color,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: 1.0,
                color: if selected { theme.accent } else { theme.border },
            },
            ..Default::default()
        }
    })
    .on_press(Message::Ui(UiMessage::SettingsTabSelected(tab)))
    .into()
}

fn active_tab_description(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::UiUx => "Theme, scale, and interface presentation.",
        SettingsTab::Terminal => "Terminal font and scroll behavior.",
        SettingsTab::Connections => "SSH reconnect behavior and VNC defaults.",
        SettingsTab::PortalProxy => "Persistent SSH sessions through Portal Proxy.",
        SettingsTab::SecurityLogs => "Credential caching, session logs, and audit logs.",
        SettingsTab::Snippets => "Snippet execution history and stored output.",
    }
}

fn active_tab_sections(
    context: &SettingsPageContext,
    theme: Theme,
    fonts: ScaledFonts,
) -> Vec<Element<'static, Message>> {
    match context.active_tab {
        SettingsTab::UiUx => vec![settings_section(
            "UI & UX",
            theme,
            fonts,
            vec![
                theme_tiles_row(context.current_theme, theme, fonts),
                ui_scale_setting(
                    context.ui_scale,
                    context.system_ui_scale,
                    context.has_ui_scale_override,
                    theme,
                    fonts,
                ),
            ],
        )],
        SettingsTab::Terminal => vec![settings_section(
            "Terminal",
            theme,
            fonts,
            vec![
                font_selector_setting(context.terminal_font, theme, fonts),
                font_size_setting(context.terminal_font_size, theme, fonts),
                terminal_scroll_speed_setting(context.terminal_scroll_speed, theme, fonts),
            ],
        )],
        SettingsTab::Connections => vec![
            settings_section(
                "SSH",
                theme,
                fonts,
                vec![
                    toggle_setting(
                        "Allow agent forwarding",
                        "Global safety switch for SSH agent forwarding",
                        context.allow_agent_forwarding,
                        |value| Message::Ui(UiMessage::AllowAgentForwarding(value)),
                        theme,
                        fonts,
                    ),
                    toggle_setting(
                        "Auto reconnect",
                        "Reconnect SSH and Portal Proxy sessions after unexpected disconnects",
                        context.auto_reconnect,
                        |value| Message::Ui(UiMessage::AutoReconnectEnabled(value)),
                        theme,
                        fonts,
                    ),
                    reconnect_attempts_setting(context.reconnect_max_attempts, theme, fonts),
                    reconnect_delay_setting(
                        "Initial reconnect delay",
                        "First retry delay before exponential backoff",
                        context.reconnect_base_delay_ms,
                        500,
                        10_000,
                        |value| Message::Ui(UiMessage::ReconnectBaseDelayChanged(value)),
                        theme,
                        fonts,
                    ),
                    reconnect_delay_setting(
                        "Maximum reconnect delay",
                        "Upper bound for exponential backoff",
                        context.reconnect_max_delay_ms,
                        5_000,
                        120_000,
                        |value| Message::Ui(UiMessage::ReconnectMaxDelayChanged(value)),
                        theme,
                        fonts,
                    ),
                ],
            ),
            settings_section(
                "VNC Defaults",
                theme,
                fonts,
                vnc_settings_items(&context.vnc_settings, theme, fonts),
            ),
        ],
        SettingsTab::PortalProxy => vec![settings_section(
            "Portal Proxy",
            theme,
            fonts,
            vec![
                toggle_setting(
                    "Use Portal Proxy",
                    "Master switch for hosts configured to use Portal Proxy",
                    context.portal_proxy.enabled,
                    |value| Message::Ui(UiMessage::PortalProxyEnabled(value)),
                    theme,
                    fonts,
                ),
                toggle_setting(
                    "Default for new SSH hosts",
                    "Enable Portal Proxy automatically when creating SSH hosts",
                    context.portal_proxy.default_for_new_ssh_hosts,
                    |value| Message::Ui(UiMessage::PortalProxyDefaultForNewHosts(value)),
                    theme,
                    fonts,
                ),
                text_setting(
                    "Host / IP",
                    "Tailscale name or IP address of the proxy",
                    context.portal_proxy.host.clone(),
                    |value| Message::Ui(UiMessage::PortalProxyHostChanged(value)),
                    theme,
                    fonts,
                ),
                text_setting(
                    "Port",
                    "SSH port for the proxy",
                    context.portal_proxy.port.to_string(),
                    |value| Message::Ui(UiMessage::PortalProxyPortChanged(value)),
                    theme,
                    fonts,
                ),
                text_setting(
                    "Username",
                    "SSH user on the proxy",
                    context.portal_proxy.username.clone(),
                    |value| Message::Ui(UiMessage::PortalProxyUsernameChanged(value)),
                    theme,
                    fonts,
                ),
                text_setting(
                    "Identity file",
                    "Optional key for authenticating to the proxy",
                    context
                        .portal_proxy
                        .identity_file
                        .as_ref()
                        .map(|path| path.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    |value| Message::Ui(UiMessage::PortalProxyIdentityFileChanged(value)),
                    theme,
                    fonts,
                ),
                portal_proxy_status_setting(
                    context.portal_proxy_status.clone(),
                    context.portal_proxy_status_error.clone(),
                    context.portal_proxy_status_loading,
                    theme,
                    fonts,
                ),
            ],
        )],
        SettingsTab::SecurityLogs => vec![settings_section(
            "Security & Logs",
            theme,
            fonts,
            vec![
                credential_timeout_setting(context.credential_timeout, theme, fonts),
                toggle_setting(
                    "Session logging",
                    "Save terminal output to a log file per session",
                    context.session_logging_enabled,
                    |value| Message::Ui(UiMessage::SessionLoggingEnabled(value)),
                    theme,
                    fonts,
                ),
                toggle_setting(
                    "Security audit logging",
                    "Write security events to an on-disk audit log",
                    context.security_audit_enabled,
                    |value| Message::Ui(UiMessage::SecurityAuditLoggingEnabled(value)),
                    theme,
                    fonts,
                ),
                read_only_setting(
                    "Audit log location",
                    "Where security audit logs are stored",
                    context.security_audit_log_location.clone(),
                    theme,
                    fonts,
                ),
            ],
        )],
        SettingsTab::Snippets => vec![settings_section(
            "Snippet History",
            theme,
            fonts,
            vec![
                toggle_setting(
                    "Enable snippet history",
                    "Save snippet execution history to disk",
                    context.snippet_history_enabled,
                    |value| Message::Ui(UiMessage::SnippetHistoryEnabled(value)),
                    theme,
                    fonts,
                ),
                toggle_setting(
                    "Store commands",
                    "Persist executed command text in history entries",
                    context.snippet_store_command,
                    |value| Message::Ui(UiMessage::SnippetHistoryStoreCommand(value)),
                    theme,
                    fonts,
                ),
                toggle_setting(
                    "Store output",
                    "Persist stdout/stderr from snippet executions",
                    context.snippet_store_output,
                    |value| Message::Ui(UiMessage::SnippetHistoryStoreOutput(value)),
                    theme,
                    fonts,
                ),
                toggle_setting(
                    "Redact sensitive values",
                    "Redact common secrets in stored commands and output",
                    context.snippet_redact_output,
                    |value| Message::Ui(UiMessage::SnippetHistoryRedactOutput(value)),
                    theme,
                    fonts,
                ),
            ],
        )],
    }
}

/// Create a settings section with title and items
fn settings_section<'a>(
    title: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
    items: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut section = Column::new().spacing(8);

    // Section title
    section = section.push(text(title).size(fonts.label).color(theme.text_muted));

    // Section card with items
    let mut card_content = Column::new().spacing(16).padding(20);
    for item in items {
        card_content = card_content.push(item);
    }

    let card = container(card_content)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    section.push(card).into()
}

/// Theme selector with visual tile previews
fn theme_tiles_row(
    current: ThemeId,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let tiles: Vec<Element<'static, Message>> = ThemeId::all()
        .iter()
        .map(|&theme_id| theme_tile(theme_id, theme_id == current, theme, fonts))
        .collect();

    let tiles_row = Row::from_vec(tiles).spacing(12);

    // Wrap in scrollable for narrow screens
    let scrollable_tiles = scrollable(tiles_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Fill);

    column![scrollable_tiles,].spacing(0).into()
}

/// Individual theme tile with mini app preview
fn theme_tile(
    tile_theme_id: ThemeId,
    is_selected: bool,
    current_theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let preview_theme = get_theme(tile_theme_id);

    // Mini app preview
    let preview = mini_app_preview(preview_theme);

    // Short theme name for the tile
    let short_name = match tile_theme_id {
        ThemeId::PortalDefault => "Default",
        ThemeId::CatppuccinLatte => "Latte",
        ThemeId::CatppuccinFrappe => "Frappé",
        ThemeId::CatppuccinMacchiato => "Macchiato",
        ThemeId::CatppuccinMocha => "Mocha",
    };

    let name = text(short_name).size(fonts.small).color(if is_selected {
        current_theme.accent
    } else {
        current_theme.text_secondary
    });

    let border_width = if is_selected { 2.0 } else { 1.0 };
    let border_color = if is_selected {
        current_theme.accent
    } else {
        current_theme.border
    };

    let tile_content = column![preview, Space::new().height(6), name,]
        .align_x(Alignment::Center)
        .spacing(0);

    let tile_container = container(tile_content)
        .padding(6)
        .style(move |_| container::Style {
            background: None, // Transparent - let preview colors show through
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: border_width,
                color: border_color,
            },
            ..Default::default()
        });

    mouse_area(tile_container)
        .on_press(Message::Ui(UiMessage::ThemeChange(tile_theme_id)))
        .into()
}

/// Mini app preview showing sidebar, main area, and accent elements
fn mini_app_preview(preview_theme: Theme) -> Element<'static, Message> {
    // Sidebar strip
    let sidebar = container(Space::new().width(14).height(48)).style(move |_| container::Style {
        background: Some(preview_theme.sidebar.into()),
        ..Default::default()
    });

    // Terminal-like lines in main area
    let line = |width: u32| {
        container(Space::new().width(width).height(3u32)).style(move |_| container::Style {
            background: Some(preview_theme.text_muted.into()),
            border: iced::Border {
                radius: 1.5.into(),
                ..Default::default()
            },
            ..Default::default()
        })
    };

    // Accent button element
    let accent_button =
        container(Space::new().width(20).height(5)).style(move |_| container::Style {
            background: Some(preview_theme.accent.into()),
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let main_content = column![
        Space::new().height(5),
        line(40),
        Space::new().height(3),
        line(28),
        Space::new().height(3),
        line(34),
        Space::new().height(6),
        accent_button,
    ]
    .padding([4, 6]);

    let main_area = container(main_content)
        .width(66)
        .height(48)
        .style(move |_| container::Style {
            background: Some(preview_theme.background.into()),
            ..Default::default()
        });

    // Combine sidebar and main area
    let preview_content = row![sidebar, main_area].spacing(0);

    container(preview_content)
        .style(move |_| container::Style {
            background: Some(preview_theme.surface.into()),
            border: iced::Border {
                radius: 4.0.into(),
                color: preview_theme.border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Font selector with tile previews
fn font_selector_setting(
    current_font: TerminalFont,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Font").size(fonts.body).color(theme.text_primary);

    let description = text("Terminal font family")
        .size(fonts.label)
        .color(theme.text_muted);

    // Create font tiles
    let tiles: Vec<Element<'static, Message>> = TerminalFont::all()
        .iter()
        .map(|&font| font_tile(font, font == current_font, theme, fonts))
        .collect();

    let tiles_row = Row::from_vec(tiles).spacing(12);

    column![
        label,
        Space::new().height(4),
        description,
        Space::new().height(12),
        tiles_row,
    ]
    .spacing(0)
    .into()
}

/// Individual font tile showing font preview
fn font_tile(
    font: TerminalFont,
    is_selected: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let iced_font = font.to_iced_font();

    // Preview text showing the font
    let preview = text("Aa")
        .size(20)
        .font(iced_font)
        .color(theme.text_primary);

    let name = text(font.display_name())
        .size(fonts.small)
        .color(if is_selected {
            theme.accent
        } else {
            theme.text_secondary
        });

    let border_width = if is_selected { 2.0 } else { 1.0 };
    let border_color = if is_selected {
        theme.accent
    } else {
        theme.border
    };

    let tile_content = column![preview, Space::new().height(6), name,]
        .align_x(Alignment::Center)
        .spacing(0);

    let tile_container =
        container(tile_content)
            .padding([12, 20])
            .style(move |_| container::Style {
                background: Some(theme.background.into()),
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    width: border_width,
                    color: border_color,
                },
                ..Default::default()
            });

    mouse_area(tile_container)
        .on_press(Message::Ui(UiMessage::FontChange(font)))
        .into()
}

/// Font size slider setting
fn font_size_setting(
    current_size: f32,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Font Size").size(fonts.body).color(theme.text_primary);

    let description = text("Terminal text size")
        .size(fonts.label)
        .color(theme.text_muted);

    let slider_widget = slider(6.0..=20.0, current_size, |v| {
        Message::Ui(UiMessage::FontSizeChange(v))
    })
    .step(1.0)
    .width(140);

    let value_text = text(format!("{}px", current_size as u32))
        .size(fonts.body)
        .color(theme.text_secondary);

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

/// Terminal scroll speed slider setting
fn terminal_scroll_speed_setting(
    current_speed: f32,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let relative_speed = current_speed / TERMINAL_SCROLL_SPEED_BASE;
    let relative_min = TERMINAL_SCROLL_SPEED_MIN / TERMINAL_SCROLL_SPEED_BASE;
    let relative_max = TERMINAL_SCROLL_SPEED_MAX / TERMINAL_SCROLL_SPEED_BASE;

    let label = text("Scroll Speed")
        .size(fonts.body)
        .color(theme.text_primary);

    let description = text("Mouse wheel and trackpad scrollback speed")
        .size(fonts.label)
        .color(theme.text_muted);

    let slider_widget = slider(relative_min..=relative_max, relative_speed, |v| {
        let rounded = (v * 4.0).round() / 4.0;
        Message::Ui(UiMessage::TerminalScrollSpeedChange(
            rounded * TERMINAL_SCROLL_SPEED_BASE,
        ))
    })
    .step(0.25)
    .width(140);

    let value_text = text(format!("{}%", (relative_speed * 100.0).round() as u32))
        .size(fonts.body)
        .color(theme.text_secondary);

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

/// UI scale slider setting
fn ui_scale_setting(
    current_scale: f32,
    system_scale: f32,
    has_override: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("UI Scale").size(fonts.body).color(theme.text_primary);

    // Show system default in description when not overridden
    let description_text = if has_override {
        format!("System default: {}%", (system_scale * 100.0).round() as u32)
    } else {
        "Scale all interface text (except terminal)".to_string()
    };
    let description = text(description_text)
        .size(fonts.label)
        .color(theme.text_muted);

    // Slider from 80% to 150% with 5% steps
    let slider_widget = slider(0.8..=1.5, current_scale, |v| {
        // Round to nearest 0.05 (5%)
        let rounded = (v * 20.0).round() / 20.0;
        Message::Ui(UiMessage::UiScaleChange(rounded))
    })
    .step(0.05)
    .width(140);

    let value_text = text(format!("{}%", (current_scale * 100.0).round() as u32))
        .size(fonts.body)
        .color(theme.text_secondary);

    // Reset button (only visible when override is set)
    let reset_button: Element<'static, Message> = if has_override {
        button(
            container(text("Reset").size(fonts.label).color(theme.text_secondary))
                .padding([4, 8])
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .padding(0)
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => None,
            };
            iced::widget::button::Style {
                background,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    width: 1.0,
                    color: theme.border,
                },
                ..Default::default()
            }
        })
        .on_press(Message::Ui(UiMessage::UiScaleReset))
        .into()
    } else {
        Space::new().width(0).into()
    };

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
            Space::new().width(8),
            reset_button,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

fn toggle_setting<F>(
    label: &'static str,
    description: &'static str,
    enabled: bool,
    on_toggle: F,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message>
where
    F: Fn(bool) -> Message + 'static,
{
    let label_text = text(label).size(fonts.body).color(theme.text_primary);

    let description_text = text(description).size(fonts.label).color(theme.text_muted);

    let toggle_label = if enabled { "Enabled" } else { "Disabled" };
    let toggle_color = if enabled {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    let toggle_button = button(
        container(text(toggle_label).size(fonts.label).color(toggle_color))
            .width(92)
            .padding([7, 0])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, status| {
        let background = match (enabled, status) {
            (true, _) => Some(theme.selected.into()),
            (false, iced::widget::button::Status::Hovered) => Some(theme.hover.into()),
            (false, _) => Some(theme.background.into()),
        };
        iced::widget::button::Style {
            background,
            text_color: toggle_color,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: 1.0,
                color: if enabled { theme.accent } else { theme.border },
            },
            ..Default::default()
        }
    })
    .on_press(on_toggle(!enabled));

    column![
        row![
            column![label_text, Space::new().height(4), description_text].spacing(0),
            Space::new().width(Length::Fill),
            toggle_button,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(0)
    .into()
}

fn read_only_setting(
    label: &'static str,
    description: &'static str,
    value: String,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label_text = text(label).size(fonts.body).color(theme.text_primary);
    let description_text = text(description).size(fonts.label).color(theme.text_muted);

    let value_text = text(value).size(fonts.label).color(theme.text_secondary);

    column![
        row![
            column![label_text, Space::new().height(4), description_text].spacing(0),
            Space::new().width(Length::Fill),
            value_text,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(0)
    .into()
}

fn portal_proxy_status_setting(
    status: Option<ProxyStatus>,
    error: Option<String>,
    loading: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label_text = text("Status").size(fonts.body).color(theme.text_primary);
    let description_text = text("Check proxy version and API compatibility")
        .size(fonts.label)
        .color(theme.text_muted);

    let status_text = if loading {
        "Checking...".to_string()
    } else if let Some(status) = status {
        format!(
            "v{} · API {} · schema {}",
            status.version, status.api_version, status.metadata_schema_version
        )
    } else if let Some(error) = error.as_ref() {
        error.clone()
    } else {
        "Not checked".to_string()
    };

    let status_color = if loading {
        theme.text_muted
    } else if error.is_some() {
        STATUS_FAILURE
    } else {
        theme.text_secondary
    };

    let check_button = button(
        container(text("Check").size(fonts.label).color(theme.text_primary))
            .padding([6, 12])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => Some(theme.surface.into()),
        };
        iced::widget::button::Style {
            background,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: 1.0,
                color: theme.border,
            },
            ..Default::default()
        }
    })
    .on_press_maybe((!loading).then_some(Message::Ui(UiMessage::PortalProxyCheckStatus)));

    column![
        row![
            column![label_text, Space::new().height(4), description_text].spacing(0),
            Space::new().width(Length::Fill),
            text(status_text)
                .size(fonts.label)
                .color(status_color)
                .width(Length::Fixed(240.0)),
            Space::new().width(12),
            check_button,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(0)
    .into()
}

fn text_setting<F>(
    label: &'static str,
    description: &'static str,
    value: String,
    on_input: F,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message>
where
    F: Fn(String) -> Message + 'static,
{
    let label_text = text(label).size(fonts.body).color(theme.text_primary);
    let description_text = text(description).size(fonts.label).color(theme.text_muted);

    let input = text_input("", &value)
        .on_input(on_input)
        .padding(8)
        .width(220)
        .style(move |_theme, _status| iced::widget::text_input::Style {
            background: theme.background.into(),
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: 1.0,
                color: theme.border,
            },
            icon: theme.text_secondary,
            placeholder: theme.text_muted,
            value: theme.text_primary,
            selection: theme.accent,
        });

    column![
        row![
            column![label_text, Space::new().height(4), description_text].spacing(0),
            Space::new().width(Length::Fill),
            input,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(0)
    .into()
}

fn choice_setting<T, F>(
    label: &'static str,
    description: &'static str,
    current: T,
    options: &[(T, &'static str)],
    on_select: F,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message>
where
    T: Copy + PartialEq + 'static,
    F: Fn(T) -> Message + Copy + 'static,
{
    let label_text = text(label).size(fonts.body).color(theme.text_primary);
    let description_text = text(description).size(fonts.label).color(theme.text_muted);

    let mut controls = Row::new().spacing(6);
    for &(value, option_label) in options {
        let selected = value == current;
        controls = controls.push(choice_button(
            option_label,
            selected,
            on_select(value),
            theme,
            fonts,
        ));
    }

    column![
        row![
            column![label_text, Space::new().height(4), description_text].spacing(0),
            Space::new().width(Length::Fill),
            controls,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(0)
    .into()
}

fn choice_button(
    label: &'static str,
    selected: bool,
    on_press: Message,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let text_color = if selected {
        theme.text_primary
    } else {
        theme.text_secondary
    };

    button(
        container(text(label).size(fonts.label).color(text_color))
            .padding([7, 10])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, status| {
        let background = match (selected, status) {
            (true, _) => Some(theme.selected.into()),
            (false, iced::widget::button::Status::Hovered) => Some(theme.hover.into()),
            (false, _) => Some(theme.background.into()),
        };
        iced::widget::button::Style {
            background,
            text_color,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: 1.0,
                color: if selected { theme.accent } else { theme.border },
            },
            ..Default::default()
        }
    })
    .on_press(on_press)
    .into()
}

fn reconnect_attempts_setting(
    current_attempts: u32,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Reconnect attempts")
        .size(fonts.body)
        .color(theme.text_primary);
    let description = text("Maximum retries before the session is closed")
        .size(fonts.label)
        .color(theme.text_muted);
    let current = current_attempts.clamp(1, 20) as f32;
    let slider_widget = slider(1.0..=20.0, current, |value| {
        Message::Ui(UiMessage::ReconnectMaxAttemptsChanged(value.round() as u32))
    })
    .step(1.0)
    .width(160);
    let value_text = text(current_attempts.clamp(1, 20).to_string())
        .size(fonts.body)
        .color(theme.text_secondary);

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

fn reconnect_delay_setting<F>(
    label_text: &'static str,
    description_text: &'static str,
    current_ms: u64,
    min_ms: u64,
    max_ms: u64,
    on_change: F,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message>
where
    F: Fn(u64) -> Message + 'static,
{
    let label = text(label_text).size(fonts.body).color(theme.text_primary);
    let description = text(description_text)
        .size(fonts.label)
        .color(theme.text_muted);
    let current = current_ms.clamp(min_ms, max_ms) as f32;
    let slider_widget = slider(min_ms as f32..=max_ms as f32, current, move |value| {
        let snapped = ((value / 500.0).round() * 500.0).clamp(min_ms as f32, max_ms as f32);
        on_change(snapped as u64)
    })
    .step(500.0)
    .width(160);
    let value_text = text(format_duration_ms(current_ms.clamp(min_ms, max_ms)))
        .size(fonts.body)
        .color(theme.text_secondary)
        .width(Length::Fixed(56.0));

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

fn vnc_settings_items(
    settings: &VncSettings,
    theme: Theme,
    fonts: ScaledFonts,
) -> Vec<Element<'static, Message>> {
    vec![
        choice_setting(
            "Quality preset",
            "Preset used when new VNC sessions start",
            settings.quality_preset,
            &[
                (VncQualityPreset::Auto, "Auto"),
                (VncQualityPreset::Speed, "Speed"),
                (VncQualityPreset::Balanced, "Balanced"),
                (VncQualityPreset::Quality, "Quality"),
                (VncQualityPreset::Lossless, "Lossless"),
            ],
            |value| Message::Ui(UiMessage::VncQualityPresetChanged(value)),
            theme,
            fonts,
        ),
        choice_setting(
            "Scaling mode",
            "How the remote framebuffer fits the viewer",
            settings.scaling_mode,
            &[
                (VncScalingMode::Fit, "Fit"),
                (VncScalingMode::Actual, "1:1"),
                (VncScalingMode::Stretch, "Stretch"),
            ],
            |value| Message::Ui(UiMessage::VncScalingModeChanged(value)),
            theme,
            fonts,
        ),
        choice_setting(
            "Encoding",
            "Preferred VNC encoding before preset overrides",
            settings.encoding,
            &[
                (VncEncodingPreference::Auto, "Auto"),
                (VncEncodingPreference::Tight, "Tight"),
                (VncEncodingPreference::Zrle, "ZRLE"),
                (VncEncodingPreference::Raw, "Raw"),
            ],
            |value| Message::Ui(UiMessage::VncEncodingPreferenceChanged(value)),
            theme,
            fonts,
        ),
        choice_setting(
            "Color depth",
            "Preferred framebuffer color depth",
            settings.color_depth,
            &[(16u8, "16-bit"), (32u8, "32-bit")],
            |value| Message::Ui(UiMessage::VncColorDepthChanged(value)),
            theme,
            fonts,
        ),
        vnc_refresh_setting(settings.refresh_fps, theme, fonts),
        vnc_pointer_interval_setting(settings.pointer_interval_ms, theme, fonts),
        toggle_setting(
            "Remote resize",
            "Ask the remote desktop to match the viewer size",
            settings.remote_resize,
            |value| Message::Ui(UiMessage::VncRemoteResizeChanged(value)),
            theme,
            fonts,
        ),
        toggle_setting(
            "Clipboard sharing",
            "Allow clipboard exchange with VNC sessions",
            settings.clipboard_sharing,
            |value| Message::Ui(UiMessage::VncClipboardSharingChanged(value)),
            theme,
            fonts,
        ),
        toggle_setting(
            "View-only default",
            "Start VNC sessions without sending input",
            settings.view_only,
            |value| Message::Ui(UiMessage::VncViewOnlyChanged(value)),
            theme,
            fonts,
        ),
        toggle_setting(
            "Cursor dot",
            "Show a local cursor position marker",
            settings.show_cursor_dot,
            |value| Message::Ui(UiMessage::VncShowCursorDotChanged(value)),
            theme,
            fonts,
        ),
        toggle_setting(
            "Stats overlay",
            "Show detailed VNC runtime statistics",
            settings.show_stats_overlay,
            |value| Message::Ui(UiMessage::VncShowStatsOverlayChanged(value)),
            theme,
            fonts,
        ),
    ]
}

fn vnc_refresh_setting(
    current_fps: u32,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Refresh rate")
        .size(fonts.body)
        .color(theme.text_primary);
    let description = text("Maximum VNC update rate")
        .size(fonts.label)
        .color(theme.text_muted);
    let current = current_fps.clamp(1, 20) as f32;
    let slider_widget = slider(1.0..=20.0, current, |value| {
        Message::Ui(UiMessage::VncRefreshFpsChanged(value.round() as u32))
    })
    .step(1.0)
    .width(160);
    let value_text = text(format!("{} fps", current_fps.clamp(1, 20)))
        .size(fonts.body)
        .color(theme.text_secondary)
        .width(Length::Fixed(56.0));

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

fn vnc_pointer_interval_setting(
    current_ms: u64,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Pointer interval")
        .size(fonts.body)
        .color(theme.text_primary);
    let description = text("Minimum time between pointer events")
        .size(fonts.label)
        .color(theme.text_muted);
    let current = current_ms.min(1000) as f32;
    let slider_widget = slider(0.0..=1000.0, current, |value| {
        Message::Ui(UiMessage::VncPointerIntervalChanged(value.round() as u64))
    })
    .step(1.0)
    .width(160);
    let value_text = text(format!("{} ms", current_ms.min(1000)))
        .size(fonts.body)
        .color(theme.text_secondary)
        .width(Length::Fixed(56.0));

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

fn format_timeout_seconds(seconds: u64) -> String {
    if seconds == 0 {
        return "Off".to_string();
    }

    if seconds % 3600 == 0 {
        let hours = seconds / 3600;
        return if hours == 1 {
            "1 hour".to_string()
        } else {
            format!("{} hours", hours)
        };
    }

    if seconds % 60 == 0 {
        let minutes = seconds / 60;
        return if minutes == 1 {
            "1 min".to_string()
        } else {
            format!("{} min", minutes)
        };
    }

    format!("{} sec", seconds)
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms % 1000 == 0 {
        format!("{}s", ms / 1000)
    } else {
        format!("{:.1}s", ms as f32 / 1000.0)
    }
}

fn credential_timeout_setting(
    timeout_seconds: u64,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Credential Timeout")
        .size(fonts.body)
        .color(theme.text_primary);

    let description = text("Cache SSH credentials in memory for this long (0 disables caching)")
        .size(fonts.label)
        .color(theme.text_muted);

    let current = timeout_seconds.min(3600) as f32;
    let slider_widget = slider(0.0..=3600.0, current, |v| {
        // Keep changes stable and predictable by snapping to 30s increments.
        let snapped = ((v / 30.0).round() * 30.0).clamp(0.0, 3600.0);
        Message::Ui(UiMessage::CredentialTimeoutChange(snapped as u64))
    })
    .step(30.0)
    .width(140);

    let value_text = text(format_timeout_seconds(timeout_seconds.min(3600)))
        .size(fonts.body)
        .color(theme.text_secondary);

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}
