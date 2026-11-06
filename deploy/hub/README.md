# Managed Portal Hub deployment

This directory owns the headless Hub image, a reviewable Compose example, and
`portal_hub_deploy.py`, the self-contained Python 3.9+ deployment protocol helper.
The desktop streams the helper directly into an isolated Python interpreter over
verified SSH and optionally uploads a Docker image archive through SFTP.
The helper creates its own Compose and Serve files; it has no Python packages,
downloaded scripts, background host services, or additional asset dependencies.
The runtime contract is [integrated-runtime.md](../../hub/docs/integrated-runtime.md)
and the shared protocol is [hub-implementation-contract.md](../../docs/hub-implementation-contract.md).

## Images and building

The default is `ghcr.io/digitalpals/portal-hub:0.5.0-beta.17`. This is **unpublished**
until the coordinated release. For testing now, build and upload a local archive;
installation does not require a registry when the selected image is uploaded or
already exists in the remote Docker daemon.

From the repository root, in the existing CI environment or an `agent-resource`
lease when running temporary host workloads:

```sh
docker build -f deploy/hub/Dockerfile \
  --build-arg HUB_VERSION=0.5.0-beta.17 \
  -t ghcr.io/digitalpals/portal-hub:0.5.0-beta.17 .
docker save -o portal-hub-0.5.0-beta.17.tar \
  ghcr.io/digitalpals/portal-hub:0.5.0-beta.17
```

The Dockerfile supports native Linux amd64/arm64 builds. It builds the root
workspace lockfile with `cargo build --locked --release -p portal-hub`; it does
not build the desktop. The runtime includes OpenSSH client/key tools, dtach,
tmux, `script` (bsdutils), curl, CA roots and the Hub executable. UID/GID are
`10001:10001`. The Hub runs its own startup/hourly retention pruning; Docker's
`local` log driver limits each service's stdout/stderr to three 10 MiB files.

Images must use an exact semantic-version tag or sha256 digest. `latest`,
`stable`, untagged references, floating major/minor tags, whitespace and shell
syntax are rejected. The image must declare these OCI/custom labels:

```text
org.opencontainers.image.version=0.5.0-beta.17
io.portal.hub.api-version=2
io.portal.hub.deployment-protocol=1
```

The actual runtime version must match the image version label and report
`metadata_schema_version:2`. CLI `api_version:1` is intentionally retained;
the HTTP `/api/info` contract is version 2. Downgrades and versions older than
beta.17 are blocked. After resolution the selected reference, local content ID,
available repository digest, version and source are saved. Compose runs the
immutable content ID, so a retagged image cannot silently change an installation.
The helper never prunes images, preserving rollback IDs.

Uploaded archives must be **uncompressed `docker save` tar** files, at most
16 GiB, containing one image and only its selected version tag. Use the same
tag in `hub_image`; archive loading by a digest reference is not supported.
The helper opens every path component without following symlinks, copies the
upload into private storage, checks its SHA-256 against `image_archive_sha256`
sent through the authenticated SSH request, validates its manifest/member paths and loads the
selected image through `docker load` stdin. It constructs a private canonical
Docker-save tar containing only the selected manifest, configuration and layer
bytes from the authenticated copy. This supports native Docker saves that also
contain OCI metadata without forwarding unchecked `index.json`, `oci-layout`,
legacy `repositories` or unrelated blobs to the importer. Ambiguous/duplicate
paths, missing references, multiple manifest images/tags, links and traversal
are rejected before Docker sees them. It never extracts image layers on the
SSH host. Canonicalization requires room for a second private tar in addition
to the frozen upload; temporary copies are removed on normal exit/cancellation.
The uploaded source tar remains owned by the
desktop transport, which removes its staging directory after the operation.
Only the Hub image is imported; first installation still needs the pinned
Tailscale image already cached or accessible from Docker Hub.

