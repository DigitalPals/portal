# Integrated runtime contract

The imported Hub retains API v2 and the existing desktop/Android OAuth, pairing,
vault and sync protocols. The CLI's legacy `api_version: 1` is unchanged.
Source provenance is in `../PROVENANCE.md`. Build from the Portal repository root
with `cargo build -p portal-hub`; minimum Rust is 1.88.

## Owner setup

Managed deployments set `PORTAL_HUB_REQUIRE_BOOTSTRAP=true`. Public
`POST /auth/register` then returns HTTP 403, including legacy password setup.
`/admin` directs the user to complete local setup. An unset, empty, `false` or `0`
value preserves legacy/manual registration. Other nonempty values fail closed.

Run these commands as the Hub service user using the deployment's local Docker
exec transport (not the Hub SSH forced-command interface):

```sh
portal-hub bootstrap-status --json
portal-hub bootstrap-owner --json
```

`bootstrap-owner` reads one stdin JSON object containing `username` and `password`
and waits for EOF. Maximum input is 8192 bytes. Account names use the existing
2–64 character rules; passwords must be 12–256 bytes. Never put credentials in
argv, environment variables, Compose files, diagnostic logs or deployment records.

Success is `{"owner_exists":true,"created":true}`. Repeating correct existing
credentials returns `created:false`. Different credentials, a passwordless legacy
owner, and invalid input fail with nonzero status; they never replace an owner or
set an existing password. Errors contain no supplied credentials. An immediate
SQLite transaction arbitrates CLI/HTTP/process races and commits the owner plus
audit record together. The service directory is private to its filesystem owner;
that local service-user/root access is the bootstrap authorization boundary.
No secret bootstrap HTTP endpoint exists.

Status emits `{"owner_exists":false}` or `true`; it does not create a database.
After bootstrap, the already-running server sees the owner without a restart.
Ordinary sign-in, tokens and encrypted vault enrollment still use API v2.

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
