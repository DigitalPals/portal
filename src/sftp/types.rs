//! SFTP types for file browser

use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// Unified file entry representation for both local and remote files
#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

impl FileEntry {
    /// Check if this is the parent directory entry (..)
    pub fn is_parent(&self) -> bool {
        self.name == ".."
    }

    /// Get file extension if any
    pub fn extension(&self) -> Option<&str> {
        if self.is_dir {
            None
        } else {
            std::path::Path::new(&self.name)
                .extension()
                .and_then(|e| e.to_str())
        }
    }

    /// Get icon for this file type
    pub fn icon(&self) -> &'static str {
        if self.is_parent() {
            "⬆"
        } else if self.is_dir {
            "📁"
        } else if self.is_symlink {
            "🔗"
        } else {
            match self.extension() {
                Some("rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "go" | "java") => "📄",
                Some("txt" | "md" | "json" | "toml" | "yaml" | "yml" | "xml") => "📝",
                Some("jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "ico") => "🖼",
                Some("mp3" | "wav" | "ogg" | "flac" | "m4a") => "🎵",
                Some("mp4" | "mkv" | "avi" | "mov" | "webm") => "🎬",
                Some("zip" | "tar" | "gz" | "xz" | "7z" | "rar") => "📦",
                Some("pdf") => "📕",
                Some("sh" | "bash" | "zsh") => "⚙",
                Some("exe" | "bin" | "app") => "⚡",
                _ => "📄",
            }
        }
    }
}

/// Sort order for file listings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    NameAsc,
}

impl SortOrder {
    /// Sort file entries according to this order
    pub fn sort(&self, entries: &mut [FileEntry]) {
        // Always keep ".." at the top, then directories, then files
        entries.sort_by(|a, b| {
            // Parent directory always first
            if a.is_parent() {
                return std::cmp::Ordering::Less;
            }
            if b.is_parent() {
                return std::cmp::Ordering::Greater;
            }

            // Directories before files
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }

            // Apply sort order
            match self {
                SortOrder::NameAsc => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
    }
}

/// Format file size for display
pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}