Tailscale is the official `tailscale/tailscale:v1.104.4`, pinned additionally to
multiarch manifest
`sha256:ed6480b33c6eb67c9cbd203ed42b9fb57e361986725b6856def71b7a3d9e276c`.
On 2026-11-06 the GitHub release API confirmed v1.104.4 was stable, published
2026-10-28; Docker Hub's registry confirmed this digest and Linux amd64/arm64
manifests. Evidence endpoints:

- <https://github.com/tailscale/tailscale/releases/tag/v1.104.4>
- <https://hub.docker.com/r/tailscale/tailscale/tags?name=v1.104.4>
- Registry `GET /v2/tailscale/tailscale/manifests/v1.104.4` after anonymous
  pull-token authentication from `auth.docker.io`.

The newer GitHub release v1.104.5 had no matching published Docker Hub image
at verification time and was not selected. The pinned containerboot source
supports `TS_BOOT_TIMEOUT`, `TS_AUTH_ONCE`, `TS_ACCEPT_DNS`, `TS_SOCKET`, and
directory watching for `TS_SERVE_CONFIG`.

## Installation and isolation

Installations live only under `/opt/portal-hub/<canonical UUID>`, mode 0700.
The root-owned 0600 deployment record binds manager version, UUID, random owner
marker and exact Compose project name `portal-hub-<UUID>`. Every mutation
requires root/sudo; the helper does not obtain sudo credentials. Docker uses
the remote machine's `/var/run/docker.sock`, independent of inherited contexts
and Docker environment variables. Root Docker registry credentials may be used
through the normal root Docker configuration; none are collected by Portal.

A stable flock file under `/opt/portal-hub/.locks/<UUID>.lock` serializes each
deployment, including uninstall/reinstall. A concurrent call returns
`deployment_busy`; status can be retried after the active action finishes.
Inspect/status/doctor/logs create no directories. Every path component must be
root-owned and not writable by other users; managed files cannot be hardlinks
or symlinks. Existing directories without a matching ownership record are
refused. Actual Engine labels are checked on both containers and networks
before Docker exec, replacement, stop, backup or removal. Foreign project
resources cause `ownership_mismatch` and are never stopped/deleted.

Each deployment has separate private **bind volumes**:

| Path | Owner | Purpose |
| --- | --- | --- |
| `hub-data/` | 10001:10001, 0700 | Database, encrypted vault, sync, session logs/metadata, SSH trust |
| `tailscale-state/` | root, 0700 | Persistent machine identity/authentication |
| `serve/` | root, 0700 | Declarative private Serve config, mounted as a directory |
| `backups/` | root, 0700 | Private durable consistent archives, 0600 each |

Hub shares only the sidecar network namespace (`network_mode: service:tailscale`),
listens on `127.0.0.1:8080`, and receives neither Tailscale storage nor its
control socket. No host ports are published. No Docker socket is mounted in
either container. No Funnel is enabled. The sidecar uses kernel networking,
`/dev/net/tun`, `cap_drop:ALL` and only `NET_ADMIN` plus `NET_RAW`. The pinned
image uses iptables-legacy, whose raw socket operations require `NET_RAW` as
well as the network administration capability. These capabilities are granted
only to the sidecar; Hub still receives none. DNS acceptance and persistent
`TS_AUTH_ONCE=true` are explicit. It owns native login through containerboot,
with a 30-minute human-login boot timeout. Hub starts without depending on a
healthy/enrolled tailnet, so local bootstrap remains available.

Hub is nonroot, drops all capabilities, has a read-only root filesystem, and
uses private UID-10001 mode-0700 `/run/portal-hub` tmpfs plus writable `/tmp`
tmpfs. Both containers use init/reaping, restart policies, healthchecks, log
rotation and 30-second stop grace. Linux `/proc`, working pidfd support (kernel
5.3+) and a real TUN device are required. Compose 2.20+ is required. Inspection
checks the Docker daemon, Compose version, TUN, architecture and host pidfd.
Runtime doctor checks the actual container/service-user environment.

`compose.template.yaml` is JSON (valid YAML), a concrete security-review fixture
generated from the embedded `compose_spec()` function. Its sample UUID and
image ID are illustrative; use the helper to generate a real installation.
The tests enforce that this fixture stays identical to the embedded renderer.

