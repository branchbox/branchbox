#!/bin/bash
# Module Interface Specification
# All modules must implement these functions to integrate with the git worktree workflow
#
# Modules are optional, composable components that provide specific functionality:
# - database: Database isolation and setup
# - tunnel: Cloudflare tunnel provisioning
# - specs: Feature specification lifecycle tracking
# - compose: Docker Compose configuration management
#
# Module Lifecycle:
#   1. module_detect() - Check if module should be enabled
#   2. module_init() - Initialize module configuration
#   3. module_setup() - Setup resources during feature-start
#   4. module_teardown() - Cleanup resources during feature-teardown
#   5. module_validate() - Validate module configuration

# Module metadata
MODULE_NAME=""
MODULE_DESCRIPTION=""
MODULE_VERSION="1.0.0"
MODULE_ENABLED=false
MODULE_DEPENDENCIES=()  # Array of module names this module depends on

# Module Interface Functions
# All modules MUST implement these functions

# module_detect() - Detect if this module should be enabled
#
# Returns:
#   0 if module should be enabled
#   1 if module should be disabled
#
# Sets:
#   MODULE_ENABLED=true/false
#   MODULE_CONFIDENCE (0-100, optional)
#
# Example:
#   module_detect() {
#     if [ -f "config/database.yml" ]; then
#       MODULE_ENABLED=true
#       MODULE_CONFIDENCE=95
#       return 0
#     fi
#     MODULE_ENABLED=false
#     return 1
#   }
module_detect() {
  log_error "module_detect() not implemented"
  return 1
}

# module_init() - Initialize module configuration
#
# Args:
#   $1 - Main worktree directory
#   $2 - Feature worktree directory (may not exist yet)
#   $3 - Module config (optional, from config file or env)
#
# Returns:
#   0 on success
#   1 on failure
#
# Sets module-specific variables
#
# Example:
#   module_init() {
#     local main_dir="$1"
#     local feature_dir="$2"
#     local config="$3"
#
#     MODULE_DATABASE_NAME="agentify_${WORK_FEATURE//-/_}"
#     MODULE_DATABASE_ENGINE="postgres"
#     return 0
#   }
module_init() {
  log_error "module_init() not implemented"
  return 1
}

# module_setup() - Setup resources during feature-start
#
# Args:
#   $1 - Main worktree directory
#   $2 - Feature worktree directory
#
# Returns:
#   0 on success
#   1 on failure (will abort feature-start)
#
# This function is called during feature-start to provision module resources
#
# Example:
#   module_setup() {
#     local main_dir="$1"
#     local feature_dir="$2"
#
#     log_info "Creating database: $MODULE_DATABASE_NAME"
#     createdb "$MODULE_DATABASE_NAME"
#
#     log_info "Running migrations..."
#     cd "$feature_dir"
#     bin/rails db:migrate
#
#     return 0
#   }
module_setup() {
  log_error "module_setup() not implemented"
  return 1
}

# module_teardown() - Cleanup resources during feature-teardown
#
# Args:
#   $1 - Main worktree directory
#   $2 - Feature worktree directory (may not exist)
#
# Returns:
#   0 on success (or partial success)
#   1 on failure (will log warning but continue teardown)
#
# This function is called during feature-teardown to clean up module resources
# Should be idempotent and handle missing resources gracefully
#
# Example:
#   module_teardown() {
#     local main_dir="$1"
#     local feature_dir="$2"
#
#     if psql -lqt | cut -d \| -f 1 | grep -qw "$MODULE_DATABASE_NAME"; then
#       log_info "Dropping database: $MODULE_DATABASE_NAME"
#       dropdb "$MODULE_DATABASE_NAME"
#     else
#       log_info "Database not found (may already be deleted)"
#     fi
#
#     return 0
#   }
module_teardown() {
  log_error "module_teardown() not implemented"
  return 1
}

# module_validate() - Validate module configuration
#
# Args:
#   $1 - Main worktree directory
#   $2 - Feature worktree directory
#
# Returns:
#   0 if valid
#   1 if invalid (will log error and suggest fixes)
#
# This function is called to validate that module resources are configured correctly
#
# Example:
#   module_validate() {
#     local main_dir="$1"
#     local feature_dir="$2"
#
#     if [ ! -f "$feature_dir/.env" ]; then
#       log_error "Missing .env file"
#       return 1
#     fi
#
#     if ! grep -q "DATABASE_URL" "$feature_dir/.env"; then
#       log_warning "Missing DATABASE_URL in .env"
#       log_info "Will use default connection settings"
#     fi
#
#     return 0
#   }
module_validate() {
  log_error "module_validate() not implemented"
  return 1
}

