# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Portal is a cross-platform SSH and VNC client with GUI built in Rust using the Iced framework. It features terminal emulation (via alacritty_terminal), SFTP file browsing with dual-pane interface, VNC remote desktop with GPU-accelerated rendering, host management with groups, and dark/light themes.

## Build Commands

```bash
./run.sh build   # Build release binary
./run.sh run     # Build and run release (default, uses nix-shell for Wayland)
./run.sh dev     # Build and run debug
./run.sh check   # Run cargo check and clippy
cargo test       # Run tests
```

## Architecture

### Elm-Style Model-View-Update

The application follows the Elm architecture pattern:
- **Model**: `Portal` struct in `src/app.rs`
- **View**: `Portal::view()` renders UI
- **Update**: `Portal::update()` dispatches messages to specialized handlers

### Nested Message System

Messages are organized hierarchically in `src/message.rs`:

```rust
pub enum Message {
    Session(SessionMessage),   // SSH terminal sessions
    Sftp(SftpMessage),         // SFTP browser operations
    Dialog(DialogMessage),     // Modal dialogs
    Tab(TabMessage),           // Tab management
    Host(HostMessage),         // Host operations
    History(HistoryMessage),   // Connection history
    Snippet(SnippetMessage),   // Command snippets
    Vnc(VncMessage),           // VNC remote desktop sessions
    Ui(UiMessage),             // UI state changes
    Noop,
}
```

Each message variant dispatches to its handler in `src/app/update/`.

### Domain Managers

State is split into specialized managers (fields on Portal struct):

- **SessionManager** (`src/app/managers/session_manager.rs`): SSH terminal sessions, maps SessionId -> ActiveSession
- **SftpManager** (`src/app/managers/sftp_manager.rs`): SFTP tabs, dual-pane state, connection pool
- **DialogManager** (`src/app/managers/dialog_manager.rs`): Enforces single-dialog constraint, manages Host/Settings/Snippets/HostKey dialogs
- **VNC sessions** (`Portal::vnc_sessions`): `HashMap<SessionId, VncActiveSession>` for active VNC connections, including framebuffer state, FPS tracking, and scaling mode

### Key Module Organization

```
src/
├── app.rs              # Portal struct, initialization, view, subscriptions
├── message.rs          # Nested message enums
├── app/
│   ├── managers/       # SessionManager, SftpManager, DialogManager
│   ├── update/         # Message handlers (session.rs, sftp.rs, dialog.rs, etc.)
│   └── actions.rs      # High-level action handlers (connect_to_host, etc.)
├── config/             # TOML-based configuration (hosts, snippets, history, settings)
├── ssh/                # russh-based SSH client, auth, host key verification
├── sftp/               # SFTP client wrapping russh-sftp
├── terminal/           # Custom Iced widget using alacritty_terminal with auto-scroll during text selection
├── vnc/                # VNC client: session, framebuffer, widget (wgpu shader), keysym mapping, quality tracking, monitor discovery
└── views/              # UI views (host_grid, sidebar, terminal_view, sftp/, vnc_view, dialogs/)
```

### Data Flow Example: SSH Connection

1. User selects host -> `HostMessage::Connect(Uuid)`
2. `handle_host()` calls `Portal::connect_to_host()`
3. Async SSH connection spawned, emits events via mpsc channel
4. Host key verification dialog if needed -> `DialogMessage::HostKeyAccept`
5. On success -> `SessionMessage::Connected` creates terminal and tab
6. Terminal receives data via `SessionMessage::Data`

### Data Flow Example: VNC Connection

1. User selects VNC host -> `HostMessage::Connect(Uuid)`
2. `handle_host()` detects VNC protocol, opens password dialog
3. After password entry -> `Portal::connect_vnc_host_with_password()`
4. Async task spawns `VncSession::connect()` (TCP + VNC/ARD auth)
5. On success -> `VncMessage::Connected` creates `VncActiveSession` and tab
6. Event loop polls framebuffer updates on `VncMessage::RenderTick`
7. Mouse/keyboard events forwarded via `VncMessage::MouseEvent` / `VncMessage::KeyEvent`

