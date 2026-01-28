#!/bin/bash
set -e

cd "$(dirname "$0")"

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
  echo "Running Portal in debug mode..."
  run_cargo run
  ;;
check)
  echo "Checking Portal..."
  cargo check
  cargo clippy
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
