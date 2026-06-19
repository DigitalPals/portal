#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

REMOTE_HOST="${PORTAL_REMOTE_HOST:-root@10.10.0.233}"
REMOTE_DIR="${PORTAL_REMOTE_DIR:-/root/Code/portal}"
SYNC=1
FETCH="${PORTAL_REMOTE_FETCH:-0}"

usage() {
  cat <<'EOF'
Usage: scripts/remote-build.sh [options] [command]

Options:
  --host HOST      SSH host for the remote builder
                  default: PORTAL_REMOTE_HOST or root@10.10.0.233
  --dir DIR        Remote checkout directory
                  default: PORTAL_REMOTE_DIR or /root/Code/portal
  --fetch          Copy the resulting portal binary back after a successful
                  build, release, or cargo build command
  --no-fetch       Do not copy build artifacts back
  --no-sync        Reuse the existing remote checkout without syncing first
  -h, --help       Show this help

Commands:
  build            cargo build (default)
  release          cargo build --release
  check            cargo check && cargo clippy --all-targets -- -D warnings
  test             cargo test
  run              cargo run --release
  dev              cargo run --profile dev-release
  cargo ...        Run cargo with the remaining arguments
  -- ...           Run an arbitrary command inside the remote dev shell

Environment:
  PORTAL_REMOTE_HOST  SSH host to use for remote builds
  PORTAL_REMOTE_DIR   Remote checkout path
  PORTAL_REMOTE_FETCH=1
                      Copy the resulting portal binary back after successful
                      build commands.
  PORTAL_REMOTE_USE_SCCACHE=1
                      Keep RUSTC_WRAPPER/SCCACHE_* in the remote environment.
                      By default they are unset so Cargo builds directly on
                      the remote host, even if remote Cargo config sets a
                      rustc-wrapper.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
  --host)
    REMOTE_HOST="${2:?missing value for --host}"
    shift 2
    ;;
  --dir)
    REMOTE_DIR="${2:?missing value for --dir}"
    shift 2
    ;;
  --fetch)
    FETCH=1
    shift
    ;;
  --no-fetch)
    FETCH=0
    shift
    ;;
  --no-sync)
    SYNC=0
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  --)
    shift
    break
    ;;
  *)
    break
    ;;
  esac
done

case "$REMOTE_DIR" in
*\'* | *$'\n'*)
  echo "Remote directory contains unsupported characters: $REMOTE_DIR" >&2
  exit 2
  ;;
esac

make_manifest() {
  local manifest="$1"
  git ls-files -z --cached --others --exclude-standard |
    while IFS= read -r -d '' path; do
      [[ -e "$path" ]] || continue
      printf '%s\0' "$path"
    done >"$manifest"
}

sync_to_remote() {
  local manifest
  manifest="$(mktemp)"
  trap 'rm -f "$manifest"' RETURN

  make_manifest "$manifest"

  local file_count
  file_count="$(tr '\0' '\n' <"$manifest" | sed '/^$/d' | wc -l)"
  echo "Syncing $file_count files to $REMOTE_HOST:$REMOTE_DIR"

  local qdir
  printf -v qdir '%q' "$REMOTE_DIR"

  ssh "$REMOTE_HOST" "/bin/bash -lc 'mkdir -p $qdir/.remote-build && cat > $qdir/.remote-build/manifest.next'" <"$manifest"

  tar --null -T "$manifest" -cf - |
    ssh "$REMOTE_HOST" "/bin/bash -lc 'mkdir -p $qdir && tar -C $qdir -xf -'"

  ssh "$REMOTE_HOST" /bin/bash -s -- "$REMOTE_DIR" <<'REMOTE_CLEANUP'
set -euo pipefail
remote_dir="$1"
python3 - "$remote_dir" <<'PY'
import os
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()
state = root / ".remote-build"
old_manifest = state / "manifest"
new_manifest = state / "manifest.next"

def read_manifest(path):
    if not path.exists():
        return set()
    data = path.read_bytes()
    return {entry.decode() for entry in data.split(b"\0") if entry}

old = read_manifest(old_manifest)
new = read_manifest(new_manifest)

for rel in sorted(old - new, key=lambda item: item.count("/"), reverse=True):
    if rel.startswith("/") or "/../" in f"/{rel}/":
        continue
    path = (root / rel).resolve()
    try:
        path.relative_to(root)
    except ValueError:
        continue
    try:
        if path.is_file() or path.is_symlink():
            path.unlink()
    except FileNotFoundError:
        pass

for rel in sorted(old - new, key=lambda item: item.count("/"), reverse=True):
    parent = (root / rel).resolve().parent
    while parent != root:
        try:
            parent.rmdir()
        except OSError:
            break
        parent = parent.parent

new_manifest.replace(old_manifest)
PY
REMOTE_CLEANUP
}

