#!/usr/bin/env bash

set -euo pipefail

MODE="regular"
SCRIPT_VERBOSE=0
PRETEND=0
STACK="${STACK:-rust}"

usage() {
  cat <<'USAGE'
Usage: manual-cli-e2e.sh [--mode regular|verbose|pretend] [--stack rust|generic|rails|node]
       manual-cli-e2e.sh [--verbose] [--pretend] [--stack <stack>]

Modes:
  regular   Default; runs the full workflow with real containers.
  verbose   Same as regular but with shell tracing and extra BranchBox logs.
  pretend   Dry-run. Logs each step without invoking BranchBox or Docker.
Stacks:
  rust      Seeds a Cargo binary and exercises the Rust devcontainer template.
  generic   Seeds a minimal README project and exercises the generic stack template.
  rails     Seeds a bare Rails skeleton (Gemfile + minimal files) for the Rails template.
  node      Seeds a simple Node.js package.json/index.js for the Node template.
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
    -s|--stack)
      shift
      STACK="${1:-}"
      ;;
    --stack=*)
      STACK="${1#*=}"
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

STACK="${STACK,,}"
case "$STACK" in
  rust|generic|rails|node) ;;
  *)
    echo "Unsupported stack: $STACK (supported: rust, generic, rails, node)" >&2
    exit 1
    ;;
esac

if [[ "$SCRIPT_VERBOSE" == "1" ]]; then
  set -x
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRANCHBOX_BIN="${BRANCHBOX_BIN:-"$REPO_ROOT/target/debug/branchbox"}"
PROJECT_NAME="${PROJECT_NAME:-cli-e2e-${STACK}-sample}"
FEATURE_NAME="${FEATURE_NAME:-cli-e2e-${STACK}-smoke}"
SECONDARY_FEATURE_NAME="${SECONDARY_FEATURE_NAME:-${FEATURE_NAME}-tunnel}"
FALLBACK_FEATURE_NAME="${FALLBACK_FEATURE_NAME:-${SECONDARY_FEATURE_NAME}-fallback}"
CLOUDFLARE_ACCOUNT_ID_VALUE="${CLOUDFLARE_ACCOUNT_ID_VALUE:-cli-e2e-account}"
CLOUDFLARE_TUNNEL_TOKEN_VALUE="${CLOUDFLARE_TUNNEL_TOKEN_VALUE:-cli-e2e-token}"
CLOUDFLARE_TUNNEL_PREFIX="${CLOUDFLARE_TUNNEL_PREFIX:-cli-e2e}"
CLOUDFLARE_DNS_ZONE_VALUE="${CLOUDFLARE_DNS_ZONE_VALUE:-cli-e2e.test}"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/branchbox-cli-e2e-XXXXXX")"
SOURCE_PARENT="$TMP_ROOT/source"
TARGET_PARENT="$TMP_ROOT/workspaces"
SOURCE_DIR="$SOURCE_PARENT/$PROJECT_NAME"
EXPECTED_CONTAINER="$TARGET_PARENT/$PROJECT_NAME"
LOG_DIR="$TMP_ROOT/logs"
mkdir -p "$LOG_DIR"

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

function seed_sample_project() {
  log "Seeding disposable $STACK repository at $SOURCE_DIR"
  rm -rf "$SOURCE_DIR"
  mkdir -p "$SOURCE_DIR"
  case "$STACK" in
    rust)
      mkdir -p "$SOURCE_DIR/src"
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
      ;;
    generic)
      cat >"$SOURCE_DIR/README.md" <<'EOF_GENERIC'
# BranchBox CLI E2E Sample (Generic)

This is a minimal project used by `scripts/manual-cli-e2e.sh` to exercise the generic stack.
EOF_GENERIC
      ;;
    rails)
      cat >"$SOURCE_DIR/Gemfile" <<'EOF_GEM'
source "https://rubygems.org"

gem "rails", "~> 7.1.0"
EOF_GEM
      cat >"$SOURCE_DIR/README.md" <<'EOF_RAILS'
# BranchBox CLI E2E Sample (Rails)

Placeholder Rails project for manual CLI harness coverage.
EOF_RAILS
      ;;
    node)
      cat >"$SOURCE_DIR/package.json" <<'EOF_NODE'
{
  "name": "cli-e2e-sample",
  "version": "1.0.0",
  "main": "index.js",
  "license": "MIT"
}
EOF_NODE
      cat >"$SOURCE_DIR/index.js" <<'EOF_INDEX'
