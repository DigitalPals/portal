//! File viewer type definitions

use std::path::PathBuf;


/// Detected file type for viewing/editing
#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    /// Text file with optional language hint for syntax highlighting
    Text { language: Option<String> },
    /// Image file (PNG, JPG, etc.)
    Image,
    /// PDF document
    Pdf,
    /// Markdown file (supports edit/preview toggle)
    Markdown,
    /// Binary file (unsupported for viewing)
    Binary,
}

impl FileType {
    /// Detect file type from file extension
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // Text files with syntax highlighting
            "rs" => Self::Text { language: Some("rust".to_string()) },
            "py" => Self::Text { language: Some("python".to_string()) },
            "js" => Self::Text { language: Some("javascript".to_string()) },
            "ts" => Self::Text { language: Some("typescript".to_string()) },
            "jsx" | "tsx" => Self::Text { language: Some("javascript".to_string()) },
            "json" => Self::Text { language: Some("json".to_string()) },
            "toml" => Self::Text { language: Some("toml".to_string()) },
            "yaml" | "yml" => Self::Text { language: Some("yaml".to_string()) },
            "html" | "htm" => Self::Text { language: Some("html".to_string()) },
            "css" => Self::Text { language: Some("css".to_string()) },
            "scss" | "sass" => Self::Text { language: Some("scss".to_string()) },
            "sh" | "bash" | "zsh" => Self::Text { language: Some("bash".to_string()) },
            "c" | "h" => Self::Text { language: Some("c".to_string()) },
            "cpp" | "cc" | "cxx" | "hpp" => Self::Text { language: Some("cpp".to_string()) },
            "go" => Self::Text { language: Some("go".to_string()) },
            "java" => Self::Text { language: Some("java".to_string()) },
            "rb" => Self::Text { language: Some("ruby".to_string()) },
            "php" => Self::Text { language: Some("php".to_string()) },
            "sql" => Self::Text { language: Some("sql".to_string()) },
            "xml" => Self::Text { language: Some("xml".to_string()) },
            "lua" => Self::Text { language: Some("lua".to_string()) },
            "vim" => Self::Text { language: Some("vim".to_string()) },
            "dockerfile" => Self::Text { language: Some("dockerfile".to_string()) },
            "makefile" => Self::Text { language: Some("makefile".to_string()) },

            // Plain text files
            "txt" | "log" | "conf" | "cfg" | "ini" | "env" => Self::Text { language: None },

            // Markdown
            "md" | "markdown" => Self::Markdown,

            // Images
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "svg" => Self::Image,

            // PDF
            "pdf" => Self::Pdf,

            // Default to binary for unknown types
            _ => Self::Binary,
        }
    }

    /// Get file type from a file path
    pub fn from_path(path: &PathBuf) -> Self {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(Self::from_extension)
            .unwrap_or(Self::Binary)
    }

    /// Check if file type is viewable
    pub fn is_viewable(&self) -> bool {
        !matches!(self, Self::Binary)
    }

    /// Check if file type is editable
    pub fn is_editable(&self) -> bool {
        matches!(self, Self::Text { .. } | Self::Markdown)
    }
}

/// Source location of the file
#[derive(Debug, Clone)]
pub enum FileSource {
    /// Local file on disk
    Local { path: PathBuf },
    /// Remote file accessed via SFTP
    Remote {
        /// Local temporary path for editing
        temp_path: PathBuf,
    },
}
