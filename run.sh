#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Default wgpu backend on Linux (override by setting WGPU_BACKEND)
if [[ "$(uname)" != "Darwin" ]]; then
  export WGPU_BACKEND="${WGPU_BACKEND:-vulkan,gl}"
fi

# Prefer the host Rust toolchain so locally built binaries use the host loader.
# Fall back to the Nix development shell when Cargo is not installed directly.
run_cargo() {
  if command -v cargo >/dev/null 2>&1; then
    cargo "$@"
  elif [[ -x "${HOME}/.cargo/bin/cargo" ]]; then
    "${HOME}/.cargo/bin/cargo" "$@"
  elif command -v nix >/dev/null 2>&1; then
    nix develop --command cargo "$@"
  else
    echo "Cargo is unavailable. Install the pinned Rust toolchain or enter the Nix development shell." >&2
    exit 127
  fi
}

case "${1:-run}" in
build)
  echo "Building Portal..."
  run_cargo build --release
  echo "Build complete: target/release/portal"
  ;;
run)
  echo "Building and running Portal..."
  run_cargo run --release
  ;;
dev)
  echo "Running Portal in dev mode (fast release build)..."
  export PORTAL_VNC_DEBUG="${PORTAL_VNC_DEBUG:-1}"
  export PORTAL_VNC_COLOR_DEPTH="${PORTAL_VNC_COLOR_DEPTH:-16}"
  run_cargo run --profile dev-release
  ;;
check)
  echo "Checking Portal..."
  run_cargo check
  run_cargo clippy --all-targets -- -D warnings
  ;;
*)
  echo "Usage: $0 {build|run|dev|check}"
  echo "  build  - Build release binary"
  echo "  run    - Build and run release (default)"
  echo "  dev    - Build and run debug"
  echo "  check  - Run cargo check and clippy"
  exit 1
  ;;
esac
