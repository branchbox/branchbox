#!/bin/bash
# Stack Adapter Interface for git-worktree workflow
# All stack adapters must implement these functions

# Adapter Interface Documentation
# ================================
#
# Each stack adapter must implement the following functions:
#
# 1. adapter_detect()
#    - Detects if this adapter should be used for the current project
#    - Sets ADAPTER_CONFIDENCE (0-100) to indicate confidence level
#    - Returns: 0 if detected, 1 if not
#
# 2. adapter_copy_secrets()
#    - Copies stack-specific secret/credential files to worktree
#    - Args: $1 = source directory (main worktree)
#            $2 = destination directory (feature worktree)
#    - Returns: 0 on success, 1 on failure
#
# 3. adapter_get_name()
#    - Returns the human-readable name of this adapter
#    - Output: String name (e.g., "Rails", "Node.js", "Generic")
#
# 4. adapter_get_service_url()
#    - Returns the service URL for Cloudflare tunnel ingress
#    - Output: String URL (e.g., "http://rails-app:3000", "http://localhost:3000")
#
# Optional functions (will use defaults if not implemented):
#
# 5. adapter_setup_database()
#    - Run stack-specific database setup commands
#    - Runs inside the worktree devcontainer
#    - Returns: 0 on success, 1 on failure
#
# 6. adapter_healthcheck()
#    - Verify the worktree is healthy and ready
#    - Returns: 0 if healthy, 1 if not
#
# 7. adapter_cleanup()
#    - Stack-specific cleanup on teardown
#    - Returns: 0 on success, 1 on failure

# Default implementations (can be overridden by specific adapters)

adapter_get_service_url() {
  # Default: generic localhost on port 3000
  echo "http://localhost:3000"
}

adapter_setup_database() {
  # Default: no database setup
  return 0
}

adapter_healthcheck() {
  # Default: always healthy
  return 0
}

adapter_cleanup() {
  # Default: no cleanup
  return 0
}

# Helper function to check if adapter function exists
adapter_function_exists() {
  local func_name="$1"
  declare -f "$func_name" >/dev/null
  return $?
}

# Load an adapter by name
# Usage: load_adapter "rails" "/path/to/lib/adapters"
load_adapter() {
  local adapter_name="$1"
  local adapters_dir="${2:-$(dirname "${BASH_SOURCE[0]}")}"
  local adapter_file="$adapters_dir/adapter-${adapter_name}.sh"

  if [ -f "$adapter_file" ]; then
    source "$adapter_file"
    return 0
  else
    return 1
  fi
}

# Auto-detect and load the best adapter for current project
# Usage: detect_and_load_adapter "/path/to/project" "/path/to/adapters"
# Sets: DETECTED_ADAPTER (global variable with adapter name)
# Returns: 0 on success, 1 on failure
detect_and_load_adapter() {
  local project_dir="$1"
  local adapters_dir="${2:-$(dirname "${BASH_SOURCE[0]}")}"

  cd "$project_dir" || return 1

  local best_adapter=""
  local best_confidence=0

  # Try each adapter
  for adapter_file in "$adapters_dir"/adapter-*.sh; do
    if [ -f "$adapter_file" ]; then
      local adapter_name=$(basename "$adapter_file" .sh | sed 's/adapter-//')

      # Skip interface file
      if [ "$adapter_name" = "interface" ]; then
        continue
      fi

      # Load adapter
      source "$adapter_file"

      # Check if it detects this project
      if adapter_detect 2>/dev/null; then
        local confidence=${ADAPTER_CONFIDENCE:-0}

        if [ "$confidence" -gt "$best_confidence" ]; then
          best_confidence=$confidence
          best_adapter=$adapter_name
        fi
      fi
    fi
  done

  # Fallback to generic if nothing detected
  if [ -z "$best_adapter" ]; then
    best_adapter="generic"
  fi

  # Reload the best adapter (this sources it in the current shell)
  load_adapter "$best_adapter" "$adapters_dir"

  # Set global variable instead of echoing
  DETECTED_ADAPTER="$best_adapter"
  return 0
}
