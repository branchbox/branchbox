#!/usr/bin/env bash
# BranchBox 1Password Integration — Host-side secret fetcher
#
# Runs on the HOST (via devcontainer.json initializeCommand) before the
# container starts.  Fetches GitHub credentials from 1Password and writes
# them to files that get mounted into the container for git, gh CLI, and
# commit signing.
#
# Requirements:
#   - 1Password CLI: brew install 1password-cli
#   - 1Password desktop app with biometric unlock enabled
#
# Override the default 1Password references by exporting in your shell
# profile (~/.zshrc or ~/.bashrc), or in your project .env file:
#
#   export OP_GITHUB_REF="op://MyVault/GitHub PAT/credential"
#   export OP_SIGNING_KEY_REF="op://MyVault/Signing Key/private key"

set -euo pipefail

# Prepend trusted paths so Homebrew-installed binaries are found first,
# preventing PATH hijacking from the project directory.
export PATH="/usr/local/bin:/opt/homebrew/bin:$HOME/.local/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TOKEN_FILE="$SCRIPT_DIR/.github-token.env"
SIGNING_KEY_FILE="$SCRIPT_DIR/.git-signing-key"
GIT_CONFIG_FILE="$SCRIPT_DIR/.gitconfig.env"

# Default 1Password references — override with env vars
OP_GITHUB_REF="${OP_GITHUB_REF:-}"
OP_SIGNING_KEY_REF="${OP_SIGNING_KEY_REF:-}"

# atomic_write: write content to a file via temp + rename to prevent
# symlink TOCTOU attacks. The rename is atomic on the same filesystem,
# so there is no window where a symlink could be followed.
atomic_write() {
    local target="$1"
    local dir
    dir="$(dirname "$target")"
    local tmp
    tmp="$(mktemp "$dir/.tmp.XXXXXX")"
    chmod 600 "$tmp"
    cat > "$tmp"
    mv -f "$tmp" "$target"
}

# Ensure files exist with restricted permissions so Docker compose volume
# mounts don't fail.  Uses atomic temp+rename so there is no TOCTOU window
# where a symlink could be swapped in between a check and a write/chmod.
for f in "$TOKEN_FILE" "$SIGNING_KEY_FILE" "$GIT_CONFIG_FILE"; do
    # Remove anything that isn't a regular file (symlinks, directories, etc.)
    if [ -e "$f" ] && ! [ -f "$f" ]; then
        rm -rf -- "$f"
    fi
    if ! [ -e "$f" ]; then
        _pholder="$(mktemp "$(dirname "$f")/.tmp.XXXXXX")"
        chmod 600 "$_pholder"
        mv -f "$_pholder" "$f"
    fi
    # Existing regular files keep their 0600 permissions from prior runs
done

if ! command -v op >/dev/null 2>&1; then
    echo "ℹ️  1Password CLI (op) not found — skipping secret injection."
    echo "   Install with: brew install 1password-cli"
    echo "   Docs: https://developer.1password.com/docs/cli/get-started/"
    exit 0
fi

if [ -z "$OP_GITHUB_REF" ]; then
    echo "ℹ️  OP_GITHUB_REF not set — skipping GitHub token fetch."
    echo "   Set it in your shell profile or .env to enable:"
    echo "   export OP_GITHUB_REF=\"op://VaultName/GitHub PAT/credential\""
else
    echo "🔐 Fetching GitHub token from 1Password..."
    GITHUB_TOKEN=$(op read "$OP_GITHUB_REF") || {
        echo "⚠️  Could not read GitHub token from 1Password."
        echo "   Reference: $OP_GITHUB_REF"
        GITHUB_TOKEN=""
    }
    if [ -n "$GITHUB_TOKEN" ]; then
        printf 'GITHUB_TOKEN=%q\n' "$GITHUB_TOKEN" | atomic_write "$TOKEN_FILE"
    fi
fi

if [ -z "$OP_SIGNING_KEY_REF" ]; then
    echo "ℹ️  OP_SIGNING_KEY_REF not set — skipping signing key fetch."
    echo "   Set it in your shell profile or .env to enable:"
    echo "   export OP_SIGNING_KEY_REF=\"op://VaultName/Signing Key/private key\""
else
    echo "🔐 Fetching signing key from 1Password..."
    SIGNING_KEY=$(op read "$OP_SIGNING_KEY_REF") || {
        echo "⚠️  Could not read signing key from 1Password."
        echo "   Reference: $OP_SIGNING_KEY_REF"
        echo "   Git commits will not be signed."
        SIGNING_KEY=""
    }
    if [ -n "$SIGNING_KEY" ]; then
        printf '%s\n' "$SIGNING_KEY" | atomic_write "$SIGNING_KEY_FILE"
    fi
fi

# Git identity from host config
if ! command -v git >/dev/null 2>&1; then
    echo "ℹ️  git not found on host — skipping identity export."
    GIT_USER_NAME=""
    GIT_USER_EMAIL=""
else
    GIT_USER_NAME=$(git config --global user.name 2>/dev/null || echo "")
    GIT_USER_EMAIL=$(git config --global user.email 2>/dev/null || echo "")
fi

if [ -n "$GIT_USER_NAME" ] && [ -n "$GIT_USER_EMAIL" ]; then
    {
        printf 'GIT_USER_NAME=%q\n' "$GIT_USER_NAME"
        printf 'GIT_USER_EMAIL=%q\n' "$GIT_USER_EMAIL"
    } | atomic_write "$GIT_CONFIG_FILE"
fi

echo "✅ BranchBox secret injection complete."
