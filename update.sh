#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

run_cargo() {
  if command -v cargo >/dev/null 2>&1 && cargo --version >/dev/null 2>&1; then
    cargo "$@"
  elif command -v nix >/dev/null 2>&1; then
    nix develop --command cargo "$@"
  else
    echo "error: cargo is not available, and nix is not installed" >&2
    exit 1
  fi
}

pin_russh_crypto_prereleases() {
  if [[ "${SKIP_RUSSH_CRYPTO_PIN:-0}" == "1" ]]; then
    return
  fi

  # russh 0.56 currently resolves through prerelease rsa/crypto crates. A broad
  # cargo update can select newer prereleases that do not compile together.
  echo "Pinning russh prerelease crypto dependencies to known-compatible versions..."
  run_cargo update -p rsa@0.10.0-rc.17 --precise 0.10.0-rc.11
  run_cargo update -p crypto-primes --precise 0.7.0-pre.5
  run_cargo update -p signature@3.0.0-rc.10 --precise 3.0.0-rc.6
  run_cargo update -p sha1@0.11.0 --precise 0.11.0-rc.3
  run_cargo update -p sha2@0.11.0 --precise 0.11.0-rc.3
  run_cargo update -p digest@0.11.2 --precise 0.11.0-rc.5
  run_cargo update -p crypto-common@0.2.1 --precise 0.2.0-rc.9
  run_cargo update -p spki@0.8.0 --precise 0.8.0-rc.4
  run_cargo update -p pkcs8@0.11.0-rc.11 --precise 0.11.0-rc.8
  run_cargo update -p der@0.8.0 --precise 0.8.0-rc.10
  run_cargo update -p crypto-bigint@0.7.3 --precise 0.7.0-rc.15
  run_cargo update -p rand_core@0.10.1 --precise 0.10.0-rc-3
}

echo "Updating Cargo.lock within Cargo.toml version constraints..."
run_cargo update
pin_russh_crypto_prereleases

if [[ "${UPDATE_MANIFEST:-0}" == "1" ]]; then
  if run_cargo upgrade --version >/dev/null 2>&1; then
    echo "Updating Cargo.toml dependency requirements with cargo-upgrade..."
    run_cargo upgrade
    echo "Refreshing Cargo.lock after manifest updates..."
    run_cargo update
    pin_russh_crypto_prereleases
  else
    echo "Skipping Cargo.toml upgrades: install cargo-edit or run inside an environment that provides cargo-upgrade."
  fi
fi

if [[ -f flake.nix && "${SKIP_NIX:-0}" != "1" ]]; then
  if command -v nix >/dev/null 2>&1; then
    echo "Updating Nix flake inputs..."
    nix flake update
  else
    echo "Skipping Nix flake update: nix is not installed."
  fi
fi

if [[ "${SKIP_CHECK:-0}" != "1" ]]; then
  echo "Running cargo check..."
  run_cargo check
fi

echo "Update complete."
