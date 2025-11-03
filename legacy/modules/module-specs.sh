#!/bin/bash
# Feature Specs Module
# Manages feature specification lifecycle tracking
#
# This module handles:
# - Feature spec discovery and search
# - Feature spec creation
# - Feature spec lifecycle management (backlog → in-progress → completed)
# - Feature spec frontmatter updating

# Module metadata
MODULE_NAME="specs"
MODULE_DESCRIPTION="Feature specification lifecycle tracking"
MODULE_VERSION="1.0.0"
MODULE_ENABLED=false
MODULE_DEPENDENCIES=()

# Module configuration (set during module_init)
MODULE_SPECS_DIR=""
MODULE_SPEC_FILE=""
MODULE_FEATURE_TITLE=""
MODULE_WORK_FEATURE=""

# Implementation of module interface

module_detect() {
  # Specs module is optional and enabled by configuration
  # Check if feature specs directory exists or should be created
  if [ -d "${FEATURES_DIR:-}" ]; then
    MODULE_ENABLED=true
    MODULE_CONFIDENCE=100
    return 0
  fi

  # Default to enabled if FEATURES_DIR is set
  if [ -n "${FEATURES_DIR:-}" ]; then
    MODULE_ENABLED=true
    MODULE_CONFIDENCE=80
    return 0
  fi

  MODULE_ENABLED=false
  return 1
}

module_init() {
  local main_dir="$1"
  local feature_dir="$2"
  local config="${3:-}"

  # Set default specs directory if not already set
  if [ -z "${FEATURES_DIR:-}" ]; then
    MODULE_SPECS_DIR="$main_dir/docs/features"
  else
    MODULE_SPECS_DIR="${FEATURES_DIR}"
  fi

  # Create directory structure if it doesn't exist
  mkdir -p "$MODULE_SPECS_DIR/backlog" "$MODULE_SPECS_DIR/in-progress" "$MODULE_SPECS_DIR/completed"

  module_log_step "$MODULE_NAME" "Initialized specs directory: $MODULE_SPECS_DIR"
  return 0
}

module_setup() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Setting up feature specification..."

  # The actual discovery and creation should be done before module_setup
  # This function handles the final setup (moving to in-progress, updating frontmatter)

  if [ -z "$MODULE_SPEC_FILE" ]; then
    module_log_warning "$MODULE_NAME" "No spec file set, skipping setup"
    return 0
  fi

  # Ensure spec is in in-progress directory
  local spec_basename=$(basename "$MODULE_SPEC_FILE")
  local spec_in_progress="$MODULE_SPECS_DIR/in-progress/$spec_basename"

  if [ "$MODULE_SPEC_FILE" != "$spec_in_progress" ] && [ -f "$MODULE_SPEC_FILE" ]; then
    # Move spec to in-progress
    mv "$MODULE_SPEC_FILE" "$spec_in_progress"
    MODULE_SPEC_FILE="$spec_in_progress"
    module_log_step "$MODULE_NAME" "Moved spec to in-progress"
  fi

  # Update frontmatter with worktree information
  if [ -f "$MODULE_SPEC_FILE" ]; then
    _update_spec_frontmatter "$MODULE_SPEC_FILE" "$feature_dir"
    module_log_success "$MODULE_NAME" "Feature spec ready: $MODULE_SPEC_FILE"
  fi

  return 0
}

module_teardown() {
  local main_dir="$1"
  local feature_dir="$2"

  module_log_step "$MODULE_NAME" "Handling feature specification..."

  # Find the spec file based on work feature name
  local work_feature=$(basename "$feature_dir")
  local spec_file="$MODULE_SPECS_DIR/in-progress/${work_feature}.md"

  if [ ! -f "$spec_file" ]; then
    module_log_step "$MODULE_NAME" "No feature spec found"
    return 0
  fi

  # Ask user if they want to move to completed (only if not in auto-accept mode)
  if [ "${ACCEPT_DEFAULTS:-false}" = "false" ]; then
    echo ""
    if confirm "Move feature spec to completed?" "n"; then
      mkdir -p "$MODULE_SPECS_DIR/completed"
      mv "$spec_file" "$MODULE_SPECS_DIR/completed/"
      module_log_success "$MODULE_NAME" "Feature spec moved to completed"
    else
      module_log_step "$MODULE_NAME" "Feature spec remains in in-progress"
    fi
  else
    module_log_step "$MODULE_NAME" "Auto-accept mode: keeping spec in in-progress"
  fi

  return 0
}

