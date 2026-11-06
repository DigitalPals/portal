//! Confirmation before discarding edits in a file viewer.

use iced::widget::{Space, button, column, row, text};
use iced::{Element, Length};

use super::common::{dialog_backdrop, primary_button_style, secondary_button_style};
use crate::message::{DialogMessage, FileViewerMessage, Message};
use crate::theme::{ScaledFonts, Theme};
use crate::views::file_viewer::FileViewerState;

pub fn unsaved_file_dialog_view(
    viewer: &FileViewerState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let cancel = button(text("Cancel").size(fonts.button_small))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::Close));
    let discard = button(text("Discard").size(fonts.button_small))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press_maybe((!viewer.is_saving).then_some(Message::FileViewer(
            FileViewerMessage::DiscardChanges(viewer.viewer_id),
        )));
    let save_label = if viewer.is_saving {
        "Saving…"
    } else {
        "Save"
    };
    let save = button(text(save_label).size(fonts.button_small))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press_maybe((!viewer.is_saving).then_some(Message::FileViewer(
            FileViewerMessage::Save(viewer.viewer_id),
        )));

    let mut content = column![
        text("Save changes?")
            .size(fonts.dialog_title)
            .color(theme.text_primary),
        text(format!("{} has unsaved changes.", viewer.file_name))
            .size(fonts.body)
            .color(theme.text_secondary),
    ]
    .spacing(12);
    if let Some(error) = &viewer.save_error {
        content = content.push(
            text(format!("Could not save: {error}"))
                .size(fonts.small)
                .color(theme.text_primary),
        );
    }
    content =
        content.push(row![Space::new().width(Length::Fill), cancel, discard, save].spacing(8));
    dialog_backdrop(content.padding(24).width(Length::Fixed(440.0)), theme)
}
