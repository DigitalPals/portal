//! Tab bar component for managing multiple sessions

use iced::widget::{Row, Space, button, column, container, row, text, tooltip};
use iced::{Alignment, Color, Element, Length, Padding};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::app::{FocusSection, SidebarState, View};
use crate::config::HostsConfig;
use crate::icons::{self, icon_with_color};
use crate::message::{Message, TabMessage, UiMessage};
use crate::theme::{ScaledFonts, Theme};
use crate::views::host_grid::os_icon_data;
use crate::widgets::drag_tab_row;
use crate::widgets::mouse_area as capture_mouse_area;

/// Represents a single tab
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: Uuid,
    pub title: String,
    pub tab_type: TabType,
    /// Host ID for looking up detected_os (None for local terminal)
    pub host_id: Option<Uuid>,
    /// Whether the tab needs attention because a background terminal event happened.
    pub needs_attention: bool,
    /// Agent activity inferred from terminal title updates.
    pub agent_status: Option<TabAgentStatus>,
    /// Stable per-host terminal session number.
    pub session_number: Option<usize>,
}

/// Type of content in a tab
#[derive(Debug, Clone, PartialEq)]
pub enum TabType {
    Terminal,
    Sftp,
    FileViewer,
    Vnc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAgentKind {
    Codex,
    Claude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAgentActivity {
    Working,
    Ready,
    NeedsInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabAgentStatus {
    pub kind: TabAgentKind,
    pub activity: TabAgentActivity,
}

impl TabAgentStatus {
    pub fn is_animated(self) -> bool {
        matches!(
            self.activity,
            TabAgentActivity::Working | TabAgentActivity::NeedsInput
        )
    }
}

impl Tab {
    pub fn new_terminal(
        id: Uuid,
        title: String,
        host_id: Option<Uuid>,
        session_number: usize,
    ) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Terminal,
            host_id,
            needs_attention: false,
            agent_status: None,
            session_number: Some(session_number),
        }
    }

    pub fn new_sftp(id: Uuid, title: String, host_id: Option<Uuid>) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Sftp,
            host_id,
            needs_attention: false,
            agent_status: None,
            session_number: None,
        }
    }

    pub fn new_vnc(id: Uuid, title: String, host_id: Option<Uuid>) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Vnc,
            host_id,
            needs_attention: false,
            agent_status: None,
            session_number: None,
        }
    }

    pub fn new_file_viewer(id: Uuid, title: String) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::FileViewer,
            host_id: None,
            needs_attention: false,
            agent_status: None,
            session_number: None,
        }
    }
}

/// Build the tab bar view
#[allow(clippy::too_many_arguments)]
pub fn tab_bar_view<'a>(
    tabs: &'a [Tab],
    active_tab: Option<Uuid>,
    _sidebar_state: SidebarState,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: usize,
    active_view: &View,
    hosts_config: &'a HostsConfig,
) -> Element<'a, Message> {
    // Determine if we should use terminal background (seamless look)
    let use_terminal_bg = matches!(
        active_view,
        View::Terminal(_) | View::DualSftp(_) | View::FileViewer(_) | View::VncViewer(_)
    );
    // Hamburger menu button for sidebar toggle
    let menu_icon = icons::ui::MENU;

    let hamburger_btn = button(
        container(icon_with_color(menu_icon, 20, theme.text_secondary)).padding(Padding::new(10.0)),
    )
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => None,
        };
        iced::widget::button::Style {
            background,
            text_color: theme.text_secondary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding(0)
    .on_press(Message::Ui(UiMessage::SidebarToggleCollapse));

    let mut tab_elements: Vec<Element<'a, Message>> = Vec::new();

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = active_tab == Some(tab.id);
        let is_focused = focus_section == FocusSection::TabBar && idx == focus_index;
        let show_session_number = should_show_session_number(tabs, tab);
        tab_elements.push(tab_button(
            tab,
            is_active,
            is_focused,
            show_session_number,
            theme,
            fonts,
            hosts_config,
        ));
    }

    // Add "+" button for new connection
    tab_elements.push(new_tab_button(theme, fonts));

    let tabs_row = Row::with_children(tab_elements)
        .spacing(4)
        .align_y(Alignment::Center);

    // Only the tabs themselves are draggable, not the trailing "+" button.
    let tabs_row = drag_tab_row(tabs_row, tabs.len())
        .on_reorder(|from, to| Message::Tab(TabMessage::Reorder { from, to }));

    container(
        row![
            // Left side: hamburger menu
            hamburger_btn,
            // Center: tabs
            container(tabs_row).padding(Padding::new(0.0).left(8.0)),
            // Right side: spacer
            container(text("")).width(Length::Fill),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding(Padding::new(8.0).left(10.0).right(10.0)),
    )
    .width(Length::Fill)
    .style(move |_theme| {
        let bg_color = if use_terminal_bg {
            theme.terminal.background
        } else {
            theme.tab_bar
        };
        container::Style {
            background: Some(bg_color.into()),
            border: iced::Border::default(),
            ..Default::default()
        }
    })
    .into()
}

