# Imported Portal Hub

Imported from the tracked tree of `~/Code/portal-hub`, upstream
`https://github.com/DigitalPals/portal-hub`, commit
`c50184ed28151e635e3ef61dd3c36c00d3073011` (version `0.5.0-beta.16`).
The original checkout is unchanged. `LICENSE` retains the original MIT license.

Included: service source, tests, API v2 schemas, documentation, examples and
ecosystem compatibility check scripts. Omitted: Git metadata, generated state,
standalone Cargo lockfile, standalone Nix environment and upstream workflows.
The root workspace lockfile and integrated CI/deployment own those concerns.

Reuse assessment: the existing implementation already covers the desktop and
Android OAuth/PKCE flows, encrypted vault enrollment, sync and SSH sessions.
There is no blocker requiring a rewrite or a shared client/server crate.
The integrated candidate is independently versioned as `0.5.0-beta.17`.
API v2 schemas and the legacy CLI API version remain compatible.

See `docs/integrated-runtime.md` for the additive runtime behavior.
