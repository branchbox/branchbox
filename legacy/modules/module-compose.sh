#!/bin/bash
# Docker Compose Module
# Manages Docker Compose configuration for feature worktrees
#
# This module handles:
# - Container naming validation and uniqueness
# - Compose configuration validation
# - Network isolation per worktree
# - Environment variable validation

# Module metadata
MODULE_NAME="compose"
MODULE_DESCRIPTION="Docker Compose configuration management"
MODULE_VERSION="1.0.0"
MODULE_ENABLED=false
MODULE_DEPENDENCIES=()

# Module configuration (set during module_init)
MODULE_COMPOSE_PROJECT_NAME=""
MODULE_DEVCONTAINER_NAME=""
MODULE_COMPOSE_FILE=""

# Implementation of module interface

module_detect() {
  # Compose module is enabled if docker compose file exists
  if [ -f ".devcontainer/compose.yaml" ] || [ -f ".devcontainer/docker-compose.yml" ]; then
    MODULE_ENABLED=true
    MODULE_CONFIDENCE=100
    return 0
  fi

  # Check for Dockerfile-only setup
  if [ -f ".devcontainer/Dockerfile" ]; then
    MODULE_ENABLED=true
    MODULE_CONFIDENCE=70
    return 0
  fi

  MODULE_ENABLED=false
  return 1
}

module_init() {
  local main_dir="$1"
  local feature_dir="$2"
  local config="${3:-}"

  local work_feature=$(basename "$feature_dir")

  # Set compose project name and devcontainer name
  MODULE_COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-${BASE_PREFIX:-app}-${work_feature}}"
  MODULE_DEVCONTAINER_NAME="${DEVCONTAINER_NAME:-${BASE_PREFIX:-app}-${work_feature}}"

  # Find compose file
  if [ -f "$main_dir/.devcontainer/compose.yaml" ]; then
    MODULE_COMPOSE_FILE="$feature_dir/.devcontainer/compose.yaml"
  elif [ -f "$main_dir/.devcontainer/docker-compose.yml" ]; then
    MODULE_COMPOSE_FILE="$feature_dir/.devcontainer/docker-compose.yml"
  else
    MODULE_COMPOSE_FILE=""
  fi

  module_log_step "$MODULE_NAME" "Project name: $MODULE_COMPOSE_PROJECT_NAME"
  module_log_step "$MODULE_NAME" "Devcontainer name: $MODULE_DEVCONTAINER_NAME"

  return 0
}

module_setup() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Validating Docker Compose configuration..."

  # Validate that COMPOSE_PROJECT_NAME is set in .env
  if [ -f "$feature_dir/.env" ]; then
    if ! grep -q "^COMPOSE_PROJECT_NAME=" "$feature_dir/.env" 2>/dev/null; then
      module_log_warning "$MODULE_NAME" "COMPOSE_PROJECT_NAME not found in .env"
      module_log_step "$MODULE_NAME" "This should have been set by feature-start"
    fi

    if ! grep -q "^DEVCONTAINER_NAME=" "$feature_dir/.env" 2>/dev/null; then
      module_log_warning "$MODULE_NAME" "DEVCONTAINER_NAME not found in .env"
      module_log_step "$MODULE_NAME" "This should have been set by feature-start"
    fi
  fi

  # Validate compose configuration (if compose file exists)
  if [ -n "$MODULE_COMPOSE_FILE" ] && [ -f "$MODULE_COMPOSE_FILE" ]; then
    _validate_compose_config "$feature_dir"
  else
    module_log_step "$MODULE_NAME" "No compose file found, skipping validation"
  fi

  # Check for container name conflicts
  _check_container_conflicts

  module_log_success "$MODULE_NAME" "Compose configuration validated"

  return 0
}

module_teardown() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Stopping and removing containers..."

  # Stop and remove containers using docker compose
  if [ -d "$feature_dir" ] && [ -f "$feature_dir/.devcontainer/compose.yaml" ]; then
    cd "$feature_dir"

    if docker compose -f .devcontainer/compose.yaml ps --quiet 2>/dev/null | grep -q .; then
      module_log_step "$MODULE_NAME" "Stopping containers for $MODULE_COMPOSE_PROJECT_NAME..."

      if docker compose -f .devcontainer/compose.yaml down -v 2>&1; then
        module_log_success "$MODULE_NAME" "Containers stopped and removed"
      else
        module_log_warning "$MODULE_NAME" "Failed to stop some containers"
      fi
    else
      module_log_step "$MODULE_NAME" "No running containers found"
    fi

    cd "$main_dir"
  else
    module_log_step "$MODULE_NAME" "No compose file found, skipping container cleanup"
  fi

  # Clean up any orphaned containers with matching project name
  _cleanup_orphaned_containers

  return 0
}