console.log("BranchBox CLI e2e smoke test (Node)");
EOF_INDEX
      ;;
    *)
      fatal "Stack '$STACK' not supported by manual harness seeding"
      ;;
  esac
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

function detect_compose_service() {
  local compose_file="$1"
  if [[ ! -f "$compose_file" ]]; then
    return
  fi

  awk '
    /^services:[[:space:]]*$/ { in_services = 1; next }
    in_services && /^[^[:space:]]/ { exit }
    in_services && /^  [A-Za-z0-9._-]+:[[:space:]]*$/ {
      service = $1
      sub(/:$/, "", service)
      print service
      exit
    }
  ' "$compose_file"
}

function resolve_devcontainer_service() {
  local devcontainer_json="$1"
  local compose_file="$2"
  local fallback="${3:-rust-dev}"
  local service=""

  if [[ -f "$devcontainer_json" ]]; then
    service="$(sed -e 's#//.*##' "$devcontainer_json" | jq -r '.service // empty' 2>/dev/null || true)"
  fi

  if [[ -z "$service" ]]; then
    service="$(detect_compose_service "$compose_file" || true)"
  fi

  if [[ -z "$service" ]]; then
    service="$fallback"
  fi

  printf '%s\n' "$service"
}

function configure_cloudflared_project() {
  local workspace="$1"
  if [[ "$PRETEND" == "1" ]]; then
    pretend_step "Would configure Cloudflared credentials under $workspace/.branchbox"
    return
  fi

  local branchbox_dir="$workspace/.branchbox"
  local secure_dir="$branchbox_dir/secure"
  mkdir -p "$secure_dir"

  local credentials="$secure_dir/cloudflared.env"
  cat >"$credentials" <<EOF
CLOUDFLARE_API_TOKEN=$CLOUDFLARE_TUNNEL_TOKEN_VALUE
CLOUDFLARE_ACCOUNT_ID=$CLOUDFLARE_ACCOUNT_ID_VALUE
EOF

  local config_path="$branchbox_dir/config.json"
  local default_config='{
    "version": "1",
    "tunnel": {
      "enabled": true,
      "default_provider": "cloudflared",
      "providers": {
        "cloudflared": {
          "account_id": "",
          "api_token_path": ".branchbox/secure/cloudflared.env",
          "tunnel_name_prefix": "",
          "dns_zone": "",
          "manual_instructions": false
        }
      }
    },
    "editor": {}
  }'

  if [[ ! -f "$config_path" ]]; then
    printf '%s\n' "$default_config" >"$config_path"
  fi

  local tmp_config
  tmp_config="$(mktemp)"
  jq \
    --arg account "$CLOUDFLARE_ACCOUNT_ID_VALUE" \
    --arg token_path ".branchbox/secure/cloudflared.env" \
    --arg prefix "$CLOUDFLARE_TUNNEL_PREFIX" \
    --arg zone "$CLOUDFLARE_DNS_ZONE_VALUE" \
    '
      .tunnel.enabled = true
      | .tunnel.default_provider = "cloudflared"
      | .tunnel.providers.cloudflared = (
          (.tunnel.providers.cloudflared // {}) + {
            account_id: $account,
            api_token_path: $token_path,
            tunnel_name_prefix: $prefix,
            dns_zone: $zone,
            manual_instructions: false
          }
        )
    ' "$config_path" >"$tmp_config"

  mv "$tmp_config" "$config_path"
}

function remove_cloudflared_credentials() {
  local workspace="$1"
  if [[ "$PRETEND" == "1" ]]; then
    pretend_step "Would delete Cloudflared credentials under $workspace/.branchbox"
    return
  fi

  local secure_dir="$workspace/.branchbox/secure"
  rm -f "$secure_dir/cloudflared.env"

  local config_path="$workspace/.branchbox/config.json"
  if [[ -f "$config_path" ]]; then
    local tmp_config
    tmp_config="$(mktemp)"
    jq '
      if .tunnel.providers.cloudflared then
        .tunnel.providers.cloudflared.account_id = null
        | .tunnel.providers.cloudflared.api_token_path = null
        | .tunnel.providers.cloudflared.manual_instructions = true
      else
        .
      end
    ' "$config_path" >"$tmp_config"
    mv "$tmp_config" "$config_path"
  fi
}

