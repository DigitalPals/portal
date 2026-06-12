use iced::Task;
use image::{GenericImageView, ImageEncoder};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use std::path::{Path, PathBuf};

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 50 * 1024 * 1024;

fn file_type_limit(file_type: &FileType) -> u64 {
    match file_type {
        FileType::Text { .. } | FileType::Markdown => MAX_TEXT_BYTES,
        FileType::Image => MAX_IMAGE_BYTES,
        FileType::Pdf => MAX_PDF_BYTES,
        FileType::Binary => 0,
    }
}

use crate::fs_utils::{
    ensure_private_dir_no_follow, read_regular_file_limited, read_regular_file_to_string_limited,
};
use crate::message::{FileViewerMessage, Message, SessionId};
use crate::sftp::SharedSftpSession;
use crate::views::file_viewer::{FileSource, FileType, FileViewerState, ViewerContent};

/// Load file content from local path based on file type
pub async fn load_local_file(path: PathBuf, file_type: FileType) -> Result<ViewerContent, String> {
    match file_type {
        FileType::Text { .. } => {
            let text = read_local_text_file(path, MAX_TEXT_BYTES, "Text").await?;
            Ok(ViewerContent::Text {
                content: iced::widget::text_editor::Content::with_text(&text),
            })
        }
        FileType::Markdown => {
            let text = read_local_text_file(path, MAX_TEXT_BYTES, "Markdown").await?;
            Ok(ViewerContent::Markdown {
                content: iced::widget::text_editor::Content::with_text(&text),
                raw_text: text,
                preview_mode: false,
            })
        }
        FileType::Image => {
            let data = read_local_file_bytes(path.clone(), MAX_IMAGE_BYTES, "Image").await?;
            let path_for_parse = path.clone();
            let data_for_parse = data.clone();
            let (width, height, is_svg) = tokio::task::spawn_blocking(move || {
                parse_image_dimensions(&path_for_parse, &data_for_parse)
            })
            .await
            .map_err(|e| format!("Image decode task failed: {}", e))??;
            Ok(ViewerContent::Image {
                data,
                zoom: 1.0,
                width,
                height,
                is_svg,
            })
        }
        FileType::Pdf => {
            enforce_local_size(&path, MAX_PDF_BYTES, "PDF").await?;
            let path_for_inspect = path.clone();
            tokio::task::spawn_blocking(move || inspect_pdf(&path_for_inspect))
                .await
                .map_err(|e| format!("PDF inspect task failed: {}", e))?
        }
        FileType::Binary => Err("Binary files cannot be viewed".to_string()),
    }
}

async fn read_local_text_file(
    path: PathBuf,
    limit: u64,
    label: &'static str,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || read_regular_file_to_string_limited(&path, limit, label))
        .await
        .map_err(|e| format!("{} read task failed: {}", label, e))?
}

async fn read_local_file_bytes(
    path: PathBuf,
    limit: u64,
    label: &'static str,
) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || read_regular_file_limited(&path, limit, label))
        .await
        .map_err(|e| format!("{} read task failed: {}", label, e))?
}

fn parse_image_dimensions(path: &Path, data: &[u8]) -> Result<(u32, u32, bool), String> {
    let is_svg = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));

    if is_svg {
        return Ok((0, 0, true));
    }

    let image = image::load_from_memory(data)
        .map_err(|e| format!("Failed to decode image {}: {}", path.display(), e))?;
    let (width, height) = image.dimensions();
    Ok((width, height, false))
}

fn inspect_pdf(path: &Path) -> Result<ViewerContent, String> {
    ensure_regular_file_sync(path, "PDF")?;

    let bindings = Pdfium::bind_to_system_library()
        .map_err(|e| format!("PDF rendering unavailable: {}", e))?;
    let pdfium = Pdfium::new(bindings);

    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("Failed to open PDF {}: {}", path.display(), e))?;

    let pages = document.pages();
    let total_pages = pages.len() as usize;

    if total_pages == 0 {
        return Err("PDF has no pages".to_string());
    }

    Ok(ViewerContent::Pdf {
        pages: vec![None; total_pages],
        rendering_pages: vec![false; total_pages],
        current_page: 0,
        total_pages,
    })
}

async fn enforce_local_size(path: &Path, limit: u64, label: &str) -> Result<(), String> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|e| format!("Failed to stat {} file: {}", label, e))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} file is a symbolic link", label));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("{} file is not a regular file", label));
    }
    let size = metadata.len();
    if size > limit {
        return Err(format!(
            "{} file too large ({} bytes, limit {})",
            label, size, limit
        ));
    }
    Ok(())
}

