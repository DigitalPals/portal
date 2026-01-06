#!/bin/bash

# Install git hooks for the portal project
# Run this script after cloning the repository

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
HOOKS_SOURCE="$SCRIPT_DIR/hooks"
HOOKS_DEST="$REPO_ROOT/.git/hooks"

echo "Installing git hooks..."

# Check if we're in a git repository
if [ ! -d "$REPO_ROOT/.git" ]; then
    echo "ERROR: Not a git repository. Run this script from within the portal repo."
    exit 1
fi

# Install pre-push hook
if [ -f "$HOOKS_SOURCE/pre-push" ]; then
    cp "$HOOKS_SOURCE/pre-push" "$HOOKS_DEST/pre-push"
    chmod +x "$HOOKS_DEST/pre-push"
    echo "  Installed pre-push hook"
fi

echo ""
echo "Git hooks installed successfully!"
echo ""
echo "The pre-push hook will run the following checks before each push:"
echo "  - cargo fmt --all --check"
echo "  - cargo clippy --all-targets -- -D warnings"
echo "  - cargo test"
