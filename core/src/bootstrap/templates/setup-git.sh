#!/usr/bin/env bash
# BranchBox 1Password Integration — Container-side git configurator
#
# Runs INSIDE the container (via devcontainer.json postStartCommand).
# Reads secrets written by init-host.sh and configures:
#   1. git credential helper for HTTPS pushes/pulls
#   2. GH_TOKEN env var for the gh CLI
#   3. SSH commit signing
#   4. Git user identity (inherited from host)

set -euo pipefail

TOKEN_FILE="/home/vscode/.github-token.env"
SIGNING_KEY_SRC="/home/vscode/.git-signing-key"
SIGNING_KEY_DST="/home/vscode/.ssh/git-signing-key"
GITCONFIG_FILE="/home/vscode/.gitconfig.env"
WORKSPACE_DIR="/workspaces"

# --- GitHub PAT ---

if [ ! -f "$TOKEN_FILE" ] || [ ! -s "$TOKEN_FILE" ]; then
    echo "ℹ️  No GitHub token found — git push/pull to GitHub won't work."
    echo "   Set OP_GITHUB_REF on your host and rebuild the container."
else
    # shellcheck disable=SC1090
    source "$TOKEN_FILE"

    if [ -n "${GITHUB_TOKEN:-}" ]; then
        # Git credential helper for github.com (references env var, not inline value)
        git config --global credential.https://github.com.helper \
            '!f() { echo "username=oauth2"; echo "password=$GITHUB_TOKEN"; }; f'

        # GH_TOKEN for gh CLI — persist in bashrc for interactive shells
        if ! grep -q "# BranchBox GitHub token" ~/.bashrc 2>/dev/null; then
            cat >> ~/.bashrc <<'GHEOF'

# BranchBox GitHub token (injected by setup-git.sh)
[ -f /home/vscode/.github-token.env ] && source /home/vscode/.github-token.env && export GH_TOKEN="$GITHUB_TOKEN"
GHEOF
        fi

        # Switch remote to HTTPS if it's SSH (find actual workspace)
        for dir in "$WORKSPACE_DIR"/*/; do
            if [ -d "$dir/.git" ] || git -C "$dir" rev-parse --git-dir &>/dev/null 2>&1; then
                REMOTE_URL=$(git -C "$dir" remote get-url origin 2>/dev/null || echo "")
                if echo "$REMOTE_URL" | grep -q "^git@github.com:"; then
                    HTTPS_URL=$(echo "$REMOTE_URL" | sed 's|^git@github.com:|https://github.com/|')
                    git -C "$dir" remote set-url origin "$HTTPS_URL"
                    echo "   Switched $(basename "$dir") remote to HTTPS"
                fi
            fi
        done

        echo "✅ GitHub authentication configured."
    fi
fi

# --- Git identity (inherited from host) ---

if [ -f "$GITCONFIG_FILE" ] && [ -s "$GITCONFIG_FILE" ]; then
    # Parse with grep+cut instead of source — values may contain spaces
    GIT_USER_NAME=$(grep '^GIT_USER_NAME=' "$GITCONFIG_FILE" | cut -d= -f2- | tr -d "'")
    GIT_USER_EMAIL=$(grep '^GIT_USER_EMAIL=' "$GITCONFIG_FILE" | cut -d= -f2- | tr -d "'")

    [ -n "${GIT_USER_NAME:-}" ] && git config --global user.name "$GIT_USER_NAME"
    [ -n "${GIT_USER_EMAIL:-}" ] && git config --global user.email "$GIT_USER_EMAIL"
    echo "   Git identity: $GIT_USER_NAME <$GIT_USER_EMAIL>"
fi

# --- SSH commit signing ---

if [ -f "$SIGNING_KEY_SRC" ] && [ -s "$SIGNING_KEY_SRC" ]; then
    mkdir -p ~/.ssh
    chmod 700 ~/.ssh

    # Copy from read-only mount to writable location with correct permissions
    cp "$SIGNING_KEY_SRC" "$SIGNING_KEY_DST"
    chmod 600 "$SIGNING_KEY_DST"

    # Extract the public key for allowed_signers
    ssh-keygen -y -f "$SIGNING_KEY_DST" > "$SIGNING_KEY_DST.pub" 2>/dev/null

    # Configure git to use SSH signing
    git config --global gpg.format ssh
    git config --global user.signingkey "$SIGNING_KEY_DST"
    git config --global commit.gpgsign true
    git config --global tag.gpgsign true

    # Create allowed_signers file so `git log --show-signature` works
    GIT_EMAIL=$(git config --global user.email 2>/dev/null || echo "")
    if [ -n "$GIT_EMAIL" ]; then
        echo "$GIT_EMAIL $(cat "$SIGNING_KEY_DST.pub")" > ~/.ssh/allowed_signers
        git config --global gpg.ssh.allowedSignersFile ~/.ssh/allowed_signers
    fi

    echo "✅ Git commit signing configured."
else
    echo "ℹ️  No signing key found — commits will not be signed."
fi
