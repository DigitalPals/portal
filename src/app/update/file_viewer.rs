//! File viewer message handler

use iced::Task;
use iced::widget::text_editor;
use std::path::Path;

use crate::app::Portal;
use crate::app::services::file_viewer;
use crate::fs_utils::write_regular_file;
use crate::message::{FileViewerMessage, Message};
use crate::sftp::SharedSftpSession;
use crate::views::file_viewer::FileSource;
use crate::views::toast::{Toast, ToastType};

/// Handle file viewer messages
pub fn handle_file_viewer(app: &mut Portal, msg: FileViewerMessage) -> Task<Message> {
    match msg {
        FileViewerMessage::ContentLoaded { viewer_id, content } => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                viewer.content = content;
                if let Some(goto_line) = viewer.pending_goto_line.take() {
                    scroll_viewer_to_line(viewer, goto_line);
                }
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
                let is_edit = action.is_edit();
                match &mut viewer.content {
                    crate::views::file_viewer::ViewerContent::Text { content } => {
                        content.perform(action);
                    }
                    crate::views::file_viewer::ViewerContent::Markdown {
                        content,
                        raw_text,
                        ..
                    } => {
                        content.perform(action);
                        *raw_text = content.text();
                    }
                    _ => {}
                }
                if is_edit {
                    viewer.mark_modified();
                }
            }
            Task::none()
        }
        FileViewerMessage::Save(viewer_id) => {
            if let Some(viewer) = app.file_viewers.get_mut(viewer_id) {
                // Get text content to save
                if let Some(text) = viewer.begin_save() {
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
                if !viewer.is_saving {
                    return Task::none();
                }
                viewer.finish_save(&result);
                match result {
                    Ok(()) => {
                        app.toast_manager
                            .push(Toast::new("File saved", ToastType::Success));
                        if !viewer.is_modified {
                            app.dialogs.close_unsaved_file(viewer_id);
                        }
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
        FileViewerMessage::DiscardChanges(viewer_id) => {
            if matches!(app.dialogs.active(), crate::app::managers::ActiveDialog::UnsavedFile(id) if *id == viewer_id)
                && app
                    .file_viewers
                    .get(viewer_id)
                    .is_some_and(|viewer| !viewer.is_saving)
            {
                app.dialogs.close_unsaved_file(viewer_id);
                app.close_tab_without_confirmation(viewer_id);
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

/// Scroll a freshly loaded text/markdown viewer so the target line sits near
/// the top of the view, with a few lines of context above it.
fn scroll_viewer_to_line(viewer: &mut crate::views::file_viewer::FileViewerState, line: usize) {
    const GOTO_LINE_CONTEXT: usize = 3;

    let scroll = line.saturating_sub(1 + GOTO_LINE_CONTEXT);
    if scroll == 0 {
        return;
    }
    let action = text_editor::Action::Scroll {
        lines: scroll.min(i32::MAX as usize) as i32,
    };
    match &mut viewer.content {
        crate::views::file_viewer::ViewerContent::Text { content } => content.perform(action),
        crate::views::file_viewer::ViewerContent::Markdown { content, .. } => {
            content.perform(action)
        }
        _ => {}
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
    use super::*;
    use crate::app::managers::ActiveDialog;
    use crate::keybindings::AppAction;
    use crate::message::{DialogMessage, TabMessage, UiMessage};
    use crate::views::file_viewer::{FileType, FileViewerState, ViewerContent};
    use crate::views::tabs::Tab;
    use uuid::Uuid;

    fn editor_app(markdown: bool, modified: bool) -> (Portal, Uuid) {
        let mut app = Portal::test_state();
        let id = Uuid::new_v4();
        let file_type = if markdown {
            FileType::Markdown
        } else {
            FileType::Text { language: None }
        };
        let mut viewer = FileViewerState::new(
            id,
            "notes.txt".into(),
            FileSource::Local {
                path: "notes.txt".into(),
            },
            file_type,
        );
        let content = text_editor::Content::with_text("Keep these edits");
        viewer.content = if markdown {
            ViewerContent::Markdown {
                raw_text: content.text(),
                content,
                preview_mode: false,
            }
        } else {
            ViewerContent::Text { content }
        };
        viewer.is_modified = modified;
        app.file_viewers.insert(viewer);
        app.tabs.push(Tab::new_file_viewer(id, "notes.txt".into()));
        app.enter_file_viewer_view(id);
        (app, id)
    }

    fn assert_editor_open(app: &Portal, id: Uuid, expected: &str, modified: bool) {
        assert!(app.tabs.iter().any(|tab| tab.id == id));
        let viewer = app.file_viewers.get(id).unwrap();
        assert_eq!(viewer.get_text().unwrap(), expected);
        assert_eq!(viewer.is_modified, modified);
    }

    #[test]
    fn closing_unmodified_file_needs_no_confirmation() {
        for markdown in [false, true] {
            let (mut app, id) = editor_app(markdown, false);
            let _ = app.update(Message::Tab(TabMessage::Close(id)));
            assert!(!app.file_viewers.contains(id));
            assert!(app.tabs.is_empty());
            assert!(!app.dialogs.is_open());
        }
    }

    #[test]
    fn closing_modified_file_preserves_edits_until_discard_is_confirmed() {
        for markdown in [false, true] {
            let (mut app, id) = editor_app(markdown, true);
            let original = app.file_viewers.get(id).unwrap().get_text().unwrap();
            let _ = app.update(Message::FileViewer(FileViewerMessage::DiscardChanges(id)));
            assert_editor_open(&app, id, &original, true);

            let _ = app.update(Message::Tab(TabMessage::Close(id)));
            assert!(
                matches!(app.dialogs.active(), ActiveDialog::UnsavedFile(target) if *target == id)
            );
            assert_editor_open(&app, id, &original, true);
            let _ = app.update(Message::Dialog(DialogMessage::Close));
            assert!(!app.dialogs.is_open());
            assert_editor_open(&app, id, &original, true);

            // The keyboard close action uses the same protection.
            let _ = app.handle_keybinding_action(AppAction::CloseSession);
            let _ = app.update(Message::FileViewer(FileViewerMessage::DiscardChanges(id)));
            assert!(!app.file_viewers.contains(id));
            assert!(app.tabs.is_empty());
            assert!(!app.dialogs.is_open());
        }
    }

    #[test]
    fn escape_cancels_close_without_losing_edits() {
        let (mut app, id) = editor_app(false, true);
        let original = app.file_viewers.get(id).unwrap().get_text().unwrap();
        app.close_tab(id);
        let _ = app.update(Message::Ui(UiMessage::KeyboardEvent(
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            iced::keyboard::Modifiers::default(),
            None,
        )));
        assert!(!app.dialogs.is_open());
        assert_editor_open(&app, id, &original, true);
    }

    #[test]
    fn saving_from_toolbar_or_close_dialog_keeps_file_open() {
        for markdown in [false, true] {
            for from_dialog in [false, true] {
                let (mut app, id) = editor_app(markdown, true);
                let original = app.file_viewers.get(id).unwrap().get_text().unwrap();
                if from_dialog {
                    app.close_tab(id);
                }
                let save = app.update(Message::FileViewer(FileViewerMessage::Save(id)));
                assert!(save.units() > 0);
                assert!(app.file_viewers.get(id).unwrap().is_saving);
                assert_eq!(
                    app.update(Message::FileViewer(FileViewerMessage::Save(id)))
                        .units(),
                    0
                );

                let result = app.update(Message::FileViewer(FileViewerMessage::SaveResult(
                    id,
                    Ok(()),
                )));
                assert_eq!(result.units(), 0, "saving must not schedule a tab close");
                assert_editor_open(&app, id, &original, false);
                assert!(!app.file_viewers.get(id).unwrap().is_saving);
                assert!(!app.dialogs.is_open());
            }
        }
    }

    #[test]
    fn failed_save_keeps_edits_and_confirmation_and_allows_retry() {
        let (mut app, id) = editor_app(false, true);
        let original = app.file_viewers.get(id).unwrap().get_text().unwrap();
        app.close_tab(id);
        let _ = app.update(Message::FileViewer(FileViewerMessage::Save(id)));
        let _ = app.update(Message::FileViewer(FileViewerMessage::SaveResult(
            id,
            Err("Permission denied".into()),
        )));
        assert_editor_open(&app, id, &original, true);
        let viewer = app.file_viewers.get(id).unwrap();
        assert!(!viewer.is_saving);
        assert_eq!(viewer.save_error.as_deref(), Some("Permission denied"));
        assert!(matches!(app.dialogs.active(), ActiveDialog::UnsavedFile(target) if *target == id));
        assert!(
            app.toast_manager
                .toasts()
                .iter()
                .any(|toast| toast.message.contains("Permission denied"))
        );

        let _ = app.update(Message::FileViewer(FileViewerMessage::Save(id)));
        assert!(app.file_viewers.get(id).unwrap().save_error.is_none());
        let _ = app.update(Message::FileViewer(FileViewerMessage::SaveResult(
            id,
            Ok(()),
        )));
        assert_editor_open(&app, id, &original, false);
        assert!(!app.dialogs.is_open());
    }

    #[test]
    fn closing_during_save_cannot_discard_or_remove_the_file() {
        let (mut app, id) = editor_app(false, true);
        let original = app.file_viewers.get(id).unwrap().get_text().unwrap();
        let _ = app.update(Message::FileViewer(FileViewerMessage::Save(id)));
        app.close_tab(id);
        let _ = app.update(Message::FileViewer(FileViewerMessage::DiscardChanges(id)));
        assert_editor_open(&app, id, &original, true);
        assert!(app.file_viewers.get(id).unwrap().is_saving);
    }

    #[test]
    fn edits_made_during_save_remain_unsaved_and_require_confirmation() {
        for markdown in [false, true] {
            let (mut app, id) = editor_app(markdown, true);
            let _ = app.update(Message::FileViewer(FileViewerMessage::Save(id)));
            let _ = app.update(Message::FileViewer(FileViewerMessage::TextChanged(
                id,
                text_editor::Action::Edit(text_editor::Edit::Insert('!')),
            )));
            let edited = app.file_viewers.get(id).unwrap().get_text().unwrap();
            let _ = app.update(Message::FileViewer(FileViewerMessage::SaveResult(
                id,
                Ok(()),
            )));
            assert_editor_open(&app, id, &edited, true);
            app.close_tab(id);
            assert!(
                matches!(app.dialogs.active(), ActiveDialog::UnsavedFile(target) if *target == id)
            );
        }
    }

    #[test]
    fn cursor_movement_does_not_mark_file_modified() {
        for markdown in [false, true] {
            let (mut app, id) = editor_app(markdown, false);
            let _ = app.update(Message::FileViewer(FileViewerMessage::TextChanged(
                id,
                text_editor::Action::Move(text_editor::Motion::Right),
            )));
            assert!(!app.file_viewers.get(id).unwrap().is_modified);
        }
    }

    #[test]
    fn close_prompts_for_multiple_files_are_queued_without_duplicates() {
        let (mut app, first) = editor_app(false, true);
        let second = Uuid::new_v4();
        let mut viewer = app.file_viewers.get(first).unwrap().clone();
        viewer.viewer_id = second;
        app.file_viewers.insert(viewer);
        app.tabs
            .push(Tab::new_file_viewer(second, "other.txt".into()));
        app.close_tab(first);
        app.close_tab(first);
        app.close_tab(second);
        assert!(matches!(app.dialogs.active(), ActiveDialog::UnsavedFile(id) if *id == first));
        let _ = app.update(Message::FileViewer(FileViewerMessage::DiscardChanges(
            second,
        )));
        assert!(
            app.file_viewers.contains(second),
            "only the visible dialog can discard"
        );
        let _ = app.update(Message::FileViewer(FileViewerMessage::DiscardChanges(
            first,
        )));
        assert!(matches!(app.dialogs.active(), ActiveDialog::UnsavedFile(id) if *id == second));
        let _ = app.update(Message::FileViewer(FileViewerMessage::DiscardChanges(
            second,
        )));
        assert!(!app.dialogs.is_open());
        assert!(app.tabs.is_empty());
    }

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
