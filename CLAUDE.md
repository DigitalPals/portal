# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Portal is a cross-platform SSH client with GUI built in Rust using the Iced framework. It features terminal emulation (via alacritty_terminal), SFTP file browsing with dual-pane interface, host management with groups, and dark/light themes.

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
├── terminal/           # Custom Iced widget using alacritty_terminal
└── views/              # UI views (host_grid, sidebar, terminal_view, sftp/, dialogs/)
```

### Data Flow Example: SSH Connection

1. User selects host -> `HostMessage::Connect(Uuid)`
2. `handle_host()` calls `Portal::connect_to_host()`
3. Async SSH connection spawned, emits events via mpsc channel
4. Host key verification dialog if needed -> `DialogMessage::HostKeyAccept`
5. On success -> `SessionMessage::Connected` creates terminal and tab
6. Terminal receives data via `SessionMessage::Data`

### Configuration

Config stored in platform-specific directory (`~/.config/portal/` on Linux):
- `hosts.toml` - SSH host definitions with groups
- `snippets.toml` - Command snippets
- `history.toml` - Connection history
- `settings.toml` - Theme, font size
- `known_hosts` - SSH host key storage

## Key Patterns

- **Single-threaded UI with async backend**: Tokio for I/O, communication via messages
- **Task<Message> return**: Handlers return Tasks for async work
- **Shared SFTP connection pool**: Connections reused across dual panes
- **Toast notifications**: Non-blocking error/info display via `views/toast.rs`
- **Transient status messages**: Auto-expire after 3 seconds
- **Responsive layout**: Sidebar auto-collapses below 800px width

## Technical Details

- **Rust Edition**: 2024, MSRV 1.85
- **GUI**: Iced 0.13
- **Terminal**: alacritty_terminal 0.24
- **SSH**: russh 0.54, russh-sftp 2.0
- **Async**: Tokio (full features)