/// Single tab button
fn tab_button<'a>(
    tab: &'a Tab,
    is_active: bool,
    is_focused: bool,
    show_session_number: bool,
    theme: Theme,
    fonts: ScaledFonts,
    hosts_config: &'a HostsConfig,
) -> Element<'a, Message> {
    let tab_id = tab.id;

    // Colors based on active state
    let text_icon_color = if is_active {
        Color::from_rgb8(0xCD, 0xD6, 0xF4) // #CDD6F4 - active
    } else {
        Color::from_rgb8(0x77, 0x77, 0x90) // #777790 - inactive
    };

    // Get icon - use distro icon if host_id is set and OS is detected
    let icon_data = if let Some(host_id) = tab.host_id {
        if let Some(host) = hosts_config.find_host(host_id) {
            if host.detected_os.is_some() {
                os_icon_data(&host.detected_os)
            } else {
                // Fallback to terminal icon if no detected OS
                icons::ui::TERMINAL
            }
        } else {
            icons::ui::TERMINAL
        }
    } else {
        // No host_id - use type-based icon
        match tab.tab_type {
            TabType::Terminal => icons::ui::TERMINAL,
            TabType::Sftp => icons::ui::FOLDER_CLOSED,
            TabType::FileViewer => icons::files::FILE_TEXT,
            TabType::Vnc => icons::ui::SERVER,
        }
    };
    let icon = icon_with_color(icon_data, 14, text_icon_color);

    // Truncate title if too long
    let title = truncate_title(&tab.title, 20);

    let session_number = if show_session_number {
        tab.session_number
            .map(|number| format!("#{}", number))
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Close button - always uses the reserved space, avoiding app-level hover updates.
    let close_button_width = 16.0;
    let close_button: Element<'_, Message> = container(
        button(text("×").size(fonts.section).color(text_icon_color))
            .style(move |_theme, status| {
                let text_color = match status {
                    iced::widget::button::Status::Hovered => Color::from_rgb8(0xCD, 0xD6, 0xF4),
                    _ => text_icon_color,
                };
                iced::widget::button::Style {
                    background: None,
                    text_color,
                    ..Default::default()
                }
            })
            .padding(0)
            .on_press(Message::Tab(TabMessage::Close(tab_id))),
    )
    .width(close_button_width)
    .align_x(Alignment::Center)
    .into();

    let status_indicator = agent_status_indicator(tab.agent_status, tab.needs_attention, fonts);

    let content = row![
        status_indicator,
        icon,
        text(title).size(fonts.body).color(text_icon_color),
        text(session_number)
            .size(fonts.caption)
            .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
        close_button,
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // Background colors
    let bg_color = if is_active {
        Color::from_rgb8(0x41, 0x43, 0x55) // #414355 - active
    } else {
        Color::from_rgb8(0x27, 0x27, 0x38) // #272738 - inactive
    };

    let tab_button = button(container(content).padding(Padding::new(6.0).left(14.0).right(8.0)))
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered if !is_active => {
                    Color::from_rgb8(0x35, 0x35, 0x48) // Slightly lighter on hover
                }
                _ => bg_color,
            };
            // Focus ring border
            let border_color = if is_focused {
                theme.focus_ring
            } else {
                Color::TRANSPARENT
            };
            let border_width = if is_focused { 2.0 } else { 0.0 };
            iced::widget::button::Style {
                background: Some(background.into()),
                text_color: text_icon_color,
                border: iced::Border {
                    color: border_color,
                    width: border_width,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(0)
        .on_press(Message::Tab(TabMessage::Select(tab_id)));

    if tab.tab_type == TabType::Terminal {
        capture_mouse_area(tab_button)
            .on_right_press(move |x, y| Message::Tab(TabMessage::ShowContextMenu(tab_id, x, y)))
            .into()
    } else {
        tab_button.into()
    }
}

fn should_show_session_number(tabs: &[Tab], tab: &Tab) -> bool {
    if tab.tab_type != TabType::Terminal {
        return false;
    }

    tabs.iter()
        .filter(|candidate| candidate.tab_type == TabType::Terminal)
        .filter(|candidate| candidate.title == tab.title)
        .count()
        > 1
}

fn truncate_title(title: &str, max_chars: usize) -> String {
    let mut chars = title.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        let keep = max_chars.saturating_sub(3);
        format!("{}...", title.chars().take(keep).collect::<String>())
    } else {
        truncated
    }
}

