use iced::Task;

use crate::message::{FileViewerMessage, Message, SessionId};
use crate::sftp::SharedSftpSession;
use crate::views::file_viewer::{FileSource, FileType, FileViewerState, ViewerContent};

/// Load file content from local path based on file type
pub async fn load_local_file(
    path: std::path::PathBuf,
    file_type: FileType,
) -> Result<ViewerContent, String> {
    match file_type {
        FileType::Text { .. } => {
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("Failed to read file: {}", e))?;
            Ok(ViewerContent::Text {
                content: iced::widget::text_editor::Content::with_text(&text),
            })
        }
        FileType::Markdown => {
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("Failed to read file: {}", e))?;
            Ok(ViewerContent::Markdown {
                content: iced::widget::text_editor::Content::with_text(&text),
                raw_text: text,
                preview_mode: false,
            })
        }
        FileType::Image => {
            let data = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("Failed to read image: {}", e))?;
            Ok(ViewerContent::Image { data, zoom: 1.0 })
        }
        FileType::Pdf => Ok(ViewerContent::Pdf {
            pages: vec![],
            current_page: 0,
            total_pages: 1,
        }),
        FileType::Binary => Err("Binary files cannot be viewed".to_string()),
    }
}

pub fn build_local_viewer(
    viewer_id: SessionId,
    file_name: String,
    path: std::path::PathBuf,
    file_type: FileType,
) -> (FileViewerState, Task<Message>) {
    let source = FileSource::Local { path: path.clone() };
    let ftype = file_type.clone();
    let task = Task::perform(
        async move { load_local_file(path, ftype).await },
        move |result| match result {
            Ok(content) => Message::FileViewer(FileViewerMessage::ContentLoaded { viewer_id, content }),
            Err(e) => Message::FileViewer(FileViewerMessage::LoadError(viewer_id, e)),
        },
    );

    let viewer_state = FileViewerState::new(viewer_id, file_name, source, file_type);

    (viewer_state, task)
}

pub fn build_remote_viewer(
    viewer_id: SessionId,
    file_name: String,
    remote_path: std::path::PathBuf,
    session_id: SessionId,
    sftp: SharedSftpSession,
    file_type: FileType,
) -> (FileViewerState, Task<Message>) {
    let temp_dir = std::env::temp_dir()
        .join("portal_viewer")
        .join(format!("{}", viewer_id));
    let temp_path = temp_dir.join(&file_name);

    let source = FileSource::Remote {
        temp_path: temp_path.clone(),
        session_id,
        remote_path: remote_path.clone(),
    };

    let ftype = file_type.clone();
    let task = Task::perform(
        async move {
            tokio::fs::create_dir_all(temp_dir)
                .await
                .map_err(|e| format!("Failed to create temp directory: {}", e))?;
            sftp.download(&remote_path, &temp_path)
                .await
                .map_err(|e| format!("Failed to download file: {}", e))?;
            load_local_file(temp_path, ftype).await
        },
        move |result| match result {
            Ok(content) => Message::FileViewer(FileViewerMessage::ContentLoaded { viewer_id, content }),
            Err(e) => Message::FileViewer(FileViewerMessage::LoadError(viewer_id, e)),
        },
    );

    let viewer_state = FileViewerState::new(viewer_id, file_name, source, file_type);

    (viewer_state, task)
}
