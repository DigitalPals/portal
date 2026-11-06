# Contributing

> Integrated checkout: this document preserves the standalone Hub guidance.
> Build the integrated service from the Portal root with `cargo build -p portal-hub`.
> For managed Docker/Tailscale setup and current runtime behavior, see [the integrated runtime contract](docs/integrated-runtime.md).

Portal Hub is pre-1.0. Keep changes conservative and operationally explicit.

Before opening a pull request:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Guidelines:

- Preserve API compatibility within the current `api_version`.
- Add tests for command parsing, JSON output, state handling, and log behavior.
- Document user-visible behavior changes in `CHANGELOG.md`.
- Treat session logs and SSH agent forwarding as security-sensitive areas.
