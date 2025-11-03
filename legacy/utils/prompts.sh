#!/bin/bash
# Interactive prompt utilities for git-worktree workflow
# Provides confirmation and input functions with default value support

# Global flag for non-interactive mode
ACCEPT_DEFAULTS=${ACCEPT_DEFAULTS:-false}

# Confirmation prompt with default value
# Usage: confirm "Do you want to continue?" "y"
# Returns: 0 if yes, 1 if no
confirm() {
  if [ "$ACCEPT_DEFAULTS" = true ]; then
    return 0
  fi

  local prompt="$1"
  local default="${2:-y}"

  if [ "$default" = "y" ]; then
    prompt="$prompt [Y/n]"
  else
    prompt="$prompt [y/N]"
  fi

  read -p "$prompt: " -r response
  response=${response:-$default}

  [[ "$response" =~ ^[Yy]$ ]]
}

# Input prompt with default value
# Usage: result=$(get_input "Enter name" "default-name")
# Returns: User input or default value
get_input() {
  local prompt="$1"
  local default="$2"

  if [ "$ACCEPT_DEFAULTS" = true ] && [ -n "$default" ]; then
    echo "$default"
    return
  fi

  if [ -n "$default" ]; then
    read -p "$prompt [$default]: " -r response
    echo "${response:-$default}"
  else
    read -p "$prompt: " -r response
    echo "$response"
  fi
}
