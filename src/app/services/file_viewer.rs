use iced::Task;
use image::{GenericImageView, ImageEncoder};
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};
use std::path::Path;

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
            let path_for_inspect = path.clone();
            tokio::task::spawn_blocking(move || inspect_pdf(&path_for_inspect))
                .await
                .map_err(|e| format!("PDF inspect task failed: {}", e))?
        }
        FileType::Binary => Err("Binary files cannot be viewed".to_string()),
    }
}

fn parse_image_dimensions(path: &Path, data: &[u8]) -> Result<(u32, u32, bool), String> {
    let is_svg = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));

    if is_svg {
        return Ok((0, 0, true));
    }

    let image = image::load_from_memory(data).map_err(|e| {
        format!(
            "Failed to decode image {}: {}",
            path.display(),
            e
        )
    })?;
    let (width, height) = image.dimensions();
    Ok((width, height, false))
}

fn inspect_pdf(path: &Path) -> Result<ViewerContent, String> {
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
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
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| format!("PDF rendering unavailable: {}", e))?;
    let pdfium = Pdfium::new(bindings);

    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("Failed to open PDF {}: {}", path.display(), e))?;

    let pages = document.pages();
    if page_index >= pages.len() as usize {
        return Err("PDF page index out of range".to_string());
    }

    let page = pages
        .get(page_index as u16)
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
        .write_image(
            &image,
            width,
            height,
            image::ColorType::Rgba8.into(),
        )
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
            Ok(content) => {
                Message::FileViewer(FileViewerMessage::ContentLoaded { viewer_id, content })
            }
            Err(e) => Message::FileViewer(FileViewerMessage::LoadError(viewer_id, e)),
        },
    );

    let viewer_state = FileViewerState::new(viewer_id, file_name, source, file_type);

    (viewer_state, task)
}