require_cmd bash
require_cmd git
require_cmd cargo
require_cmd jq
if [[ "$PRETEND" == "0" ]]; then
  require_cmd docker
fi

log "Running manual CLI e2e in '$MODE' mode (stack: $STACK)"

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

seed_sample_project

(cd "$SOURCE_DIR" && git init -b main >/dev/null)
(cd "$SOURCE_DIR" && git add . >/dev/null && git commit -m "Seed sample project" >/dev/null)

log "Running branchbox init (forcing reorganize, parent structure is default)"
INIT_LOG="$LOG_DIR/init.log"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "BRANCHBOX_SKIP_HOST_VALIDATION=1 BRANCHBOX_PROJECTS_DIR=$TARGET_PARENT $BRANCHBOX_BIN init --stack $STACK --reorganize -y"
else
  if ! (cd "$SOURCE_DIR" && BRANCHBOX_SKIP_HOST_VALIDATION=1 BRANCHBOX_PROJECTS_DIR="$TARGET_PARENT" "$BRANCHBOX_BIN" init --stack "$STACK" --reorganize -y) | tee "$INIT_LOG"; then
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
  cat >"$FEATURES_DIR_PATH/backlog/$SECONDARY_FEATURE_NAME.md" <<EOF
---
status: backlog
title: $SECONDARY_FEATURE_NAME
---

# Placeholder
EOF
  cat >"$FEATURES_DIR_PATH/backlog/$FALLBACK_FEATURE_NAME.md" <<EOF
---
status: backlog
title: $FALLBACK_FEATURE_NAME
---

# Placeholder
EOF
else
  pretend_step "Would seed backlog spec at $FEATURES_DIR_PATH/backlog/$FEATURE_NAME.md"
  pretend_step "Would seed backlog spec at $FEATURES_DIR_PATH/backlog/$SECONDARY_FEATURE_NAME.md"
  pretend_step "Would seed backlog spec at $FEATURES_DIR_PATH/backlog/$FALLBACK_FEATURE_NAME.md"
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
  SERVICE_NAME="$(resolve_devcontainer_service "$MAIN_DEVCONTAINER_JSON" "$MAIN_COMPOSE" "rust-dev")"
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
SECONDARY_DIR="$FEATURE_PARENT/$SECONDARY_FEATURE_NAME"
SECONDARY_BRANCH="feature/$SECONDARY_FEATURE_NAME"
SECONDARY_START_LOG="$LOG_DIR/feature-start-cloud.log"
FALLBACK_DIR="$FEATURE_PARENT/$FALLBACK_FEATURE_NAME"
FALLBACK_BRANCH="feature/$FALLBACK_FEATURE_NAME"
FALLBACK_START_LOG="$LOG_DIR/feature-start-fallback.log"

log "Starting feature '$FEATURE_NAME'"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature start $FEATURE_NAME"
else
  if ! (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature start "$FEATURE_NAME") | tee "$START_LOG"; then
    fatal "branchbox feature start failed (see $START_LOG)"
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  assert_file_contains "$START_LOG" "Tunnel          | ⏭ disabled"
else
  pretend_step "Would verify tunnel module emitted manual setup warning"
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
SECONDARY_COMPOSE="$SECONDARY_DIR/.devcontainer/compose.yaml"
SECONDARY_DEVCONTAINER_JSON="$SECONDARY_DIR/.devcontainer/devcontainer.json"

if [[ "$PRETEND" == "0" && -f "$FEATURE_COMPOSE" && -f "$FEATURE_DEVCONTAINER_JSON" ]]; then
  FEATURE_SERVICE="$(resolve_devcontainer_service "$FEATURE_DEVCONTAINER_JSON" "$FEATURE_COMPOSE" "rust-dev")"
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

log "Configuring Cloudflared credentials for tunnel coverage"
configure_cloudflared_project "$MAIN_DIR"

