//! Snippet execution results panel
//!
//! Shows detailed execution results for a snippet, including
//! per-host status, output, and timing information.

use std::time::Duration;

use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
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
use crate::theme::{Theme, RESULTS_PANEL_WIDTH};

/// Data for displaying a host result (cloned from HostResult)
struct HostResultData {
    host_id: Uuid,
    host_name: String,
    status: ExecutionStatus,
    stdout: String,
    duration: Duration,
    expanded: bool,
}

/// Build the results panel for a snippet execution
#[allow(clippy::too_many_arguments)]
pub fn execution_results_panel(
    snippet_id: Uuid,
    snippet_name: &str,
    command: &str,
    host_results: Option<&[crate::app::managers::HostResult]>,
    completed: bool,
    success_count: usize,
    failure_count: usize,
    history_entries: &[&SnippetExecutionEntry],
    viewed_entry: Option<&SnippetExecutionEntry>,
    theme: Theme,
) -> Element<'static, Message> {
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
        build_history_view(snippet_id, &snippet_name, entry, history_entries, close_btn, theme)
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
        )
    } else {
        // No current execution, show history-only view
        build_history_only_view(snippet_id, &snippet_name, history_entries, close_btn, theme)
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
            iced::Color::from_rgb8(0x40, 0xa0, 0x2b)
        } else {
            iced::Color::from_rgb8(0xd2, 0x0f, 0x39)
        }
    } else {
        theme.accent
    };

    let header = row![
        column![
            text(snippet_name).size(16).color(theme.text_primary),
            text(status_text).size(13).color(status_color),
        ]
        .spacing(4),
        Space::new().width(Length::Fill),
        close_btn,
    ]
    .align_y(Alignment::Center);

    // Command display
    let command_display = container(text(command).size(12).color(theme.text_secondary))
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

    // Clone host results data
    let host_data: Vec<HostResultData> = host_results
        .iter()
        .map(|r| HostResultData {
            host_id: r.host_id,
            host_name: r.host_name.clone(),
            status: r.status.clone(),
            stdout: r.stdout.clone(),
            duration: r.duration,
            expanded: r.expanded,
        })
        .collect();

    // Host results list
    let results: Vec<Element<'static, Message>> = host_data
        .into_iter()
        .map(|data| host_result_row(data, snippet_id, theme))
        .collect();

    let results_list =
        scrollable(Column::with_children(results).spacing(8).width(Fill)).height(Length::Fixed(300.0));

    // Clear button (only if completed)
    let clear_btn = if completed {
        button(text("Clear Results").size(12).color(theme.text_primary))
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
            .on_press(Message::Snippet(SnippetMessage::ClearResults(snippet_id)))
    } else {
        button(text("Clear Results").size(12).color(theme.text_muted))
            .style(move |_theme, _status| button::Style {
                background: Some(theme.surface.into()),
                text_color: theme.text_muted,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .padding([6, 12])
    };

    // Build history section
    let history_section: Element<'static, Message> = if !history_entries.is_empty() {
        let history_items: Vec<Element<'static, Message>> = history_entries
            .iter()
            .map(|entry| history_entry_row(entry, None, theme))
            .collect();

        column![
            Space::new().height(16),
            text("History").size(14).color(theme.text_secondary),
            Space::new().height(8),
            scrollable(Column::with_children(history_items).spacing(4).width(Fill))
                .height(Length::Fixed(150.0)),
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
        text("Results").size(14).color(theme.text_secondary),
        Space::new().height(8),
        results_list,
        Space::new().height(12),
        clear_btn,
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
) -> Column<'static, Message> {
    let snippet_name = snippet_name.to_string();
    let time_ago = entry.time_ago();

    // Status
    let status_text = if entry.failure_count == 0 {
        format!("{} succeeded", entry.success_count)
    } else if entry.success_count == 0 {
        format!("{} failed", entry.failure_count)
    } else {
        format!("{} succeeded, {} failed", entry.success_count, entry.failure_count)
    };

    let status_color = if entry.failure_count == 0 {
        iced::Color::from_rgb8(0x40, 0xa0, 0x2b)
    } else {
        iced::Color::from_rgb8(0xd2, 0x0f, 0x39)
    };

    let header = row![
        column![
            text(snippet_name).size(16).color(theme.text_primary),
            row![
                text(status_text).size(13).color(status_color),
                Space::new().width(8),
                text(format!("• {}", time_ago)).size(13).color(theme.text_muted),
            ].align_y(Alignment::Center),
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
            text("Back to Current").size(12).color(theme.text_primary),
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
    let command_display = container(text(command).size(12).color(theme.text_secondary))
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
        .map(|r| historical_host_result_row(r, theme))
        .collect();

    let results_list =
        scrollable(Column::with_children(results).spacing(8).width(Fill)).height(Length::Fixed(300.0));

    // History section (for navigation)
    let viewed_id = entry.id;
    let history_items: Vec<Element<'static, Message>> = history_entries
        .iter()
        .map(|e| history_entry_row(e, Some(viewed_id), theme))
        .collect();

    let history_section = column![
        Space::new().height(16),
        text("History").size(14).color(theme.text_secondary),
        Space::new().height(8),
        scrollable(Column::with_children(history_items).spacing(4).width(Fill))
            .height(Length::Fixed(150.0)),
    ];

    column![
        header,
        Space::new().height(8),
        back_btn,
        Space::new().height(12),
        command_display,
        Space::new().height(16),
        text("Results").size(14).color(theme.text_secondary),
        Space::new().height(8),
        results_list,
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
) -> Column<'static, Message> {
    let snippet_name = snippet_name.to_string();

    let header = row![
        column![
            text(snippet_name).size(16).color(theme.text_primary),
            text("No recent execution").size(13).color(theme.text_muted),
        ]
        .spacing(4),
        Space::new().width(Length::Fill),
        close_btn,
    ]
    .align_y(Alignment::Center);

    // History list
    let history_items: Vec<Element<'static, Message>> = history_entries
        .iter()
        .map(|entry| history_entry_row(entry, None, theme))
        .collect();

    let history_section = if !history_items.is_empty() {
        column![
            text("History").size(14).color(theme.text_secondary),
            Space::new().height(8),
            scrollable(Column::with_children(history_items).spacing(4).width(Fill))
                .height(Length::Fixed(400.0)),
        ]
    } else {
        column![
            text("No execution history").size(14).color(theme.text_muted),
        ]
    };

    column![
        header,
        Space::new().height(16),
        history_section,
    ]
    .padding(16)
}

/// Single host result row
fn host_result_row(
    data: HostResultData,
    snippet_id: Uuid,
    theme: Theme,
) -> Element<'static, Message> {
    let host_id = data.host_id;

    // Status icon
    let (status_icon, status_color) = match &data.status {
        ExecutionStatus::Pending => (icons::ui::REFRESH, theme.text_muted),
        ExecutionStatus::Running => (icons::ui::REFRESH, theme.accent),
        ExecutionStatus::Success => (icons::ui::CHECK, iced::Color::from_rgb8(0x40, 0xa0, 0x2b)),
        ExecutionStatus::Failed(_) => (icons::ui::X, iced::Color::from_rgb8(0xd2, 0x0f, 0x39)),
    };

    let icon = icon_with_color(status_icon, 14, status_color);

    // Duration text
    let duration_text = if data.duration.as_millis() > 0 {
        format!("{:.1}s", data.duration.as_secs_f64())
    } else {
        String::new()
    };

    // Error message if failed
    let error_text: Element<'static, Message> = match &data.status {
        ExecutionStatus::Failed(err) => text(err.clone())
            .size(11)
            .color(iced::Color::from_rgb8(0xd2, 0x0f, 0x39))
            .into(),
        _ => Space::new().into(),
    };

    // Header row
    let header_row = row![
        icon,
        Space::new().width(8),
        text(data.host_name).size(14).color(theme.text_primary),
        Space::new().width(Length::Fill),
        text(duration_text).size(12).color(theme.text_muted),
        Space::new().width(8),
        if data.expanded {
            icon_with_color(icons::ui::CHEVRON_DOWN, 12, theme.text_muted)
        } else {
            icon_with_color(icons::ui::CHEVRON_RIGHT, 12, theme.text_muted)
        },
    ]
    .align_y(Alignment::Center);

    // Output section (if expanded)
    let output_section: Element<'static, Message> = if data.expanded && !data.stdout.is_empty() {
        // Sanitize output: replace tabs with spaces, remove control chars
        let sanitized_output = sanitize_output(&data.stdout);

        let output = container(
            scrollable(
                text(sanitized_output)
                    .size(12)
                    .font(MONOSPACE_FONT)
                    .color(theme.text_secondary),
            )
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(Length::Fixed(150.0))
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

        column![Space::new().height(8), output, error_text,].into()
    } else if data.expanded {
        column![error_text,].into()
    } else {
        Space::new().height(0).into()
    };

    // Clickable row container
    let row_content = column![header_row, output_section,];

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
}

/// History entry row showing a past execution (clickable)
fn history_entry_row(
    entry: &SnippetExecutionEntry,
    selected_id: Option<Uuid>,
    theme: Theme,
) -> Element<'static, Message> {
    let entry_id = entry.id;
    let time_ago = entry.time_ago();
    let success = entry.success_count;
    let failed = entry.failure_count;
    let is_selected = selected_id == Some(entry_id);

    // Status icon
    let (icon, color) = if failed == 0 {
        (icons::ui::CHECK, iced::Color::from_rgb8(0x40, 0xa0, 0x2b))
    } else if success == 0 {
        (icons::ui::X, iced::Color::from_rgb8(0xd2, 0x0f, 0x39))
    } else {
        (icons::ui::X, iced::Color::from_rgb8(0xd2, 0x8f, 0x39)) // Orange for partial
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
        text(status).size(12).color(color),
        Space::new().width(Length::Fill),
        text(time_ago).size(11).color(theme.text_muted),
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

/// Historical host result row (read-only, no expand/collapse)
fn historical_host_result_row(
    result: &crate::config::HistoricalHostResult,
    theme: Theme,
) -> Element<'static, Message> {
    // Status icon
    let (status_icon, status_color) = if result.success {
        (icons::ui::CHECK, iced::Color::from_rgb8(0x40, 0xa0, 0x2b))
    } else {
        (icons::ui::X, iced::Color::from_rgb8(0xd2, 0x0f, 0x39))
    };

    let icon = icon_with_color(status_icon, 14, status_color);

    // Duration text
    let duration_text = format!("{:.1}s", result.duration_ms as f64 / 1000.0);

    // Error message if failed
    let error_section: Element<'static, Message> = if let Some(err) = &result.error {
        column![
            Space::new().height(4),
            text(err.clone())
                .size(11)
                .color(iced::Color::from_rgb8(0xd2, 0x0f, 0x39)),
        ]
        .into()
    } else {
        Space::new().height(0).into()
    };

    // Output section (always show if available)
    let output_section: Element<'static, Message> = if !result.stdout.is_empty() {
        let sanitized_output = sanitize_output(&result.stdout);
        column![
            Space::new().height(8),
            container(
                scrollable(
                    text(sanitized_output)
                        .size(12)
                        .font(MONOSPACE_FONT)
                        .color(theme.text_secondary),
                )
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .height(Length::Fixed(100.0))
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
            }),
        ]
        .into()
    } else {
        Space::new().height(0).into()
    };

    // Header row
    let header_row = row![
        icon,
        Space::new().width(8),
        text(result.host_name.clone()).size(14).color(theme.text_primary),
        Space::new().width(Length::Fill),
        text(duration_text).size(12).color(theme.text_muted),
    ]
    .align_y(Alignment::Center);

    let row_content = column![header_row, error_section, output_section,];

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
