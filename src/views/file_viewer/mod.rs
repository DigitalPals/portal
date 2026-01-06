//! File viewer module for in-app file viewing and editing
//!
//! Supports text files (with syntax highlighting), images, PDFs, and markdown.

mod state;
mod types;

pub use state::{FileViewerState, ViewerContent};
pub use types::{FileSource, FileType};

use iced::widget::{
    Space, Image, Svg, button, column, container, row, scrollable, text, text_editor,
};
use iced::{Alignment, Color, Element, Fill, Length};

use crate::message::{FileViewerMessage, Message, SessionId};
use crate::theme::Theme;

// Error color constant
const ERROR_COLOR: Color = Color::from_rgb(0.9, 0.3, 0.3);

/// Main file viewer view
pub fn file_viewer_view(state: &FileViewerState, theme: Theme) -> Element<'_, Message> {
    let toolbar = file_viewer_toolbar(state, theme);

    let content: Element<'_, Message> = match &state.content {
        ViewerContent::Loading => container(text("Loading...").size(16).color(theme.text_muted))
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into(),
        ViewerContent::Error(err) => container(
            column![
                text("Error loading file").size(18).color(ERROR_COLOR),
                Space::new().height(8),
                text(err).size(14).color(theme.text_secondary),
            ]
            .align_x(Alignment::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into(),
        ViewerContent::Text { content } => text_editor_view(state.viewer_id, content, theme),
        ViewerContent::Markdown {
            content,
            raw_text,
            preview_mode,
        } => {
            if *preview_mode {
                markdown_preview_view(raw_text, theme)
            } else {
                text_editor_view(state.viewer_id, content, theme)
            }
        }
        ViewerContent::Image {
            data,
            zoom,
            width,
            height,
            is_svg,
        } => image_viewer_view(
            data,
            *zoom,
            *width,
            *height,
            *is_svg,
            state.viewer_id,
            theme,
        ),
        ViewerContent::Pdf {
            pages,
            rendering_pages,
            current_page,
            total_pages,
        } => pdf_viewer_view(
            pages,
            rendering_pages,
            *current_page,
            *total_pages,
            state.viewer_id,
            theme,
        ),
    };

    let main_content = column![toolbar, content].spacing(0);

    container(main_content)
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}

/// File viewer toolbar with file name, save button, and controls
fn file_viewer_toolbar(state: &FileViewerState, theme: Theme) -> Element<'_, Message> {
    let viewer_id = state.viewer_id;

    // File name with modified indicator
    let file_name_text = if state.is_modified {
        format!("{} \u{25CF}", state.file_name) // bullet for modified
    } else {
        state.file_name.clone()
    };

    let file_name = text(file_name_text).size(14).color(theme.text_primary);

    // Save button (only for editable content)
    let save_btn = if state.file_type.is_editable() && state.is_modified {
        button(text("Save").size(13).color(iced::Color::WHITE))
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
                    _ => theme.accent,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .padding([6, 16])
            .on_press(Message::FileViewer(FileViewerMessage::Save(viewer_id)))
    } else {
        button(text("Save").size(13).color(theme.text_muted))
            .style(move |_theme, _status| button::Style {
                background: Some(theme.surface.into()),
                text_color: theme.text_muted,
                border: iced::Border {
                    radius: 4.0.into(),
                    color: theme.border,
                    width: 1.0,
                },
                ..Default::default()
            })
            .padding([6, 16])
    };

    // Preview toggle for markdown
    let preview_toggle: Element<'_, Message> = if matches!(state.file_type, FileType::Markdown) {
        let is_preview = matches!(
            &state.content,
            ViewerContent::Markdown {
                preview_mode: true,
                ..
            }
        );
        let label = if is_preview { "Edit" } else { "Preview" };

        button(text(label).size(13).color(theme.text_primary))
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme.hover,
                    _ => theme.surface,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: theme.text_primary,
                    border: iced::Border {
                        radius: 4.0.into(),
                        color: theme.border,
                        width: 1.0,
                    },
                    ..Default::default()
                }
            })
            .padding([6, 12])
            .on_press(Message::FileViewer(
                FileViewerMessage::MarkdownTogglePreview(viewer_id),
            ))
            .into()
    } else {
        Space::new().width(0).into()
    };

    let toolbar_content = row![
        file_name,
        Space::new().width(Length::Fill),
        preview_toggle,
        Space::new().width(8),
        save_btn,
    ]
    .align_y(Alignment::Center)
    .padding([8, 16]);

    container(toolbar_content)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Text editor view using iced's text_editor widget
fn text_editor_view(
    viewer_id: SessionId,
    content: &text_editor::Content,
    theme: Theme,
) -> Element<'_, Message> {
    let editor = text_editor(content)
        .on_action(move |action| {
            Message::FileViewer(FileViewerMessage::TextChanged(viewer_id, action))
        })
        .height(Fill)
        .padding(16)
        .style(move |_theme, _status| text_editor::Style {
            background: theme.background.into(),
            border: iced::Border::default(),
            placeholder: theme.text_muted,
            value: theme.text_primary,
            selection: theme.selected,
        });

    container(editor).width(Fill).height(Fill).into()
}