fn ensure_regular_file_sync(path: &Path, label: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to stat {} file: {}", label, e))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} file is a symbolic link", label));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("{} file is not a regular file", label));
    }
    Ok(())
}

async fn ensure_private_dir(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        ensure_private_dir_no_follow(&path)
            .map_err(|e| format!("Failed to prepare directory {}: {}", path.display(), e))
    })
    .await
    .map_err(|e| format!("Directory preparation task failed: {}", e))?
}

async fn prepare_remote_viewer_temp_dir(temp_dir: &Path) -> Result<(), String> {
    let base_dir = temp_dir
        .parent()
        .ok_or_else(|| format!("Cannot determine temp base for {}", temp_dir.display()))?;
    ensure_private_dir(base_dir).await?;
    ensure_private_dir(temp_dir).await
}

pub async fn render_pdf_page(source: FileSource, page_index: usize) -> Result<Vec<u8>, String> {
    let path = match source {
        FileSource::Local { path } => path,
        FileSource::Remote { temp_path, .. } => temp_path,
    };

    tokio::task::spawn_blocking(move || render_pdf_page_sync(&path, page_index))
        .await
        .map_err(|e| format!("PDF render task failed: {}", e))?
}

fn render_pdf_page_sync(path: &Path, page_index: usize) -> Result<Vec<u8>, String> {
    ensure_regular_file_sync(path, "PDF")?;

    let bindings = Pdfium::bind_to_system_library()
        .map_err(|e| format!("PDF rendering unavailable: {}", e))?;
    let pdfium = Pdfium::new(bindings);

    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("Failed to open PDF {}: {}", path.display(), e))?;

    let pages = document.pages();
    if page_index >= pages.len() as usize {
        return Err("PDF page index out of range".to_string());
    }

    let page_index_u16 =
        u16::try_from(page_index).map_err(|_| "PDF page index out of range".to_string())?;
    let page = pages
        .get(page_index_u16)
        .map_err(|e| format!("Failed to load PDF page {}: {}", page_index + 1, e))?;
    let bitmap = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(1200))
        .map_err(|e| format!("Failed to render PDF page {}: {}", page_index + 1, e))?;

    let width = bitmap.width() as u32;
    let height = bitmap.height() as u32;
    let rgba = bitmap.as_rgba_bytes();
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| format!("Failed to build PDF image for page {}", page_index + 1))?;

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&image, width, height, image::ColorType::Rgba8.into())
        .map_err(|e| format!("Failed to encode PDF page {}: {}", page_index + 1, e))?;

    Ok(png)
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
            Ok(content) => {
                Message::FileViewer(FileViewerMessage::ContentLoaded { viewer_id, content })
            }
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
    let temp_file_name = safe_temp_file_name(&file_name);
    let temp_path = temp_dir.join(temp_file_name);

    let source = FileSource::Remote {
        temp_path: temp_path.clone(),
        session_id,
        remote_path: remote_path.clone(),
    };

    let ftype = file_type.clone();
    let task = Task::perform(
        async move {
            prepare_remote_viewer_temp_dir(&temp_dir).await?;
            let limit = file_type_limit(&ftype);
            if limit > 0 {
                let size = sftp
                    .file_size(&remote_path)
                    .await
                    .map_err(|e| format!("Failed to stat remote file: {}", e))?;
                if size > limit {
                    return Err(format!(
                        "Remote file too large ({} bytes, limit {})",
                        size, limit
                    ));
                }
            }
            sftp.download(&remote_path, &temp_path)
                .await
                .map_err(|e| format!("Failed to download file: {}", e))?;
            load_local_file(temp_path, ftype).await
        },
        move |result| match result {
            Ok(content) => {
                Message::FileViewer(FileViewerMessage::ContentLoaded { viewer_id, content })
            }
            Err(e) => Message::FileViewer(FileViewerMessage::LoadError(viewer_id, e)),
        },
    );

    let viewer_state = FileViewerState::new(viewer_id, file_name, source, file_type);

    (viewer_state, task)
}

