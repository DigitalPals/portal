//! Snippet execution results panel
//!
//! Shows detailed execution results for a snippet, including
//! per-host status, output, and timing information.

use std::time::Duration;

use iced::widget::{Column, Row, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Font, Length};
use uuid::Uuid;

/// Monospace font for output display
const MONOSPACE_FONT: Font = Font::with_name("monospace");

/// Sanitize command output for display
/// Replaces tabs with spaces and removes other control characters
fn sanitize_output(output: &str) -> String {
    output
        .replace('\t', "    ") // Replace tabs with 4 spaces
        .chars()
        .filter(|c| !c.is_control() || *c == '\n') // Keep newlines, remove other control chars
        .collect()
}

use crate::app::managers::ExecutionStatus;
use crate::config::SnippetExecutionEntry;
use crate::icons::{self, icon_with_color};
use crate::message::{Message, SnippetMessage};
use crate::theme::{
    RESULTS_PANEL_WIDTH, STATUS_FAILURE, STATUS_PARTIAL, STATUS_SUCCESS, ScaledFonts, Theme,
};

/// Data for displaying a host result row
struct HostResultData {
    host_name: String,
    success: bool,
    error: Option<String>,
    stdout: String,
    duration: Duration,
    expanded: bool,
    /// If Some, row is clickable for expand/collapse
    click_action: Option<(Uuid, Uuid)>, // (snippet_id, host_id)
}

impl HostResultData {
    /// Create from current execution result
    fn from_current(result: &crate::app::managers::HostResult, snippet_id: Uuid) -> Self {
        let (success, error) = match &result.status {
            ExecutionStatus::Success => (true, None),
            ExecutionStatus::Failed(err) => (false, Some(err.clone())),
            ExecutionStatus::Pending | ExecutionStatus::Running => (true, None),
        };
        Self {
            host_name: result.host_name.clone(),
            success,
            error,
            stdout: result.stdout.clone(),
            duration: result.duration,
            expanded: result.expanded,
            click_action: Some((snippet_id, result.host_id)),
        }
    }

    /// Create from historical result
    fn from_historical(result: &crate::config::HistoricalHostResult) -> Self {
        Self {
            host_name: result.host_name.clone(),
            success: result.success,
            error: result.error.clone(),
            stdout: result.stdout.clone(),
            duration: Duration::from_millis(result.duration_ms),
            expanded: true, // Always expanded for historical
            click_action: None,
        }
    }
}

/// Context for rendering the results panel
pub struct ResultsPanelContext<'a> {
    pub snippet_id: Uuid,
    pub snippet_name: &'a str,
    pub command: &'a str,
    pub host_results: Option<&'a [crate::app::managers::HostResult]>,
    pub completed: bool,
    pub success_count: usize,
    pub failure_count: usize,
    pub history_entries: &'a [&'a SnippetExecutionEntry],
    pub viewed_entry: Option<&'a SnippetExecutionEntry>,
    pub theme: Theme,
    pub fonts: ScaledFonts,
}

