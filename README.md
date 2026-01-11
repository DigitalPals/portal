<div align="center">

<img src="assets/logo.png" alt="portal" width="75%" />

# Portal

**A modern, fast SSH client for macOS and Linux**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![macOS](https://img.shields.io/badge/macOS-000000?logo=apple&logoColor=white)](https://github.com/DigitalPals/portal/releases)
[![AppImage](https://img.shields.io/badge/AppImage-x86__64-blue)](https://github.com/DigitalPals/portal/releases)
[![DEB](https://img.shields.io/badge/DEB-Debian%2FUbuntu-A81D33)](https://github.com/DigitalPals/portal/releases)
[![RPM](https://img.shields.io/badge/RPM-Fedora%2FRHEL-294172)](https://github.com/DigitalPals/portal/releases)
[![Tarball](https://img.shields.io/badge/Tarball-manual-grey)](https://github.com/DigitalPals/portal/releases)

![Portal Screenshot](assets/screenshots/hosts.png)

</div>

---

Portal is a native SSH client built for speed and simplicity. Manage your servers with an intuitive interface that's equally comfortable with keyboard shortcuts or mouse navigation. Built with Rust for native performance, with full Wayland support on Linux.

## Highlights

**`>_` Multi-Tab Terminal** — Manage multiple SSH sessions in tabs. Switch between servers instantly without juggling windows.

**`< >` Dual-Pane SFTP** — Browse local and remote files side by side. Drag, drop, copy, and manage files with ease.

**`{ }` Smart OS Detection** — Automatically detects 20+ operating systems and displays branded icons for Ubuntu, Debian, Arch, Fedora, and more.

**`#!` Built-in File Viewer** — View code with syntax highlighting, preview images and PDFs, edit markdown—all without leaving Portal.

**`::` Beautiful Themes** — Choose from 5 built-in themes including the popular Catppuccin palette in both light and dark variants.

**`/>` Command Snippets** — Save frequently used commands and insert them into any session with a click. Never retype complex commands.

## Features

### Terminal

- **Multi-tab sessions** — Open multiple SSH connections in tabs
- **Local terminal** — Launch local shell sessions alongside remote connections
- **Adjustable font size** — Scale from 6px to 20px for your preference
- **SSH key installation** — Install your public key on remote servers with `Ctrl+Shift+K`
- **Status bar** — See hostname and connection duration at a glance
- **Session history** — Quick reconnect to recent servers

### SFTP File Browser

- **Dual-pane interface** — Local filesystem on one side, remote on the other
- **File operations** — Copy, rename, delete, and change permissions
- **Hidden files toggle** — Show or hide dotfiles with one click
- **Quick filter** — Search files in the current directory
- **Breadcrumb navigation** — Click any part of the path to jump there
- **Context menus** — Right-click for common actions

### Host Management

- **Host groups** — Organize servers into folders
- **Quick connect** — Type `user@hostname` to connect instantly
- **Search & filter** — Find hosts as you type
- **Connection history** — See when you last connected and for how long
- **OS detection** — Automatic identification with branded icons for:
  - Ubuntu, Debian, Fedora, Arch, CentOS, RHEL
  - openSUSE, NixOS, Manjaro, Linux Mint, Pop!_OS
  - Gentoo, Alpine, Kali, Rocky, AlmaLinux
  - macOS, FreeBSD, OpenBSD, NetBSD, Windows

### File Viewer

- **Syntax highlighting** — Support for 20+ languages including Rust, Python, JavaScript, Go, and more
- **Image viewer** — View PNG, JPG, GIF, WebP, SVG with zoom controls
- **PDF viewer** — Read PDF documents with page navigation
- **Markdown preview** — Toggle between edit and rendered preview
- **In-app editing** — Make quick edits without leaving Portal

### Customization

- **5 built-in themes**
  - Portal Default
  - Catppuccin Latte (light)
  - Catppuccin Frappé (dark)
  - Catppuccin Macchiato (dark)
  - Catppuccin Mocha (dark)
- **Responsive layout** — Sidebar auto-collapses on narrow windows
- **Keyboard-first** — Full keyboard navigation support

![SFTP Browser](assets/screenshots/sftp.png)

## Installation

### Download

Download from the Releases page:
https://github.com/DigitalPals/portal/releases

Pick the file that matches your OS and CPU:

| Platform | Asset name |
|----------|------------|
| macOS (Apple Silicon) | `portal-*-macos-arm64.app.zip` |
| macOS (Intel) | `portal-*-macos-x86_64.app.zip` |
| Linux (arm64) | `portal-*-linux-arm64.deb` / `.rpm` / `.tar.gz` |
| Linux (x86_64) | `portal-*-linux-x86_64.AppImage` / `.deb` / `.rpm` / `.tar.gz` |

### Install on macOS

1. Download the matching `.app.zip` for your architecture from Releases.
2. Extract and move `Portal.app` to your Applications folder.

If macOS blocks the app, open **System Settings > Privacy & Security** and click **Open Anyway** for Portal, then run it again.

### Install on Linux

Pick one of these options:

**AppImage (x86_64):**
```bash
chmod +x portal-*-linux-x86_64.AppImage
./portal-*-linux-x86_64.AppImage
```

**DEB (Debian/Ubuntu):**
```bash
sudo dpkg -i portal-*-linux-*.deb
```

**RPM (Fedora/RHEL):**
```bash
sudo rpm -i portal-*-linux-*.rpm
```

**Tarball (manual):**
```bash
tar -xzf portal-*-linux-*.tar.gz
sudo mv portal /usr/local/bin/
portal
```

Portal is built for Wayland. If it does not start, install your distro's Wayland/XKB/Vulkan runtime packages and try again.

### Build from Source

Requires Rust 1.85 or later.

```bash
git clone https://github.com/DigitalPals/portal.git
cd portal
./run.sh build
./run.sh run
```

**Build commands:**

```bash
./run.sh build   # Build release binary
./run.sh run     # Build and run release
./run.sh dev     # Build and run debug
./run.sh check   # Run cargo check and clippy
```

## Operations

Portal logs to the console and to a daily rotating file in the config logs
directory. The default log level is INFO in debug builds and WARN in release
builds. Override it with `RUST_LOG`.

Environment variables:

- `PORTAL_LOG_DIR` (optional) - set a custom log directory. Set to an empty
  string to disable file logging.
- `PORTAL_MAX_COMMAND_OUTPUT_BYTES` (optional) - cap command output collected
  from SSH exec calls. Default: 4194304 (4 MiB).

Example:

```bash
RUST_LOG=portal=info PORTAL_LOG_DIR=/var/log/portal ./portal
```

### NixOS / Nix Flakes

Portal is available as a Nix flake with binaries cached on [Cachix](https://app.cachix.org/cache/digitalpals).

**Run directly:**

```bash
nix run github:DigitalPals/portal/release
```

**Install in NixOS configuration** (`flake.nix`):

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Use the release branch for stable builds with cachix cache hits
    # Don't use inputs.nixpkgs.follows for portal - it breaks cachix
    portal.url = "github:DigitalPals/portal/release";
  };

  outputs = { nixpkgs, portal, ... }: {
    nixosConfigurations.yourhostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          # Enable cachix for pre-built binaries
          nix.settings.substituters = [ "https://digitalpals.cachix.org" ];
          nix.settings.trusted-public-keys = [ "digitalpals.cachix.org-1:YWuWBw08EbEeTsIccpPfRTaqksfo4QtAVQaTRljYFm8=" ];

          environment.systemPackages = [ portal.packages.${pkgs.system}.default ];
        })
      ];
    };
  };
}
```

> **Note:** Do not add `inputs.nixpkgs.follows = "nixpkgs"` to the portal input. This changes the derivation hash and prevents cachix from providing pre-built binaries.

> **Note:** The `release` branch contains stable releases. The `main` branch may contain unreleased development changes.

**Build from source:**

```bash
nix build
./result/bin/portal
```

## Quick Start

1. **Launch Portal** — Run the application
2. **Add a host** — Click the `+` button and enter your server details
3. **Connect** — Double-click a host card or press Enter to connect

That's it. You're in.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+K` | Install SSH public key on remote server |
| `Ctrl+Tab` | Switch to next tab |
| `Ctrl+Shift+Tab` | Switch to previous tab |
| `Ctrl+W` | Close current tab |

## Built With

- [Rust](https://www.rust-lang.org/) — Systems programming language
- [Iced](https://iced.rs/) — Cross-platform GUI framework
- [Alacritty Terminal](https://github.com/alacritty/alacritty) — Terminal emulation
- [Russh](https://github.com/warp-tech/russh) — SSH protocol implementation

## Configuration

Portal stores configuration in your platform's config directory:

- **Linux:** `~/.config/portal/`
- **macOS:** `~/Library/Application Support/portal/`

Configuration files:
- `hosts.toml` — Saved host definitions
- `snippets.toml` — Command snippets
- `snippet_history.toml` — Snippet execution history (`enabled`, `store_command`, `store_output`, `redact_output`)
- `settings.toml` — Theme and font preferences
- `history.toml` — Connection history
- `known_hosts` — SSH host key storage

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<div align="center">

**[Report Bug](../../issues)** · **[Request Feature](../../issues)**

</div>
