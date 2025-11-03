#!/bin/bash
# Git worktree operations for git-worktree workflow
# Handles worktree creation, removal, and management

# Source logging for error messages
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../utils/logging.sh" 2>/dev/null || true

# Create git worktree with branch
# Usage: create_worktree "/path/to/main" "feature-name" "feature/branch-name" ["base-branch"]
# Args:
#   $1 - main directory path
#   $2 - work feature name (directory name)
#   $3 - branch name to create/use
#   $4 - (optional) base branch to branch from (defaults to current HEAD)
# Returns: 0 on success, 1 on failure
create_worktree() {
  local main_dir="$1"
  local work_feature="$2"
  local branch_name="$3"
  local base_branch="${4:-}"
  local worktree_path="$(dirname "$main_dir")/$work_feature"

  cd "$main_dir" || return 1

  # Check if branch exists locally
  if git show-ref --verify --quiet "refs/heads/$branch_name"; then
    # Branch exists locally
    git worktree add "../$work_feature" "$branch_name"
  elif git show-ref --verify --quiet "refs/remotes/origin/$branch_name"; then
    # Branch exists remotely
    git worktree add "../$work_feature" -b "$branch_name" "origin/$branch_name"
  else
    # Create new branch
    if [ -n "$base_branch" ]; then
      # Branch from specified base branch
      git worktree add -b "$branch_name" "../$work_feature" "$base_branch"
    else
      # Branch from current HEAD
      git worktree add -b "$branch_name" "../$work_feature"
    fi
  fi

  # Convert absolute path to relative path in .git file
  # This makes it work in both container and host environments
  if [ -f "$worktree_path/.git" ]; then
    # Find the ACTUAL main worktree (the one with .git as a directory, not a file)
    # This handles the case where we're creating a worktree from a feature branch
    local actual_main_dir="$main_dir"
    if [ -f "$main_dir/.git" ]; then
      # main_dir is itself a worktree, find the actual main
      local parent_dir=$(dirname "$main_dir")
      if [ -d "$parent_dir/main/.git" ] && [ ! -f "$parent_dir/main/.git" ]; then
        actual_main_dir="$parent_dir/main"
      fi
    fi

    local main_name=$(basename "$actual_main_dir")
    echo "gitdir: ../$main_name/.git/worktrees/$work_feature" > "$worktree_path/.git"
  fi

  return 0
}

# Remove git worktree
# Usage: remove_worktree "/path/to/worktree"
# Returns: 0 on success, 1 on failure
remove_worktree() {
  local worktree_path="$1"

  if [ ! -d "$worktree_path" ]; then
    return 1
  fi

  # Remove from git worktree list
  if git worktree list | grep -q "$worktree_path"; then
    git worktree remove "$worktree_path" --force 2>/dev/null || true
  fi

  # Remove directory if still exists
  if [ -d "$worktree_path" ]; then
    rm -rf "$worktree_path"
  fi

  # Clean up git metadata
  local worktree_name=$(basename "$worktree_path")
  local main_dir=$(git worktree list | grep "(bare)" | awk '{print $1}' || git rev-parse --show-toplevel)
  local git_metadata="$main_dir/.git/worktrees/$worktree_name"

  if [ -d "$git_metadata" ]; then
    rm -rf "$git_metadata"
  fi

  return 0
}

# Check if worktree exists and is valid
# Usage: worktree_exists "/path/to/worktree" && echo "exists"
# Returns: 0 if exists and valid, 1 otherwise
worktree_exists() {
  local worktree_path="$1"

  if [ ! -d "$worktree_path" ]; then
    return 1
  fi

  if [ -f "$worktree_path/.git" ] && git worktree list | grep -q "$worktree_path"; then
    return 0
  fi

  return 1
}

# Get current branch of a worktree
# Usage: branch=$(get_worktree_branch "/path/to/worktree")
get_worktree_branch() {
  local worktree_path="$1"

  if [ ! -d "$worktree_path" ]; then
    return 1
  fi

  (cd "$worktree_path" && git rev-parse --abbrev-ref HEAD)
}

# Create git worktree path fix script for devcontainer
# This fixes the .git file to use relative paths that work in both host and container
# Usage: create_git_fix_script "/path/to/worktree"
create_git_fix_script() {
  local worktree_path="$1"
  local worktree_name=$(basename "$worktree_path")

  cat > "$worktree_path/.devcontainer/fix-git-worktree.sh" <<'GITFIX'
#!/bin/bash
# Fix git worktree path for devcontainer
# This script is automatically created by git-worktree workflow

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKTREE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKTREE_NAME=$(basename "$WORKTREE_DIR")
GIT_FILE="$WORKTREE_DIR/.git"

if [ -f "$GIT_FILE" ]; then
  # Read current gitdir path
  CURRENT_PATH=$(cat "$GIT_FILE" | sed 's/gitdir: //')

  # Find the main worktree directory name
  PARENT_DIR=$(dirname "$WORKTREE_DIR")
  MAIN_NAME=""

  if [ -d "$PARENT_DIR/main" ]; then
    MAIN_NAME="main"
  elif [[ "$CURRENT_PATH" =~ /([^/]+)/.git/worktrees/ ]]; then
    # Extract from current path
    MAIN_NAME="${BASH_REMATCH[1]}"
  fi

  # Default to "main" if we couldn't determine
  MAIN_NAME="${MAIN_NAME:-main}"

  # Use RELATIVE path - works in both container and host!
  CORRECT_PATH="../$MAIN_NAME/.git/worktrees/$WORKTREE_NAME"

  # Update .git file to point to relative path
  echo "gitdir: $CORRECT_PATH" > "$GIT_FILE"
  echo "✓ Fixed git worktree path (using relative path)"
else
  echo "⚠ No .git file found"
fi
GITFIX

  chmod +x "$worktree_path/.devcontainer/fix-git-worktree.sh"
}
