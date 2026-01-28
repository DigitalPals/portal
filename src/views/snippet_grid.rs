//! Snippets page view with grid layout
//!
//! Displays command snippets in a card grid layout, similar to the hosts page.
//! Supports snippet search, editing, and execution with results display.

use iced::widget::{
    Column, Row, Space, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::app::managers::{SnippetExecution, SnippetExecutionManager};
use crate::app::{SidebarState, SnippetEditState};
use crate::config::{DetectedOs, Snippet, SnippetHistoryConfig};
use crate::icons::{self, icon_with_color};
use crate::message::{Message, SnippetMessage};
use crate::theme::{
    BORDER_RADIUS, CARD_BORDER_RADIUS, GRID_PADDING, GRID_SPACING,
    MIN_SNIPPET_CARD_WIDTH, RESULTS_PANEL_WIDTH, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED,
    STATUS_FAILURE, STATUS_SUCCESS, STATUS_SUCCESS_DARK, ScaledFonts, Theme,
};
use crate::views::snippet_results::{ResultsPanelContext, execution_results_panel};

/// Search input ID for auto-focus
pub fn snippet_search_input_id() -> iced::widget::Id {
    iced::widget::Id::new("snippets_search")
}

/// Card height for snippet cards (slightly taller than host cards for extra info)
const SNIPPET_CARD_HEIGHT: f32 = 80.0;

/// Calculate the number of columns based on available width
pub fn calculate_columns(
    window_width: f32,
    sidebar_state: SidebarState,
    results_panel_visible: bool,
) -> usize {
    let sidebar_width = match sidebar_state {
        SidebarState::Hidden => 0.0,
        SidebarState::IconsOnly => SIDEBAR_WIDTH_COLLAPSED,
        SidebarState::Expanded => SIDEBAR_WIDTH,
    };

    let panel_width = if results_panel_visible {
        RESULTS_PANEL_WIDTH
    } else {
        0.0
    };

    let content_width = window_width - sidebar_width - GRID_PADDING - panel_width;
    let columns =
        ((content_width + GRID_SPACING) / (MIN_SNIPPET_CARD_WIDTH + GRID_SPACING)).floor() as usize;
    columns.clamp(1, 4)
}

/// Build the action bar with search and new snippet button
fn build_action_bar(search_query: &str, theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    // Search input - pill-shaped
    let search_input: iced::widget::TextInput<'static, Message> =
        text_input("Search snippets...", search_query)
            .id(snippet_search_input_id())
            .on_input(|s| Message::Snippet(SnippetMessage::SearchChanged(s)))
            .padding([12, 20])
            .width(Length::Fill)
            .style(move |_theme, status| {
                use iced::widget::text_input::{Status, Style};
                let border_color = match status {
                    Status::Focused { .. } => theme.accent,
                    _ => theme.border,
                };
                Style {
                    background: theme.background.into(),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 22.0.into(),
                    },
                    icon: theme.text_muted,
                    placeholder: theme.text_muted,
                    value: theme.text_primary,
                    selection: theme.selected,
                }
            });

    // New Snippet button - pill-shaped with border
    let new_btn = button(
        row![
            icon_with_color(icons::ui::PLUS, 14, theme.text_primary),
            text("New Snippet")
                .size(fonts.button_small)
                .color(theme.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => theme.background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 22.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([12, 20])
    .on_press(Message::Snippet(SnippetMessage::New));

    // Build the bar row
    let bar_content =
        row![search_input, Space::new().width(12), new_btn,].align_y(Alignment::Center);

    container(bar_content)
        .width(Fill)
        .padding([16, 24])
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border::default(),
            ..Default::default()
        })
        .into()
}

/// Context for rendering the snippet page
pub struct SnippetPageContext<'a> {
    pub snippets: &'a [Snippet],
    pub search_query: &'a str,
    pub editing: Option<&'a SnippetEditState>,
    pub hosts: &'a [(Uuid, String, Option<DetectedOs>)],
    pub executions: &'a SnippetExecutionManager,
    pub snippet_history: &'a SnippetHistoryConfig,
    pub column_count: usize,
    pub theme: Theme,
    pub fonts: ScaledFonts,
    pub hovered_snippet: Option<Uuid>,
    pub selected_snippet: Option<Uuid>,
    pub viewed_history_entry: Option<Uuid>,
}

