//! File viewer message handler

use iced::Task;

use crate::app::Portal;
use crate::message::{FileViewerMessage, Message};
use crate::views::toast::{Toast, ToastType};

/// Handle file viewer messages
pub fn handle_file_viewer(app: &mut Portal, msg: FileViewerMessage) -> Task<Message> {
    match msg {
        FileViewerMessage::ContentLoaded { viewer_id, content } => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.content = content;
            }
            Task::none()
        }
        FileViewerMessage::LoadError(viewer_id, error) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.set_error(error);
            }
            Task::none()
        }
        FileViewerMessage::TextChanged(viewer_id, action) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                match &mut viewer.content {
                    crate::views::file_viewer::ViewerContent::Text { content } => {
                        content.perform(action);
                        viewer.mark_modified();
                    }
                    crate::views::file_viewer::ViewerContent::Markdown { content, raw_text, .. } => {
                        content.perform(action);
                        *raw_text = content.text();
                        viewer.mark_modified();
                    }
                    _ => {}
                }
            }
            Task::none()
        }
        FileViewerMessage::Save(viewer_id) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                if viewer.is_saving {
                    return Task::none();
                }
                viewer.is_saving = true;

                // Get text content to save
                if let Some(text) = viewer.get_text() {
                    let source = viewer.file_source.clone();

                    return Task::perform(
                        async move {
                            save_file_content(source, text).await
                        },
                        move |result| {
                            Message::FileViewer(FileViewerMessage::SaveResult(viewer_id, result))
                        },
                    );
                }
            }
            Task::none()
        }
        FileViewerMessage::SaveResult(viewer_id, result) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.is_saving = false;
                match result {
                    Ok(()) => {
                        viewer.mark_saved();
                        // Update tab title (remove modified indicator)
                        if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == viewer_id) {
                            tab.title = viewer.file_name.clone();
                        }
                        app.toast_manager.push(Toast::new("File saved", ToastType::Success));
                    }
                    Err(e) => {
                        app.toast_manager.push(Toast::new(format!("Failed to save: {}", e), ToastType::Error));
                    }
                }
            }
            Task::none()
        }
        FileViewerMessage::PdfPageChange(viewer_id, page) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.set_pdf_page(page);
            }
            Task::none()
        }
        FileViewerMessage::MarkdownTogglePreview(viewer_id) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.toggle_preview();
            }
            Task::none()
        }
        FileViewerMessage::ImageZoom(viewer_id, zoom) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.set_zoom(zoom);
            }
            Task::none()
        }
    }
}

/// Save file content to local or remote location
async fn save_file_content(
    source: crate::views::file_viewer::FileSource,
    text: String,
) -> Result<(), String> {
    use crate::views::file_viewer::FileSource;

    match source {
        FileSource::Local { path } => {
            tokio::fs::write(&path, text)
                .await
                .map_err(|e| format!("Failed to write file: {}", e))?;
            Ok(())
        }
        FileSource::Remote { temp_path } => {
            // Save to temp path - actual SFTP upload would be handled separately
            tokio::fs::write(&temp_path, text)
                .await
                .map_err(|e| format!("Failed to write temp file: {}", e))?;

            // TODO: Upload temp file back to remote via SFTP
            // For now, just save locally
            Ok(())
        }
    }
}