log "Starting feature '$SECONDARY_FEATURE_NAME' with simulated Cloudflared credentials"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH CLOUDFLARE_TUNNEL_TOKEN=$CLOUDFLARE_TUNNEL_TOKEN_VALUE BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature start $SECONDARY_FEATURE_NAME"
else
  if ! (
    cd "$MAIN_DIR" &&
      FEATURES_DIR="$FEATURES_DIR_PATH" \
      BRANCHBOX_SKIP_HOST_VALIDATION=1 \
      BRANCHBOX_POLICY_ENFORCED_MODULES=tunnel \
      CLOUDFLARE_TUNNEL_TOKEN="$CLOUDFLARE_TUNNEL_TOKEN_VALUE" \
      "$BRANCHBOX_BIN" feature start "$SECONDARY_FEATURE_NAME"
  ) | tee "$SECONDARY_START_LOG"; then
    fatal "branchbox feature start (Cloudflared) failed (see $SECONDARY_START_LOG)"
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  if [[ ! -d "$SECONDARY_DIR" ]]; then
    record_bug "Cloudflared feature worktree missing at $SECONDARY_DIR"
  fi
  if ! git -C "$MAIN_DIR" rev-parse --verify "$SECONDARY_BRANCH" >/dev/null 2>&1; then
    record_bug "git branch $SECONDARY_BRANCH not found after Cloudflared feature start"
  fi
  if [[ -f "$REGISTRY_PATH" ]]; then
    if ! jq -e --arg feature "$SECONDARY_FEATURE_NAME" '.features[] | select(.work_feature == $feature)' "$REGISTRY_PATH" >/dev/null; then
      record_bug "feature registry missing entry for $SECONDARY_FEATURE_NAME"
    fi
  fi
else
  pretend_step "Would verify registry entry for $SECONDARY_FEATURE_NAME"
fi

if [[ "$PRETEND" == "0" && -f "$SECONDARY_COMPOSE" && -f "$SECONDARY_DEVCONTAINER_JSON" ]]; then
  SECONDARY_SERVICE="$(resolve_devcontainer_service "$SECONDARY_DEVCONTAINER_JSON" "$SECONDARY_COMPOSE" "rust-dev")"
  log "Booting Cloudflared feature devcontainer service '$SECONDARY_SERVICE'"
  if docker compose -f "$SECONDARY_COMPOSE" --project-directory "$(dirname "$SECONDARY_COMPOSE")" up -d --build >/dev/null; then
    COMPOSE_STACKS+=("$SECONDARY_COMPOSE")
    if ! docker compose -f "$SECONDARY_COMPOSE" --project-directory "$(dirname "$SECONDARY_COMPOSE")" exec -T "$SECONDARY_SERVICE" git --version >/dev/null; then
      record_bug "git binary missing inside Cloudflared feature devcontainer (service $SECONDARY_SERVICE)"
    fi
  else
    record_bug "docker compose up failed for Cloudflared devcontainer (see $SECONDARY_COMPOSE)"
  fi
else
  pretend_step "Would build and verify Cloudflared feature devcontainer"
fi

if [[ "$PRETEND" == "0" ]]; then
  assert_file_exists "$SECONDARY_DIR/.devcontainer/.branchbox.env"
  CLOUD_ENV="$SECONDARY_DIR/.devcontainer/.cloudflared.env"
  assert_file_exists "$CLOUD_ENV"
  assert_file_contains "$CLOUD_ENV" "TUNNEL_TOKEN=$CLOUDFLARE_TUNNEL_TOKEN_VALUE"
else
  pretend_step "Would verify Cloudflared module output and .cloudflared.env contents"
fi