## JSON protocol

Portal uses `/usr/bin/python3 -I` under root or sudo with a fixed in-memory loader
that verifies the streamed source length and SHA-256 before executing it. The
source is followed by one bounded UTF-8 JSON object and **stdin is closed**.
For manual use, place the helper in root-controlled storage and run
`python3 -I /root/portal_hub_deploy.py --request-stdin`. Never have sudo reopen a
helper stored in an SSH-user-writable directory. The
bootstrap CLI also needs EOF; the helper passes credentials through `docker
exec -i --user 10001:10001 ... portal-hub bootstrap-owner --json` and closes the
pipe. Do not put passwords, auth keys or tokens in shell command strings.

Every request starts with:

```json
{"protocol_version":1,"action":"status","deployment_id":"7bdf71ad-a270-440e-8ceb-590652b47293"}
```

Every response is one final JSON object on stdout, with exit status zero for
`ok:true`, one for `ok:false`. Subprocess output never goes directly to stdout.
Success has `protocol_version`, `ok`, `phase`, `message`, `data`; errors also
have a stable `error_code`. Input is capped at 16 KiB. Commands use argv arrays,
a fixed environment, deadlines, bounded continuously drained output, and
process-group termination on cancellation/timeout. Diagnostics are capped at
64 KiB and redact known credentials, secret fields, bearer tokens, private keys,
Tailscale auth keys, browser URLs, and terminal controls. Enrollment URLs appear
only in dedicated validated JSON fields. Browser URLs require HTTPS, port 443,
no credentials/fragments, and an exact `login.tailscale.com` or
`console.tailscale.com` host. Endpoint names come from native Self.DNSName and
must be actual `*.ts.net` FQDNs, rather than a guessed requested hostname.

| Action | Additional request fields | Behavior |
| --- | --- | --- |
| `inspect` | none | Read-only prerequisite/existing-record report, including blockers |
| `install` | `hostname`, `hub_image`, optional `image_archive`, `install_docker` | Prepare/start or resume the owned installation; default hostname `portal-hub-<UUID prefix>` and default beta.17 image |
| `status` | none | Native login/approval, owner, actual application/TLS readiness, versions, sessions and recovery state |
| `configure_https` | optional `allow_session_interruptions` | Discover actual origin, guard/recreate for environment changes, request HTTPS certificate, return consent URL, write Serve only for an owner |
| `claim` | `username`, `password`; optional `allow_session_interruptions` | Verify/bootstrap owner over stdin, then configure private HTTPS |
| `doctor` | none | Prerequisites, runtime doctor, local API, TLS API, ownership, namespace and operation-journal checks |
| `logs` | optional `lines` (1–500, default 200) | Bounded redacted service logs |
| `update` | `hub_image` and/or `image_archive`; optional `allow_session_interruptions` | Pinned image change, consistent pre-update snapshot, readiness verification and automatic rollback |
| `backup` | optional `allow_session_interruptions` | Stop both services, archive durable state consistently, resume previous running services |
| `uninstall` | optional `allow_session_interruptions`, `delete_data` | Remove only labelled owned containers/network; retain all files unless explicit boolean `delete_data:true` |

Every `image_archive` requires a lowercase 64-character `image_archive_sha256`.
Portal calculates it while uploading, so a server-side replacement fails before
Docker can load the archive. Booleans must be JSON booleans, and unknown request fields are rejected. Owner
names follow runtime rules (2–64 ASCII letters/digits/dot/underscore/dash/@),
and passwords must contain 12–256 UTF-8 bytes. Passwords are never persisted,
put in Compose/environment/argv, or returned in error strings. On install retry,
an archive for the unchanged image selection is ignored after the content ID
has been recorded and is still available; a missing image can be restored from
that archive or its recorded digest. Use `update` for any deliberate image change. Install also
accepts `allow_session_interruptions` when repairing an existing live stack.

