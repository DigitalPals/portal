#!/bin/bash
set -e

cd "$(dirname "$0")"

# Default wgpu backend on Linux (override by setting WGPU_BACKEND)
if [[ "$(uname)" != "Darwin" ]]; then
  export WGPU_BACKEND="${WGPU_BACKEND:-vulkan,gl}"
fi

# Use nix develop on Linux, direct cargo on macOS
run_cargo() {
  if [[ "$(uname)" == "Darwin" ]]; then
    cargo "$@"
  else
    nix develop --command cargo "$@"
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
remote-build)
  exec scripts/remote-build.sh --fetch build
  ;;
remote-release)
  exec scripts/remote-build.sh --fetch release
  ;;
remote-check)
  exec scripts/remote-build.sh check
  ;;
remote-test)
  exec scripts/remote-build.sh test
  ;;
*)
  echo "Usage: $0 {build|run|dev|check|remote-build|remote-release|remote-check|remote-test}"
  echo "  build  - Build release binary"
  echo "  run    - Build and run release (default)"
  echo "  dev    - Build and run debug"
  echo "  check  - Run cargo check and clippy"
  echo "  remote-build    - Sync to The Beast and run cargo build"
  echo "  remote-release  - Sync to The Beast and run cargo build --release"
  echo "  remote-check    - Sync to The Beast and run check/clippy"
  echo "  remote-test     - Sync to The Beast and run cargo test"
  exit 1
  ;;
esac
