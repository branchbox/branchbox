#!/usr/bin/env bash
set -euo pipefail

# Quick sanity check: make sure we're inside a git worktree
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  exit 0
fi

WORK_FEATURE="${WORK_FEATURE:-main}"
MAIN_NAME="${BRANCHBOX_MAIN_NAME:-main}"

# Main workspace includes its own .git directory; nothing to repair.
if [[ "${WORK_FEATURE}" == "main" ]]; then
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKTREE_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
GIT_FILE="${WORKTREE_DIR}/.git"

if [[ -d "${GIT_FILE}" ]]; then
  echo "BranchBox warning: ${WORKTREE_DIR} has a .git directory instead of a worktree pointer; manual cleanup required." >&2
  exit 0
fi

if [[ ! -e "${GIT_FILE}" ]]; then
  echo "BranchBox warning: missing .git file in ${WORKTREE_DIR}; recreating worktree link." >&2
fi

cat > "${GIT_FILE}" <<EOF
gitdir: ../${MAIN_NAME}/.git/worktrees/${WORK_FEATURE}
EOF

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "BranchBox: repaired git worktree link for ${WORK_FEATURE}."
else
  echo "BranchBox warning: git worktree still unavailable after repair attempt." >&2
fi