fn safe_temp_file_name(file_name: &str) -> String {
    let mut safe = String::with_capacity(file_name.len());
    for ch in file_name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            safe.push(ch);
        } else if ch.is_whitespace() {
            safe.push('_');
        }
    }

    let safe = safe.trim_matches(['.', '_', '-']);
    if safe.is_empty() || safe == ".." {
        "remote_file".to_string()
    } else {
        safe.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_TEXT_BYTES, enforce_local_size, ensure_regular_file_sync, load_local_file,
        prepare_remote_viewer_temp_dir, safe_temp_file_name,
    };
    use crate::views::file_viewer::{FileType, ViewerContent};

    #[test]
    fn safe_temp_file_name_removes_path_components() {
        assert_eq!(safe_temp_file_name("../../report.md"), "report.md");
    }

    #[test]
    fn safe_temp_file_name_falls_back_for_empty_names() {
        assert_eq!(safe_temp_file_name(".."), "remote_file");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn enforce_local_size_rejects_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, "secret").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(
            enforce_local_size(&link, MAX_TEXT_BYTES, "Text")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn enforce_local_size_rejects_non_regular_files() {
        let temp = tempfile::tempdir().unwrap();
        let socket_path = temp.path().join("viewer.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

        let error = enforce_local_size(&socket_path, MAX_TEXT_BYTES, "Text")
            .await
            .expect_err("non-regular file should be rejected");

        assert!(error.contains("not a regular file"));
    }

    #[tokio::test]
    async fn load_local_file_reads_text_with_checked_open() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("note.txt");
        std::fs::write(&path, "hello").unwrap();

        let content = load_local_file(path, FileType::Text { language: None })
            .await
            .expect("regular text file should load");

        match content {
            ViewerContent::Text { content } => assert_eq!(content.text(), "hello"),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn load_local_file_rejects_text_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, "secret").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = load_local_file(link, FileType::Text { language: None })
            .await
            .expect_err("symlink text file should be rejected");

        assert!(error.contains("symbolic link"));
    }

    #[test]
    fn ensure_regular_file_sync_allows_regular_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file.pdf");
        std::fs::write(&path, "content").unwrap();

        ensure_regular_file_sync(&path, "PDF").unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn ensure_regular_file_sync_rejects_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.pdf");
        let link = temp.path().join("link.pdf");
        std::fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_regular_file_sync(&link, "PDF")
            .expect_err("symlink should be rejected before PDF rendering");

        assert!(error.contains("symbolic link"));
    }

    #[test]
    #[cfg(unix)]
    fn ensure_regular_file_sync_rejects_non_regular_files() {
        let temp = tempfile::tempdir().unwrap();
        let socket_path = temp.path().join("viewer.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

        let error = ensure_regular_file_sync(&socket_path, "PDF")
            .expect_err("non-regular file should be rejected before PDF rendering");

        assert!(error.contains("not a regular file"));
    }

    #[tokio::test]
    async fn prepare_remote_viewer_temp_dir_creates_private_tree() {
        let temp = tempfile::tempdir().unwrap();
        let viewer_dir = temp.path().join("portal_viewer").join("viewer-id");

        prepare_remote_viewer_temp_dir(&viewer_dir).await.unwrap();

        assert!(viewer_dir.is_dir());
        assert!(viewer_dir.parent().unwrap().is_dir());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn prepare_remote_viewer_temp_dir_makes_tree_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let viewer_dir = temp.path().join("portal_viewer").join("viewer-id");

        prepare_remote_viewer_temp_dir(&viewer_dir).await.unwrap();

        let base_mode = std::fs::metadata(viewer_dir.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let viewer_mode = std::fs::metadata(&viewer_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(base_mode, 0o700);
        assert_eq!(viewer_mode, 0o700);
    }

    #[tokio::test]
    async fn prepare_remote_viewer_temp_dir_rejects_file_base() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("portal_viewer");
        let viewer_dir = base.join("viewer-id");
        std::fs::write(&base, "not a directory").unwrap();

        let error = prepare_remote_viewer_temp_dir(&viewer_dir)
            .await
            .expect_err("file base should be rejected");

        assert!(error.contains("directory"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn prepare_remote_viewer_temp_dir_rejects_symlink_base_without_creating_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        let base = temp.path().join("portal_viewer");
        let viewer_dir = base.join("viewer-id");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &base).unwrap();

        let error = prepare_remote_viewer_temp_dir(&viewer_dir)
            .await
            .expect_err("symlink base should be rejected");

        assert!(error.contains("symbolic link"));
        assert!(!outside.join("viewer-id").exists());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn prepare_remote_viewer_temp_dir_rejects_symlink_leaf_without_changing_target() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("portal_viewer");
        let outside = temp.path().join("outside");
        let viewer_dir = base.join("viewer-id");
        std::fs::create_dir(&base).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&outside, &viewer_dir).unwrap();

        let error = prepare_remote_viewer_temp_dir(&viewer_dir)
            .await
            .expect_err("symlink leaf should be rejected");

        assert!(error.contains("symbolic link"));
        let outside_mode = std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777;
        assert_eq!(outside_mode, 0o755);
    }
}