/// Main snippet page view
pub fn snippet_page_view(ctx: SnippetPageContext<'_>) -> Element<'static, Message> {
    let SnippetPageContext {
        snippets,
        search_query,
        editing,
        hosts,
        executions,
        snippet_history,
        column_count,
        theme,
        fonts,
        hovered_snippet,
        selected_snippet,
        viewed_history_entry,
    } = ctx;
    // If editing, show edit form instead of grid
    if let Some(edit_state) = editing {
        return super::snippet_edit::snippet_edit_view(edit_state, hosts, theme, fonts);
    }

    // Filter snippets by search query
    let filtered_snippets: Vec<&Snippet> = snippets
        .iter()
        .filter(|s| {
            search_query.is_empty()
                || s.name.to_lowercase().contains(&search_query.to_lowercase())
                || s.command
                    .to_lowercase()
                    .contains(&search_query.to_lowercase())
        })
        .collect();

    // Action bar
    let action_bar = build_action_bar(search_query, theme, fonts);

    // Grid content
    let grid_content = if filtered_snippets.is_empty() && snippets.is_empty() {
        empty_state(theme, fonts)
    } else if filtered_snippets.is_empty() {
        no_results_state(theme, fonts)
    } else {
        build_snippet_grid(
            filtered_snippets,
            executions,
            column_count,
            theme,
            fonts,
            hovered_snippet,
            selected_snippet,
        )
    };

    let scrollable_content = scrollable(grid_content).height(Fill).width(Fill);

    // Main layout: action bar at top, scrollable content fills remaining space
    let main_content = column![action_bar, scrollable_content];

    // Check if we should show results panel for selected snippet
    let results_panel: Option<Element<'static, Message>> =
        selected_snippet.and_then(|snippet_id| {
            // Find the snippet
            let snippet = snippets.iter().find(|s| s.id == snippet_id)?;

            // Get execution results (either active or last completed)
            let execution = executions
                .get_active(snippet_id)
                .or_else(|| executions.get_last_result(snippet_id));

            // Get history entries for this snippet
            let history_entries = snippet_history.entries_for_snippet(snippet_id);

            // Find the viewed history entry if any
            let viewed_entry = viewed_history_entry.and_then(|id| snippet_history.find_entry(id));

            // Show panel if we have execution OR history
            if execution.is_none() && history_entries.is_empty() {
                return None;
            }

            Some(execution_results_panel(ResultsPanelContext {
                snippet_id,
                snippet_name: &snippet.name,
                command: &snippet.command,
                host_results: execution.map(|e| e.host_results.as_slice()),
                completed: execution.map(|e| e.completed).unwrap_or(true),
                success_count: execution.map(|e| e.success_count()).unwrap_or(0),
                failure_count: execution.map(|e| e.failure_count()).unwrap_or(0),
                history_entries: &history_entries,
                viewed_entry,
                theme,
                fonts,
            }))
        });

    // Layout with optional results panel
    let page_content: Element<'static, Message> = if let Some(panel) = results_panel {
        row![container(main_content).width(Fill).height(Fill), panel,].into()
    } else {
        container(main_content).width(Fill).height(Fill).into()
    };

    container(page_content)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}

/// Build the snippet grid
fn build_snippet_grid(
    snippets: Vec<&Snippet>,
    executions: &SnippetExecutionManager,
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
    hovered_snippet: Option<Uuid>,
    selected_snippet: Option<Uuid>,
) -> Element<'static, Message> {
    let section_header = text("Snippets")
        .size(fonts.section)
        .color(theme.text_primary);

    // Build grid of snippet cards
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for snippet in snippets {
        let is_hovered = hovered_snippet == Some(snippet.id);
        let is_selected = selected_snippet == Some(snippet.id);
        let is_running = executions.is_running(snippet.id);
        let last_execution = executions.get_last_result(snippet.id);

        current_row.push(snippet_card(
            snippet,
            is_hovered,
            is_selected,
            is_running,
            last_execution,
            theme,
            fonts,
        ));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING)
                    .into(),
            );
        }
    }

    // Add remaining cards in the last row with spacers
    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(
                Space::new()
                    .width(Length::Fixed(MIN_SNIPPET_CARD_WIDTH))
                    .into(),
            );
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    let grid = Column::with_children(rows).spacing(GRID_SPACING);

    column![section_header, grid]
        .spacing(12)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0))
        .into()
}