Typical setup is `inspect`, `install`, then repeated `status` until native
Tailscale login/approval completes. `AuthURL` is read directly from `tailscale
status --json` even when the CLI exits nonzero during login. `NeedsMachineAuth`
produces `awaiting_approval` and the native URL or the Tailscale machines page.
For HTTPS, `configure_https` may return the Tailscale DNS admin consent URL; it
requests a certificate into sidecar tmpfs without publishing the application.
Return after consent, then `claim`. Claim may also precede HTTPS consent; the
owner remains durable and the endpoint stays closed until HTTPS configuration
succeeds. No Tailscale account password, API token or auth key is requested.

Phases are `not_installed`, `awaiting_login`, `awaiting_approval`,
`awaiting_https`, `awaiting_owner`, and `ready`; inspect/logs/backup use
`inspected`, `logs`, and `backed_up`. A stopped/starting/broken stack stays in
the applicable waiting phase with detailed readiness fields; it never becomes
`ready` solely because it has a tailnet IP. Ready requires owner existence,
local API v2, a Serve HTTPS configuration without Funnel, matching container
network namespaces, and a real certificate-validated request to the actual
Tailscale address returning API v2 and the correct advertised HTTPS origin.
Status also checks that the running Hub image/environment matches the record,
so cancellation between saving a new origin and applying Compose cannot bypass
the session guard on retry. An already matching installation with live sessions
does not call Compose up during an idempotent install retry.
This verifies the node endpoint from its namespace; client tailnet ACL/access
and human login are additional integration checks performed by the root agent.

Automatic Docker installation needs explicit `install_docker:true`. It supports
Debian 13 (`docker.io`, `docker-compose` v2) and Ubuntu 24.04 (`docker.io`,
`docker-compose-v2`) distro packages. It
uses isolated apt metadata and fixed official HTTPS archive sources with the
installed distro archive keyring (`signed-by`); unsigned metadata, untrusted
packages, third-party sources and local repository pinning are excluded. It
then enables Docker through systemd. Other distros, Debian 12, missing archive
keyrings and hosts without systemd require operator-provided Docker/Compose.
No curl-to-shell installer or external repository key is trusted implicitly.

## Maintenance, cancellation and recovery

**Container stop/replacement terminates live sessions.** Update, backup,
uninstall, origin changes and network-namespace repairs guard actual runtime
session lists. Active or unknown session state requires explicit
`allow_session_interruptions:true`. Pull/load can take time, so update rechecks
sessions immediately before stop. A currently stopped Hub has zero live
container sessions. Successful same-image update is a no-op unless repairing
a stale sidecar dependency. Sidecar recreation always replaces Hub together;
status/doctor compare actual `/proc/self/ns/net` identities to detect an
out-of-band restart/replacement, not merely matching container names.

Update first stops both services and creates a private `pre-update-*.tar.gz`.
It records the previous exact selection/ID in an atomic durable operation
journal before changing images. Failure before publication restores the prior application data
snapshot and image ID, recreates both containers, and reports
`update_rolled_back`. Candidate readiness is checked while Serve remains closed.
The helper durably records `update_committed` before opening Serve; after this
point recovery preserves the candidate and any newly accepted writes.
After publication, an update polls HTTPS for up
to 30 seconds with bounded status/curl requests, allowing containerboot to
observe the new Serve configuration. A persistently failed endpoint on a
Running sidecar reports `endpoint_not_ready` with its recovery journal retained;
retry install/configure HTTPS to repair networking without reverting accepted
data. Native login/approval remains a setup phase.
Initial installation and recovery inspect both exact image selections before
pulling, so a cached Hub and pinned Tailscale image support offline install.
Tailscale uses the same pinned image throughout and retains
its most recent identity state; application rollback does not revert rotated
machine keys. Failed rollback reports `rollback_failed`, retaining the snapshot
and journal for retry. Downgrades cannot be requested as ordinary updates.

