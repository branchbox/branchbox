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

# initializeCommand runs via /bin/sh without a login shell,
# so add common paths where Homebrew installs binaries.
export PATH="$PATH:/usr/local/bin:/opt/homebrew/bin:$HOME/.local/bin"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TOKEN_FILE="$SCRIPT_DIR/.github-token.env"
SIGNING_KEY_FILE="$SCRIPT_DIR/.git-signing-key"
GIT_CONFIG_FILE="$SCRIPT_DIR/.gitconfig.env"

# Default 1Password references — override with env vars
OP_GITHUB_REF="${OP_GITHUB_REF:-}"
OP_SIGNING_KEY_REF="${OP_SIGNING_KEY_REF:-}"

# Ensure files exist with restricted permissions so Docker compose volume mounts don't fail
(umask 077 && touch "$TOKEN_FILE" "$SIGNING_KEY_FILE" "$GIT_CONFIG_FILE")

if ! command -v op &>/dev/null; then
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
        printf 'GITHUB_TOKEN=%q\n' "$GITHUB_TOKEN" > "$TOKEN_FILE"
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
        echo "$SIGNING_KEY" > "$SIGNING_KEY_FILE"
    fi
fi

# Git identity from host config
if ! command -v git &>/dev/null; then
    echo "ℹ️  git not found on host — skipping identity export."
    GIT_USER_NAME=""
    GIT_USER_EMAIL=""
else
    GIT_USER_NAME=$(git config --global user.name 2>/dev/null || echo "")
    GIT_USER_EMAIL=$(git config --global user.email 2>/dev/null || echo "")
fi

if [ -n "$GIT_USER_NAME" ] && [ -n "$GIT_USER_EMAIL" ]; then
    printf 'GIT_USER_NAME=%q\n' "$GIT_USER_NAME" > "$GIT_CONFIG_FILE"
    printf 'GIT_USER_EMAIL=%q\n' "$GIT_USER_EMAIL" >> "$GIT_CONFIG_FILE"
fi

echo "✅ BranchBox secret injection complete."
