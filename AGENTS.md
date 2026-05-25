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
- `direnv` is expected. If the environment is not active, run `nix develop`.
- Do not install missing build tools globally; add project-specific tools and libraries to `flake.nix`.
- Common commands: `cargo test`, `./run.sh build`, `./run.sh check`.

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
  - Portal releases are created by pushing the release version to the `release`
    branch.
  - Portal Hub releases are created by pushing a `v*` tag.
  - Portal Android currently has CI but no GitHub release workflow in this repo.
- Decide whether a new release is needed before waiting on CI. If a release is
  needed, bump versions first, then run local focused checks, then push once so
  CI validates the final release candidate commit.
- Before triggering release workflows, wait for CI on the final pushed heads to
  pass. For Portal Hub, include both `CI` and `Contract compatibility`.
- If a workflow fails and logs are not accessible, reproduce it in a fresh
  sibling checkout layout that matches GitHub Actions:
  `/tmp/portal-ci-repro/{portal,portal-hub,portal-android}`.
- Keep cross-repo CI dependency parity in mind. The Portal Hub contract workflow
  compiles Portal desktop tests, so it needs the same Linux packages as Portal
  CI: `pkg-config`, `libxkbcommon-dev`, `libwayland-dev`, `libvulkan-dev`,
  `libglib2.0-dev`, and `libdbus-1-dev`.
- When polling long GitHub Actions jobs, check job-level status after the first
  wait so it is clear whether the run is queued, building, stuck, or failed.
- Do not trigger GitHub releases until release-candidate CI is green and human
  testing on the Portal Hub LXC has been completed when applicable.