if [[ "$PRETEND" == "0" ]]; then
  SYNC_MARKER="// e2e-sync-check"
  if ! grep -q "$SYNC_MARKER" "$MAIN_DEVCONTAINER_JSON"; then
    printf "  %s\n" "$SYNC_MARKER" >>"$MAIN_DEVCONTAINER_JSON"
  fi
  if ! (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" devcontainer sync --strategy copy >/dev/null); then
    record_bug "branchbox devcontainer sync --strategy copy failed"
  else
    if ! grep -q "$SYNC_MARKER" "$FEATURE_DEVCONTAINER_JSON"; then
      record_bug "devcontainer sync --strategy copy did not propagate changes to feature worktree"
    fi
    if [[ -f "$SECONDARY_DEVCONTAINER_JSON" ]] && ! grep -q "$SYNC_MARKER" "$SECONDARY_DEVCONTAINER_JSON"; then
      record_bug "devcontainer sync --strategy copy did not update Cloudflared worktree"
    fi
  fi
else
  pretend_step "Would mutate main devcontainer and run branchbox devcontainer sync --strategy copy"
fi

if [[ "$PRETEND" == "0" ]]; then
  log "Running devcontainer sync dry-runs"
  DRY_RUN_LOG="$LOG_DIR/devcontainer-sync-dry-run.log"
  if ! (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" devcontainer sync --dry-run --strategy copy) | tee "$DRY_RUN_LOG"; then
    record_bug "branchbox devcontainer sync --dry-run --strategy copy failed"
  else
    for worktree in "$FEATURE_DIR" "$SECONDARY_DIR"; do
      work_name="$(basename "$worktree")"
      if ! grep -q "$work_name" "$DRY_RUN_LOG"; then
        record_bug "devcontainer sync dry-run output missing $work_name"
      fi
    done
  fi
  if ! (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" devcontainer sync --dry-run --strategy symlink >/dev/null); then
    record_bug "branchbox devcontainer sync --dry-run --strategy symlink failed"
  fi
else
  pretend_step "Would run devcontainer sync dry-runs"
fi

if [[ "$PRETEND" == "0" ]]; then
  SPEC_INPROGRESS="$FEATURES_DIR_PATH/in-progress/$FEATURE_NAME.md"
  assert_file_exists "$SPEC_INPROGRESS"
  assert_file_contains "$SPEC_INPROGRESS" "status: in-progress"
  SPEC2_INPROGRESS="$FEATURES_DIR_PATH/in-progress/$SECONDARY_FEATURE_NAME.md"
  assert_file_exists "$SPEC2_INPROGRESS"
  assert_file_contains "$SPEC2_INPROGRESS" "status: in-progress"
else
  pretend_step "Would verify spec moved to in-progress"
fi

if [[ "$PRETEND" == "0" ]]; then
  ACTIVE_LIST_JSON="$LOG_DIR/feature-list-active.json"
  if (cd "$MAIN_DIR" && "$BRANCHBOX_BIN" feature list --json >"$ACTIVE_LIST_JSON"); then
    if ! jq -e --arg feature "$FEATURE_NAME" 'any(.[]; select(.work_feature == $feature and ((.status // "") | ascii_downcase) == "active"))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json missing active entry for $FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$SECONDARY_FEATURE_NAME" 'any(.[]; select(.work_feature == $feature and ((.status // "") | ascii_downcase) == "active"))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json missing active entry for $SECONDARY_FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$FEATURE_NAME" 'any(.[]; select(.work_feature == $feature) | (.devcontainer_outdated == false))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json reports devcontainer_outdated for $FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$SECONDARY_FEATURE_NAME" 'any(.[]; select(.work_feature == $feature) | (.devcontainer_outdated == false))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json reports devcontainer_outdated for $SECONDARY_FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$FEATURE_NAME" 'any(.[]; select(.work_feature == $feature) | (((.sync_strategy // "") | ascii_downcase) == "copy"))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json missing sync_strategy Copy for $FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$SECONDARY_FEATURE_NAME" 'any(.[]; select(.work_feature == $feature) | (((.sync_strategy // "") | ascii_downcase) == "copy"))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "feature list --json missing sync_strategy Copy for $SECONDARY_FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$FEATURE_NAME" 'any(.[]; select(.work_feature == $feature) | any((.module_outcomes // [])[]; ((.module // "") | ascii_downcase) == "tunnel" and ((.status // "") | ascii_downcase) == "skipped"))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "module_outcomes missing tunnel skip record for $FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$SECONDARY_FEATURE_NAME" 'any(.[]; select(.work_feature == $feature) | any((.module_outcomes // [])[]; ((.module // "") | ascii_downcase) == "tunnel" and ((.status // "") | ascii_downcase) == "success"))' "$ACTIVE_LIST_JSON" >/dev/null; then
      record_bug "module_outcomes missing tunnel success record for $SECONDARY_FEATURE_NAME"
    fi
    for feature in "$FEATURE_NAME" "$SECONDARY_FEATURE_NAME"; do
      if ! jq -e --arg feature "$feature" 'any(.[]; select(.work_feature == $feature) | (.removed_at == null))' "$ACTIVE_LIST_JSON" >/dev/null; then
        record_bug "feature list --json has non-null removed_at for active feature $feature"
      fi
      if ! jq -e --arg feature "$feature" 'any(.[]; select(.work_feature == $feature) | (.last_sync_at != null))' "$ACTIVE_LIST_JSON" >/dev/null; then
        record_bug "feature list --json missing last_sync_at for active feature $feature"
      fi
    done
  else
    record_bug "branchbox feature list --json failed (see $ACTIVE_LIST_JSON)"
  fi
else
  pretend_step "Would run feature list --json to verify active state"
fi

log "Tearing down feature '$SECONDARY_FEATURE_NAME'"
SECONDARY_TEARDOWN_LOG="$LOG_DIR/feature-teardown-cloud.log"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature teardown $SECONDARY_FEATURE_NAME --delete-branch --complete-spec"
else
  TEARDOWN2_OK=1
  if (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature teardown "$SECONDARY_FEATURE_NAME" --delete-branch --complete-spec) | tee "$SECONDARY_TEARDOWN_LOG"; then
    TEARDOWN2_OK=0
  fi
  if [[ $TEARDOWN2_OK -ne 0 ]]; then
    log "Cloudflared feature teardown failed; retrying with --force (see $SECONDARY_TEARDOWN_LOG)"
    if ! (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature teardown "$SECONDARY_FEATURE_NAME" --delete-branch --complete-spec --force >>"$SECONDARY_TEARDOWN_LOG" 2>&1); then
      fatal "forced Cloudflared feature teardown also failed (see $SECONDARY_TEARDOWN_LOG)"
    fi
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  if [[ -d "$SECONDARY_DIR" ]]; then
    record_bug "Cloudflared feature directory still exists after teardown: $SECONDARY_DIR"
  fi
  if git -C "$MAIN_DIR" branch --list "$SECONDARY_BRANCH" | grep -q "$SECONDARY_BRANCH"; then
    record_bug "branch $SECONDARY_BRANCH still present after teardown"
  fi
  if [[ -f "$REGISTRY_PATH" ]]; then
    if jq -e --arg feature "$SECONDARY_FEATURE_NAME" '.features[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) != "removed")' "$REGISTRY_PATH" >/dev/null; then
      record_bug "registry entry for $SECONDARY_FEATURE_NAME not marked removed"
    fi
  fi
  SPEC2_COMPLETED="$FEATURES_DIR_PATH/completed/$SECONDARY_FEATURE_NAME.md"
  assert_file_exists "$SPEC2_COMPLETED"
  assert_file_contains "$SPEC2_COMPLETED" "status: completed"
  CLOUD_ENV="$SECONDARY_DIR/.devcontainer/.cloudflared.env"
  if [[ -e "$CLOUD_ENV" ]]; then
    record_bug ".cloudflared.env still present after teardown at $CLOUD_ENV"
  fi
else
  pretend_step "Would verify Cloudflared feature cleanup and registry removal"
fi

if [[ "$PRETEND" == "0" && -f "$FEATURE_DIR/.devcontainer/devcontainer.json" ]]; then
  printf '  // dirty-teardown-marker %s\n' "$(date -u +%s)" >>"$FEATURE_DIR/.devcontainer/devcontainer.json"
fi

log "Simulating Cloudflared credential loss"
remove_cloudflared_credentials "$MAIN_DIR"

log "Starting fallback feature '$FALLBACK_FEATURE_NAME' after credential loss"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature start $FALLBACK_FEATURE_NAME"
else
  if ! (
    cd "$MAIN_DIR" &&
      FEATURES_DIR="$FEATURES_DIR_PATH" \
      BRANCHBOX_SKIP_HOST_VALIDATION=1 \
      "$BRANCHBOX_BIN" feature start "$FALLBACK_FEATURE_NAME"
  ) | tee "$FALLBACK_START_LOG"; then
    fatal "branchbox feature start (fallback) failed (see $FALLBACK_START_LOG)"
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  assert_file_contains "$FALLBACK_START_LOG" "Tunnel          | ⏭ disabled"
  if [[ -f "$FALLBACK_DIR/.devcontainer/.cloudflared.env" ]]; then
    record_bug ".cloudflared.env unexpectedly created for fallback feature"
  fi
  if [[ -f "$REGISTRY_PATH" ]]; then
    if ! jq -e --arg feature "$FALLBACK_FEATURE_NAME" '.features[] | select(.work_feature == $feature)' "$REGISTRY_PATH" >/dev/null; then
      record_bug "feature registry missing entry for $FALLBACK_FEATURE_NAME"
    fi
  fi
else
  pretend_step "Would verify fallback registry entry and tunnel skip"
fi

if [[ "$PRETEND" == "0" ]]; then
  FALLBACK_SPEC="$FEATURES_DIR_PATH/in-progress/$FALLBACK_FEATURE_NAME.md"
  assert_file_exists "$FALLBACK_SPEC"
  assert_file_contains "$FALLBACK_SPEC" "status: in-progress"
fi

log "Tearing down fallback feature '$FALLBACK_FEATURE_NAME'"
FALLBACK_TEARDOWN_LOG="$LOG_DIR/feature-teardown-fallback.log"
if [[ "$PRETEND" == "1" ]]; then
  pretend_step "FEATURES_DIR=$FEATURES_DIR_PATH BRANCHBOX_SKIP_HOST_VALIDATION=1 $BRANCHBOX_BIN feature teardown $FALLBACK_FEATURE_NAME --delete-branch --complete-spec"
else
  if ! (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature teardown "$FALLBACK_FEATURE_NAME" --delete-branch --complete-spec) | tee "$FALLBACK_TEARDOWN_LOG"; then
    log "Fallback feature teardown failed; forcing cleanup"
    if ! (cd "$MAIN_DIR" && FEATURES_DIR="$FEATURES_DIR_PATH" BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature teardown "$FALLBACK_FEATURE_NAME" --delete-branch --complete-spec --force >>"$FALLBACK_TEARDOWN_LOG" 2>&1); then
      fatal "forced fallback teardown also failed (see $FALLBACK_TEARDOWN_LOG)"
    fi
  fi
fi

if [[ "$PRETEND" == "0" ]]; then
  if [[ -d "$FALLBACK_DIR" ]]; then
    record_bug "Fallback feature directory still exists after teardown: $FALLBACK_DIR"
  fi
  if git -C "$MAIN_DIR" branch --list "$FALLBACK_BRANCH" | grep -q "$FALLBACK_BRANCH"; then
    record_bug "branch $FALLBACK_BRANCH still present after teardown"
  fi
  if [[ -f "$REGISTRY_PATH" ]]; then
    if jq -e --arg feature "$FALLBACK_FEATURE_NAME" '.features[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) != "removed")' "$REGISTRY_PATH" >/dev/null; then
      record_bug "registry entry for $FALLBACK_FEATURE_NAME not marked removed"
    fi
  fi
  FALLBACK_SPEC_COMPLETED="$FEATURES_DIR_PATH/completed/$FALLBACK_FEATURE_NAME.md"
  assert_file_exists "$FALLBACK_SPEC_COMPLETED"
  assert_file_contains "$FALLBACK_SPEC_COMPLETED" "status: completed"
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
    if jq -e --arg feature "$SECONDARY_FEATURE_NAME" '.features[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) != "removed")' "$REGISTRY_PATH" >/dev/null; then
      record_bug "registry entry for $SECONDARY_FEATURE_NAME not marked removed"
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
    if ! jq -e --arg feature "$SECONDARY_FEATURE_NAME" '.[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) == "removed")' "$REMOVED_LIST_JSON" >/dev/null; then
      record_bug "feature list --json --all missing removed entry for $SECONDARY_FEATURE_NAME"
    fi
    if ! jq -e --arg feature "$FALLBACK_FEATURE_NAME" '.[] | select(.work_feature == $feature and ((.status // "") | ascii_downcase) == "removed")' "$REMOVED_LIST_JSON" >/dev/null; then
      record_bug "feature list --json --all missing removed entry for $FALLBACK_FEATURE_NAME"
    fi
    for feature in "$FEATURE_NAME" "$SECONDARY_FEATURE_NAME" "$FALLBACK_FEATURE_NAME"; do
      if ! jq -e --arg feature "$feature" 'any(.[]; select(.work_feature == $feature) | (.removed_at != null))' "$REMOVED_LIST_JSON" >/dev/null; then
        record_bug "feature list --json --all missing removed_at for $feature"
      fi
      if ! jq -e --arg feature "$feature" 'any(.[]; select(.work_feature == $feature) | (.tunnel.status // "disabled" | ascii_downcase) == "disabled")' "$REMOVED_LIST_JSON" >/dev/null; then
        record_bug "feature list --json --all missing tunnel disabled status for $feature"
      fi
    done
  else
    record_bug "branchbox feature list --json --all failed (see $REMOVED_LIST_JSON)"
  fi

  assert_file_contains "$TEARDOWN_LOG" "Detected devcontainer/compose changes"
  assert_file_contains "$TEARDOWN_LOG" "Tunnel descriptor missing"
else
  pretend_step "Would verify feature cleanup and registry removal"
  pretend_step "Would run feature list --json (--all) to confirm removal state"
fi

report_results

if ((${#BUGS[@]})); then
  exit 1
fi