### Configuration

Config stored in platform-specific directory (`~/.config/portal/` on Linux):
- `hosts.toml` - SSH and VNC host definitions with groups
- `snippets.toml` - Command snippets
- `history.toml` - Connection history
- `settings.toml` - Theme, font size, VNC settings (encoding, color depth, refresh rate, scaling mode)
- `known_hosts` - SSH host key storage

### Terminal Widget Features

The terminal widget (`src/terminal/widget.rs`) is a custom Iced component built on `alacritty_terminal`:

**Text Selection**:
- Single-click: character-by-character selection
- Double-click: word-by-word selection
- Triple-click: line-by-line selection
- **Auto-scroll on edge**: When dragging selection near viewport edges (30px zone), terminal automatically scrolls up/down to reveal more content
  - Scroll speed: 1-3 lines based on proximity to edge
  - Throttled at 50ms intervals to prevent performance issues
  - Respects alternate screen mode (no auto-scroll in vim, htop, etc.)

**Clipboard Integration**:
- Ctrl+Shift+C / Ctrl+Insert: copy selected text
- Ctrl+Shift+V / Shift+Insert: paste from clipboard
- Ctrl+Shift+A: select all visible content

**Scrollback**:
- Mouse wheel scrolling through terminal history
- Trackpad pixel-perfect smooth scrolling
- Scroll to bottom on user input

### VNC Framebuffer Rendering

The VNC widget uses a custom wgpu shader (`src/vnc/widget.rs`) with a `FrameBuffer` (`src/vnc/framebuffer.rs`) holding BGRA pixels. The `prepare()` method uploads dirty regions to the GPU texture.

**Important invariant**: `FrameBuffer::new()` and `FrameBuffer::resize()` must NOT mark the framebuffer as dirty. Their pixels are all-black placeholders — uploading them causes a black flash before real server data arrives. Instead, `prepare()` detects texture dimension mismatches and forces a full upload of the current pixel buffer when recreating the GPU texture, ensuring the texture always has valid content.

## Key Patterns

- **Single-threaded UI with async backend**: Tokio for I/O, communication via messages
- **Task<Message> return**: Handlers return Tasks for async work
- **Shared SFTP connection pool**: Connections reused across dual panes
- **Toast notifications**: Non-blocking error/info display via `views/toast.rs`
- **Transient status messages**: Auto-expire after 3 seconds
- **Responsive layout**: Sidebar auto-collapses below 800px width

## Technical Details

- **Rust Edition**: 2024, MSRV 1.85
- **GUI**: Iced 0.14
- **Terminal**: alacritty_terminal 0.25
- **SSH**: russh 0.56, russh-sftp 2.1
- **VNC**: vnc-rs 0.5 (vendored, with ARD authentication support)
- **Async**: Tokio (full features)

## Release Process

### Branch Structure

- **`main`** — Development branch for ongoing work
- **`release`** — Stable release branch, triggers CI/CD builds

### Creating a Release

1. **Update version** in `Cargo.toml`:
   ```toml
   version = "X.Y.Z"
   ```

2. **Commit and push to main**:
   ```bash
   git add Cargo.toml
   git commit -m "chore: bump version to X.Y.Z"
   git push origin main
   ```

3. **Merge to release branch**:
   ```bash
   git checkout release
   git merge main
   git push origin release
   ```

4. **Automated CI/CD** (`.github/workflows/release.yml`) will:
   - Build for Linux (x86_64, arm64) and macOS (x86_64, arm64)
   - Create DEB, RPM, AppImage, and tarball packages
   - Push Nix build to Cachix
   - Create GitHub release with tag `vX.Y.Z`

### Release Artifacts

| Platform | Artifacts |
|----------|-----------|
| Linux x86_64 | `.tar.gz`, `.deb`, `.rpm`, `.AppImage` |
| Linux arm64 | `.tar.gz`, `.deb`, `.rpm` |
| macOS x86_64 | `.app.zip` |
| macOS arm64 | `.app.zip` |
