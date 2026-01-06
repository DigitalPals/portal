#!/bin/bash
set -e

cd "$(dirname "$0")"

case "${1:-run}" in
    build)
        echo "Building Portal2..."
        nix-shell --run "cargo build --release"
        echo "Build complete: target/release/portal"
        ;;
    run)
        echo "Building and running Portal2..."
        nix-shell --run "cargo run --release"
        ;;
    dev)
        echo "Running Portal2 in debug mode..."
        nix-shell --run "cargo run"
        ;;
    check)
        echo "Checking Portal2..."
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
