#!/usr/bin/env bash

set -euo pipefail

MODE="regular"
SCRIPT_VERBOSE=0
PRETEND=0

usage() {
  cat <<'USAGE'
Usage: manual-cli-e2e.sh [--mode regular|verbose|pretend]
       manual-cli-e2e.sh [--verbose] [--pretend]

Modes:
  regular   Default; runs the full workflow with real containers.
  verbose   Same as regular but with shell tracing and extra BranchBox logs.
  pretend   Dry-run. Logs each step without invoking BranchBox or Docker.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--mode)
      shift
      MODE="${1:-}"
      ;;
    --mode=*)
      MODE="${1#*=}"
      ;;
    -v|--verbose)
      MODE="verbose"
      ;;
    -p|--pretend|--dry-run)
      MODE="pretend"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

MODE="${MODE,,}"
case "$MODE" in
  regular) ;;
  verbose) SCRIPT_VERBOSE=1 ;;
  pretend) PRETEND=1 ;;
  *)
    echo "Invalid mode: $MODE" >&2
    usage
    exit 1
    ;;
esac

if [[ "$SCRIPT_VERBOSE" == "1" ]]; then
  set -x
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRANCHBOX_BIN="${BRANCHBOX_BIN:-"$REPO_ROOT/target/debug/branchbox"}"
PROJECT_NAME="${PROJECT_NAME:-cli-e2e-sample}"
FEATURE_NAME="${FEATURE_NAME:-cli-e2e-smoke}"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/branchbox-cli-e2e-XXXXXX")"
SOURCE_PARENT="$TMP_ROOT/source"
TARGET_PARENT="$TMP_ROOT/workspaces"
SOURCE_DIR="$SOURCE_PARENT/$PROJECT_NAME"
EXPECTED_CONTAINER="$TARGET_PARENT/$PROJECT_NAME"
LOG_DIR="$TMP_ROOT/logs"
mkdir -p "$SOURCE_DIR/src" "$LOG_DIR"

BUGS=()
COMPOSE_STACKS=()

function cleanup() {
  if [[ "${PRETEND}" == "1" ]]; then
    if [[ "${KEEP_E2E_TMP:-0}" != "1" ]]; then
      rm -rf "$TMP_ROOT"
    fi
    return
  fi

  for compose_file in "${COMPOSE_STACKS[@]}"; do
    docker compose -f "$compose_file" \
      --project-directory "$(dirname "$compose_file")" \
      down -v --remove-orphans >/dev/null 2>&1 || true
  done

  if [[ "${KEEP_E2E_TMP:-0}" == "1" ]]; then
    echo "ℹ️  Preserving artifacts under $TMP_ROOT"
  else
    rm -rf "$TMP_ROOT"
  fi
}
trap cleanup EXIT

function log() {
  printf '==> %s\n' "$*"
}

function record_bug() {
  BUGS+=("$1")
  printf '!! %s\n' "$1" >&2
}

function fatal() {
  record_bug "$1"
  report_results
  exit 1
}

function require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fatal "Missing required command: $1"
}

function pretend_step() {
  log "[pretend] $*"
}

