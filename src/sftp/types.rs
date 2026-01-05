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

    /// Get file type description for the Kind column
    pub fn kind_description(&self) -> &'static str {
        if self.is_parent() {
            "Parent Directory"
        } else if self.is_dir {
            "Folder"
        } else if self.is_symlink {
            "Symbolic Link"
        } else {
            match self.extension() {
                Some("rs") => "Rust Source",
                Some("py") => "Python Script",
                Some("js") => "JavaScript",
                Some("ts") => "TypeScript",
                Some("c") => "C Source",
                Some("cpp" | "cc" | "cxx") => "C++ Source",
                Some("h" | "hpp") => "Header File",
                Some("go") => "Go Source",
                Some("java") => "Java Source",
                Some("rb") => "Ruby Script",
                Some("php") => "PHP Script",
                Some("swift") => "Swift Source",
                Some("kt") => "Kotlin Source",
                Some("txt") => "Plain Text",
                Some("md" | "markdown") => "Markdown",
                Some("rst") => "reStructuredText",
                Some("doc" | "docx") => "Word Document",
                Some("rtf") => "Rich Text",
                Some("json") => "JSON",
                Some("toml") => "TOML Config",
                Some("yaml" | "yml") => "YAML",
                Some("xml") => "XML",
                Some("ini" | "conf" | "cfg") => "Config File",
                Some("jpg" | "jpeg") => "JPEG Image",
                Some("png") => "PNG Image",
                Some("gif") => "GIF Image",
                Some("bmp") => "Bitmap Image",
                Some("svg") => "SVG Image",
                Some("ico") => "Icon",
                Some("webp") => "WebP Image",
                Some("tiff") => "TIFF Image",
                Some("mp3") => "MP3 Audio",
                Some("wav") => "WAV Audio",
                Some("ogg") => "Ogg Audio",
                Some("flac") => "FLAC Audio",
                Some("m4a") => "M4A Audio",
                Some("aac") => "AAC Audio",
                Some("wma") => "WMA Audio",
                Some("mp4") => "MP4 Video",
                Some("mkv") => "Matroska Video",
                Some("avi") => "AVI Video",
                Some("mov") => "QuickTime Video",
                Some("webm") => "WebM Video",
                Some("wmv") => "WMV Video",
                Some("flv") => "Flash Video",
                Some("zip") => "ZIP Archive",
                Some("tar") => "TAR Archive",
                Some("gz" | "gzip") => "Gzip Archive",
                Some("xz") => "XZ Archive",
                Some("7z") => "7-Zip Archive",
                Some("rar") => "RAR Archive",
                Some("bz2") => "Bzip2 Archive",
                Some("tgz") => "Tarball",
                Some("sh" | "bash" | "zsh" | "fish") => "Shell Script",
                Some("bat" | "cmd") => "Batch File",
                Some("ps1") => "PowerShell Script",
                Some("exe") => "Executable",
                Some("bin") => "Binary",
                Some("app") => "Application",
                Some("dmg") => "Disk Image",
                Some("deb") => "Debian Package",
                Some("rpm") => "RPM Package",
                Some("pdf") => "PDF Document",
                Some("html" | "htm") => "HTML Document",
                Some("css") => "Stylesheet",
                Some("sql") => "SQL Script",
                Some("log") => "Log File",
                _ => "File",
            }
        }
    }

    /// Format modified date for display
    pub fn formatted_modified(&self) -> String {
        match &self.modified {
            Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
            None => "—".to_string(),
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
