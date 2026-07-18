# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Portal is a Linux-first SSH and VNC client with GUI built in Rust using the Iced framework. It features terminal emulation (via alacritty_terminal) for remote and local shells, SFTP file browsing with dual-pane interface, VNC remote desktop with GPU-accelerated rendering, a built-in file viewer (code/images/PDF/markdown), host management with groups, an encrypted key vault, and optional Portal Hub integration (persistent proxied sessions + profile sync).

The Portal ecosystem spans three repos: this desktop client, Portal Hub (`~/Code/portal-hub`, the sync/proxy service), and Portal Android (`~/Code/portal-android`). See `AGENTS.md` for cross-repo workflow, remote builds ("The Beast"), and the release preflight checklist.

## Build Commands

```bash
nix develop      # enter the project shell if direnv is not active
./run.sh build   # Build release binary
./run.sh run     # Build and run release (default, uses nix-shell for Wayland)
./run.sh dev     # Build and run debug
./run.sh check   # Run cargo check and clippy
cargo test --lib # Run unit tests (integration tests need Docker, see tests/)
```

`direnv` is configured through `.envrc`, so an allowed checkout should enter the Nix shell automatically. Add project-specific tools and native libraries to `flake.nix` instead of installing them globally. CI enforces `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`, and `cargo audit` (see `.github/workflows/test.yml`).

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
    Session(SessionMessage),   // SSH/local/proxy terminal sessions
    Sftp(SftpMessage),         // SFTP browser operations
    FileViewer(FileViewerMessage), // Built-in file viewer tabs
    Dialog(DialogMessage),     // Modal dialogs
    Tab(TabMessage),           // Tab management
    Host(HostMessage),         // Host operations
    History(HistoryMessage),   // Connection history
    Snippet(SnippetMessage),   // Command snippets
    Vnc(VncMessage),           // VNC remote desktop sessions
    ProxySessions(ProxySessionsMessage), // Portal Hub sessions dashboard
    Vault(VaultMessage),       // Encrypted key vault
    Ui(UiMessage),             // UI state changes
    Noop,
}
```

Each message variant dispatches to its handler in `src/app/update/` (keyboard shortcuts and settings live under `src/app/update/ui/`).

### Domain Managers

State is split into specialized managers (fields on Portal struct), all in `src/app/managers/`:

- **SessionManager**: terminal sessions, maps SessionId -> ActiveSession. A session's `backend` is `SessionBackend::Ssh` (russh), `::Local` (portable-pty), or `::Proxy` (Portal Hub WebSocket)
- **SftpManager**: SFTP tabs, dual-pane state, shared connection pool
- **DialogManager**: enforces the single-active-dialog constraint (`ActiveDialog` enum: Host, HostKey, AuthPrompt, PasswordPrompt, PassphrasePrompt, About, ...)
- **FileViewerManager**, **TransferManager** (SFTP transfer queue/progress), **ProxySessionsManager** (Hub dashboard), **SnippetExecutionManager**
- **VNC sessions** (`Portal::vnc_sessions`): `HashMap<SessionId, VncActiveSession>` with framebuffer, FPS tracking, and transport (`via`) state

### Key Module Organization

```
src/
├── app.rs              # Portal struct, initialization, view, subscriptions
├── message.rs          # Nested message enums
├── app/
│   ├── managers/       # Domain managers (see above)
│   ├── update/         # Message handlers (session.rs, sftp.rs, vnc.rs, ui/, ...)
│   ├── services/       # Reusable connect/history/file-viewer task builders
│   └── actions.rs      # High-level action handlers (connect_to_host, close_tab, ...)
├── config/             # TOML config: hosts (+ssh_config import), snippets, history, settings
├── ssh/                # russh client: auth flows, agent, known_hosts, ProxyJump chains
│                       # (tunnel.rs), port forwards (local/remote/dynamic SOCKS5),
│                       # auto-reconnect (reconnect.rs), connection_pool
├── sftp/               # SFTP client wrapping russh-sftp (recursive ops, transfers)
├── local/              # Local terminal sessions via portable-pty
├── proxy/              # Portal Hub proxied terminal sessions (WebSocket)
├── hub/                # Portal Hub client: OAuth (auth.rs), sync, encrypted vault,
│                       # vault enrollment, diagnostics
├── terminal/           # Custom Iced widget on alacritty_terminal: selection with
│                       # edge auto-scroll, scrollback search (search.rs), clickable
│                       # links (links.rs), session logging (logger.rs)
├── vnc/                # VNC client: session, framebuffer, wgpu widget, keysym
│                       # mapping, encodings, quality/stats tracking
├── views/              # UI: host_grid, sidebar, tabs, terminal_view, sftp/, vnc_view,
│                       # settings_page, vault_page, command_palette, history_view,
│                       # proxy_sessions, file_viewer/, dialogs/, toast
├── widgets/            # Small reusable widgets (e.g. animated_width)
├── keybindings.rs      # 10 rebindable shortcut actions (defaults + parser)
├── security_log.rs     # Security audit log (auth, host keys, agent forwarding,
│                       # Hub key provisioning)
├── fs_utils.rs         # Hardened local filesystem helpers
└── validation.rs       # Host/port/username input validation
```

### Data Flow Example: SSH Connection

1. User selects host -> `HostMessage::Connect(Uuid)`
2. `handle_host()` calls `Portal::connect_to_host()`
3. Async SSH connection spawned, emits events via mpsc channel
4. Host key verification dialog if needed -> `DialogMessage::HostKeyAccept`
5. On success -> `SessionMessage::Connected` creates terminal and tab
6. Terminal receives data via `SessionMessage::Data`

Hosts with `HubRouting::Hub` (or Auto with the Hub default on) instead spawn a `proxy::ProxySession` over a Portal Hub WebSocket; key-file/vault hosts send their private key to the Hub at session start (see `proxy_private_key`, logged via `security_log`, warned about in the host dialog).

### Data Flow Example: VNC Connection

1. User selects VNC host -> `HostMessage::Connect(Uuid)`
2. `handle_host()` detects VNC protocol; password comes from a vault secret or a prompt dialog
3. `Portal::connect_vnc_host_with_password()` spawns `VncSession::connect()` (direct TCP, or a `direct-tcpip` channel when `vnc_via_ssh_host_id` is set)
4. On success -> `VncMessage::Connected` creates `VncActiveSession` and tab
5. Framebuffer updates arrive event-driven with frame-arrival throttling
6. Mouse/keyboard events forwarded via `VncMessage::MouseEvent` / `VncMessage::KeyEvent`
7. The toolbar always shows transport state: "via <host>" when tunneled, a clickable "Unencrypted" warning chip otherwise

### Configuration

Config stored in platform-specific directory (`~/.config/portal/` on Linux), written atomically with 0600 permissions:
- `hosts.toml` - SSH and VNC host definitions with groups, tags, port forwards, jump hosts, Hub routing
- `snippets.toml` / `snippet_history.toml` - Command snippets and execution history
- `history.toml` - Connection history
- `settings.toml` - Theme (6 built-in), fonts/metrics, scroll speed, keybindings, VNC settings, Portal Hub settings, reconnect policy, session/security logging
- `known_hosts` - SSH host key storage (supports `@revoked` / `@cert-authority`)
- `hub_vault.json` - Encrypted vault blobs (XChaCha20-Poly1305, Argon2id; unlock secret in the OS keychain)

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

**Scrollback Search** (`src/terminal/search.rs`, Ctrl+Shift+F):
- Literal whole-buffer search with case toggle, next/prev with wrap, match counter

**Clickable Links (Ctrl+hover / Ctrl+click)**:
- Detection in `src/terminal/links.rs`: regex scan of the visible viewport for URLs and file paths (`src/foo.rs:123`, `/abs/path`, `~/file`, `./rel`, bare `name.ext`), plus explicit OSC 8 hyperlinks on cells
- Holding Ctrl underlines the link under the cursor; Ctrl+click opens it (works even when the TUI enables mouse reporting)
- URLs open in the default browser (http/https/mailto only); file paths open in the built-in file viewer, scrolled to the `:line` suffix
- Remote paths open over a fresh SFTP channel on the session's existing SSH connection (registered in the SFTP pool so viewer Save works; dropped when the viewer tab closes); relative paths resolve against the shell's OSC 7 cwd (`TerminalEvent::CwdChanged`), falling back to the home directory
- Portal Hub proxy sessions show a "not supported yet" toast for file links

**Scrollback**:
- Mouse wheel scrolling through terminal history
- Trackpad pixel-perfect smooth scrolling
- Configurable mouse wheel / trackpad scroll speed via `terminal_scroll_speed` in settings
- Scroll to bottom on user input

### VNC Framebuffer Rendering

The VNC widget uses a custom wgpu shader (`src/vnc/widget.rs`) with a `FrameBuffer` (`src/vnc/framebuffer.rs`) holding BGRA pixels. The `prepare()` method uploads dirty regions to the GPU texture.

**Important invariant**: `FrameBuffer::new()` and `FrameBuffer::resize()` must NOT mark the framebuffer as dirty. Their pixels are all-black placeholders — uploading them causes a black flash before real server data arrives. Instead, `prepare()` detects texture dimension mismatches and forces a full upload of the current pixel buffer when recreating the GPU texture, ensuring the texture always has valid content.

## Key Patterns

- **Single-threaded UI with async backend**: Tokio for I/O, communication via messages
- **Task<Message> return**: Handlers return Tasks for async work
- **Shared SFTP connection pool**: Connections reused across dual panes and link-opened viewers
- **Secrets**: `secrecy::SecretString` for passwords/passphrases/keys; never serialized to config, never logged; security-relevant events go to `security_log`
- **Toast notifications**: Non-blocking error/info display via `views/toast.rs`
- **Transient status messages**: Auto-expire after 3 seconds
- **Responsive layout**: Sidebar auto-collapses below 800px width
- **Headless UI tests**: `iced_test::simulator` drives real pointer/keyboard events against views and asserts published messages (see `src/views/terminal_view.rs`, `src/views/tabs.rs` tests)

## Technical Details

- **Rust Edition**: 2024, MSRV 1.88
- **GUI**: Iced 0.14 (`iced_widget` vendored with local patches, see `[patch.crates-io]`)
- **Terminal**: alacritty_terminal 0.26, portable-pty 0.8 (local shells)
- **SSH**: russh 0.61, russh-sftp 2.3
- **VNC**: vnc-rs 0.5 (vendored: ARD auth, Tight/TRLE/ZRLE encodings)
- **Async**: Tokio (full features)
- **Hub**: reqwest + tokio-tungstenite (rustls), keyring for tokens/vault secret

## Release Process

Releases are triggered by pushing a `v*` tag whose version matches `Cargo.toml` (there is no separate release branch). Follow the preflight checklist in `AGENTS.md` before tagging (fmt, audit, `nix build` when deps changed, green CI on main).

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

3. **Create a release tag** (only after CI on main is green):
   ```bash
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```

4. **Automated CI/CD** (`.github/workflows/release.yml`) will:
   - Build for Linux (x86_64, arm64)
   - Create DEB, RPM, tarball, and x86_64 AppImage packages
   - Push Nix build to Cachix
   - Add `SHA256SUMS` for release asset verification
   - Create GitHub release with tag `vX.Y.Z`

### Release Artifacts

| Platform | Artifacts |
|----------|-----------|
| Linux x86_64 | `.tar.gz`, `.deb`, `.rpm`, `.AppImage` |
| Linux arm64 | `.tar.gz`, `.deb`, `.rpm` |
