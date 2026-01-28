//! SFTP pane rendering
//!
//! This module contains the rendering functions for individual panes
//! in the dual-pane SFTP browser.

use std::path::PathBuf;

use iced::widget::{
    Column, Row, Space, button, column, container, pick_list, row, scrollable, text, text_input,
    tooltip,
};
use iced::{Alignment, Color, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{Message, SessionId, SftpMessage};
use crate::sftp::{FileEntry, FileIcon, format_size};
use crate::theme::{
    FONT_SIZE_BODY, FONT_SIZE_BUTTON_SMALL, FONT_SIZE_LABEL, FONT_SIZE_SECTION, Theme,
};
use crate::widgets::{column_resize_handle, mouse_area};

use super::state::FilePaneState;
use super::types::{ColumnWidths, PaneId, PaneSource, SftpColumn};

/// Get SVG icon data for a file icon type
fn file_icon_data(icon_type: FileIcon) -> &'static [u8] {
    match icon_type {
        FileIcon::ParentDir => icons::ui::CHEVRON_LEFT,
        FileIcon::Folder => icons::files::FOLDER,
        FileIcon::Symlink => icons::files::FILE,
        FileIcon::Code => icons::files::FILE_CODE,
        FileIcon::Text => icons::files::FILE_TEXT,
        FileIcon::Image => icons::files::IMAGE,
        FileIcon::Audio => icons::files::MUSIC,
        FileIcon::Video => icons::files::VIDEO,
        FileIcon::Archive => icons::files::ARCHIVE,
        FileIcon::Config => icons::files::FILE_JSON,
        FileIcon::Executable => icons::files::FILE_COG,
        FileIcon::File => icons::files::FILE,
    }
}

/// Build a single pane view for the dual-pane browser
#[allow(clippy::too_many_arguments)]
pub fn single_pane_view<'a>(
    state: &'a FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    available_hosts: Vec<(Uuid, String)>,
    is_active: bool,
    context_menu_open: bool,
    column_widths: &'a ColumnWidths,
    theme: Theme,
) -> Element<'a, Message> {
    let header = pane_header(state, pane_id, tab_id, available_hosts, is_active, theme);
    let breadcrumbs = pane_breadcrumb_bar(state, pane_id, tab_id, theme);
    let file_list = pane_file_list(
        state,
        pane_id,
        tab_id,
        context_menu_open,
        column_widths,
        theme,
    );
    let footer = pane_footer(state, pane_id, tab_id, theme);

    let content = column![header, breadcrumbs, file_list, footer].spacing(0);

    let main = container(content)
        .width(Length::FillPortion(1))
        .height(Fill);

    // Overlay actions menu if open
    if state.actions_menu_open {
        iced::widget::stack![main, actions_menu_overlay(state, pane_id, tab_id, theme)].into()
    } else {
        main.into()
    }
}

