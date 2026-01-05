//! SFTP types for file browser

use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// Icon type for file entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIcon {
    ParentDir,
    Folder,
    Symlink,
    Code,
    Text,
    Image,
    Audio,
    Video,
    Archive,
    Config,
    Executable,
    File,
}

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

    /// Get icon type for this file
    pub fn icon_type(&self) -> FileIcon {
        if self.is_parent() {
            FileIcon::ParentDir
        } else if self.is_dir {
            FileIcon::Folder
        } else if self.is_symlink {
            FileIcon::Symlink
        } else {
            match self.extension() {
                Some("rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "go" | "java" | "rb" | "php" | "swift" | "kt") => FileIcon::Code,
                Some("txt" | "md" | "markdown" | "rst" | "doc" | "docx" | "rtf") => FileIcon::Text,
                Some("json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "conf" | "cfg") => FileIcon::Config,
                Some("jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "ico" | "webp" | "tiff") => FileIcon::Image,
                Some("mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma") => FileIcon::Audio,
                Some("mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv") => FileIcon::Video,
                Some("zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "bz2" | "tgz") => FileIcon::Archive,
                Some("sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" | "ps1") => FileIcon::Executable,
                Some("exe" | "bin" | "app" | "dmg" | "deb" | "rpm") => FileIcon::Executable,
                _ => FileIcon::File,
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