/// Markdown preview view (simple text display for now)
fn markdown_preview_view(raw_text: &str, theme: Theme) -> Element<'_, Message> {
    // Simple text preview for markdown - could be enhanced with proper markdown rendering
    let preview_text = text(raw_text.to_string())
        .size(14)
        .color(theme.text_primary);

    let content = scrollable(container(preview_text).padding(16))
        .width(Fill)
        .height(Fill);

    container(content).width(Fill).height(Fill).into()
}

/// Image viewer with zoom controls
fn image_viewer_view(
    data: &[u8],
    zoom: f32,
    width: u32,
    height: u32,
    is_svg: bool,
    viewer_id: SessionId,
    theme: Theme,
) -> Element<'_, Message> {
    let zoom_controls = row![
        button(text("-").size(16))
            .padding([4, 12])
            .on_press(Message::FileViewer(FileViewerMessage::ImageZoom(
                viewer_id,
                zoom - 0.1
            ))),
        Space::new().width(8),
        button(text("+").size(16))
            .padding([4, 12])
            .on_press(Message::FileViewer(FileViewerMessage::ImageZoom(
                viewer_id,
                zoom + 0.1
            ))),
        Space::new().width(12),
        text(format!("Zoom: {:.0}%", zoom * 100.0))
            .size(12)
            .color(theme.text_secondary),
    ]
    .align_y(Alignment::Center);

    let image_element: Element<'_, Message> = if is_svg {
        let base = 600.0;
        let size = Length::Fixed((base * zoom).max(80.0));
        Svg::new(iced::widget::svg::Handle::from_memory(data.to_vec()))
            .width(size)
            .height(size)
            .into()
    } else {
        let scaled_width = (width as f32 * zoom).max(1.0);
        let scaled_height = (height as f32 * zoom).max(1.0);
        Image::new(iced::widget::image::Handle::from_bytes(data.to_vec()))
            .width(Length::Fixed(scaled_width))
            .height(Length::Fixed(scaled_height))
            .scale(zoom)
            .into()
    };

    let content = scrollable(
        container(image_element)
            .padding(16)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .height(Fill);

    let layout = column![
        row![
            text("Image Viewer").size(16).color(theme.text_primary),
            Space::new().width(Length::Fill),
            zoom_controls,
        ]
        .align_y(Alignment::Center)
        .padding([8, 16]),
        content,
    ];

    container(layout).width(Fill).height(Fill).into()
}

/// PDF viewer with page navigation
fn pdf_viewer_view<'a>(
    pages: &'a [Option<Vec<u8>>],
    rendering_pages: &'a [bool],
    current_page: usize,
    total_pages: usize,
    viewer_id: SessionId,
    theme: Theme,
) -> Element<'a, Message> {
    let page_controls = row![
        button(text("<").size(16)).padding([4, 12]).on_press_maybe(
            (current_page > 0).then_some(Message::FileViewer(
                FileViewerMessage::PdfPageChange(viewer_id, current_page - 1)
            ))
        ),
        Space::new().width(16),
        button(text(">").size(16)).padding([4, 12]).on_press_maybe(
            (current_page + 1 < total_pages).then_some(Message::FileViewer(
                FileViewerMessage::PdfPageChange(viewer_id, current_page + 1)
            ))
        ),
        Space::new().width(12),
        text(format!("Page {} of {}", current_page + 1, total_pages))
            .size(12)
            .color(theme.text_secondary),
    ]
    .align_y(Alignment::Center);

    let page_data = pages
        .get(current_page)
        .and_then(|slot| slot.as_ref());
    let is_rendering = rendering_pages
        .get(current_page)
        .copied()
        .unwrap_or(false);

    let body: Element<'_, Message> = if let Some(data) = page_data {
        let page_image = Image::new(iced::widget::image::Handle::from_bytes(data.clone()))
            .width(Fill)
            .height(Length::Shrink)
            .expand(true);
        scrollable(container(page_image).padding(16))
            .width(Fill)
            .height(Fill)
            .into()
    } else if is_rendering {
        container(text("Rendering page...").size(14).color(theme.text_secondary))
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    } else {
        let retry_button = button(text("Render page").size(12))
            .padding([6, 12])
            .on_press(Message::FileViewer(FileViewerMessage::PdfRenderPage(
                viewer_id,
                current_page,
            )));
        container(
            column![
                text("Page not rendered").size(14).color(theme.text_muted),
                Space::new().height(8),
                retry_button,
            ]
            .align_x(Alignment::Center),
        )
            .width(Fill)
            .height(Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    };

    let layout = column![
        row![
            text("PDF Viewer").size(16).color(theme.text_primary),
            Space::new().width(Length::Fill),
            page_controls,
        ]
        .align_y(Alignment::Center)
        .padding([8, 16]),
        body,
    ];

    container(layout).width(Fill).height(Fill).into()
}