# Optional module functions (not required but recommended)

# module_get_name() - Get human-readable module name
#
# Returns:
#   Module name as string
module_get_name() {
  echo "${MODULE_NAME:-unknown}"
}

# module_get_description() - Get module description
#
# Returns:
#   Module description as string
module_get_description() {
  echo "${MODULE_DESCRIPTION:-No description available}"
}

# module_get_version() - Get module version
#
# Returns:
#   Module version as string
module_get_version() {
  echo "${MODULE_VERSION:-1.0.0}"
}

# module_get_dependencies() - Get list of module dependencies
#
# Returns:
#   Space-separated list of module names
module_get_dependencies() {
  echo "${MODULE_DEPENDENCIES[@]:-}"
}

# module_health_check() - Check if module resources are healthy
#
# Args:
#   $1 - Feature worktree directory
#
# Returns:
#   0 if healthy
#   1 if unhealthy
#
# Optional function for health checking module resources
module_health_check() {
  return 0
}

# Module Helper Functions
# These are utility functions available to all modules

# module_log_step() - Log a module step with consistent formatting
#
# Args:
#   $1 - Module name
#   $2 - Step description
module_log_step() {
  local module_name="$1"
  local step_desc="$2"
  log_info "[${module_name}] ${step_desc}"
}

# module_log_success() - Log a module success message
#
# Args:
#   $1 - Module name
#   $2 - Success message
module_log_success() {
  local module_name="$1"
  local message="$2"
  log_success "[${module_name}] ${message}"
}

# module_log_error() - Log a module error message
#
# Args:
#   $1 - Module name
#   $2 - Error message
module_log_error() {
  local module_name="$1"
  local message="$2"
  log_error "[${module_name}] ${message}"
}

# module_log_warning() - Log a module warning message
#
# Args:
#   $1 - Module name
#   $2 - Warning message
module_log_warning() {
  local module_name="$1"
  local message="$2"
  log_warning "[${module_name}] ${message}"
}

# Module Discovery and Loading

# Global array to store loaded modules
declare -a LOADED_MODULES=()

# load_module() - Load a module from file
#
# Args:
#   $1 - Module name (e.g., "database", "tunnel")
#   $2 - Modules directory path
#
# Returns:
#   0 on success
#   1 on failure
load_module() {
  local module_name="$1"
  local modules_dir="$2"
  local module_file="$modules_dir/module-${module_name}.sh"

  if [ ! -f "$module_file" ]; then
    log_error "Module not found: $module_name (looked for $module_file)"
    return 1
  fi

  # Source the module file
  source "$module_file"

  # Add to loaded modules list
  LOADED_MODULES+=("$module_name")

  log_info "Loaded module: $module_name"
  return 0
}

# load_all_modules() - Load all available modules
#
# Args:
#   $1 - Modules directory path
#
# Returns:
#   0 on success
load_all_modules() {
  local modules_dir="$1"

  if [ ! -d "$modules_dir" ]; then
    log_error "Modules directory not found: $modules_dir"
    return 1
  fi

  # Find all module files
  while IFS= read -r -d '' module_file; do
    local module_name=$(basename "$module_file" .sh | sed 's/^module-//')
    load_module "$module_name" "$modules_dir" || true
  done < <(find "$modules_dir" -name "module-*.sh" -type f -print0 2>/dev/null)

  log_info "Loaded ${#LOADED_MODULES[@]} modules"
  return 0
}

# detect_modules() - Detect which modules should be enabled
#
# Args:
#   $1 - Main worktree directory
#
# Returns:
#   0 on success
detect_modules() {
  local main_dir="$1"
  local modules_dir="$(dirname "${BASH_SOURCE[0]}")"
  local enabled_count=0

  log_info "Detecting enabled modules..."

  for module_name in "${LOADED_MODULES[@]}"; do
    local module_file="$modules_dir/module-${module_name}.sh"
    if [ -f "$module_file" ]; then
      # Source the module file to get its specific function definitions
      source "$module_file"
      if module_detect; then
        log_info "  ✓ $module_name (enabled)"
        ((enabled_count++))
      else
        log_info "  ✗ $module_name (disabled)"
      fi
    fi
  done

  log_info "Enabled $enabled_count modules"
  return 0
}

