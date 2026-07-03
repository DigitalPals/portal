//! Terminal paste helpers for native clipboard image support.

use std::path::{Path, PathBuf};

use chrono::Utc;
use image::ImageEncoder;
use uuid::Uuid;

const MAX_CLIPBOARD_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum TerminalPastePayload {
    Text(String),
    ImagePng {
        filename: String,
        png: Vec<u8>,
        width: u32,
        height: u32,
    },
}

pub(crate) fn read_clipboard_payload() -> Result<TerminalPastePayload, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("Failed to open clipboard: {error}"))?;

    let image_error = match clipboard.get_image() {
        Ok(image) => return clipboard_image_to_png_payload(image),
        Err(error) => error,
    };

    match clipboard.get_text() {
        Ok(text) => Ok(TerminalPastePayload::Text(text)),
        Err(text_error) => Err(format!(
            "Clipboard does not contain pasteable text or image data (image: {image_error}; text: {text_error})"
        )),
    }
}

pub(crate) fn remote_paste_dir(home_dir: &Path) -> PathBuf {
    home_dir.join(".cache").join("portal").join("pastes")
}

pub(crate) fn paste_text_for_uploaded_path(path: &str) -> String {
    path.to_string()
}

fn clipboard_image_to_png_payload(
    image: arboard::ImageData<'static>,
) -> Result<TerminalPastePayload, String> {
    if image.width == 0 || image.height == 0 || image.bytes.is_empty() {
        return Err("Clipboard image is empty".to_string());
    }

    let expected_len = image
        .width
        .checked_mul(image.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Clipboard image dimensions are too large".to_string())?;

    if expected_len != image.bytes.len() {
        return Err(format!(
            "Clipboard image has invalid RGBA data: expected {expected_len} bytes, got {}",
            image.bytes.len()
        ));
    }

    if image.bytes.len() > MAX_CLIPBOARD_IMAGE_BYTES {
        return Err(format!(
            "Clipboard image is too large: {} bytes",
            image.bytes.len()
        ));
    }

    let width =
        u32::try_from(image.width).map_err(|_| "Clipboard image width is too large".to_string())?;
    let height = u32::try_from(image.height)
        .map_err(|_| "Clipboard image height is too large".to_string())?;

    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.bytes.as_ref(),
            width,
            height,
            image::ColorType::Rgba8.into(),
        )
        .map_err(|error| format!("Failed to encode clipboard image as PNG: {error}"))?;

    Ok(TerminalPastePayload::ImagePng {
        filename: clipboard_image_filename(),
        png,
        width,
        height,
    })
}

fn clipboard_image_filename() -> String {
    format!(
        "portal-paste-{}-{}.png",
        Utc::now().format("%Y%m%d-%H%M%S"),
        Uuid::new_v4().simple()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_paste_dir_uses_private_portal_cache_path() {
        assert_eq!(
            remote_paste_dir(Path::new("/home/john")),
            PathBuf::from("/home/john/.cache/portal/pastes")
        );
    }

    #[test]
    fn uploaded_path_paste_text_is_plain_path() {
        assert_eq!(
            paste_text_for_uploaded_path("/home/john/.cache/portal/pastes/a.png"),
            "/home/john/.cache/portal/pastes/a.png"
        );
    }
}
