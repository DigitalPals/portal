//! File viewer message handler

use iced::Task;
use std::path::Path;

use crate::app::Portal;
use crate::app::services::file_viewer;
use crate::fs_utils::write_regular_file;
use crate::message::{FileViewerMessage, Message, TabMessage};
use crate::sftp::SharedSftpSession;
use crate::views::file_viewer::FileSource;
use crate::views::toast::{Toast, ToastType};

/// Handle file viewer messages
pub fn handle_file_viewer(app: &mut Portal, msg: FileViewerMessage) -> Task<Message> {
    match msg {
        FileViewerMessage::ContentLoaded { viewer_id, content } => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.content = content;
                if let crate::views::file_viewer::ViewerContent::Pdf {
                    pages,
                    current_page,
                    rendering_pages,
                    ..
                } = &mut viewer.content
                {
                    let page = *current_page;
                    if pages.get(page).and_then(|slot| slot.as_ref()).is_none()
                        && !rendering_pages.get(page).copied().unwrap_or(false)
                    {
                        viewer.set_pdf_rendering(page, true);
                        let source = viewer.file_source.clone();
                        return Task::perform(
                            async move { file_viewer::render_pdf_page(source, page).await },
                            move |result| {
                                Message::FileViewer(FileViewerMessage::PdfPageRendered(
                                    viewer_id, page, result,
                                ))
                            },
                        );
                    }
                }
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
                    crate::views::file_viewer::ViewerContent::Markdown {
                        content,
                        raw_text,
                        ..
                    } => {
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

                // Get text content to save
                if let Some(text) = viewer.get_text() {
                    viewer.is_saving = true;
                    let source = viewer.file_source.clone();

                    // Get SFTP session if this is a remote file
                    let sftp_session = if let FileSource::Remote { session_id, .. } = &source {
                        app.sftp.get_connection(*session_id).cloned()
                    } else {
                        None
                    };

                    return Task::perform(
                        async move { save_file_content(source, text, sftp_session).await },
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
                        app.toast_manager
                            .push(Toast::new("File saved", ToastType::Success));
                        // Close the tab after successful save
                        return Task::done(Message::Tab(TabMessage::Close(viewer_id)));
                    }
                    Err(e) => {
                        app.toast_manager.push(Toast::new(
                            format!("Failed to save: {}", e),
                            ToastType::Error,
                        ));
                    }
                }
            }
            Task::none()
        }
        FileViewerMessage::PdfPageChange(viewer_id, page) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.set_pdf_page(page);
                if let crate::views::file_viewer::ViewerContent::Pdf {
                    pages,
                    rendering_pages,
                    ..
                } = &mut viewer.content
                    && pages.get(page).and_then(|slot| slot.as_ref()).is_none()
                    && !rendering_pages.get(page).copied().unwrap_or(false)
                {
                    viewer.set_pdf_rendering(page, true);
                    let source = viewer.file_source.clone();
                    return Task::perform(
                        async move { file_viewer::render_pdf_page(source, page).await },
                        move |result| {
                            Message::FileViewer(FileViewerMessage::PdfPageRendered(
                                viewer_id, page, result,
                            ))
                        },
                    );
                }
            }
            Task::none()
        }
        FileViewerMessage::PdfRenderPage(viewer_id, page) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id)
                && let crate::views::file_viewer::ViewerContent::Pdf {
                    pages,
                    rendering_pages,
                    ..
                } = &mut viewer.content
                && pages.get(page).and_then(|slot| slot.as_ref()).is_none()
                && !rendering_pages.get(page).copied().unwrap_or(false)
            {
                viewer.set_pdf_rendering(page, true);
                let source = viewer.file_source.clone();
                return Task::perform(
                    async move { file_viewer::render_pdf_page(source, page).await },
                    move |result| {
                        Message::FileViewer(FileViewerMessage::PdfPageRendered(
                            viewer_id, page, result,
                        ))
                    },
                );
            }
            Task::none()
        }
        FileViewerMessage::PdfPageRendered(viewer_id, page, result) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.set_pdf_rendering(page, false);
                match result {
                    Ok(data) => {
                        viewer.set_pdf_page_data(page, data);
                    }
                    Err(e) => {
                        app.toast_manager.push(Toast::new(
                            format!("PDF render failed: {}", e),
                            ToastType::Error,
                        ));
                    }
                }
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
    source: FileSource,
    text: String,
    sftp_session: Option<SharedSftpSession>,
) -> Result<(), String> {
    match source {
        FileSource::Local { path } => {
            write_text_file(&path, text, "file").await?;
            Ok(())
        }
        FileSource::Remote {
            temp_path,
            remote_path,
            ..
        } => {
            // Save to temp path first
            write_text_file(&temp_path, text, "temp file").await?;

            // Upload to remote via SFTP
            let sftp = sftp_session.ok_or_else(|| "SFTP connection not available".to_string())?;
            sftp.upload(&temp_path, &remote_path)
                .await
                .map_err(|e| format!("Failed to upload file: {}", e))?;

            Ok(())
        }
    }
}

async fn write_text_file(path: &Path, text: String, label: &str) -> Result<(), String> {
    let path = path.to_path_buf();
    let label = label.to_string();
    let task_label = label.clone();
    tokio::task::spawn_blocking(move || write_regular_file(&path, text.as_bytes(), &task_label))
        .await
        .map_err(|e| format!("{} save task failed: {}", label, e))?
}

#[cfg(test)]
mod tests {
    use super::write_text_file;

    #[tokio::test]
    async fn write_text_file_updates_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("note.txt");
        std::fs::write(&path, "old").unwrap();

        write_text_file(&path, "new".to_string(), "file")
            .await
            .expect("regular file should be writable");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[tokio::test]
    async fn write_text_file_rejects_directory_target() {
        let temp = tempfile::tempdir().unwrap();

        let error = write_text_file(temp.path(), "new".to_string(), "file")
            .await
            .expect_err("directory target should be rejected");

        assert!(error.contains("directory"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_text_file_rejects_symlink_target_without_writing_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = write_text_file(&link, "changed".to_string(), "file")
            .await
            .expect_err("symlink target should be rejected");

        assert!(error.contains("symbolic link"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_text_file_rejects_non_regular_target() {
        let temp = tempfile::tempdir().unwrap();
        let socket_path = temp.path().join("viewer.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

        let error = write_text_file(&socket_path, "changed".to_string(), "file")
            .await
            .expect_err("non-regular target should be rejected");

        assert!(error.contains("non-regular"));
    }
}