/// Header with source dropdown, filter input, and actions menu
pub fn pane_header(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    available_hosts: Vec<(Uuid, String)>,
    _is_active: bool,
    theme: Theme,
) -> Element<'_, Message> {
    // Build source options: Local + all configured hosts
    let mut source_options: Vec<String> = vec!["Local".to_string()];
    for (_host_id, host_name) in &available_hosts {
        source_options.push(host_name.clone());
    }

    let current_source = state.source.display_name().to_string();

    // Clone hosts for the closure
    let hosts_for_closure = available_hosts.clone();

    // Source dropdown
    let source_picker = pick_list(
        source_options.clone(),
        Some(current_source.clone()),
        move |selected| {
            if selected == "Local" {
                Message::Sftp(SftpMessage::PaneSourceChanged(
                    tab_id,
                    pane_id,
                    PaneSource::Local,
                ))
            } else {
                // Find the host ID by name and trigger connection
                if let Some((host_id, _)) =
                    hosts_for_closure.iter().find(|(_, name)| name == &selected)
                {
                    Message::Sftp(SftpMessage::ConnectHost(tab_id, pane_id, *host_id))
                } else {
                    Message::Noop
                }
            }
        },
    )
    .width(Length::Fixed(150.0))
    .text_size(FONT_SIZE_BODY)
    .padding([4, 8]);

    // Filter input
    let filter_value = state.filter_text.clone();
    let filter_input = text_input("Filter...", &filter_value)
        .on_input(move |value| Message::Sftp(SftpMessage::FilterChanged(tab_id, pane_id, value)))
        .padding([4, 8])
        .size(FONT_SIZE_BODY)
        .width(Length::Fixed(120.0))
        .style(move |_theme, _status| text_input::Style {
            background: theme.background.into(),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            icon: theme.text_muted,
            placeholder: theme.text_muted,
            value: theme.text_primary,
            selection: theme.accent,
        });

    // Actions dropdown button
    let actions_btn = button(
        row![
            text("Actions")
                .size(FONT_SIZE_BODY)
                .color(theme.text_primary),
            icon_with_color(icons::ui::CHEVRON_DOWN, 14, theme.text_primary)
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => Some(theme.surface.into()),
        };
        iced::widget::button::Style {
            background: bg,
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([4, 8])
    .on_press(Message::Sftp(SftpMessage::ToggleActionsMenu(
        tab_id, pane_id,
    )));

    container(
        row![
            source_picker,
            Space::new().width(Fill),
            filter_input,
            actions_btn
        ]
        .spacing(8)
        .padding(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    })
    .into()
}

/// Breadcrumb navigation bar with back/forward buttons and clickable path segments
pub fn pane_breadcrumb_bar(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    theme: Theme,
) -> Element<'_, Message> {
    let path = &state.current_path;

    // Back button (navigate to parent)
    let back_btn = button(icon_with_color(
        icons::ui::CHEVRON_LEFT,
        14,
        theme.text_primary,
    ))
    .style(move |_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => None,
        };
        iced::widget::button::Style {
            background: bg,
            text_color: theme.text_primary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding([2, 6])
    .on_press(Message::Sftp(SftpMessage::PaneNavigateUp(tab_id, pane_id)));

    // Forward button (placeholder - disabled for now)
    let forward_btn = button(icon_with_color(
        icons::ui::CHEVRON_RIGHT,
        14,
        theme.text_muted,
    ))
    .style(move |_theme, _status| iced::widget::button::Style {
        background: None,
        text_color: theme.text_muted,
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .padding([2, 6]);

    // Build breadcrumb segments
    let components: Vec<_> = path.components().collect();
    let mut breadcrumb_elements: Vec<Element<'_, Message>> = vec![];

    for (i, component) in components.iter().enumerate() {
        let component_name = component.as_os_str().to_string_lossy().to_string();
        let display_name = if component_name.is_empty()
            || component_name == "/"
            || component_name == std::path::MAIN_SEPARATOR_STR
        {
            "/".to_string()
        } else {
            component_name
        };

        // Build path up to this component
        let segment_path: PathBuf = components[..=i].iter().collect();

        // Add separator if not first
        if i > 0 {
            breadcrumb_elements.push(
                text(">")
                    .size(FONT_SIZE_BUTTON_SMALL)
                    .color(theme.text_muted)
                    .into(),
            );
        }

        // Folder icon + clickable segment
        let segment = button(
            row![
                icon_with_color(icons::files::FOLDER, 14, theme.text_secondary),
                text(display_name)
                    .size(FONT_SIZE_BUTTON_SMALL)
                    .color(theme.text_primary)
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .style(move |_theme, status| {
            let bg = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => None,
            };
            iced::widget::button::Style {
                background: bg,
                text_color: theme.text_primary,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([2, 6])
        .on_press(Message::Sftp(SftpMessage::PaneBreadcrumbNavigate(
            tab_id,
            pane_id,
            segment_path,
        )));

        breadcrumb_elements.push(segment.into());
    }

    let breadcrumb_row = Row::with_children(breadcrumb_elements)
        .spacing(4)
        .align_y(Alignment::Center);

    // Wrap breadcrumbs in scrollable for long paths
    let scrollable_breadcrumbs = scrollable(breadcrumb_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Fill);

    container(
        row![back_btn, forward_btn, scrollable_breadcrumbs]
            .spacing(4)
            .padding([4, 8])
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(move |_| container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    })
    .into()
}

/// File list for a single pane
pub fn pane_file_list<'a>(
    state: &'a FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    context_menu_open: bool,
    column_widths: &ColumnWidths,
    theme: Theme,
) -> Element<'a, Message> {
    if state.loading {
        return container(
            text("Loading...")
                .size(FONT_SIZE_BODY)
                .color(theme.text_muted),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    if let Some(ref error) = state.error {
        return container(
            column![
                text("Error")
                    .size(FONT_SIZE_SECTION)
                    .color(theme.text_primary),
                text(error).size(FONT_SIZE_LABEL).color(theme.text_muted),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    // Get filtered entries
    let visible = state.visible_entries();

    if visible.is_empty() {
        let message = if state.entries.is_empty() {
            "Empty directory"
        } else {
            "No matching files"
        };
        return container(
            column![
                icon_with_color(icons::files::FOLDER, 32, theme.text_muted),
                text(message).size(FONT_SIZE_BODY).color(theme.text_muted),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into();
    }

    // Column headers with resize handles at right edge
    // Name column with resize handle at right edge
    let name_header: Element<'_, Message> = container(
        row![
            text("Name")
                .size(FONT_SIZE_BODY)
                .color(theme.text_muted)
                .wrapping(text::Wrapping::None),
            Space::new().width(Fill), // Push resize handle to right edge
            column_resize_handle()
                .on_drag_start(move |x| Message::Sftp(SftpMessage::ColumnResizeStart(
                    tab_id,
                    pane_id,
                    SftpColumn::Name,
                    x
                )))
                .on_drag(move |x| Message::Sftp(SftpMessage::ColumnResizing(tab_id, x)))
                .on_drag_end(Message::Sftp(SftpMessage::ColumnResizeEnd(tab_id))),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(column_widths.name))
    .clip(true)
    .into();

    // Date Modified column with resize handle at right edge
    let date_header: Element<'_, Message> = container(
        row![
            text("Date Modified")
                .size(FONT_SIZE_BODY)
                .color(theme.text_muted)
                .wrapping(text::Wrapping::None),
            Space::new().width(Fill),
            column_resize_handle()
                .on_drag_start(move |x| Message::Sftp(SftpMessage::ColumnResizeStart(
                    tab_id,
                    pane_id,
                    SftpColumn::DateModified,
                    x
                )))
                .on_drag(move |x| Message::Sftp(SftpMessage::ColumnResizing(tab_id, x)))
                .on_drag_end(Message::Sftp(SftpMessage::ColumnResizeEnd(tab_id))),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(column_widths.date_modified))
    .clip(true)
    .into();

    // Size column with resize handle at right edge
    let size_header: Element<'_, Message> = container(
        row![
            text("Size")
                .size(FONT_SIZE_BODY)
                .color(theme.text_muted)
                .wrapping(text::Wrapping::None),
            Space::new().width(Fill),
            column_resize_handle()
                .on_drag_start(move |x| Message::Sftp(SftpMessage::ColumnResizeStart(
                    tab_id,
                    pane_id,
                    SftpColumn::Size,
                    x
                )))
                .on_drag(move |x| Message::Sftp(SftpMessage::ColumnResizing(tab_id, x)))
                .on_drag_end(Message::Sftp(SftpMessage::ColumnResizeEnd(tab_id))),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(column_widths.size))
    .clip(true)
    .into();

    // Kind column (last column, no resize handle - fills remaining space)
    let kind_header: Element<'_, Message> = container(
        text("Kind")
            .size(FONT_SIZE_BODY)
            .color(theme.text_muted)
            .wrapping(text::Wrapping::None),
    )
    .width(Fill)
    .clip(true)
    .into();

    let headers = container(
        Row::with_children(vec![name_header, date_header, size_header, kind_header])
            .spacing(8)
            .padding(Padding::new(8.0).left(12.0).right(12.0))
            .align_y(Alignment::Center)
            .width(Fill),
    )
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    });

    // File entries - use visible_entries which returns (original_index, &FileEntry)
    let entries: Vec<Element<'_, Message>> = visible
        .iter()
        .map(|(original_index, entry)| {
            pane_file_entry_row(
                entry,
                *original_index,
                state.is_selected(*original_index),
                tab_id,
                pane_id,
                context_menu_open,
                column_widths,
                theme,
            )
        })
        .collect();

    // Scrollbar styling: show only when hovered/dragged
    let scrollbar_color = theme.text_muted.scale_alpha(0.5);

    // Vertical scrollable for file entries
    let file_list = scrollable(Column::with_children(entries).spacing(0).width(Fill))
        .id(state.scrollable_id.clone())
        .height(Fill)
        .width(Fill)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(6).scroller_width(6),
        ))
        .style(move |_theme, status| {
            let scroller_color = match status {
                scrollable::Status::Active { .. } => Color::TRANSPARENT,
                scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. } => {
                    scrollbar_color
                }
            };
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: scrollable::Rail {
                    background: None,
                    border: iced::Border::default(),
                    scroller: scrollable::Scroller {
                        background: scroller_color.into(),
                        border: iced::Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                    },
                },
                horizontal_rail: scrollable::Rail {
                    background: None,
                    border: iced::Border::default(),
                    scroller: scrollable::Scroller {
                        background: Color::TRANSPARENT.into(),
                        border: iced::Border::default(),
                    },
                },
                gap: None,
                auto_scroll: scrollable::AutoScroll {
                    background: Color::TRANSPARENT.into(),
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    icon: Color::TRANSPARENT,
                },
            }
        });

    column![headers, file_list].spacing(0).into()
}

/// Single file entry row for a pane
#[allow(clippy::too_many_arguments)]
pub fn pane_file_entry_row(
    entry: &FileEntry,
    index: usize,
    is_selected: bool,
    tab_id: SessionId,
    pane_id: PaneId,
    context_menu_open: bool,
    column_widths: &ColumnWidths,
    theme: Theme,
) -> Element<'static, Message> {
    let icon_type = entry.icon_type();
    let icon_data = file_icon_data(icon_type);
    let name = entry.name.clone();
    let size = if entry.is_dir {
        "—".to_string()
    } else {
        format_size(entry.size)
    };

    let bg_color = if is_selected {
        theme.accent
    } else {
        theme.background
    };

    let text_color = if is_selected {
        theme.background
    } else {
        theme.text_primary
    };

    let icon_color = if is_selected {
        theme.background
    } else {
        theme.text_secondary
    };

    let path = entry.path.clone();
    let is_dir = entry.is_dir;

    let modified = entry.formatted_modified();
    let kind = entry.kind_description();

    // Clone name for tooltip
    let tooltip_name = name.clone();

    let name_row = row![
        icon_with_color(icon_data, 16, icon_color),
        text(name)
            .size(FONT_SIZE_BUTTON_SMALL)
            .color(text_color)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Wrap name in tooltip showing full name on hover (1 second delay)
    let name_with_tooltip = tooltip(
        container(name_row).width(Fill).clip(true),
        text(tooltip_name).size(FONT_SIZE_LABEL),
        tooltip::Position::Top,
    )
    .delay(std::time::Duration::from_secs(1))
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .padding(6);

    let secondary_color = if is_selected {
        text_color
    } else {
        theme.text_secondary
    };

    let content = row![
        container(name_with_tooltip).width(Length::Fixed(column_widths.name)),
        container(
            text(modified)
                .size(FONT_SIZE_BODY)
                .color(secondary_color)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fixed(column_widths.date_modified))
        .clip(true),
        container(
            text(size)
                .size(FONT_SIZE_BODY)
                .color(secondary_color)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fixed(column_widths.size))
        .clip(true),
        container(
            text(kind)
                .size(FONT_SIZE_BODY)
                .color(secondary_color)
                .wrapping(text::Wrapping::None),
        )
        .width(Fill)
        .clip(true),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Fill);

    let btn = button(
        container(content)
            .padding(Padding::new(6.0).left(12.0).right(12.0))
            .width(Fill),
    )
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered if !is_selected && !context_menu_open => {
                theme.hover
            }
            _ => bg_color,
        };
        iced::widget::button::Style {
            background: Some(background.into()),
            text_color,
            border: iced::Border::default(),
            ..Default::default()
        }
    })
    .padding(0)
    .width(Fill);

    let btn = if context_menu_open {
        btn
    } else if is_dir {
        btn.on_press(Message::Sftp(SftpMessage::PaneNavigate(
            tab_id, pane_id, path,
        )))
    } else {
        btn.on_press(Message::Sftp(SftpMessage::PaneSelect(
            tab_id, pane_id, index,
        )))
    };

    // Wrap in mouse_area to handle right-click
    let area = mouse_area(btn);
    if context_menu_open {
        area.into()
    } else {
        area.on_right_press(move |x, y| {
            Message::Sftp(SftpMessage::ShowContextMenu(
                tab_id,
                pane_id,
                x,
                y,
                Some(index),
            ))
        })
        .into()
    }
}

/// Footer with status for a pane
pub fn pane_footer<'a>(
    state: &'a FilePaneState,
    _pane_id: PaneId,
    _tab_id: SessionId,
    theme: Theme,
) -> Element<'a, Message> {
    let total_count = state.entries.len();
    let visible_count = state.visible_entries().len();
    let selected_count = state.selected_indices.len();

    let status = if state.loading {
        "Loading...".to_string()
    } else if selected_count > 0 {
        format!("{} of {} items selected", selected_count, visible_count)
    } else if visible_count != total_count {
        format!("{} of {} items (filtered)", visible_count, total_count)
    } else {
        format!("{} items", total_count)
    };

    container(
        row![text(status).size(FONT_SIZE_LABEL).color(theme.text_muted),]
            .spacing(8)
            .padding(8)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        ..Default::default()
    })
    .into()
}

/// Actions dropdown menu overlay
pub fn actions_menu_overlay(
    state: &FilePaneState,
    pane_id: PaneId,
    tab_id: SessionId,
    theme: Theme,
) -> Element<'_, Message> {
    if !state.actions_menu_open {
        return Space::new().into();
    }

    let show_hidden_label = if state.show_hidden {
        "Hide Hidden Files"
    } else {
        "Show Hidden Files"
    };

    let menu_item = button(
        text(show_hidden_label)
            .size(FONT_SIZE_BUTTON_SMALL)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .width(Length::Fixed(180.0))
    .style(move |_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => None,
        };
        iced::widget::button::Style {
            background: bg,
            text_color: theme.text_primary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .on_press(Message::Sftp(SftpMessage::ToggleShowHidden(
        tab_id, pane_id,
    )));

    let menu = container(column![menu_item])
        .padding(8)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.15),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });

    // Position menu at top-right of pane (below the Actions button)
    // Note: dismiss background is rendered at app level for window-wide click handling
    container(menu)
        .width(Fill)
        .padding(Padding::new(0.0).top(40.0).right(8.0))
        .align_x(Alignment::End)
        .into()
}
