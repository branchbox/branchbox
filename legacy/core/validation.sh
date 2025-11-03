#!/bin/bash
# Environment validation utilities for git-worktree workflow
# Validates prerequisites and environment setup

# Check if running in a valid git worktree (main or feature branch)
# Usage: validate_git_worktree "/path/to/worktree" || exit 1
# Note: Function name kept as validate_main_worktree for backward compatibility
validate_main_worktree() {
  local worktree_dir="$1"

  # Check if .git exists (either as directory for main worktree or file for feature worktree)
  if [ ! -e "$worktree_dir/.git" ]; then
    return 1
  fi

  # Main worktree: .git is a directory with config file
  if [ -d "$worktree_dir/.git" ] && [ -f "$worktree_dir/.git/config" ]; then
    return 0
  fi

  # Feature worktree: .git is a file containing gitdir reference
  if [ -f "$worktree_dir/.git" ]; then
    # Verify it's a valid worktree by checking if git commands work
    if (cd "$worktree_dir" && git rev-parse --git-dir >/dev/null 2>&1); then
      return 0
    fi

    # Git file exists but git commands don't work
    # This often means the .git file has wrong path (container vs host)
    # Or it has an old absolute path that needs converting to relative path
    # Try to auto-fix it by using relative paths
    local current_gitdir=$(cat "$worktree_dir/.git" | sed 's/gitdir: //')
    local worktree_name=$(basename "$worktree_dir")
    local parent_dir=$(dirname "$worktree_dir")

    # Try to find main worktree
    local main_dir=""
    if [ -d "$parent_dir/main" ]; then
      main_dir="$parent_dir/main"
    elif [ -d "$parent_dir/../main" ]; then
      main_dir="$(cd "$parent_dir/../main" && pwd)"
    fi

    # If we found main dir, update the .git file with RELATIVE path
    # This makes it work in both container and host environments
    if [ -n "$main_dir" ] && [ -d "$main_dir/.git/worktrees/$worktree_name" ]; then
      local main_name=$(basename "$main_dir")
      echo "gitdir: ../$main_name/.git/worktrees/$worktree_name" > "$worktree_dir/.git"

      # Verify it works now
      if (cd "$worktree_dir" && git rev-parse --git-dir >/dev/null 2>&1); then
        # Source logging if available to show success message
        if type log_success >/dev/null 2>&1; then
          log_success "Auto-fixed git worktree path (using relative path)"
        fi
        return 0
      fi
    fi

    # Could not auto-fix
    export GIT_PATH_MISMATCH=1
    return 1
  fi

  return 1
}

# Check if running on host (not in container)
# Usage: validate_host_environment || exit 1
validate_host_environment() {
  if [ -f "/.dockerenv" ] || [ -n "${DOCKER_CONTAINER:-}" ]; then
    return 1
  fi

  return 0
}

# Validate .env file exists and has required variables
# Usage: validate_env_file "/path/to/.env" "APP_URL" || exit 1
validate_env_file() {
  local env_file="$1"
  local required_var="${2:-}"

  if [ ! -f "$env_file" ]; then
    return 1
  fi

  # If a specific variable is required, check for it
  if [ -n "$required_var" ]; then
    if ! grep -q "^${required_var}=" "$env_file"; then
      return 1
    fi
  fi

  return 0
}

# Parse APP_URL from .env and split into components
# Sets: APP_URL, BASE_PREFIX, BASE_DOMAIN
# Usage: parse_app_url "/path/to/.env"
parse_app_url() {
  local env_file="$1"

  # Disable xtrace to prevent debug output contamination
  local xtrace_was_set=false
  if [[ $- =~ x ]]; then
    xtrace_was_set=true
    set +x
  fi

  # Extract APP_URL from .env without sourcing
  # Use head -1 to get only the first match (avoid duplication if feature-specific section exists)
  APP_URL=$(grep "^APP_URL=" "$env_file" | head -1 | cut -d'=' -f2- | tr -d '"' | tr -d "'" | tr -d '\n\r' | sed 's|^https\?://||' | xargs)

  if [ -z "$APP_URL" ]; then
    [ "$xtrace_was_set" = true ] && set -x
    return 1
  fi

  # Parse APP_URL to extract base prefix and domain
  # Example: rida-mbp-agentify.rida.me -> prefix=rida-mbp-agentify, domain=rida.me
  BASE_PREFIX=$(echo "$APP_URL" | sed 's/\..*//' | tr -d '\n\r' | xargs)
  BASE_DOMAIN=$(echo "$APP_URL" | sed 's/^[^.]*\.//' | tr -d '\n\r' | xargs)

  [ "$xtrace_was_set" = true ] && set -x
  return 0
}

# Read Cloudflare credentials from .env
# Sets: CLOUDFLARE_API_KEY, CLOUDFLARE_ACCOUNT_ID (if present)
# Usage: read_cloudflare_credentials "/path/to/.env"
read_cloudflare_credentials() {
  local env_file="$1"

  # Only read if not already set
  if [ -z "${CLOUDFLARE_API_KEY:-}" ]; then
    CLOUDFLARE_API_KEY=$(grep "^CLOUDFLARE_API_KEY=" "$env_file" 2>/dev/null | cut -d'=' -f2- | tr -d '"' | tr -d "'")
  fi

  if [ -z "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
    CLOUDFLARE_ACCOUNT_ID=$(grep "^CLOUDFLARE_ACCOUNT_ID=" "$env_file" 2>/dev/null | cut -d'=' -f2- | tr -d '"' | tr -d "'")
  fi

  return 0
}