# resolve_module_dependencies() - Resolve module dependencies and return sorted list
#
# Args:
#   None (uses LOADED_MODULES)
#
# Returns:
#   0 on success
#
# NOTE: Simplified for bash 3.2 compatibility (macOS default)
# Full topological sort with circular dependency detection requires bash 4.0+ (associative arrays)
# This version uses a simple approach: check dependencies exist, log warnings if missing
resolve_module_dependencies() {
  log_info "Checking module dependencies..."
  local modules_dir="$(dirname "${BASH_SOURCE[0]}")"

  # Simple dependency check (no topological sort)
  for module_name in "${LOADED_MODULES[@]}"; do
    local module_file="$modules_dir/module-${module_name}.sh"
    if [ ! -f "$module_file" ]; then continue; fi
    source "$module_file"

    local deps=$(module_get_dependencies)

    if [ -n "$deps" ]; then
      for dep in $deps; do
        # Check if dependency is loaded
        if [[ ! " ${LOADED_MODULES[@]} " =~ " ${dep} " ]]; then
          log_warning "Module $module_name depends on $dep, but $dep is not loaded"
        fi
      done
    fi
  done

  log_info "Module dependency resolution complete"
  return 0
}

# run_module_lifecycle() - Run a lifecycle function for all enabled modules
#
# Args:
#   $1 - Lifecycle function name (setup, teardown, validate)
#   $@ - Additional arguments to pass to lifecycle function
#
# Returns:
#   0 if all modules succeeded
#   1 if any module failed
run_module_lifecycle() {
  local lifecycle_func="module_$1"
  shift
  local args=("$@")

  local failed_count=0

  for module in "${LOADED_MODULES[@]}"; do
    if [ "${MODULE_ENABLED:-false}" = "true" ]; then
      log_info "Running ${lifecycle_func} for module: $module"

      if ! "$lifecycle_func" "${args[@]}"; then
        log_error "Module $module ${lifecycle_func} failed"
        ((failed_count++))
      fi
    fi
  done

  if [ $failed_count -gt 0 ]; then
    log_error "$failed_count module(s) failed"
    return 1
  fi

  return 0
}

# Module Configuration System
#
# NOTE: Bash 3.2 compatible version (macOS default)
# Configuration is read from environment variables directly
# No persistent storage in associative arrays (requires bash 4.0+)
#
# Configuration precedence (highest to lowest):
#   1. Environment variables (GIT_WORKTREE_MODULE_*)
#   2. Module defaults

# load_module_config() - Load module configuration from various sources
#
# Args:
#   $1 - Main worktree directory
#
# Returns:
#   0 on success
load_module_config() {
  local main_dir="$1"

  log_info "Module configuration: using environment variables"
  log_info "Set GIT_WORKTREE_MODULE_* variables to configure modules"
  return 0
}

# get_module_config() - Get configuration value for a key
#
# Args:
#   $1 - Configuration key
#   $2 - Default value (optional)
#
# Returns:
#   Configuration value or default
get_module_config() {
  local key="$1"
  local default="${2:-}"

  # Convert key to uppercase env var name
  local env_var="GIT_WORKTREE_MODULE_$(echo "$key" | tr '[:lower:]' '[:upper:]')"

  # Get value from environment or use default
  echo "${!env_var:-$default}"
}

# set_module_config() - Set configuration value for a key
#
# Args:
#   $1 - Configuration key
#   $2 - Configuration value
#
# NOTE: In bash 3.2 compatible mode, this exports environment variables
set_module_config() {
  local key="$1"
  local value="$2"

  # Convert key to uppercase env var name
  local env_var="GIT_WORKTREE_MODULE_$(echo "$key" | tr '[:lower:]' '[:upper:]')"

  # Export as environment variable
  export "${env_var}=${value}"
}

# is_module_enabled_by_config() - Check if a module is enabled by configuration
#
# Args:
#   $1 - Module name
#
# Returns:
#   0 if enabled
#   1 if disabled
#   2 if not configured (let module_detect decide)
is_module_enabled_by_config() {
  local module_name="$1"

  # Check if module is explicitly enabled
  local enabled=$(get_module_config "${module_name}_enabled")

  if [ "$enabled" = "true" ] || [ "$enabled" = "1" ]; then
    return 0
  elif [ "$enabled" = "false" ] || [ "$enabled" = "0" ]; then
    return 1
  fi

  # Not configured - let module_detect decide
  return 2
}