remote_command() {
  local action="${1:-build}"
  if [[ $# -gt 0 ]]; then
    shift
  fi

  local cmd=()
  case "$action" in
  build)
    cmd=(cargo build)
    ;;
  release)
    cmd=(cargo build --release)
    ;;
  check)
    cmd=(bash -lc 'cargo check && cargo clippy --all-targets -- -D warnings')
    ;;
  test)
    cmd=(cargo test)
    ;;
  run)
    cmd=(cargo run --release)
    ;;
  dev)
    cmd=(cargo run --profile dev-release)
    ;;
  cargo)
    cmd=(cargo "$@")
    ;;
  "")
    cmd=(cargo build)
    ;;
  *)
    cmd=("$action" "$@")
    ;;
  esac

  local qdir qcmd
  printf -v qdir '%q' "$REMOTE_DIR"
  printf -v qcmd ' %q' "${cmd[@]}"

  echo "Running on $REMOTE_HOST:$REMOTE_DIR:${qcmd}"
  local env_setup='unset RUSTC_WRAPPER SCCACHE_CONF SCCACHE_DIR SCCACHE_RECACHE SCCACHE_ERROR_LOG; export CARGO_BUILD_RUSTC_WRAPPER=;'
  if [[ "${PORTAL_REMOTE_USE_SCCACHE:-0}" == "1" ]]; then
    env_setup=':;'
  fi
  ssh "$REMOTE_HOST" "/bin/bash -lc 'cd $qdir && $env_setup if command -v nix >/dev/null 2>&1; then exec nix develop --command$qcmd; else exec$qcmd; fi'"
}

artifact_for_command() {
  local action="${1:-build}"
  shift || true

  case "$action" in
  build)
    echo "target/debug/portal"
    ;;
  release)
    echo "target/release/portal"
    ;;
  cargo)
    case " $* " in
    *" --release "*)
      echo "target/release/portal"
      ;;
    *" build "* | " "*)
      echo "target/debug/portal"
      ;;
    esac
    ;;
  *)
    ;;
  esac
}

fetch_artifact() {
  local artifact="$1"
  [[ -n "$artifact" ]] || return 0

  local qdir qartifact
  printf -v qdir '%q' "$REMOTE_DIR"
  printf -v qartifact '%q' "$artifact"

  ssh "$REMOTE_HOST" "/bin/bash -lc 'test -f $qdir/$qartifact'"
  mkdir -p "$(dirname "$artifact")"
  echo "Fetching $REMOTE_HOST:$REMOTE_DIR/$artifact -> $artifact"
  local tmp_artifact
  tmp_artifact="$(mktemp "$(dirname "$artifact")/.remote-fetch.XXXXXX")"
  ssh "$REMOTE_HOST" "/bin/bash -lc 'cat $qdir/$qartifact'" >"$tmp_artifact"
  chmod +x "$tmp_artifact"
  mv -f "$tmp_artifact" "$artifact"
}

if [[ "$SYNC" -eq 1 ]]; then
  sync_to_remote
fi

args=("$@")
if [[ "${#args[@]}" -eq 0 ]]; then
  args=(build)
fi

remote_command "${args[@]}"

if [[ "$FETCH" -eq 1 ]]; then
  fetch_artifact "$(artifact_for_command "${args[@]}")"
fi
