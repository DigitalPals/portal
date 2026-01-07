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
                Some(
                    "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "go" | "java" | "rb" | "php"
                    | "swift" | "kt",
                ) => FileIcon::Code,
                Some("txt" | "md" | "markdown" | "rst" | "doc" | "docx" | "rtf") => FileIcon::Text,
                Some("json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "conf" | "cfg") => {
                    FileIcon::Config
                }
                Some("jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "ico" | "webp" | "tiff") => {
                    FileIcon::Image
                }
                Some("mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma") => FileIcon::Audio,
                Some("mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv") => FileIcon::Video,
                Some("zip" | "tar" | "gz" | "xz" | "7z" | "rar" | "bz2" | "tgz") => {
                    FileIcon::Archive
                }
                Some("sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" | "ps1") => {
                    FileIcon::Executable
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_file(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/test/{}", name)),
            is_dir: false,
            is_symlink: false,
            size: 1024,
            modified: None,
        }
    }

    fn make_dir(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/test/{}", name)),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
        }
    }

    fn make_symlink(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/test/{}", name)),
            is_dir: false,
            is_symlink: true,
            size: 0,
            modified: None,
        }
    }

    // === FileEntry::is_parent tests ===

    #[test]
    fn is_parent_returns_true_for_dotdot() {
        let entry = FileEntry {
            name: "..".to_string(),
            path: PathBuf::from("/"),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
        };
        assert!(entry.is_parent());
    }

    #[test]
    fn is_parent_returns_false_for_regular_file() {
        assert!(!make_file("test.txt").is_parent());
    }

    #[test]
    fn is_parent_returns_false_for_directory() {
        assert!(!make_dir("subdir").is_parent());
    }

    // === FileEntry::extension tests ===

    #[test]
    fn extension_returns_ext_for_file() {
        assert_eq!(make_file("test.txt").extension(), Some("txt"));
        assert_eq!(make_file("script.py").extension(), Some("py"));
        assert_eq!(make_file("archive.tar.gz").extension(), Some("gz"));
    }

    #[test]
    fn extension_returns_none_for_no_extension() {
        assert_eq!(make_file("Makefile").extension(), None);
        assert_eq!(make_file("README").extension(), None);
    }

    #[test]
    fn extension_returns_none_for_directories() {
        let dir = make_dir("folder.d");
        assert_eq!(dir.extension(), None);
    }

    #[test]
    fn extension_returns_none_for_dotfiles() {
        assert_eq!(make_file(".gitignore").extension(), None);
        assert_eq!(make_file(".bashrc").extension(), None);
    }

    #[test]
    fn extension_handles_multiple_dots() {
        assert_eq!(make_file("file.test.rs").extension(), Some("rs"));
        assert_eq!(make_file("backup.2024.01.tar").extension(), Some("tar"));
    }

    // === FileEntry::icon_type tests ===

    #[test]
    fn icon_type_parent_dir() {
        let entry = FileEntry {
            name: "..".to_string(),
            path: PathBuf::from("/"),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
        };
        assert_eq!(entry.icon_type(), FileIcon::ParentDir);
    }

    #[test]
    fn icon_type_folder() {
        assert_eq!(make_dir("subdir").icon_type(), FileIcon::Folder);
    }

    #[test]
    fn icon_type_symlink() {
        assert_eq!(make_symlink("link").icon_type(), FileIcon::Symlink);
    }

    #[test]
    fn icon_type_code_files() {
        assert_eq!(make_file("main.rs").icon_type(), FileIcon::Code);
        assert_eq!(make_file("script.py").icon_type(), FileIcon::Code);
        assert_eq!(make_file("app.js").icon_type(), FileIcon::Code);
        assert_eq!(make_file("index.ts").icon_type(), FileIcon::Code);
        assert_eq!(make_file("main.go").icon_type(), FileIcon::Code);
        assert_eq!(make_file("Main.java").icon_type(), FileIcon::Code);
    }

    #[test]
    fn icon_type_text_files() {
        assert_eq!(make_file("readme.txt").icon_type(), FileIcon::Text);
        assert_eq!(make_file("README.md").icon_type(), FileIcon::Text);
        assert_eq!(make_file("CHANGELOG.markdown").icon_type(), FileIcon::Text);
    }

    #[test]
    fn icon_type_config_files() {
        assert_eq!(make_file("config.json").icon_type(), FileIcon::Config);
        assert_eq!(make_file("Cargo.toml").icon_type(), FileIcon::Config);
        assert_eq!(
            make_file("docker-compose.yaml").icon_type(),
            FileIcon::Config
        );
        assert_eq!(make_file("settings.yml").icon_type(), FileIcon::Config);
        assert_eq!(make_file("pom.xml").icon_type(), FileIcon::Config);
    }

    #[test]
    fn icon_type_image_files() {
        assert_eq!(make_file("photo.jpg").icon_type(), FileIcon::Image);
        assert_eq!(make_file("logo.png").icon_type(), FileIcon::Image);
        assert_eq!(make_file("icon.svg").icon_type(), FileIcon::Image);
        assert_eq!(make_file("animation.gif").icon_type(), FileIcon::Image);
    }

    #[test]
    fn icon_type_audio_files() {
        assert_eq!(make_file("song.mp3").icon_type(), FileIcon::Audio);
        assert_eq!(make_file("sound.wav").icon_type(), FileIcon::Audio);
        assert_eq!(make_file("music.flac").icon_type(), FileIcon::Audio);
    }

    #[test]
    fn icon_type_video_files() {
        assert_eq!(make_file("movie.mp4").icon_type(), FileIcon::Video);
        assert_eq!(make_file("clip.mkv").icon_type(), FileIcon::Video);
        assert_eq!(make_file("recording.webm").icon_type(), FileIcon::Video);
    }

    #[test]
    fn icon_type_archive_files() {
        assert_eq!(make_file("backup.zip").icon_type(), FileIcon::Archive);
        assert_eq!(make_file("source.tar").icon_type(), FileIcon::Archive);
        assert_eq!(make_file("data.gz").icon_type(), FileIcon::Archive);
        assert_eq!(make_file("package.7z").icon_type(), FileIcon::Archive);
    }

    #[test]
    fn icon_type_executable_files() {
        assert_eq!(make_file("run.sh").icon_type(), FileIcon::Executable);
        assert_eq!(make_file("install.bash").icon_type(), FileIcon::Executable);
        assert_eq!(make_file("setup.exe").icon_type(), FileIcon::Executable);
        assert_eq!(make_file("app.bin").icon_type(), FileIcon::Executable);
    }

    #[test]
    fn icon_type_unknown_extension() {
        assert_eq!(make_file("data.xyz").icon_type(), FileIcon::File);
        assert_eq!(make_file("unknown.asdf").icon_type(), FileIcon::File);
    }

    // === FileEntry::kind_description tests ===

    #[test]
    fn kind_description_parent() {
        let entry = FileEntry {
            name: "..".to_string(),
            path: PathBuf::from("/"),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
        };
        assert_eq!(entry.kind_description(), "Parent Directory");
    }

    #[test]
    fn kind_description_folder() {
        assert_eq!(make_dir("subdir").kind_description(), "Folder");
    }

    #[test]
    fn kind_description_symlink() {
        assert_eq!(make_symlink("link").kind_description(), "Symbolic Link");
    }

    #[test]
    fn kind_description_code_files() {
        assert_eq!(make_file("main.rs").kind_description(), "Rust Source");
        assert_eq!(make_file("app.py").kind_description(), "Python Script");
        assert_eq!(make_file("index.js").kind_description(), "JavaScript");
        assert_eq!(make_file("main.go").kind_description(), "Go Source");
    }

    #[test]
    fn kind_description_document_files() {
        assert_eq!(make_file("readme.txt").kind_description(), "Plain Text");
        assert_eq!(make_file("README.md").kind_description(), "Markdown");
        assert_eq!(make_file("data.json").kind_description(), "JSON");
        assert_eq!(make_file("report.pdf").kind_description(), "PDF Document");
    }

    #[test]
    fn kind_description_unknown() {
        assert_eq!(make_file("data.xyz").kind_description(), "File");
    }

    // === FileEntry::formatted_modified tests ===

    #[test]
    fn formatted_modified_with_date() {
        let entry = FileEntry {
            name: "test.txt".to_string(),
            path: PathBuf::from("/test.txt"),
            is_dir: false,
            is_symlink: false,
            size: 100,
            modified: Some(Utc.with_ymd_and_hms(2024, 6, 15, 14, 30, 0).unwrap()),
        };
        assert_eq!(entry.formatted_modified(), "2024-06-15 14:30");
    }

    #[test]
    fn formatted_modified_without_date() {
        let entry = make_file("test.txt");
        assert_eq!(entry.formatted_modified(), "—");
    }

    // === SortOrder tests ===

    #[test]
    fn sort_keeps_parent_first() {
        let mut entries = vec![
            make_file("zebra.txt"),
            FileEntry {
                name: "..".to_string(),
                path: PathBuf::from("/"),
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified: None,
            },
            make_file("apple.txt"),
        ];

        SortOrder::NameAsc.sort(&mut entries);

        assert_eq!(entries[0].name, "..");
    }

    #[test]
    fn sort_directories_before_files() {
        let mut entries = vec![
            make_file("zebra.txt"),
            make_dir("apple"),
            make_file("banana.txt"),
            make_dir("cherry"),
        ];

        SortOrder::NameAsc.sort(&mut entries);

        assert!(entries[0].is_dir);
        assert!(entries[1].is_dir);
        assert!(!entries[2].is_dir);
        assert!(!entries[3].is_dir);
    }

    #[test]
    fn sort_alphabetically_case_insensitive() {
        let mut entries = vec![
            make_file("Zebra.txt"),
            make_file("apple.txt"),
            make_file("BANANA.txt"),
        ];

        SortOrder::NameAsc.sort(&mut entries);

        assert_eq!(entries[0].name, "apple.txt");
        assert_eq!(entries[1].name, "BANANA.txt");
        assert_eq!(entries[2].name, "Zebra.txt");
    }

    #[test]
    fn sort_combined_rules() {
        let mut entries = vec![
            make_file("zebra.txt"),
            make_dir("docs"),
            FileEntry {
                name: "..".to_string(),
                path: PathBuf::from("/"),
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified: None,
            },
            make_file("apple.txt"),
            make_dir("src"),
        ];

        SortOrder::NameAsc.sort(&mut entries);

        // Order: parent, then dirs alphabetically, then files alphabetically
        assert_eq!(entries[0].name, "..");
        assert_eq!(entries[1].name, "docs");
        assert_eq!(entries[2].name, "src");
        assert_eq!(entries[3].name, "apple.txt");
        assert_eq!(entries[4].name, "zebra.txt");
    }

    #[test]
    fn sort_empty_list() {
        let mut entries: Vec<FileEntry> = vec![];
        SortOrder::NameAsc.sort(&mut entries);
        assert!(entries.is_empty());
    }

    #[test]
    fn sort_single_entry() {
        let mut entries = vec![make_file("only.txt")];
        SortOrder::NameAsc.sort(&mut entries);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "only.txt");
    }

    // === format_size tests ===

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1), "1 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(10240), "10.0 KB");
        assert_eq!(format_size(1024 * 1024 - 1), "1024.0 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 + 512 * 1024), "1.5 MB");
        assert_eq!(format_size(100 * 1024 * 1024), "100.0 MB");
    }

    #[test]
    fn format_size_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(
            format_size(2 * 1024 * 1024 * 1024 + 512 * 1024 * 1024),
            "2.5 GB"
        );
        assert_eq!(format_size(100 * 1024 * 1024 * 1024), "100.0 GB");
    }

    #[test]
    fn format_size_boundary_values() {
        // Just below KB threshold
        assert_eq!(format_size(1023), "1023 B");
        // Exactly KB
        assert_eq!(format_size(1024), "1.0 KB");
        // Just below MB threshold
        assert_eq!(format_size(1024 * 1024 - 1), "1024.0 KB");
        // Exactly MB
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        // Just below GB threshold
        assert_eq!(format_size(1024 * 1024 * 1024 - 1), "1024.0 MB");
        // Exactly GB
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    // === FileIcon equality tests ===

    #[test]
    fn file_icon_equality() {
        assert_eq!(FileIcon::Folder, FileIcon::Folder);
        assert_ne!(FileIcon::Folder, FileIcon::File);
        assert_ne!(FileIcon::Code, FileIcon::Text);
    }

    // === FileEntry equality tests ===

    #[test]
    fn file_entry_equality() {
        let entry1 = make_file("test.txt");
        let entry2 = make_file("test.txt");
        let entry3 = make_file("other.txt");

        assert_eq!(entry1, entry2);
        assert_ne!(entry1, entry3);
    }

    #[test]
    fn file_entry_clone() {
        let entry = FileEntry {
            name: "test.txt".to_string(),
            path: PathBuf::from("/test/test.txt"),
            is_dir: false,
            is_symlink: false,
            size: 1024,
            modified: Some(Utc::now()),
        };

        let cloned = entry.clone();
        assert_eq!(entry, cloned);
    }
}
