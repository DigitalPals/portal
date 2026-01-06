//! File viewer state management

use iced::widget::text_editor;

use super::types::{FileSource, FileType};
use crate::message::SessionId;

/// Content held by the viewer based on file type
#[derive(Debug, Clone)]
pub enum ViewerContent {
    /// Loading state - content not yet available
    Loading,
    /// Text content for editing
    Text { content: text_editor::Content },
    /// Markdown with edit/preview toggle
    Markdown {
        content: text_editor::Content,
        raw_text: String,
        preview_mode: bool,
    },
    /// Image data (bytes and dimensions)
    Image {
        data: Vec<u8>,
        zoom: f32,
        width: u32,
        height: u32,
        is_svg: bool,
    },
    /// PDF pages
    Pdf {
        pages: Vec<Option<Vec<u8>>>, // Rendered page images (lazy)
        rendering_pages: Vec<bool>,
        current_page: usize,
        total_pages: usize,
    },
    /// Error loading content
    Error(String),
}

/// State for a single file viewer instance
#[derive(Debug, Clone)]
pub struct FileViewerState {
    /// Unique identifier for this viewer
    pub viewer_id: SessionId,
    /// Display name of the file
    pub file_name: String,
    /// Source location of the file
    pub file_source: FileSource,
    /// Detected file type
    pub file_type: FileType,
    /// Content being viewed/edited
    pub content: ViewerContent,
    /// Whether the content has been modified
    pub is_modified: bool,
    /// Whether a save operation is in progress
    pub is_saving: bool,
}

impl FileViewerState {
    /// Create a new file viewer state in loading state
    pub fn new(
        viewer_id: SessionId,
        file_name: String,
        file_source: FileSource,
        file_type: FileType,
    ) -> Self {
        Self {
            viewer_id,
            file_name,
            file_source,
            file_type,
            content: ViewerContent::Loading,
            is_modified: false,
            is_saving: false,
        }
    }

    /// Set error state
    pub fn set_error(&mut self, error: String) {
        self.content = ViewerContent::Error(error);
    }

    /// Get current text content as string (for saving)
    pub fn get_text(&self) -> Option<String> {
        match &self.content {
            ViewerContent::Text { content } => Some(content.text()),
            ViewerContent::Markdown { content, .. } => Some(content.text()),
            _ => None,
        }
    }

    /// Mark content as modified
    pub fn mark_modified(&mut self) {
        self.is_modified = true;
    }

    /// Mark content as saved
    pub fn mark_saved(&mut self) {
        self.is_modified = false;
        self.is_saving = false;
    }

    /// Toggle markdown preview mode
    pub fn toggle_preview(&mut self) {
        if let ViewerContent::Markdown {
            preview_mode,
            content,
            raw_text,
        } = &mut self.content
        {
            *preview_mode = !*preview_mode;
            // Update raw_text when leaving preview mode
            if !*preview_mode {
                *raw_text = content.text();
            }
        }
    }

    /// Change PDF page
    pub fn set_pdf_page(&mut self, page: usize) {
        if let ViewerContent::Pdf {
            current_page,
            total_pages,
            ..
        } = &mut self.content
        {
            if page < *total_pages {
                *current_page = page;
            }
        }
    }

    /// Mark a PDF page as rendering
    pub fn set_pdf_rendering(&mut self, page: usize, rendering: bool) {
        if let ViewerContent::Pdf {
            rendering_pages, ..
        } = &mut self.content
        {
            if let Some(slot) = rendering_pages.get_mut(page) {
                *slot = rendering;
            }
        }
    }

    /// Store rendered data for a PDF page
    pub fn set_pdf_page_data(&mut self, page: usize, data: Vec<u8>) {
        if let ViewerContent::Pdf { pages, .. } = &mut self.content {
            if let Some(slot) = pages.get_mut(page) {
                *slot = Some(data);
            }
        }
    }

    /// Set image zoom level
    pub fn set_zoom(&mut self, zoom: f32) {
        if let ViewerContent::Image {
            zoom: current_zoom, ..
        } = &mut self.content
        {
            *current_zoom = zoom.clamp(0.1, 5.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor;
    use std::path::PathBuf;

    #[test]
    fn toggle_preview_updates_mode_and_raw_text() {
        let mut state = FileViewerState::new(
            uuid::Uuid::new_v4(),
            "notes.md".to_string(),
            FileSource::Local {
                path: PathBuf::from("notes.md"),
            },
            FileType::Markdown,
        );

        state.content = ViewerContent::Markdown {
            content: text_editor::Content::with_text("initial"),
            raw_text: "initial".to_string(),
            preview_mode: false,
        };

        state.toggle_preview();
        match &state.content {
            ViewerContent::Markdown {
                preview_mode,
                raw_text,
                ..
            } => {
                assert!(*preview_mode);
                assert_eq!(raw_text, "initial");
            }
            _ => panic!("expected markdown content"),
        }

        if let ViewerContent::Markdown { content, .. } = &mut state.content {
            *content = text_editor::Content::with_text("updated");
        }

        state.toggle_preview();
        match &state.content {
            ViewerContent::Markdown {
                preview_mode,
                raw_text,
                ..
            } => {
                assert!(!*preview_mode);
                assert_eq!(raw_text, "updated");
            }
            _ => panic!("expected markdown content"),
        }
    }
}
