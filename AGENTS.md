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