function report_results() {
  echo
  if ((${#BUGS[@]} == 0)); then
    echo "✅ CLI manual e2e test passed."
    if [[ "${KEEP_E2E_TMP:-0}" == "1" ]]; then
      echo "Artifacts available at $TMP_ROOT"
    fi
  else
    echo "❌ Detected ${#BUGS[@]} issue(s):"
    for bug in "${BUGS[@]}"; do
      echo " - $bug"
    done
    echo "Re-run with KEEP_E2E_TMP=1 to preserve $LOG_DIR for debugging."
  fi
}

function assert_file_exists() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    record_bug "Expected file missing: $path"
  fi
}

function assert_file_contains() {
  local path="$1"
  local needle="$2"
  if [[ ! -f "$path" ]]; then
    record_bug "Expected file missing for content check: $path"
    return
  fi
  if ! grep -q "$needle" "$path"; then
    record_bug "File $path missing expected content: $needle"
  fi
}

function assert_file_not_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    record_bug "Expected file to be absent but found: $path"
  fi
}

require_cmd bash
require_cmd git
require_cmd cargo
require_cmd jq
if [[ "$PRETEND" == "0" ]]; then
  require_cmd docker
fi

log "Running manual CLI e2e in '$MODE' mode"

log "Building branchbox CLI"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "cargo build -p branchbox-cli"
else
  (cd "$REPO_ROOT" && cargo build -p branchbox-cli >/dev/null) || fatal "cargo build -p branchbox-cli failed"
fi

export GIT_AUTHOR_NAME="${GIT_AUTHOR_NAME:-BranchBox Tester}"
export GIT_AUTHOR_EMAIL="${GIT_AUTHOR_EMAIL:-branchbox-tester@example.com}"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

log "Seeding disposable Rust repository at $SOURCE_DIR"
cat >"$SOURCE_DIR/Cargo.toml" <<'EOF_CARGO'
[package]
name = "cli-e2e-sample"
version = "0.1.0"
edition = "2021"

[dependencies]
EOF_CARGO

cat >"$SOURCE_DIR/src/main.rs" <<'EOF_MAIN'
fn main() {
    println!("BranchBox CLI e2e smoke test");
}
EOF_MAIN

(cd "$SOURCE_DIR" && git init -b main >/dev/null)
(cd "$SOURCE_DIR" && git add . >/dev/null && git commit -m "Seed sample project" >/dev/null)

log "Running branchbox init (forcing reorganize + parent structure)"
INIT_LOG="$LOG_DIR/init.log"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "BRANCHBOX_SKIP_HOST_VALIDATION=1 BRANCHBOX_PROJECTS_DIR=$TARGET_PARENT $BRANCHBOX_BIN init --stack rust --reorganize --use-parent-structure -y"
else
  if ! (cd "$SOURCE_DIR" && BRANCHBOX_SKIP_HOST_VALIDATION=1 BRANCHBOX_PROJECTS_DIR="$TARGET_PARENT" "$BRANCHBOX_BIN" init --stack rust --reorganize --use-parent-structure -y) | tee "$INIT_LOG"; then
    fatal "branchbox init failed (see $INIT_LOG)"
  fi
fi

CONTAINER_DIR=""
MAIN_DIR=""
if [[ "$PRETEND" == "1" ]]; then
  CONTAINER_DIR="$EXPECTED_CONTAINER"
  MAIN_DIR="$EXPECTED_CONTAINER/main"
else
  if [[ -d "$EXPECTED_CONTAINER/main/.git" ]]; then
    CONTAINER_DIR="$EXPECTED_CONTAINER"
    MAIN_DIR="$EXPECTED_CONTAINER/main"
  else
    if [[ -d "$EXPECTED_CONTAINER/.git" ]]; then
      CONTAINER_DIR="$EXPECTED_CONTAINER"
      MAIN_DIR="$EXPECTED_CONTAINER"
      record_bug "init did not create 'main/' worktree parent structure under $EXPECTED_CONTAINER"
    else
      FOUND_GIT="$(find "$TARGET_PARENT" -maxdepth 3 -type d -name .git -print -quit)"
      if [[ -n "$FOUND_GIT" ]]; then
        MAIN_DIR="$(dirname "$FOUND_GIT")"
        CONTAINER_DIR="$(dirname "$MAIN_DIR")"
        record_bug "reorganized repo landed at $MAIN_DIR but lacks expected main/ layout"
      else
        fatal "Unable to locate git repo after branchbox init (expected under $EXPECTED_CONTAINER)"
      fi
    fi
  fi
fi

log "Detected container dir: $CONTAINER_DIR"
log "Detected main worktree: $MAIN_DIR"

if [[ "$PRETEND" == "0" ]]; then
  if [[ ! -d "$MAIN_DIR/.devcontainer" ]]; then
    record_bug "branchbox init did not generate .devcontainer inside $MAIN_DIR"
  fi

  if [[ ! -f "$MAIN_DIR/.branchbox/registry.json" ]]; then
    record_bug "branchbox registry missing at $MAIN_DIR/.branchbox/registry.json"
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  if [[ ! -f "$MAIN_DIR/.env" ]]; then
    if [[ -f "$MAIN_DIR/.env.sample" ]]; then
      cp "$MAIN_DIR/.env.sample" "$MAIN_DIR/.env"
    else
      record_bug "missing .env.sample in $MAIN_DIR; creating minimal stub"
      cat >"$MAIN_DIR/.env" <<'EOF_ENV'
APP_URL=http://localhost:3000
COMPOSE_PROJECT_NAME=cli-e2e-main
EOF_ENV
    fi
  fi
else
  pretend_step "Would ensure .env is present"
fi

FEATURES_DIR_PATH="$MAIN_DIR/docs/features"
if [[ "$PRETEND" == "0" ]]; then
  mkdir -p "$FEATURES_DIR_PATH/backlog"
  cat >"$FEATURES_DIR_PATH/backlog/$FEATURE_NAME.md" <<EOF
---
status: backlog
title: CLI E2E Spec
---

# Placeholder
EOF
else
  pretend_step "Would seed backlog spec at $FEATURES_DIR_PATH/backlog/$FEATURE_NAME.md"
fi

if [[ "$PRETEND" == "0" ]]; then
  INIT_CHANGES="$(git -C "$MAIN_DIR" status --porcelain 2>/dev/null || true)"
  if [[ -n "$INIT_CHANGES" ]]; then
    log "Recording init artifacts in git"
    git -C "$MAIN_DIR" add . >/dev/null 2>&1 || record_bug "failed to stage init artifacts"
    git -C "$MAIN_DIR" commit -m "chore: capture branchbox init artifacts" >/dev/null 2>&1 || record_bug "failed to commit init artifacts inside $MAIN_DIR"
  fi
else
  pretend_step "Would record init artifacts"
fi

MAIN_COMPOSE="$MAIN_DIR/.devcontainer/compose.yaml"
MAIN_DEVCONTAINER_JSON="$MAIN_DIR/.devcontainer/devcontainer.json"

if [[ "$PRETEND" == "0" && -f "$MAIN_COMPOSE" && -f "$MAIN_DEVCONTAINER_JSON" ]]; then
  if ! grep -q "e2e-jsonc-comment" "$MAIN_DEVCONTAINER_JSON"; then
    tmp_jsonc="$(mktemp)"
    {
      head -n 1 "$MAIN_DEVCONTAINER_JSON"
      echo '  // e2e-jsonc-comment ensures JSONC parsing is honored'
      tail -n +2 "$MAIN_DEVCONTAINER_JSON"
    } >"$tmp_jsonc"
    mv "$tmp_jsonc" "$MAIN_DEVCONTAINER_JSON"
  fi
  SERVICE_NAME="$(jq -r '.service // "rust-dev"' "$MAIN_DEVCONTAINER_JSON" 2>/dev/null || echo "rust-dev")"
  log "Booting main devcontainer service '$SERVICE_NAME'"
  if docker compose -f "$MAIN_COMPOSE" --project-directory "$(dirname "$MAIN_COMPOSE")" up -d --build >/dev/null; then
    COMPOSE_STACKS+=("$MAIN_COMPOSE")
    if ! docker compose -f "$MAIN_COMPOSE" --project-directory "$(dirname "$MAIN_COMPOSE")" exec -T "$SERVICE_NAME" git --version >/dev/null; then
      record_bug "git binary missing inside main devcontainer (service $SERVICE_NAME)"
    fi
  else
    record_bug "docker compose up failed for main devcontainer (see $MAIN_COMPOSE)"
  fi
else
  pretend_step "Would build and verify main devcontainer"
fi

FEATURE_PARENT="$(dirname "$MAIN_DIR")"
FEATURE_DIR="$FEATURE_PARENT/$FEATURE_NAME"
FEATURE_BRANCH="feature/$FEATURE_NAME"
START_LOG="$LOG_DIR/feature-start.log"

log "Starting feature '$FEATURE_NAME'"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature start $FEATURE_NAME"
else
  if ! (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature start "$FEATURE_NAME") | tee "$START_LOG"; then
    fatal "branchbox feature start failed (see $START_LOG)"
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  if [[ ! -d "$FEATURE_DIR" ]]; then
    record_bug "feature worktree directory missing at $FEATURE_DIR"
  fi

  if ! git -C "$MAIN_DIR" rev-parse --verify "$FEATURE_BRANCH" >/dev/null 2>&1; then
    record_bug "git branch $FEATURE_BRANCH not found after feature start"
  fi
else
  pretend_step "Would verify feature directory and branch"
fi

REGISTRY_PATH="$MAIN_DIR/.branchbox/registry.json"
if [[ "$PRETEND" == "0" && -f "$REGISTRY_PATH" ]]; then
  if ! jq -e --arg feature "$FEATURE_NAME" '.features[] | select(.work_feature == $feature)' "$REGISTRY_PATH" >/dev/null; then
    record_bug "feature registry missing entry for $FEATURE_NAME"
  fi
else
  pretend_step "Would verify registry entry for $FEATURE_NAME"
fi

FEATURE_COMPOSE="$FEATURE_DIR/.devcontainer/compose.yaml"
FEATURE_DEVCONTAINER_JSON="$FEATURE_DIR/.devcontainer/devcontainer.json"

if [[ "$PRETEND" == "0" && -f "$FEATURE_COMPOSE" && -f "$FEATURE_DEVCONTAINER_JSON" ]]; then
  FEATURE_SERVICE="$(jq -r '.service // "rust-dev"' "$FEATURE_DEVCONTAINER_JSON" 2>/dev/null || echo "rust-dev")"
  log "Booting feature devcontainer service '$FEATURE_SERVICE'"
  if docker compose -f "$FEATURE_COMPOSE" --project-directory "$(dirname "$FEATURE_COMPOSE")" up -d --build >/dev/null; then
    COMPOSE_STACKS+=("$FEATURE_COMPOSE")
    if ! docker compose -f "$FEATURE_COMPOSE" --project-directory "$(dirname "$FEATURE_COMPOSE")" exec -T "$FEATURE_SERVICE" git --version >/dev/null; then
      record_bug "git binary missing inside feature devcontainer (service $FEATURE_SERVICE)"
    fi
  else
    record_bug "docker compose up failed for feature devcontainer (see $FEATURE_COMPOSE)"
  fi
else
  pretend_step "Would build and verify feature devcontainer"
fi

if [[ "$PRETEND" == "0" ]]; then
  assert_file_exists "$MAIN_DIR/.devcontainer/.branchbox.env"
  assert_file_exists "$FEATURE_DIR/.devcontainer/.branchbox.env"
else
  pretend_step "Would verify .branchbox.env overlays"
fi

if [[ "$PRETEND" == "0" ]]; then
  log "Running devcontainer sync dry-runs"
  if ! (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" devcontainer sync --dry-run --strategy copy >/dev/null); then
    record_bug "branchbox devcontainer sync --dry-run --strategy copy failed"
  fi
  if ! (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" devcontainer sync --dry-run --strategy symlink >/dev/null); then
    record_bug "branchbox devcontainer sync --dry-run --strategy symlink failed"
  fi
else
  pretend_step "Would run devcontainer sync dry-runs"
fi

if [[ "$PRETEND" == "0" ]]; then
  SPEC_BACKLOG="$FEATURES_DIR_PATH/backlog/$FEATURE_NAME.md"
  SPEC_INPROGRESS="$FEATURES_DIR_PATH/in-progress/$FEATURE_NAME.md"
  assert_file_not_exists "$SPEC_BACKLOG"
  assert_file_exists "$SPEC_INPROGRESS"
  assert_file_contains "$SPEC_INPROGRESS" "status: in-progress"
else
  pretend_step "Would verify spec moved to in-progress"
fi

if [[ "$PRETEND" == "0" ]]; then
  ACTIVE_LIST_JSON="$LOG_DIR/feature-list-active.json"
  if (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" feature list --json >"$ACTIVE_LIST_JSON"); then
    if ! jq -e --arg feature "$FEATURE_NAME" '.[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) == "active")' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json missing active entry for $FEATURE_NAME"
    fi
  else
    record_bug "branchbox feature list --json failed (see $ACTIVE_LIST_JSON)"
  fi
else
  pretend_step "Would run feature list --json to verify active state"
fi

log "Tearing down feature '$FEATURE_NAME'"
TEARDOWN_LOG="$LOG_DIR/feature-teardown.log"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature teardown $FEATURE_NAME --delete-branch --complete-spec"
else
  TEARDOWN_OK=1
  if (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature teardown "$FEATURE_NAME" --delete-branch --complete-spec) | tee "$TEARDOWN_LOG"; then
    TEARDOWN_OK=0
  fi
  if [[ $TEARDOWN_OK -ne 0 ]]; then
    log "Initial feature teardown failed; retrying with --force (see $TEARDOWN_LOG)"
    if ! (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature teardown "$FEATURE_NAME" --delete-branch --complete-spec --force >>"$TEARDOWN_LOG" 2>&1); then
      fatal "forced feature teardown also failed (see $TEARDOWN_LOG)"
    fi
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  if [[ -d "$FEATURE_DIR" ]]; then
    record_bug "feature directory still exists after teardown: $FEATURE_DIR"
  fi

  if git -C "$MAIN_DIR" branch --list "$FEATURE_BRANCH" | grep -q "$FEATURE_BRANCH"; then
    record_bug "branch $FEATURE_BRANCH still present after teardown"
  fi

  if [[ -f "$REGISTRY_PATH" ]]; then
    if jq -e --arg feature "$FEATURE_NAME" '.features[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) != "removed")' "$REGISTRY_PATH" >/dev/null; then
      record_bug "registry entry for $FEATURE_NAME not marked removed"
    fi
  fi

  SPEC_COMPLETED="$FEATURES_DIR_PATH/completed/$FEATURE_NAME.md"
  assert_file_exists "$SPEC_COMPLETED"
  assert_file_contains "$SPEC_COMPLETED" "status: completed"

  REMOVED_LIST_JSON="$LOG_DIR/feature-list-removed.json"
  if (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" feature list --json --all >"$REMOVED_LIST_JSON"); then
    if ! jq -e --arg feature "$FEATURE_NAME" '.[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) == "removed")' "$REMOVED_LIST_JSON" >/dev/null; then
      record_bug "feature list --json --all missing removed entry for $FEATURE_NAME"
    fi
  else
    record_bug "branchbox feature list --json --all failed (see $REMOVED_LIST_JSON)"
  fi
else
  pretend_step "Would verify feature cleanup and registry removal"
  pretend_step "Would run feature list --json (--all) to confirm removal state"
fi

report_results

if ((${#BUGS[@]})); then
  exit 1
fi