fn agent_status_indicator<'a>(
    status: Option<TabAgentStatus>,
    needs_attention: bool,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let Some(status) = status else {
        let color = if needs_attention {
            Color::from_rgb8(0xf9, 0xe2, 0xaf)
        } else {
            Color::from_rgba8(0xff, 0xff, 0xff, 0.0)
        };

        return container(text("•").size(fonts.body).color(color))
            .width(14.0)
            .height(14.0)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();
    };

    let label = match status.kind {
        TabAgentKind::Codex => "Codex",
        TabAgentKind::Claude => "Claude",
    };
    let activity_label = match status.activity {
        TabAgentActivity::Working => "working",
        TabAgentActivity::Ready => "ready",
        TabAgentActivity::NeedsInput => "needs input",
    };
    let tooltip_label = format!("{} {}", label, activity_label);

    let base = match status.activity {
        TabAgentActivity::NeedsInput => Color::from_rgb8(0xf9, 0xe2, 0xaf),
        _ => agent_accent(status.kind),
    };
    let alpha = match status.activity {
        TabAgentActivity::Working => pulse_alpha(2400, 0.45, 1.0),
        TabAgentActivity::Ready => 1.0,
        TabAgentActivity::NeedsInput => pulse_alpha(1200, 0.45, 1.0),
    };
    let dot_color = Color { a: alpha, ..base };

    const DOT_SIZE: f32 = 8.0;
    let dot = container(Space::new())
        .width(Length::Fixed(DOT_SIZE))
        .height(Length::Fixed(DOT_SIZE))
        .style(move |_| container::Style {
            background: Some(dot_color.into()),
            border: iced::Border {
                radius: (DOT_SIZE / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let indicator = container(dot)
        .width(14.0)
        .height(14.0)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    tooltip(
        indicator,
        column![
            text(tooltip_label)
                .size(fonts.label)
                .color(Color::from_rgb8(0xCD, 0xD6, 0xF4)),
            Space::new().height(Length::Fixed(1.0))
        ],
        tooltip::Position::Bottom,
    )
    .style(move |_theme| container::Style {
        background: Some(Color::from_rgb8(0x1e, 0x1e, 0x2e).into()),
        border: iced::Border {
            color: base,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .padding(8)
    .into()
}

fn agent_accent(kind: TabAgentKind) -> Color {
    match kind {
        TabAgentKind::Codex => Color::from_rgb8(0x8a, 0xb4, 0xf0),
        TabAgentKind::Claude => Color::from_rgb8(0xe5, 0xa9, 0x7a),
    }
}

fn pulse_alpha(period_ms: u128, min: f32, max: f32) -> f32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let phase = ((now % period_ms) as f32) / (period_ms as f32);
    let normalized = (1.0 - (phase * std::f32::consts::TAU).cos()) * 0.5;
    min + (max - min) * normalized
}

/// New tab "+" button
fn new_tab_button(theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    button(
        container(text("+").size(fonts.heading).color(theme.text_secondary))
            .padding(Padding::new(7.0).left(12.0).right(12.0)),
    )
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => None,
        };
        iced::widget::button::Style {
            background,
            text_color: theme.text_secondary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding(0)
    .on_press(Message::Tab(TabMessage::New))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeId, get_theme};
    use iced::advanced::mouse;
    use iced::{Event, Point};

    fn make_tabs() -> Vec<Tab> {
        ["alpha", "beta", "gamma"]
            .into_iter()
            .enumerate()
            .map(|(i, title)| Tab::new_terminal(Uuid::new_v4(), title.to_string(), None, i + 1))
            .collect()
    }

    fn tab_bar_element<'a>(tabs: &'a [Tab], hosts: &'a HostsConfig) -> Element<'a, Message> {
        tab_bar_view(
            tabs,
            Some(tabs[0].id),
            SidebarState::Expanded,
            get_theme(ThemeId::default()),
            ScaledFonts::new(1.0),
            FocusSection::Content,
            0,
            &View::HostGrid,
            hosts,
        )
    }

    /// Dragging the first tab past the last tab's midpoint must publish a
    /// reorder from index 0 to index 2, while a release without movement
    /// must keep publishing plain Select.
    #[test]
    fn dragging_a_tab_across_its_siblings_publishes_reorder() {
        let tabs = make_tabs();
        let hosts = HostsConfig::default();
        let mut ui = iced_test::simulator(tab_bar_element(&tabs, &hosts));

        let start = ui
            .find("alpha")
            .expect("first tab should be present")
            .visible_bounds()
            .expect("first tab should be visible")
            .center();
        let gamma_bounds = ui
            .find("gamma")
            .expect("last tab should be present")
            .visible_bounds()
            .expect("last tab should be visible");
        // Well past the last tab's midpoint.
        let end = Point::new(gamma_bounds.x + gamma_bounds.width + 10.0, start.y);

        ui.point_at(start);
        let _ = ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
        ui.point_at(end);
        let _ = ui.simulate([
            Event::Mouse(mouse::Event::CursorMoved { position: end }),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);

        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.iter().any(|message| matches!(
                message,
                Message::Tab(TabMessage::Reorder { from: 0, to: 2 })
            )),
            "expected Reorder {{ from: 0, to: 2 }}, got: {messages:?}"
        );
    }

    #[test]
    fn clicking_a_tab_without_dragging_still_selects_it() {
        let tabs = make_tabs();
        let beta_id = tabs[1].id;
        let hosts = HostsConfig::default();
        let mut ui = iced_test::simulator(tab_bar_element(&tabs, &hosts));

        let _ = ui.click("beta").expect("second tab should be clickable");

        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|message| matches!(
                    message,
                    Message::Tab(TabMessage::Select(id)) if *id == beta_id
                )),
            "expected Select({beta_id}), got: {messages:?}"
        );
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::Tab(TabMessage::Reorder { .. }))),
            "a plain click must not reorder, got: {messages:?}"
        );
    }
}
