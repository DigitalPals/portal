//! SVG icons embedded at compile time
//!
//! Icons sourced from:
//! - UI icons: Lucide (https://lucide.dev) - ISC License
//! - OS logos: Simple Icons (https://simpleicons.org) - CC0 License

use iced::widget::svg::{Handle, Svg};
use iced::{Color, Length};

/// UI icons for sidebar and general interface
pub mod ui {
    pub const SERVER: &[u8] = include_bytes!("../assets/icons/ui/server.svg");
    pub const HARD_DRIVE: &[u8] = include_bytes!("../assets/icons/ui/hard-drive.svg");
    pub const CODE: &[u8] = include_bytes!("../assets/icons/ui/code.svg");
    pub const HISTORY: &[u8] = include_bytes!("../assets/icons/ui/history.svg");
    pub const SETTINGS: &[u8] = include_bytes!("../assets/icons/ui/settings.svg");
    pub const CHEVRON_LEFT: &[u8] = include_bytes!("../assets/icons/ui/chevron-left.svg");
    pub const CHEVRON_RIGHT: &[u8] = include_bytes!("../assets/icons/ui/chevron-right.svg");
    pub const CHEVRON_DOWN: &[u8] = include_bytes!("../assets/icons/ui/chevron-down.svg");
    pub const PLUS: &[u8] = include_bytes!("../assets/icons/ui/plus.svg");
    pub const FOLDER_CLOSED: &[u8] = include_bytes!("../assets/icons/ui/folder-closed.svg");
    pub const TERMINAL: &[u8] = include_bytes!("../assets/icons/ui/terminal.svg");
    pub const REFRESH: &[u8] = include_bytes!("../assets/icons/ui/refresh-cw.svg");
    pub const X: &[u8] = include_bytes!("../assets/icons/ui/x.svg");
    pub const ALERT_TRIANGLE: &[u8] = include_bytes!("../assets/icons/ui/alert-triangle.svg");
    pub const CHECK: &[u8] = include_bytes!("../assets/icons/ui/check.svg");
    pub const PANEL_LEFT_CLOSE: &[u8] = include_bytes!("../assets/icons/ui/panel-left-close.svg");
    pub const PANEL_LEFT_OPEN: &[u8] = include_bytes!("../assets/icons/ui/panel-left-open.svg");
    pub const MENU: &[u8] = include_bytes!("../assets/icons/ui/menu.svg");
    pub const PENCIL: &[u8] = include_bytes!("../assets/icons/ui/pencil.svg");
}

/// File type icons for SFTP browser
pub mod files {
    pub const FOLDER: &[u8] = include_bytes!("../assets/icons/files/folder.svg");
    pub const FILE: &[u8] = include_bytes!("../assets/icons/files/file.svg");
    pub const FILE_CODE: &[u8] = include_bytes!("../assets/icons/files/file-code.svg");
    pub const FILE_TEXT: &[u8] = include_bytes!("../assets/icons/files/file-text.svg");
    pub const IMAGE: &[u8] = include_bytes!("../assets/icons/files/image.svg");
    pub const MUSIC: &[u8] = include_bytes!("../assets/icons/files/music.svg");
    pub const VIDEO: &[u8] = include_bytes!("../assets/icons/files/video.svg");
    pub const ARCHIVE: &[u8] = include_bytes!("../assets/icons/files/archive.svg");
    pub const FILE_JSON: &[u8] = include_bytes!("../assets/icons/files/file-json.svg");
    pub const FILE_COG: &[u8] = include_bytes!("../assets/icons/files/file-cog.svg");
}

/// Operating system and distribution logos
pub mod os {
    // Linux distributions
    pub const LINUX: &[u8] = include_bytes!("../assets/icons/os/linux.svg");
    pub const UBUNTU: &[u8] = include_bytes!("../assets/icons/os/ubuntu.svg");
    pub const DEBIAN: &[u8] = include_bytes!("../assets/icons/os/debian.svg");
    pub const FEDORA: &[u8] = include_bytes!("../assets/icons/os/fedora.svg");
    pub const ARCH: &[u8] = include_bytes!("../assets/icons/os/arch.svg");
    pub const CENTOS: &[u8] = include_bytes!("../assets/icons/os/centos.svg");
    pub const REDHAT: &[u8] = include_bytes!("../assets/icons/os/redhat.svg");
    pub const OPENSUSE: &[u8] = include_bytes!("../assets/icons/os/opensuse.svg");
    pub const NIXOS: &[u8] = include_bytes!("../assets/icons/os/nixos.svg");
    pub const MANJARO: &[u8] = include_bytes!("../assets/icons/os/manjaro.svg");
    pub const MINT: &[u8] = include_bytes!("../assets/icons/os/mint.svg");
    pub const POPOS: &[u8] = include_bytes!("../assets/icons/os/popos.svg");
    pub const GENTOO: &[u8] = include_bytes!("../assets/icons/os/gentoo.svg");
    pub const ALPINE: &[u8] = include_bytes!("../assets/icons/os/alpine.svg");
    pub const KALI: &[u8] = include_bytes!("../assets/icons/os/kali.svg");
    pub const ROCKY: &[u8] = include_bytes!("../assets/icons/os/rocky.svg");
    pub const ALMA: &[u8] = include_bytes!("../assets/icons/os/alma.svg");

    // Other operating systems
    pub const APPLE: &[u8] = include_bytes!("../assets/icons/os/apple.svg");
    pub const WINDOWS: &[u8] = include_bytes!("../assets/icons/os/windows.svg");
    pub const FREEBSD: &[u8] = include_bytes!("../assets/icons/os/freebsd.svg");
    pub const OPENBSD: &[u8] = include_bytes!("../assets/icons/os/openbsd.svg");
    pub const NETBSD: &[u8] = include_bytes!("../assets/icons/os/netbsd.svg");
    pub const UNKNOWN: &[u8] = include_bytes!("../assets/icons/os/unknown.svg");
}

/// Create an SVG icon widget with specified size and color
pub fn icon_with_color(data: &'static [u8], size: u16, color: Color) -> Svg<'static> {
    Svg::new(Handle::from_memory(data))
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .style(move |_theme, _status| iced::widget::svg::Style {
            color: Some(color),
        })
}
