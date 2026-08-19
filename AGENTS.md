# Agent Notes

## Portal Ecosystem Context

Keep the whole Portal ecosystem in mind when making product, architecture, protocol,
or compatibility decisions:

- Portal (`~/Code/portal`) is the desktop client.
- Portal Hub (`~/Code/portal-hub`) is the main hub for SSH proxying, the key vault,
  host storage, and related shared services.
- Portal Android (`~/Code/portal-android`) is the Android Portal client and works
  only with Portal Hub.

## Development Environment

- Run commands from the repository root.
- Build and test Portal on the local machine; do not use a remote builder.
- `run.sh` prefers the host rustup Cargo toolchain and falls back to `nix develop`
  when Nix is available.
- `direnv` uses the Nix flake when available and otherwise exposes the rustup
  toolchain from `~/.cargo/bin`.
- Do not install missing build tools globally; add project-specific tools and libraries to `flake.nix`.
- Common commands: `cargo test`, `./run.sh build`, `./run.sh check`.

## Local Binary Builds

- Use `./run.sh build` for the release binary at `target/release/portal`.
- Use direct Cargo commands for other build profiles when needed.
- Linux builds vendor libdbus through Cargo so this machine does not need the
  distro `dbus-devel` package.

## Human Testing Before Release

- The default Portal Hub human-testing target is the LXC at `root@10.10.0.13`.
- Before creating a new GitHub release for changes that affect Portal Hub behavior,
  Android pairing, authorization, vault access, SSH proxying, sync, or cross-client
  compatibility, test the feature on this LXC after automated checks pass.
- Use SSH to access the LXC. Treat it as the staging Portal Hub environment for
  release validation, not as a place for untracked source changes.

## Commit, Push, CI, and Release Procedure

When asked to commit, push, verify CI, and create releases across the Portal
ecosystem, use this order to avoid long feedback loops:

- Start by checking `gh auth status`. If GitHub CLI auth is broken, say so
  immediately and use Git over SSH plus public GitHub API polling where possible;
  note that private logs and workflow reruns may be unavailable until auth is
  fixed.
- Inspect all relevant worktrees first: `~/Code/portal`, `~/Code/portal-hub`,
  and `~/Code/portal-android`. Commit only intentional files and leave generated
  state such as `.direnv/` alone.
- Identify release triggers before pushing release changes:
  - Portal releases are created by pushing a `v*` tag whose version matches
    `Cargo.toml`.
  - Portal Hub releases are created by pushing a `v*` tag.
  - Portal Android currently has CI but no GitHub release workflow in this repo.
- Decide whether a new release is needed before waiting on CI. If a release is
  needed, bump versions first, then run local focused checks, then push once so
  CI validates the final release candidate commit.
- For Portal release candidates, validate the release-only paths before pushing
  the release tag:
  - Run `cargo fmt -- --check` and the same security audit command used by CI
    before pushing. `./run.sh check` alone does not cover these CI blockers:
    `cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195`.
  - Run `nix build --print-build-logs` whenever `flake.nix`, `flake.lock`,
    `Cargo.lock`, `.github/workflows/release.yml`, or Rust dependencies changed.
    This build is slow because it compiles release artifacts and runs release-mode
    tests, but it catches Nix vendoring and Cachix failures before a tag triggers
    the full GitHub release.
  - If `Cargo.lock` or `nixpkgs` changes and `flake.nix` uses `cargoHash`, update
    the hash locally by first using `pkgs.lib.fakeHash`, then replace it with the
    hash reported by Nix.
  - Treat GitHub Actions deprecation warnings in release workflows as blockers
    while preparing a release. Update actions before tagging so failures do not
    appear after the expensive binary builds have already run.
- Before triggering release workflows, wait for CI on the final pushed heads to
  pass. For Portal Hub, include both `CI` and `Contract compatibility`.
- For Portal, create and push the `v*` tag only after the final `main` CI run is
  green. If a tag workflow fails before a GitHub release is created, fix `main`,
  verify CI again, delete and recreate the tag on the corrected commit, then
  re-run the release.
- For Portal Hub LXC validation on `root@10.10.0.13`:
  - Run remote shell snippets with `bash -lc`; the default shell may be `fish`.
  - Do not assume `rsync` exists on the LXC. To sync the committed Hub source
    while preserving remote `target/`, use a tracked-file tar stream such as
    `git ls-files -z | tar --null -T - -czf - | ssh root@10.10.0.13 'bash -lc "mkdir -p /root/Code/portal-hub && tar -xzf - -C /root/Code/portal-hub"'`.
  - Use `/usr/local/bin/portal-hub version`; `--version` is not supported.
  - Run `portal-hub doctor` as the `portal-hub` service user, not as root:
    `runuser -u portal-hub -- /usr/local/bin/portal-hub doctor`.
- If a workflow fails and logs are not accessible, reproduce it in a fresh
  sibling checkout layout that matches GitHub Actions:
  `/tmp/portal-ci-repro/{portal,portal-hub,portal-android}`.
- Keep cross-repo CI dependency parity in mind. The Portal Hub contract workflow
  compiles Portal desktop tests, so it needs the same Linux packages as Portal
  CI: `pkg-config`, `libxkbcommon-dev`, `libwayland-dev`, `libvulkan-dev`,
  and `libglib2.0-dev`.
- When polling long GitHub Actions jobs, check job-level status after the first
  wait so it is clear whether the run is queued, building, stuck, or failed.
- Portal release jobs can legitimately take 30 minutes or more on a cold cache.
  Poll job-level status and name the slow job explicitly; do not assume the run
  is stuck until the job stops advancing or hits its workflow timeout.
- Do not trigger GitHub releases until release-candidate CI is green and human
  testing on the Portal Hub LXC has been completed when applicable.