/// Single snippet card
fn snippet_card(
    snippet: &Snippet,
    is_hovered: bool,
    is_selected: bool,
    is_running: bool,
    last_execution: Option<&SnippetExecution>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let snippet_id = snippet.id;

    // Terminal/code icon with accent background
    let icon_widget = container(icon_with_color(icons::ui::CODE, 20, iced::Color::WHITE))
        .width(40)
        .height(40)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(theme.accent.into()),
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    // Command preview (truncated)
    let cmd_preview = if snippet.command.len() > 40 {
        format!("{}...", &snippet.command[..40])
    } else {
        snippet.command.clone()
    };

    // Status text
    let status_text = if is_running {
        format!("Running on {} hosts...", snippet.host_ids.len())
    } else if snippet.host_ids.is_empty() {
        "No hosts assigned".to_string()
    } else {
        format!("{} hosts", snippet.host_ids.len())
    };

    // Execution status indicator
    let status_indicator: Element<'static, Message> = if let Some(exec) = last_execution {
        if exec.completed {
            let success = exec.success_count();
            let failed = exec.failure_count();
            if failed == 0 {
                row![
                    icon_with_color(icons::ui::CHECK, 12, STATUS_SUCCESS),
                    text(format!("{} OK", success))
                        .size(fonts.small)
                        .color(STATUS_SUCCESS),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
            } else {
                row![
                    icon_with_color(icons::ui::X, 12, STATUS_FAILURE),
                    text(format!("{} failed", failed))
                        .size(fonts.small)
                        .color(STATUS_FAILURE),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
            }
        } else {
            text("Running...")
                .size(fonts.small)
                .color(theme.accent)
                .into()
        }
    } else {
        Space::new().into()
    };

    // Info column
    let info = column![
        text(snippet.name.clone())
            .size(fonts.section)
            .color(theme.text_primary),
        text(cmd_preview)
            .size(fonts.label)
            .color(theme.text_muted),
        row![
            text(status_text)
                .size(fonts.small)
                .color(theme.text_secondary),
            Space::new().width(8),
            status_indicator,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(2);

    // Run button (visible on hover, disabled if no hosts or already running)
    let run_button: Element<'static, Message> =
        if is_hovered && !snippet.host_ids.is_empty() && !is_running {
            button(
                row![
                    icon_with_color(icons::ui::CHEVRON_RIGHT, 14, iced::Color::WHITE),
                    text("RUN").size(fonts.label).color(iced::Color::WHITE),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => STATUS_SUCCESS,
                    _ => STATUS_SUCCESS_DARK,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .padding([6, 12])
            .on_press(Message::Snippet(SnippetMessage::Run(snippet_id)))
            .into()
        } else if is_running {
            text("...")
                .size(fonts.body)
                .color(theme.text_muted)
                .into()
        } else {
            Space::new().into()
        };

    // Edit button (visible on hover)
    let edit_button: Element<'static, Message> = if is_hovered {
        button(icon_with_color(icons::ui::PENCIL, 16, theme.text_secondary))
            .padding(8)
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme.hover,
                    _ => iced::Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: theme.text_secondary,
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::Snippet(SnippetMessage::Edit(snippet_id)))
            .into()
    } else {
        Space::new().into()
    };

    // Action buttons in a tight row
    let action_buttons = row![run_button, edit_button,]
        .spacing(4)
        .align_y(Alignment::Center);

    let card_content = row![
        icon_widget,
        info,
        Space::new().width(Length::Fill),
        action_buttons,
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let card_button = button(
        container(card_content)
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fixed(SNIPPET_CARD_HEIGHT))
            .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let card_bg = iced::Color::from_rgb8(0x28, 0x2B, 0x3D);
        let (bg, shadow_alpha) = match (status, is_selected, is_hovered) {
            (_, true, _) => (theme.hover, 0.25),
            (_, _, true) => (theme.hover, 0.25),
            (button::Status::Hovered, _, _) => (theme.hover, 0.25),
            _ => (card_bg, 0.15),
        };
        let border = if is_selected {
            iced::Border {
                color: theme.accent,
                width: 2.0,
                radius: 12.0.into(),
            }
        } else {
            iced::Border {
                radius: 12.0.into(),
                ..Default::default()
            }
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border,
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, shadow_alpha),
                offset: iced::Vector::new(0.0, 3.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        }
    })
    .padding(0)
    .width(Length::Fixed(MIN_SNIPPET_CARD_WIDTH))
    .height(Length::Fixed(SNIPPET_CARD_HEIGHT))
    .on_press(Message::Snippet(SnippetMessage::Select(snippet_id)));

    // Wrap in mouse_area for hover detection
    mouse_area(card_button)
        .on_enter(Message::Snippet(SnippetMessage::Hover(Some(snippet_id))))
        .on_exit(Message::Snippet(SnippetMessage::Hover(None)))
        .into()
}

/// Empty state when no snippets are configured
fn empty_state(theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::CODE, 48, theme.text_muted),
        text("No snippets yet")
            .size(fonts.heading)
            .color(theme.text_primary),
        text("Create a snippet to run commands on multiple hosts")
            .size(fonts.body)
            .color(theme.text_muted),
        Space::new().height(16),
        button(
            row![
                icon_with_color(icons::ui::PLUS, 14, iced::Color::WHITE),
                text("NEW SNIPPET")
                    .size(fonts.body)
                    .color(iced::Color::WHITE),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
                _ => theme.accent,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([10, 20])
        .on_press(Message::Snippet(SnippetMessage::New)),
    ]
    .spacing(8)
    .align_x(Alignment::Center);

    container(content)
        .width(Fill)
        .height(Length::Fixed(300.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// No results state when search has no matches
fn no_results_state(theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::CODE, 48, theme.text_muted),
        text("No matching snippets")
            .size(fonts.heading)
            .color(theme.text_primary),
        text("Try a different search term")
            .size(fonts.body)
            .color(theme.text_muted),
    ]
    .spacing(8)
    .align_x(Alignment::Center);

    container(content)
        .width(Fill)
        .height(Length::Fixed(300.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0))
        .into()
}