module_validate() {
  local main_dir="$1"
  local feature_dir="$2"

  # Validate that specs directory exists
  if [ ! -d "$MODULE_SPECS_DIR" ]; then
    module_log_error "$MODULE_NAME" "Specs directory not found: $MODULE_SPECS_DIR"
    return 1
  fi

  # Validate directory structure
  for subdir in backlog in-progress completed; do
    if [ ! -d "$MODULE_SPECS_DIR/$subdir" ]; then
      module_log_warning "$MODULE_NAME" "Missing directory: $MODULE_SPECS_DIR/$subdir"
      mkdir -p "$MODULE_SPECS_DIR/$subdir"
      module_log_step "$MODULE_NAME" "Created $subdir directory"
    fi
  done

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

# Module-specific helper functions

# spec_discover() - Search for feature specs matching query
#
# Args:
#   $1 - Feature query (search term)
#
# Sets:
#   MODULE_SPEC_FILE - Path to selected spec file
#   MODULE_FEATURE_TITLE - Title of the feature
#
# Returns:
#   0 if spec found or created
#   1 if user cancelled
spec_discover() {
  local feature_query="$1"

  module_log_step "$MODULE_NAME" "Searching for: '$feature_query'"

  # Search in backlog
  local feature_files=()
  if [ -d "$MODULE_SPECS_DIR/backlog" ]; then
    # Search by content (case-insensitive)
    while IFS= read -r -d '' file; do
      feature_files+=("$file")
    done < <(find "$MODULE_SPECS_DIR/backlog" -type f -name "*.md" -print0 | \
      xargs -0 grep -il "$(echo "$feature_query" | tr '[:upper:]' '[:lower:]')" 2>/dev/null || true)

    # Search by filename
    while IFS= read -r -d '' file; do
      if [[ ! " ${feature_files[@]:-} " =~ " ${file} " ]]; then
        feature_files+=("$file")
      fi
    done < <(find "$MODULE_SPECS_DIR/backlog" -type f -name "*$(echo "$feature_query" | tr ' ' '-' | tr '[:upper:]' '[:lower:]')*.md" -print0 || true)
  fi

  # Handle search results
  if [ ${#feature_files[@]} -eq 0 ]; then
    log_info "No matching feature found in backlog"

    if confirm "Create new feature specification?" "y"; then
      _create_new_spec "$feature_query"
      return 0
    else
      log_error "Feature specification required to proceed"
      return 1
    fi

  elif [ ${#feature_files[@]} -eq 1 ]; then
    MODULE_SPEC_FILE="${feature_files[0]}"
    log_success "Found: $(basename "$MODULE_SPEC_FILE")"
    _extract_feature_title "$MODULE_SPEC_FILE"
    return 0

  else
    # Multiple matches - let user choose
    echo -e "\n${CYAN}Multiple matches found:${NC}"
    for i in "${!feature_files[@]}"; do
      echo "  $((i+1)). $(basename "${feature_files[$i]}")"
    done

    read -p "Select feature (1-${#feature_files[@]}): " -r selection

    if [[ "$selection" =~ ^[0-9]+$ ]] && [ "$selection" -ge 1 ] && [ "$selection" -le "${#feature_files[@]}" ]; then
      MODULE_SPEC_FILE="${feature_files[$((selection-1))]}"
      log_success "Selected: $(basename "$MODULE_SPEC_FILE")"
      _extract_feature_title "$MODULE_SPEC_FILE"
      return 0
    else
      log_error "Invalid selection"
      return 1
    fi
  fi
}

# _create_new_spec() - Create a new feature specification file
#
# Args:
#   $1 - Feature title/query
#
# Sets:
#   MODULE_SPEC_FILE - Path to created spec file
#   MODULE_FEATURE_TITLE - Title of the feature
_create_new_spec() {
  local feature_title="$1"
  local feature_filename=$(echo "$feature_title" | tr '[:upper:]' '[:lower:]' | tr ' ' '-' | sed 's/[^a-z0-9-]//g')

  MODULE_SPEC_FILE="$MODULE_SPECS_DIR/in-progress/${feature_filename}.md"
  MODULE_FEATURE_TITLE="$feature_title"

  # Don't create the file yet - just set the variables
  # The file will be created during module_setup
  log_success "Will create: $MODULE_SPEC_FILE"
}

# _extract_feature_title() - Extract feature title from spec file
#
# Args:
#   $1 - Spec file path
#
# Sets:
#   MODULE_FEATURE_TITLE - Title of the feature
_extract_feature_title() {
  local spec_file="$1"

  if [ -f "$spec_file" ]; then
    MODULE_FEATURE_TITLE=$(grep -m1 '^# ' "$spec_file" | sed 's/^# //' || basename "$spec_file" .md)
  else
    MODULE_FEATURE_TITLE=$(basename "$spec_file" .md)
  fi
}

# _update_spec_frontmatter() - Update spec frontmatter with worktree info
#
# Args:
#   $1 - Spec file path
#   $2 - Feature worktree directory
_update_spec_frontmatter() {
  local spec_file="$1"
  local feature_dir="$2"
  local work_feature=$(basename "$feature_dir")
  local branch_name="${BRANCH_NAME:-feature/$work_feature}"
  local feature_url="${FEATURE_URL:-unknown}"

  # Create temp file for new content
  local temp_file="${spec_file}.tmp"

  # Write frontmatter
  cat > "$temp_file" <<EOF
---
worktree: $feature_dir
branch: $branch_name
work_feature: $work_feature
url: https://$feature_url
status: in_progress
created: $(date +%Y-%m-%d)
---

EOF

  # Append existing content (skip old frontmatter if present)
  if grep -q "^---$" "$spec_file" 2>/dev/null; then
    awk 'BEGIN{p=0} /^---$/{p=p+1; next} p>1' "$spec_file" >> "$temp_file"
  else
    # No existing frontmatter, append entire file
    if [ -f "$spec_file" ]; then
      cat "$spec_file" >> "$temp_file"
    else
      # Brand new file, create template
      cat >> "$temp_file" <<EOF
# $MODULE_FEATURE_TITLE

## Overview

[Add feature description here]

## Requirements

- [ ] Requirement 1
- [ ] Requirement 2

## Implementation Plan

### Phase 1: [Phase name]

- [ ] Task 1
- [ ] Task 2

## Testing Strategy

- [ ] Unit tests
- [ ] Integration tests
- [ ] System tests

## Documentation

- [ ] Update README
- [ ] API documentation
- [ ] User guide

## Success Criteria

- [ ] Criterion 1
- [ ] Criterion 2
EOF
    fi
  fi

  # Move temp file to replace original
  mv "$temp_file" "$spec_file"
}

# Export functions for use by main scripts
export -f spec_discover