module_validate() {
  local main_dir="$1"
  local feature_dir="$2"

  # Validate compose file syntax
  if [ -n "$MODULE_COMPOSE_FILE" ] && [ -f "$MODULE_COMPOSE_FILE" ]; then
    if ! docker compose -f "$MODULE_COMPOSE_FILE" config > /dev/null 2>&1; then
      module_log_error "$MODULE_NAME" "Docker Compose configuration is invalid"
      echo ""
      echo -e "${YELLOW}Running validation with detailed output:${NC}"
      docker compose -f "$MODULE_COMPOSE_FILE" config 2>&1 | head -20
      echo ""
      module_log_step "$MODULE_NAME" "Common issues:"
      module_log_step "$MODULE_NAME" "  - Check .cloudflared.env for invalid characters"
      module_log_step "$MODULE_NAME" "  - Verify all environment files are properly formatted"
      module_log_step "$MODULE_NAME" "  - Ensure no stray API responses in environment variables"
      return 1
    fi

    module_log_success "$MODULE_NAME" "Compose configuration is valid"
  fi

  return 0
}

module_get_name() {
  echo "$MODULE_NAME"
}

module_get_description() {
  echo "$MODULE_DESCRIPTION"
}

module_get_version() {
  echo "$MODULE_VERSION"
}

module_health_check() {
  local feature_dir="$1"

  # Check if any containers are running
  if [ -f "$feature_dir/.devcontainer/compose.yaml" ]; then
    cd "$feature_dir"

    if docker compose -f .devcontainer/compose.yaml ps --quiet 2>/dev/null | grep -q .; then
      return 0
    fi

    cd - > /dev/null
  fi

  return 1
}

# Module-specific helper functions

# _validate_compose_config() - Validate Docker Compose configuration
#
# Args:
#   $1 - Feature worktree directory
_validate_compose_config() {
  local feature_dir="$1"

  cd "$feature_dir"

  if docker compose -f .devcontainer/compose.yaml config > /dev/null 2>&1; then
    module_log_step "$MODULE_NAME" "Docker Compose configuration is valid"
  else
    module_log_error "$MODULE_NAME" "Docker Compose configuration validation failed"
    echo ""
    echo -e "${YELLOW}Running validation with detailed output:${NC}"
    docker compose -f .devcontainer/compose.yaml config 2>&1 | head -20
    echo ""
    module_log_step "$MODULE_NAME" "Common issues:"
    module_log_step "$MODULE_NAME" "  - Check .cloudflared.env for invalid characters"
    module_log_step "$MODULE_NAME" "  - Verify all environment files are properly formatted"
    module_log_step "$MODULE_NAME" "  - Ensure no stray API responses in environment variables"
  fi

  cd - > /dev/null
}

# _check_container_conflicts() - Check for container name conflicts
_check_container_conflicts() {
  # Check if containers with the same project name are already running
  local running_containers=$(docker ps --filter "label=com.docker.compose.project=$MODULE_COMPOSE_PROJECT_NAME" --format "{{.Names}}" 2>/dev/null)

  if [ -n "$running_containers" ]; then
    module_log_warning "$MODULE_NAME" "Containers with project name '$MODULE_COMPOSE_PROJECT_NAME' are already running:"
    echo "$running_containers" | sed 's/^/  - /'
    module_log_step "$MODULE_NAME" "These will be stopped during teardown"
  else
    module_log_step "$MODULE_NAME" "No container name conflicts detected"
  fi
}

# _cleanup_orphaned_containers() - Clean up orphaned containers
_cleanup_orphaned_containers() {
  # Find and remove any containers with matching project name
  local orphaned=$(docker ps -a --filter "label=com.docker.compose.project=$MODULE_COMPOSE_PROJECT_NAME" --format "{{.ID}}" 2>/dev/null)

  if [ -n "$orphaned" ]; then
    module_log_step "$MODULE_NAME" "Removing orphaned containers..."
    echo "$orphaned" | xargs docker rm -f 2>/dev/null || true
    module_log_success "$MODULE_NAME" "Orphaned containers removed"
  else
    module_log_step "$MODULE_NAME" "No orphaned containers found"
  fi

  # Clean up orphaned networks
  local orphaned_networks=$(docker network ls --filter "label=com.docker.compose.project=$MODULE_COMPOSE_PROJECT_NAME" --format "{{.ID}}" 2>/dev/null)

  if [ -n "$orphaned_networks" ]; then
    module_log_step "$MODULE_NAME" "Removing orphaned networks..."
    echo "$orphaned_networks" | xargs docker network rm 2>/dev/null || true
    module_log_success "$MODULE_NAME" "Orphaned networks removed"
  fi
}
