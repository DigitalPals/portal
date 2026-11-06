# Integrated runtime contract

The imported Hub retains API v2 and the existing desktop/Android OAuth, pairing,
vault and sync protocols. The CLI's legacy `api_version: 1` is unchanged.
Source provenance is in `../PROVENANCE.md`. Build from the Portal repository root
with `cargo build -p portal-hub`; minimum Rust is 1.88.

## Owner setup and sign-in

The owner signs in with passkeys only. There is no password sign-up, password
sign-in or public owner registration; `/auth/register` and `/auth/login` do not
exist. Passkeys belong to the host of the Hub's public HTTPS URL (its Tailscale
MagicDNS name in a managed deployment), so a Hub needs HTTPS, or a
`*.localhost` development name, before an owner can be created.

Every passkey registration starts from a one-time setup link that only the local
service administrator can create. Run it as the Hub service user through the
deployment's local Docker exec transport (not the Hub SSH forced-command
interface, which rejects it):

```sh
portal-hub owner-setup --json --username NAME          # new Hub: create the owner
portal-hub owner-setup --json --mode add               # another passkey
portal-hub owner-setup --json --mode reset             # recovery: replace all passkeys
portal-hub bootstrap-status --json
```

`owner-setup` prints `token`, `mode`, `username`, `expires_at` and, when
`PORTAL_HUB_PUBLIC_URL` is an HTTPS or `*.localhost` address, `setup_url`
(`<public URL>/setup#token=<token>`). Without `--json` it prints the link as a
sentence. The token is 256 bits, valid for 30 minutes and single use; the
database keeps only its SHA-256 hash, and creating a link invalidates any
earlier unused one.

- `setup` needs `--username` (2–64 letters, digits, `.`, `_`, `-`, `@`) when no
  owner exists. When an owner from an older password-based Hub exists without a
  passkey, it enrolls that owner's first passkey, keeps the account name, and
  erases the stored password hash. It fails once the owner has a passkey.
- `add` registers another passkey and keeps the existing ones.
- `reset` replaces every passkey and revokes all access tokens, refresh tokens
  and authorization codes, so every device signs in again with the new passkey.

`/setup` reads the token from the URL fragment, which browsers never send to the
server, and removes it from the address bar. If the page was opened with an
OAuth PKCE request (as Portal desktop does), finishing the registration returns
an authorization code to that client, so creating the passkey also signs the
client in. The link is consumed in the same SQLite transaction that stores the
passkey and, for a new Hub, creates the owner with its audit records.

`bootstrap-status` emits `{"owner_exists":bool,"owner_ready":bool}` and does not
create a database; `owner_ready` means the owner has at least one passkey. The
running server sees CLI changes without a restart. Ordinary sign-in at
`/oauth/authorize` uses the owner's passkey and then the existing OAuth/PKCE and
API v2 flows. Managed deployments set `PORTAL_HUB_REQUIRE_BOOTSTRAP=true`, which
makes `doctor` fail its `owner_bootstrap` check until the owner has a passkey.

## Durable and transient state

`PORTAL_HUB_STATE_DIR` defaults to `/var/lib/portal-hub`. Persist the database,
session metadata/logs, sync data and SSH known-host decisions there.

Managed containers must set `PORTAL_HUB_RUN_DIR=/run/portal-hub` and mount it as
private writable tmpfs owned by the Hub user. It contains `sockets/`, `ctl/`,
`web-identities/`, generation metadata and process locks. Temporary private keys
are mode 0600 and directories mode 0700. When the environment variable is unset,
manual/local installations use a dedicated `run/` subdirectory under state;
configure a tmpfs runtime directory for disk-free ephemeral keys.

Generation identity binds runtime metadata to the host boot and PID namespace
(including PID 1 start time). Deleted runtime storage produces a new generation.
Only matching-generation sessions with live sockets are active. Socket probes
are nonblocking; a full accept queue remains active so maintenance cannot discard
a busy session. Keep the runtime path short enough for Unix sockets (the default
container path fits). Startup
reconciles stale metadata, clears stale PIDs/socket paths and removes leftover
ephemeral keys, including old `web-identities/` files from standalone versions.
A five-second grace protects sessions currently being spawned from reconciliation.

Old metadata remains readable. Moving a standalone installation's live sessions
to the new runtime layout ends their usability; schedule downtime. Container
replacement terminates current sessions. Durable logs are retained according to
pruning policy; live sessions do not survive recreation.

Session deletion never signals process/group IDs loaded from disk. On Linux it
checks the live socket peer UID, locates the actual listener owner through
`/proc/net/unix` and process file descriptors, verifies its executable and socket
argument, and uses a pidfd to avoid signaling a reused PID. The recorded socket
peer PID alone is insufficient because dtach binds before daemonizing. Linux 5.3+
pidfd support and `/proc` are required for safe session termination. Failure to
verify process ownership fails closed. No root capability is needed.

## Diagnostics and lifecycle

`PORTAL_HUB_NETWORK_MODE=sidecar` makes `doctor --json` check Hub prerequisites
without requiring a local Tailscale executable. It still checks dtach, OpenSSH
client/key tools, the shell, script when logging is enabled, writable durable and
runtime directories, readable owner state, safe pidfd signaling and the actual
nonroot effective UID.
JSON includes `network_mode`, `run_dir`, `owner_exists`, `active_sessions` and
individual checks. Managed mode is not ready until an owner exists. Host mode
retains the Tailscale executable check. The deployment helper must additionally
verify the sidecar's login/approval, HTTPS Serve and actual application endpoint.

Set `PORTAL_HUB_PUBLIC_URL` to the discovered private HTTPS origin before normal
client use, so `/api/info` and Android QR pairing advertise the correct URL.
Changing this setting requires recreating/restarting Hub with the updated value;
the deployment helper must guard active sessions and obtain interruption consent.
Do not enable Serve before successful owner bootstrap. There is no need for a
Tailscale binary, socket or credential mount inside Hub.

`web` handles SIGTERM/SIGINT, stops new sessions, bounds connection draining to
five seconds, terminates verified detached sessions and clears ephemeral keys.
Use Docker `init: true` and a stop grace period of at least 15 seconds. The init
process reaps orphaned descendants; container teardown ultimately stops remaining
processes. No live-session continuity across recreation is promised.

The web process runs existing pruning at startup and hourly: ended sessions older
than 14 days are removed, ended logs are bounded by `PORTAL_HUB_MAX_LOG_BYTES`
(default 16 MiB), and existing live-log compaction continues. The local `prune`
command supports explicit retention, byte limits and dry runs. Configure bounded
Docker stdout/stderr rotation separately; do not install a root cron daemon in
the Hub container. Pause/stop the Hub for consistent filesystem backups.

The shared integration contract's public command and environment names are
unchanged. Deployment/CI integration must build `-p portal-hub`, retain the root
workspace lockfile, supply `/proc`, dtach/OpenSSH/util-linux, nonroot UID ownership,
runtime tmpfs and an init/reaper. Tests: `cargo test -p portal-hub` and
`cargo clippy -p portal-hub --all-targets -- -D warnings`.
