# Portal Hub

> Portal Hub is developed, versioned and released together with Portal in this
> repository; the former standalone repository is retired. Portal installs and
> manages the Hub for you (Settings → Portal Hub → Set up on a server). Build the
> service from the repository root with `cargo build -p portal-hub`. Current
> runtime behavior: [the integrated runtime contract](docs/integrated-runtime.md).

Portal Hub is the always-on companion service for Portal. Run it on a small
Linux host or LXC in your tailnet, then let Portal use it for persistent SSH
sessions, browser sign-in, device sync, session previews, and encrypted key
vault sync.

It is built for the practical homelab and ops workflow: start a terminal from
Portal, close your laptop, switch networks, come back later, and reconnect to
the same shell.

Status: beta. The core flows are working and versioned, but the project is
still pre-1.0 and API or storage details may change before a stable release.

## What It Does

| Feature | What you get |
| --- | --- |
| Persistent SSH sessions | Remote shells keep running on the Hub after Portal disconnects. |
| Session resume | Reopen active sessions from Portal, including sessions detached from another device. |
| Terminal thumbnails | Portal can show session previews from Hub replay logs. |
| Web terminal transport | Portal connects through an OAuth-authenticated WebSocket instead of relying only on SSH forced-command mode. |
| Passkey sign-in | The owner signs in with a passkey in the browser (OAuth authorization code + PKCE); Portal stores tokens in the OS keychain. There are no passwords. |
| Profile sync | Hosts, settings, snippets, and vault metadata sync between Portal devices. |
| Encrypted key vault sync | Private keys are stored as Portal-encrypted blobs; Hub never receives the vault passphrase or decrypted keys. |
| Tailscale Serve support | Use a clean HTTPS `.ts.net` URL while the Hub service binds safely to loopback. |
| Operational tooling | `doctor`, `version --json`, `list`, `prune`, target allowlists, bounded logs, and systemd units. |

## How It Works

Portal signs in to the Hub web service with OAuth + PKCE. For persistent
terminal sessions, Portal opens an authenticated WebSocket and asks Hub to start
or attach to a target SSH session. Hub runs that target session inside `dtach`,
records a bounded replay log with `script`, and reconnects Portal to the same
session later. Web clients can resume without replaying stored output, while
the replay log remains available for thumbnails and explicit history features.

Typing `exit` in the remote shell ends the real target session. Closing Portal,
losing Wi-Fi, or moving to another machine only detaches the Portal client.

Portal Hub also keeps a revisioned sync store for Portal data:

- Hosts, settings, and snippets are stored as readable JSON in the Hub state
  directory.
- Vault entries are stored as encrypted blobs produced by Portal.
- Sync v2 tracks each service independently and exposes an SSE event stream so
  other Portal clients can refresh quickly after a change.

The legacy OpenSSH forced-command mode is still available for manual operation
and older clients; see
[the deployment guide](docs/deployment.md#optional-legacy-ssh-forced-command).

## Install

Let Portal install it: in Portal, open Settings → Portal Hub, choose **Set up on
a server**, pick an SSH host and select **Deploy Hub**. Portal installs Docker
when needed, runs the Hub image of its own version with a private Tailscale
sidecar, and asks you to sign in to Tailscale and create your owner passkey in
the browser. The steps and requirements are in the
[Portal README](../README.md#portal-hub-beta); how the managed installation
works is in [the deployment protocol](../deploy/hub/README.md).

To run the Hub yourself, follow the [manual deployment guide](docs/deployment.md)
and connect Portal with **Connect an existing Hub**.

A small server or LXC is enough for personal use: 1 vCPU, 512 MB to 1 GB RAM,
10 GB disk. Disk is the main resource to watch because replay logs power
reconnect previews and thumbnails.

The `install-debian.sh` installer and `hub-v*` releases of the retired
standalone repository are not part of this repository and are unsupported.

## Security Model

Portal Hub is designed for Tailscale-only access.

- Do not expose the Hub SSH port or web service to the public internet.
- Use Tailscale ACLs to restrict who can reach the Hub host.
- Run Hub as a dedicated non-root user (the managed deployment does this for
  you).
- Keep `/var/lib/portal-hub` private to the Hub user.
- Treat terminal replay logs as sensitive; they can contain command output,
  tokens, passwords, environment values, and pasted secrets.
- Use `PORTAL_HUB_ALLOWED_TARGETS` on shared Hub hosts to restrict where Hub can
  connect.
- Keep the legacy SSH forced-command entry restricted if you enable it.

Portal Hub can sync encrypted vault blobs, but it must never receive the vault
passphrase, derived key, or decrypted private keys. Android vault enrollment
keeps that model: Hub stores an Android device public key and an encrypted
unlock-key envelope produced by Portal desktop, but not the plaintext unlock
key.

## Session Logs And Retention

Hub stores terminal output in:

```text
/var/lib/portal-hub/logs
```

By default, live logs use a 16 MiB moving window. Older replay output may be
discarded, but the target session keeps running.

Useful commands:

```sh
sudo -u portal-hub portal-hub list --active --include-preview --format v1
sudo -u portal-hub portal-hub prune --dry-run
sudo -u portal-hub portal-hub prune --ended-older-than-days 14 --max-log-bytes 16777216
```

Disable replay logging if you do not want terminal output stored:

```text
PORTAL_HUB_LOGGING_MODE=disabled
```

Sessions still persist across disconnects, but replay and thumbnails are not
available.

## Operations

Common commands:

```sh
portal-hub doctor
portal-hub doctor --json
portal-hub version --json
portal-hub web --bind 127.0.0.1:8080 --public-url https://portal-hub.example.ts.net
portal-hub list --active --include-preview --format v1
portal-hub sync get --format v1
portal-hub prune --dry-run
```

Common environment variables:

```text
PORTAL_HUB_STATE_DIR=/var/lib/portal-hub
PORTAL_HUB_PUBLIC_URL=https://portal-hub.example.ts.net
PORTAL_HUB_MAX_LOG_BYTES=16777216
PORTAL_HUB_LOGGING_MODE=full
PORTAL_HUB_ALLOWED_TARGETS=*.internal,192.0.2.0/24
```

`PORTAL_HUB_ALLOWED_TARGETS` supports exact hostnames, `*` wildcard patterns,
and IP CIDR ranges.

## API And Integrations

Portal Hub exposes versioned CLI JSON and OAuth-authenticated web APIs:

- `GET /api/info`
- `GET /api/me`
- `GET /api/sync`
- `PUT /api/sync`
- `GET /api/sync/v2`
- `PUT /api/sync/v2`
- `GET /api/sync/v2/events`
- `GET /api/sessions`
- `GET /api/sessions/terminal`

See [docs/api.md](docs/api.md) for request and response details.

## Current Limitations

- SSH terminal sessions are supported; browser/PWA SSH is not part of this
  release.
- Legacy forced-command target authentication depends on SSH agent forwarding.
- Replay logs are bounded, so older terminal output can be trimmed while the
  target session continues running.
- The project is pre-1.0 and may still make breaking changes.

## More Docs

- [Manual deployment guide](docs/deployment.md)
- [Managed deployment protocol](../deploy/hub/README.md)
- [Integrated runtime contract](docs/integrated-runtime.md)
- [API contract](docs/api.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