/// Build the results panel for a snippet execution
pub fn execution_results_panel(ctx: ResultsPanelContext<'_>) -> Element<'static, Message> {
    let ResultsPanelContext {
        snippet_id,
        snippet_name,
        command,
        host_results,
        completed,
        success_count,
        failure_count,
        history_entries,
        viewed_entry,
        theme,
        fonts,
    } = ctx;
    // Clone the data we need
    let snippet_name = snippet_name.to_string();

    // Close button
    let close_btn = button(icon_with_color(icons::ui::X, 16, theme.text_muted))
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => theme.hover,
                _ => iced::Color::TRANSPARENT,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_muted,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding(4)
        .on_press(Message::Snippet(SnippetMessage::Deselect));

    // Determine what to show:
    // 1. If viewing a specific history entry, show that
    // 2. If there's current execution data, show current view
    // 3. Otherwise show history-only view
    let content = if let Some(entry) = viewed_entry {
        // Viewing historical entry
        build_history_view(
            snippet_id,
            &snippet_name,
            entry,
            history_entries,
            close_btn,
            theme,
            fonts,
        )
    } else if let Some(results) = host_results {
        // Viewing current execution
        build_current_view(
            snippet_id,
            &snippet_name,
            command,
            results,
            completed,
            success_count,
            failure_count,
            history_entries,
            close_btn,
            theme,
            fonts,
        )
    } else {
        // No current execution, show history-only view
        build_history_only_view(
            snippet_id,
            &snippet_name,
            history_entries,
            close_btn,
            theme,
            fonts,
        )
    };

    container(content)
        .width(Length::Fixed(RESULTS_PANEL_WIDTH))
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Build the current execution view
#[allow(clippy::too_many_arguments)]
fn build_current_view(
    snippet_id: Uuid,
    snippet_name: &str,
    command: &str,
    host_results: &[crate::app::managers::HostResult],
    completed: bool,
    success_count: usize,
    failure_count: usize,
    history_entries: &[&SnippetExecutionEntry],
    close_btn: iced::widget::Button<'static, Message>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Column<'static, Message> {
    let snippet_name = snippet_name.to_string();
    let command = command.to_string();

    // Header with snippet name and overall status
    let status_text = if completed {
        if failure_count == 0 {
            format!("{} succeeded", success_count)
        } else if success_count == 0 {
            format!("{} failed", failure_count)
        } else {
            format!("{} succeeded, {} failed", success_count, failure_count)
        }
    } else {
        "Running...".to_string()
    };

    let status_color = if completed {
        if failure_count == 0 {
            STATUS_SUCCESS
        } else {
            STATUS_FAILURE
        }
    } else {
        theme.accent
    };

    let header = row![
        column![
            text(snippet_name)
                .size(fonts.section)
                .color(theme.text_primary),
            text(status_text)
                .size(fonts.button_small)
                .color(status_color),
        ]
        .spacing(4),
        Space::new().width(Length::Fill),
        close_btn,
    ]
    .align_y(Alignment::Center);

    // Command display
    let command_display = container(text(command).size(fonts.label).color(theme.text_secondary))
        .padding(12)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    // Clone host results data and build result rows
    let results: Vec<Element<'static, Message>> = host_results
        .iter()
        .map(|r| host_result_row(HostResultData::from_current(r, snippet_id), theme, fonts))
        .collect();

    let results_list = scrollable(Column::with_children(results).spacing(8).width(Fill))
        .height(Length::Fixed(300.0));

    // Build history section
    let history_section: Element<'static, Message> = if !history_entries.is_empty() {
        let history_items: Vec<Element<'static, Message>> = history_entries
            .iter()
            .map(|entry| history_entry_row(entry, None, theme, fonts))
            .collect();

        column![
            Space::new().height(16),
            text("History").size(fonts.body).color(theme.text_secondary),
            Space::new().height(8),
            scrollable(Column::with_children(history_items).spacing(4).width(Fill))
                .height(Length::Fixed(108.0)),
        ]
        .into()
    } else {
        Space::new().height(0).into()
    };

    column![
        header,
        Space::new().height(12),
        command_display,
        Space::new().height(16),
        text("Results").size(fonts.body).color(theme.text_secondary),
        Space::new().height(8),
        results_list,
        Space::new().height(Fill),
        history_section,
    ]
    .padding(16)
}

/// Build the history view showing a past execution
fn build_history_view(
    _snippet_id: Uuid,
    snippet_name: &str,
    entry: &SnippetExecutionEntry,
    history_entries: &[&SnippetExecutionEntry],
    close_btn: iced::widget::Button<'static, Message>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Column<'static, Message> {
    let snippet_name = snippet_name.to_string();
    let time_ago = entry.time_ago();

    // Status
    let status_text = if entry.failure_count == 0 {
        format!("{} succeeded", entry.success_count)
    } else if entry.success_count == 0 {
        format!("{} failed", entry.failure_count)
    } else {
        format!(
            "{} succeeded, {} failed",
            entry.success_count, entry.failure_count
        )
    };

    let status_color = if entry.failure_count == 0 {
        STATUS_SUCCESS
    } else {
        STATUS_FAILURE
    };

    let header = row![
        column![
            text(snippet_name)
                .size(fonts.section)
                .color(theme.text_primary),
            row![
                text(status_text)
                    .size(fonts.button_small)
                    .color(status_color),
                Space::new().width(8),
                text(format!("• {}", time_ago))
                    .size(fonts.button_small)
                    .color(theme.text_muted),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(4),
        Space::new().width(Length::Fill),
        close_btn,
    ]
    .align_y(Alignment::Center);

    // Back button
    let back_btn = button(
        row![
            icon_with_color(icons::ui::CHEVRON_LEFT, 14, theme.text_primary),
            text("Back to Current")
                .size(fonts.label)
                .color(theme.text_primary),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => theme.surface,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([6, 12])
    .on_press(Message::Snippet(SnippetMessage::ViewCurrentResults));

    // Command display
    let command = entry.command.clone();
    let command_display = container(text(command).size(fonts.label).color(theme.text_secondary))
        .padding(12)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    // Historical host results
    let results: Vec<Element<'static, Message>> = entry
        .host_results
        .iter()
        .map(|r| host_result_row(HostResultData::from_historical(r), theme, fonts))
        .collect();

    let results_list = scrollable(Column::with_children(results).spacing(8).width(Fill))
        .height(Length::Fixed(300.0));

    // History section (for navigation)
    let viewed_id = entry.id;
    let history_items: Vec<Element<'static, Message>> = history_entries
        .iter()
        .map(|e| history_entry_row(e, Some(viewed_id), theme, fonts))
        .collect();

    let history_section = column![
        Space::new().height(16),
        text("History").size(fonts.body).color(theme.text_secondary),
        Space::new().height(8),
        scrollable(Column::with_children(history_items).spacing(4).width(Fill))
            .height(Length::Fixed(108.0)),
    ];

    column![
        header,
        Space::new().height(8),
        back_btn,
        Space::new().height(12),
        command_display,
        Space::new().height(16),
        text("Results").size(fonts.body).color(theme.text_secondary),
        Space::new().height(8),
        results_list,
        Space::new().height(Fill),
        history_section,
    ]
    .padding(16)
}

/// Build the history-only view (no current execution)
fn build_history_only_view(
    _snippet_id: Uuid,
    snippet_name: &str,
    history_entries: &[&SnippetExecutionEntry],
    close_btn: iced::widget::Button<'static, Message>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Column<'static, Message> {
    let snippet_name = snippet_name.to_string();

    let header = row![
        column![
            text(snippet_name)
                .size(fonts.section)
                .color(theme.text_primary),
            text("No recent execution")
                .size(fonts.button_small)
                .color(theme.text_muted),
        ]
        .spacing(4),
        Space::new().width(Length::Fill),
        close_btn,
    ]
    .align_y(Alignment::Center);

    // History list
    let history_items: Vec<Element<'static, Message>> = history_entries
        .iter()
        .map(|entry| history_entry_row(entry, None, theme, fonts))
        .collect();

    let history_section = if !history_items.is_empty() {
        column![
            text("History").size(fonts.body).color(theme.text_secondary),
            Space::new().height(8),
            scrollable(Column::with_children(history_items).spacing(4).width(Fill))
                .height(Length::Fixed(400.0)),
        ]
    } else {
        column![
            text("No execution history")
                .size(fonts.body)
                .color(theme.text_muted),
        ]
    };

    column![header, Space::new().height(16), history_section,].padding(16)
}

/// Single host result row (unified for both current and historical results)
fn host_result_row(
    data: HostResultData,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    // Status icon
    let (status_icon, status_color) = if data.success {
        (icons::ui::CHECK, STATUS_SUCCESS)
    } else {
        (icons::ui::X, STATUS_FAILURE)
    };

    let icon = icon_with_color(status_icon, 14, status_color);

    // Duration text
    let duration_text = if data.duration.as_millis() > 0 {
        format!("{:.1}s", data.duration.as_secs_f64())
    } else {
        String::new()
    };

    // Error message if failed
    let error_section: Element<'static, Message> = if let Some(err) = &data.error {
        column![
            Space::new().height(4),
            text(err.clone()).size(fonts.small).color(STATUS_FAILURE),
        ]
        .into()
    } else {
        Space::new().height(0).into()
    };

    // Header row - include expand/collapse chevron only for clickable rows
    let is_clickable = data.click_action.is_some();
    let header_row: Row<'static, Message> = if is_clickable {
        row![
            icon,
            Space::new().width(8),
            text(data.host_name.clone())
                .size(fonts.body)
                .color(theme.text_primary),
            Space::new().width(Length::Fill),
            text(duration_text)
                .size(fonts.label)
                .color(theme.text_muted),
            Space::new().width(8),
            if data.expanded {
                icon_with_color(icons::ui::CHEVRON_DOWN, 12, theme.text_muted)
            } else {
                icon_with_color(icons::ui::CHEVRON_RIGHT, 12, theme.text_muted)
            },
        ]
        .align_y(Alignment::Center)
    } else {
        row![
            icon,
            Space::new().width(8),
            text(data.host_name.clone())
                .size(fonts.body)
                .color(theme.text_primary),
            Space::new().width(Length::Fill),
            text(duration_text)
                .size(fonts.label)
                .color(theme.text_muted),
        ]
        .align_y(Alignment::Center)
    };

    // Output section (if expanded)
    let output_section: Element<'static, Message> = if data.expanded && !data.stdout.is_empty() {
        let sanitized_output = sanitize_output(&data.stdout);
        let output_height = if is_clickable { 150.0 } else { 100.0 };

        let output = container(
            scrollable(
                text(sanitized_output)
                    .size(fonts.label)
                    .font(MONOSPACE_FONT)
                    .color(theme.text_secondary),
            )
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(Length::Fixed(output_height))
            .width(Fill),
        )
        .padding(10)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        column![Space::new().height(8), output, error_section,].into()
    } else if data.expanded {
        error_section
    } else {
        Space::new().height(0).into()
    };

    // Row content
    let row_content = column![header_row, output_section,];

    // Return clickable button or static container based on click_action
    if let Some((snippet_id, host_id)) = data.click_action {
        button(container(row_content).padding(8).width(Fill))
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme.hover,
                    _ => iced::Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: theme.text_primary,
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .padding(0)
            .width(Fill)
            .on_press(Message::Snippet(SnippetMessage::ToggleResultExpand(
                snippet_id, host_id,
            )))
            .into()
    } else {
        container(row_content)
            .padding(8)
            .width(Fill)
            .style(move |_theme| container::Style {
                background: Some(iced::Color::TRANSPARENT.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}

/// History entry row showing a past execution (clickable)
fn history_entry_row(
    entry: &SnippetExecutionEntry,
    selected_id: Option<Uuid>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let entry_id = entry.id;
    let time_ago = entry.time_ago();
    let success = entry.success_count;
    let failed = entry.failure_count;
    let is_selected = selected_id == Some(entry_id);

    // Status icon
    let (icon, color) = if failed == 0 {
        (icons::ui::CHECK, STATUS_SUCCESS)
    } else if success == 0 {
        (icons::ui::X, STATUS_FAILURE)
    } else {
        (icons::ui::X, STATUS_PARTIAL)
    };

    // Status text
    let status = if failed == 0 {
        format!("{} OK", success)
    } else if success == 0 {
        format!("{} failed", failed)
    } else {
        format!("{}/{} OK", success, success + failed)
    };

    let row_content = row![
        icon_with_color(icon, 12, color),
        Space::new().width(6),
        text(status).size(fonts.label).color(color),
        Space::new().width(Length::Fill),
        text(time_ago).size(fonts.small).color(theme.text_muted),
    ]
    .align_y(Alignment::Center);

    button(container(row_content).padding([6, 8]).width(Fill))
        .style(move |_theme, status| {
            let bg = match (status, is_selected) {
                (_, true) => theme.accent.scale_alpha(0.2),
                (button::Status::Hovered, _) => theme.hover,
                _ => theme.surface,
            };
            let border = if is_selected {
                iced::Border {
                    color: theme.accent,
                    width: 1.0,
                    radius: 4.0.into(),
                }
            } else {
                iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                }
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border,
                ..Default::default()
            }
        })
        .padding(0)
        .width(Fill)
        .on_press(Message::Snippet(SnippetMessage::ViewHistoryEntry(entry_id)))
        .into()
}
