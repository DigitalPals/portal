# Integrated Hub implementation contract

This document records the integration boundaries for Portal Hub inside Portal.
The existing Hub implementation is reused to preserve API v2, Android pairing,
encrypted vault enrollment, sync, and SSH/session behavior. It remains a separate
headless executable and independently versioned container image.

## Repository layout

- Hub/runtime: `hub/`, workspace declarations and lockfile in root Cargo files.
- Deployment: `deploy/hub/`, including Dockerfile, Compose, management helper,
  and deployment helper tests.
- Desktop: `src/`, desktop tests, managed onboarding and SSH management transport.
- CI, release workflows and root documentation cover both workspace members.

## Runtime contract

- Workspace member `hub` is named `portal-hub`; desktop remains at repository root.
- Canonical existing API schemas live in `hub/contracts/portal-hub/v2`.
- Durable Hub state: `/var/lib/portal-hub`.
- Transient Hub state: `/run/portal-hub`, selected by `PORTAL_HUB_RUN_DIR`.
- Managed containers set `PORTAL_HUB_REQUIRE_BOOTSTRAP=true` to prohibit public
  first-owner registration, and `PORTAL_HUB_NETWORK_MODE=sidecar` for diagnostics.
- `portal-hub bootstrap-owner --json` reads one JSON object from stdin:
  `{"username":"...","password":"..."}`. It atomically establishes an owner,
  never overwrites an existing owner, and emits JSON with `owner_exists` and
  `created`. Repeating with the existing correct credentials is idempotent.
- `portal-hub bootstrap-status --json` emits `{"owner_exists":true|false}`.
- Normal OAuth/PKCE and existing API v2 remain the application authorization path.
- Container replacement terminates current sessions. Detect/reconcile stale
  runtime metadata, and require explicit maintenance consent for live sessions.

## Deployment protocol v1

`deploy/hub/portal_hub_deploy.py` is a self-contained Python 3 helper streamed by
Portal over verified SSH into `/usr/bin/python3 -I`. A fixed loader checks the
source length and SHA-256 against the authenticated command before executing
those same in-memory bytes. Root never executes an SSH-user-writable script or
imports modules from its directory. The helper then reads one JSON request from
the remaining stdin with `--request-stdin`.
No passwords, enrollment keys, or tokens may be embedded in shell command text,
Compose environment, saved deployment records, diagnostic logs, or errors.

Request fields: `protocol_version:1`, `action`, `deployment_id` (UUID), plus
action-specific fields. Default installation root is `/opt/portal-hub/<UUID>`.
All mutation is restricted to a validated, explicitly owned deployment record.
The helper emits one final JSON response on stdout (tool output goes to bounded,
redacted diagnostics):

```json
{"protocol_version":1,"ok":true,"phase":"awaiting_login","message":"Connect Tailscale","data":{}}
```

Errors use `ok:false`, `phase:"error"`, `message`, and a stable `error_code`.
Actions:

- `inspect`: read-only prerequisite and existing-deployment report. Data includes
  `docker_available`, `compose_available`, `tun_available`, `architecture`,
  `os_id`, `installed`, and blockers. Root/sudo is needed for mutations.
- `install`: idempotently prepare/start stack. Fields: `hostname`, `hub_image`,
  optional `image_archive` (uploaded tar) with `image_archive_sha256` (the hash of
  bytes uploaded by Portal), `install_docker` explicit boolean. The helper checks
  that checksum against its private frozen copy before Docker loads the image.
  The Hub image must use a pinned version or digest, never `latest`.
- `status`: report phase plus `auth_url`, `approval_url`, `public_url`,
  `owner_exists`, `tailscale_state`, `active_sessions`, and installed versions
  when available. Missing fields may be null. Phases include `not_installed`,
  `awaiting_login`, `awaiting_approval`, `awaiting_https`, `awaiting_owner`, `ready`.
- `configure_https`: configure private Serve only; return any required consent
  URL as `auth_url` and discovered `public_url`. Never enable Funnel. Do not
  expose Hub until owner bootstrap has completed.
- `claim`: fields `username`, `password`; execute the stdin bootstrap command as
  the Hub user and enable the private Serve endpoint only after success.
- `doctor`: structured actual application/network readiness, not just IP presence.
- `logs`: bounded redacted diagnostic text.
- `update`: new `hub_image` or optional `image_archive`; reject active sessions
  unless `allow_session_interruptions:true`. Preserve data and original image
  selection for rollback. Sidecar replacement must recreate dependents together.
- `backup`: produce a private durable backup path in the owned installation;
  stop/quiesce consistently and guard active sessions as for updates.
- `uninstall`: remove only owned service resources, retain durable data by default.
  Destructive data removal requires a separate explicit `delete_data:true`.

Implementations may add response fields compatibly. Keep this contract synchronized
with executable behavior and update both producer and consumer when fields change.

## Container design

Use official version-pinned Tailscale plus a separate nonroot Hub container,
`network_mode: service:tailscale`, kernel networking, TUN, and NET_ADMIN/NET_RAW
on the sidecar for the pinned image's legacy firewall backend. The Hub drops all
capabilities. Persist identity and application data separately. Enable tailnet DNS,
persist authentication with `TS_AUTH_ONCE=true`, provide enough boot time for
human login, and configure Serve declaratively. Publish no Hub ports and mount
neither the Docker socket nor Tailscale credentials into Hub. Use runtime tmpfs,
an init/reaper, supported amd64/arm64 images, and scheduled bounded log pruning.

## Desktop behavior

New setup and existing-Hub connection are separate choices. Reuse direct SSH
host definitions, verification, auth and jump hosts; never depend on the Hub
being installed to manage that Hub. Support root, passwordless sudo and prompted
sudo without saving/logging the sudo password. Maintain a local deployment UUID
and management-host reference for resumable setup and maintenance. Allow a local
image archive for testing/offline or unpublished builds.

Keep all endpoint, auth, sync and routing choices staged until setup and consent
succeed. Changing/cancelling a wizard must preserve the previous working Hub.
Capability discovery must not enable sync. Auth completion must not immediately
sync a not-yet-committed draft. Validate returned browser URLs and stale async
results. Provide visible status, retry, cancellation, diagnostics, and maintenance
controls. Ordinary existing-Hub login must continue to work.

## Verification boundaries

No external releases, production upgrades, or real tailnet enrollment are
authorized implicitly by implementation. Tests using temporary host resources
must use agent-resource (read the server policy first). Do not enroll existing
services into cleanup. Build/test Portal locally. Avoid broad concurrent Cargo
builds; focus on component checks and let the orchestrator run integrated checks.