Cancellation leaves durable data and an operation journal. Read-only status
returns `recovery_required:true`; it does not secretly restart anything. The
next mutation recovers interrupted installs, backups or updates before doing
new work; `install` is a convenient general retry. Recovery still guards any
new live sessions before stopping services. Rollback persists a
`rollback_restoring` phase, closes Serve, and stops both services before
restoring data. Restored files, directories and their final rename are fsynced;
then an atomic fsynced `rollback_committed` journal phase records the previous
image before either service resumes or Serve reopens. Recovery can repeat an
interrupted restoration while services remain stopped. After commit it only
resumes services: cancellation after Serve reopens cannot replay the snapshot
over newly accepted sync/vault writes. An already matching resumed stack is
left running, including when it has new live sessions. An interrupted
uninstall must be retried as uninstall.
Neither cancellation nor loss of the SSH connection removes the installation,
tailnet identity, or existing local Hub configuration in Portal. A package
manager operation already handed to systemd/the OS may finish independently.

Backups contain database/vault state and **Tailscale machine identity secrets**.
Keep them private and encrypted if copied off-host; never enroll two running
nodes from the same identity backup. Stop/quiesce is deliberate, including for
backups; there is no claim of preserving active sessions. Backups exclude
runtime tmpfs, sockets, device files and the backups directory itself; unexpected
links or mount points in durable data cause a safe failure. Backup creation
and restoration, and destructive uninstall, consult `/proc/self/mountinfo`
before traversing the deployment directory. The check decodes escaped mount
names and rejects mounts at or below that directory, including same-filesystem
directory and file bind mounts. Missing/unparseable mount information fails
closed. Ancestor mounts are allowed. Operators must not change mounts during
maintenance; this check does not serialize concurrent privileged mount changes.
Backup creation always attempts to resume the previously running services;
interrupted resumption is journaled for retry. Backups are retained until explicit data
removal/operator archival; no automatic destructive retention policy deletes
them.

For manual recovery, stop both services, keep the existing installation/backup
private, and restore `hub-data`, `tailscale-state`, and the desired private
configuration on the same owned host. Keep the root-owned ownership record;
the helper intentionally provides no arbitrary-path extract/restore action.
Use the recorded immutable image ID or saved image archive, then retry install.
Do not restore stale runtime sockets or migrate a live Tailscale identity.

Default uninstall retains the record, volumes, identity, backups, and image
selection for reinstall. `delete_data:true` additionally removes only the
validated UUID directory, refusing nested mount points. It never removes
images, unrelated deployments, Docker itself or the Tailscale machine from the
tailnet administration console. A zero-byte private flock file may remain
under `.locks/` to prevent races between uninstall and a new operation.

## Verification and handoff

Run from the repository root (no containers are created):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s deploy/hub/tests -v
python3 -m py_compile deploy/hub/portal_hub_deploy.py
docker compose --project-directory / -f "$PWD/deploy/hub/compose.template.yaml" config --quiet
```

Tests use a stateful mocked Engine boundary and actual temporary files/modes,
flocks, atomic records and tar streams. Root UID/chown are simulated so tests
need no root, daemon, TUN, login or credentials. Fixtures cover native nonzero
login JSON, approval, HTTPS consent, owner bootstrap and no pre-owner Serve,
real endpoint checks, private backups, archive attacks, logs/secret redaction,
ownership collisions, idempotency, session guards, same-image repair, data plus
image rollback, and cancellation/recovery. Separate process tests exercise
bounded streaming, stdin transport and deadline/process-group cleanup.
Regression coverage includes conflicting OCI/Docker archive metadata,
same-filesystem directory/file mounts with escaped names, rollback interruption
before/after restoration commit and Serve reopening, delayed/permanently failed
HTTPS, and cached-image offline installation. Engine/network behavior is mocked;
real image imports, sidecar capabilities and mount namespaces need the leased
runtime checks owned by the root agent.

The orchestrator owns `scripts/test-hub-container.py`, `scripts/test-hub-ssh.py`,
`tests/docker/managed-hub/`,
CI/release integration, and real container/tailnet smoke tests. This deployment
implementation does not itself publish images, enroll a tailnet, create a
release, or start temporary host resources. Coordinate the real image build,
nonroot doctor/bootstrap verification, Tailscale enrollment and final dev
service restart with that root agent.
