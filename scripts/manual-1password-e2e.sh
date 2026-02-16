#!/usr/bin/env bash

set -euo pipefail

MODE="regular"
STACK="${STACK:-generic}"
FEATURE_NAME="${FEATURE_NAME:-onepassword-e2e}"
CHECK_FAILURE_PATH=0
SKIP_BUILD=0
COMPOSE_CMD=()

usage() {
  cat <<'USAGE'
Usage: manual-1password-e2e.sh [options]

Options:
  --mode regular|pretend       Run real workflow (default) or print planned steps.
  --stack rust|generic|rails|node
                               Stack passed to `branchbox init` (default: generic).
  --feature-name <name>        Feature name used for worktree start (default: onepassword-e2e).
  --origin-ssh-url <url>       SSH remote URL to validate SSH -> HTTPS conversion.
  --op-github-ref <ref>        1Password ref for GitHub PAT (op://.../token).
  --op-signing-key-ref <ref>   1Password ref for SSH signing private key (op://.../private key).
  --check-failure-path         Also run invalid-reference restart smoke check.
  --skip-build                 Skip `cargo build -p branchbox-cli`.
  --help                       Show this help text.

Environment overrides:
  BRANCHBOX_BIN                BranchBox binary path (default: <repo>/target/debug/branchbox).
  OP_GITHUB_REF                Same as --op-github-ref.
  OP_SIGNING_KEY_REF           Same as --op-signing-key-ref.
  ORIGIN_SSH_URL               Same as --origin-ssh-url.
  KEEP_E2E_TMP=1               Keep temp workspace for debugging.
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
    -s|--stack)
      shift
      STACK="${1:-}"
      ;;
    --stack=*)
      STACK="${1#*=}"
      ;;
    --feature-name)
      shift
      FEATURE_NAME="${1:-}"
      ;;
    --feature-name=*)
      FEATURE_NAME="${1#*=}"
      ;;
    --origin-ssh-url)
      shift
      ORIGIN_SSH_URL="${1:-}"
      ;;
    --origin-ssh-url=*)
      ORIGIN_SSH_URL="${1#*=}"
      ;;
    --op-github-ref)
      shift
      OP_GITHUB_REF="${1:-}"
      ;;
    --op-github-ref=*)
      OP_GITHUB_REF="${1#*=}"
      ;;
    --op-signing-key-ref)
      shift
      OP_SIGNING_KEY_REF="${1:-}"
      ;;
    --op-signing-key-ref=*)
      OP_SIGNING_KEY_REF="${1#*=}"
      ;;
    --check-failure-path)
      CHECK_FAILURE_PATH=1
      ;;
    --skip-build)
      SKIP_BUILD=1
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

MODE="$(printf '%s' "$MODE" | tr '[:upper:]' '[:lower:]')"
STACK="$(printf '%s' "$STACK" | tr '[:upper:]' '[:lower:]')"

case "$MODE" in
  regular|pretend) ;;
  *)
    echo "Invalid mode: $MODE (supported: regular, pretend)" >&2
    exit 1
    ;;
esac

case "$STACK" in
  rust|generic|rails|node) ;;
  *)
    echo "Unsupported stack: $STACK (supported: rust, generic, rails, node)" >&2
    exit 1
    ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRANCHBOX_BIN="${BRANCHBOX_BIN:-"$REPO_ROOT/target/debug/branchbox"}"
ORIGIN_SSH_URL="${ORIGIN_SSH_URL:-}"
OP_GITHUB_REF="${OP_GITHUB_REF:-}"
OP_SIGNING_KEY_REF="${OP_SIGNING_KEY_REF:-}"

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/branchbox-1password-e2e-XXXXXX")"
MAIN_DIR="$TMP_ROOT/main"
FEATURE_DIR="$TMP_ROOT/$FEATURE_NAME"
LOG_DIR="$TMP_ROOT/logs"
mkdir -p "$LOG_DIR"

BUGS=()
RUN_TAG="$(date -u +%Y%m%d%H%M%S)"
MAIN_DEVCONTAINER_NAME="bb1p-main-${RUN_TAG}"
FEATURE_DEVCONTAINER_NAME="bb1p-feature-${RUN_TAG}"

log() {
  printf '==> %s\n' "$*"
}

record_bug() {
  BUGS+=("$1")
  printf '!! %s\n' "$1" >&2
}

fatal() {
  record_bug "$1"
  report_results
  exit 1
}

report_results() {
  echo
  if ((${#BUGS[@]} == 0)); then
    echo "✅ 1Password devcontainer e2e passed."
    if [[ "${KEEP_E2E_TMP:-0}" == "1" ]]; then
      echo "Artifacts preserved at $TMP_ROOT"
    fi
  else
    echo "❌ 1Password devcontainer e2e found ${#BUGS[@]} issue(s):"
    for bug in "${BUGS[@]}"; do
      echo " - $bug"
    done
    echo "Set KEEP_E2E_TMP=1 to keep artifacts under $TMP_ROOT."
  fi
}

cleanup() {
  if [[ "$MODE" == "regular" ]]; then
    devcontainer down --workspace-folder "$FEATURE_DIR" >/dev/null 2>&1 || true
    devcontainer down --workspace-folder "$MAIN_DIR" >/dev/null 2>&1 || true
  fi

  if [[ "${KEEP_E2E_TMP:-0}" == "1" ]]; then
    echo "ℹ️ Preserving artifacts at $TMP_ROOT"
  else
    rm -rf "$TMP_ROOT"
  fi
}
trap cleanup EXIT

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fatal "Missing required command: $1"
}

assert_file_exists() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    record_bug "Expected file missing: $path"
  fi
}

require_file_exists() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    fatal "Expected file missing: $path"
  fi
}

detect_compose_service() {
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

read_devcontainer_service() {
  local workspace="$1"
  devcontainer read-configuration \
    --workspace-folder "$workspace" \
    --log-format json 2>/dev/null \
    | jq -r 'select(type == "object" and has("configuration")) | .configuration.service // empty' \
    | tail -n 1
}

resolve_devcontainer_service() {
  local devcontainer_json="$1"
  local compose_file="$2"
  local service_name=""
  local workspace=""

  if [[ -f "$devcontainer_json" ]]; then
    workspace="$(cd "$(dirname "$devcontainer_json")/.." 2>/dev/null && pwd || true)"
  fi

  if [[ -n "$workspace" ]]; then
    service_name="$(read_devcontainer_service "$workspace" || true)"
  fi

  if [[ -z "$service_name" && -f "$devcontainer_json" ]]; then
    service_name="$(sed -e 's#//.*##' "$devcontainer_json" | jq -r '.service // empty' 2>/dev/null || true)"
  fi

  if [[ -z "$service_name" ]]; then
    service_name="$(detect_compose_service "$compose_file" || true)"
  fi

  printf '%s\n' "$service_name"
}

resolve_container_id() {
  local workspace="$1"
  local devcontainer_json="$workspace/.devcontainer/devcontainer.json"
  local compose_file="$workspace/.devcontainer/compose.yaml"
  local env_file="$workspace/.devcontainer/.branchbox.env"

  local service_name
  service_name="$(resolve_devcontainer_service "$devcontainer_json" "$compose_file")"
  if [[ -z "$service_name" ]]; then
    return 1
  fi

  if [[ -f "$env_file" ]]; then
    "${COMPOSE_CMD[@]}" \
      --env-file "$env_file" \
      -f "$compose_file" \
      --project-directory "$workspace/.devcontainer" \
      ps -q "$service_name" | head -n 1
  else
    "${COMPOSE_CMD[@]}" \
      -f "$compose_file" \
      --project-directory "$workspace/.devcontainer" \
      ps -q "$service_name" | head -n 1
  fi
}

configure_compose_command() {
  if docker compose version >/dev/null 2>&1; then
    COMPOSE_CMD=(docker compose)
  elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_CMD=(docker-compose)
    log "docker compose plugin unavailable; falling back to docker-compose"
  else
    fatal "Neither docker compose nor docker-compose is available."
  fi
}

extract_container_id_from_log() {
  local log_path="$1"
  if [[ ! -f "$log_path" ]]; then
    return 1
  fi

  grep -Eo '\{[^{}]*"containerId":"[^"]+"[^{}]*\}' "$log_path" \
    | tail -n 1 \
    | jq -r '.containerId // empty'
}

set_devcontainer_name() {
  local workspace="$1"
  local name="$2"
  local env_file="$workspace/.devcontainer/.branchbox.env"
  local tmp_file="${env_file}.tmp"

  mkdir -p "$workspace/.devcontainer"
  if [[ -f "$env_file" ]]; then
    awk '!/^DEVCONTAINER_NAME=/' "$env_file" >"$tmp_file"
    mv "$tmp_file" "$env_file"
  else
    : >"$env_file"
  fi

  if [[ -s "$env_file" ]] && [[ "$(tail -c 1 "$env_file" 2>/dev/null || true)" != $'\n' ]]; then
    printf '\n' >>"$env_file"
  fi

  printf 'DEVCONTAINER_NAME=%s\n' "$name" >>"$env_file"
}

assert_log_contains() {
  local path="$1"
  local needle="$2"
  if [[ ! -f "$path" ]]; then
    record_bug "Expected log missing: $path"
    return
  fi
  if ! grep -q "$needle" "$path"; then
    record_bug "Expected '$needle' in $path"
  fi
}

seed_sample_project() {
  rm -rf "$MAIN_DIR"
  mkdir -p "$MAIN_DIR"

  case "$STACK" in
    rust)
      mkdir -p "$MAIN_DIR/src"
      cat >"$MAIN_DIR/Cargo.toml" <<'EOF_CARGO'
[package]
name = "onepassword-e2e"
version = "0.1.0"
edition = "2021"

[dependencies]
EOF_CARGO
      cat >"$MAIN_DIR/src/main.rs" <<'EOF_MAIN'
fn main() {
    println!("BranchBox 1Password E2E");
}
EOF_MAIN
      ;;
    generic)
      cat >"$MAIN_DIR/README.md" <<'EOF_GENERIC'
# BranchBox 1Password E2E (Generic)
EOF_GENERIC
      ;;
    rails)
      cat >"$MAIN_DIR/Gemfile" <<'EOF_GEM'
source "https://rubygems.org"
gem "rails", "~> 7.1.0"
EOF_GEM
      mkdir -p "$MAIN_DIR/bin"
      cat >"$MAIN_DIR/bin/setup" <<'EOF_SETUP'
#!/usr/bin/env bash
set -euo pipefail
echo "BranchBox rails e2e setup stub"
EOF_SETUP
      chmod +x "$MAIN_DIR/bin/setup"
      cat >"$MAIN_DIR/README.md" <<'EOF_RAILS'
# BranchBox 1Password E2E (Rails)
EOF_RAILS
      ;;
    node)
      cat >"$MAIN_DIR/package.json" <<'EOF_NODE'
{
  "name": "onepassword-e2e",
  "version": "1.0.0",
  "main": "index.js"
}
EOF_NODE
      cat >"$MAIN_DIR/index.js" <<'EOF_INDEX'
console.log("BranchBox 1Password E2E");
EOF_INDEX
      ;;
    *)
      fatal "Unsupported stack for seed: $STACK"
      ;;
  esac

  (
    cd "$MAIN_DIR"
    git init -b main >/dev/null
    git config user.name "${GIT_AUTHOR_NAME:-BranchBox Tester}"
    git config user.email "${GIT_AUTHOR_EMAIL:-branchbox-tester@example.com}"
    git add .
    git commit -m "Seed 1Password e2e project" >/dev/null
  )
}

validate_workspace_container() {
  local workspace="$1"
  local label="$2"
  local container_id="$3"
  local log_path="$LOG_DIR/validate-${label}.log"
  local workspace_name
  workspace_name="$(basename "$workspace")"

  if [[ -z "$container_id" ]]; then
    record_bug "No container ID resolved for $label workspace."
    return
  fi

  if ! docker exec \
    -e CHECK_LABEL="$label" \
    -e WORKSPACE_NAME="$workspace_name" \
    "$container_id" \
    bash -c '
      set -euo pipefail

      fail() {
        echo "validation failure: $*" >&2
        exit 1
      }

      home_dir="${HOME:-/home/vscode}"
      token_file="${home_dir}/.github-token.env"
      signing_key_file="${home_dir}/.git-signing-key"
      signing_key_dst="${home_dir}/.ssh/branchbox-signing-key"
      allowed_signers_file="${home_dir}/.ssh/allowed_signers"

      cd "/workspaces/${WORKSPACE_NAME}"
      git rev-parse --is-inside-work-tree >/dev/null 2>&1 \
        || fail "workspace /workspaces/${WORKSPACE_NAME} is not a git worktree"

      test -s "$token_file" || fail "missing ${token_file}"
      token="$(grep "^GITHUB_TOKEN=" "$token_file" | cut -d= -f2-)"
      test -n "$token" || fail "empty GITHUB_TOKEN in ${token_file}"

      helper="$(git config --global --get credential.https://github.com.helper || true)"
      test -n "$helper" || fail "missing git credential helper for github.com"

      remote="$(git remote get-url origin)"
      case "$remote" in
        https://github.com/*) ;;
        *)
          fail "origin remote did not convert to HTTPS: $remote"
          ;;
      esac

      signing_configured=0
      if [[ -s "$signing_key_file" ]] && ssh-keygen -y -f "$signing_key_file" >/dev/null 2>&1; then
        signing_configured=1
        test "$(git config --global --get gpg.format)" = "ssh" || fail "git gpg.format is not ssh"
        signing_key="$(git config --global --get user.signingkey)"
        test "$signing_key" = "$signing_key_dst" || fail "unexpected user.signingkey: $signing_key"
        test -s "$signing_key" || fail "missing private signing key at $signing_key"
        test -s "${signing_key}.pub" || fail "missing signing public key at ${signing_key}.pub"
        test -s "$allowed_signers_file" || fail "missing allowed_signers file"
      fi

      candidate_gh_token="${GH_TOKEN:-}"
      if [[ -z "$candidate_gh_token" ]]; then
        candidate_gh_token="$(grep "^GITHUB_TOKEN=" "$token_file" | cut -d= -f2-)"
      fi
      test -n "$candidate_gh_token" || fail "GH_TOKEN not set and fallback token missing"

      marker_file=".branchbox-1password-${CHECK_LABEL}.txt"
      printf "%s\n" "${CHECK_LABEL}-$(date -u +%s)" >>"$marker_file"
      git add "$marker_file"
      git commit -m "1Password e2e ${CHECK_LABEL}" >/dev/null

      if [[ "$signing_configured" == "1" ]]; then
        signature_output="$(git log --show-signature -1 2>&1 || true)"
        printf "%s\n" "$signature_output" | grep -q 'Good "git" signature'
      fi
    ' 2>&1 | tee "$log_path"; then
    record_bug "Container validation failed for $label (see $log_path)"
  fi
}

if [[ "$MODE" == "pretend" ]]; then
  log "[pretend] Would run 1Password e2e with stack '$STACK' and feature '$FEATURE_NAME'."
  log "[pretend] Would seed repo at $MAIN_DIR, run branchbox init, and start devcontainers."
  log "[pretend] Would validate token/signing config in main and feature worktrees."
  if [[ "$CHECK_FAILURE_PATH" == "1" ]]; then
    log "[pretend] Would run invalid-reference failure-path smoke check."
  fi
  report_results
  exit 0
fi

require_cmd bash
require_cmd git
require_cmd jq
require_cmd docker
require_cmd devcontainer
require_cmd op

if [[ "$SKIP_BUILD" == "0" ]]; then
  require_cmd cargo
fi

if ! docker info >/dev/null 2>&1; then
  fatal "Docker daemon is not reachable."
fi
configure_compose_command

if [[ -z "$ORIGIN_SSH_URL" ]]; then
  fatal "Set ORIGIN_SSH_URL (or pass --origin-ssh-url) to an SSH GitHub remote."
fi
if [[ ! "$ORIGIN_SSH_URL" =~ ^git@github\.com: ]] && [[ ! "$ORIGIN_SSH_URL" =~ ^ssh://git@github\.com/ ]]; then
  fatal "ORIGIN_SSH_URL must target GitHub over SSH (git@github.com:... or ssh://git@github.com/...)."
fi
if [[ -z "$OP_GITHUB_REF" ]]; then
  fatal "Set OP_GITHUB_REF (or pass --op-github-ref)."
fi
if [[ -z "$OP_SIGNING_KEY_REF" ]]; then
  fatal "Set OP_SIGNING_KEY_REF (or pass --op-signing-key-ref)."
fi

log "Running 1Password devcontainer e2e (stack: $STACK)"

if [[ "$SKIP_BUILD" == "0" ]]; then
  log "Building branchbox CLI"
  if ! (cd "$REPO_ROOT" && cargo build -p branchbox-cli >/dev/null); then
    fatal "cargo build -p branchbox-cli failed"
  fi
fi

if [[ ! -x "$BRANCHBOX_BIN" ]]; then
  fatal "branchbox binary not found at $BRANCHBOX_BIN"
fi

seed_sample_project

log "Configuring SSH origin for conversion test"
(
  cd "$MAIN_DIR"
  git remote add origin "$ORIGIN_SSH_URL"
)

log "Running branchbox init"
INIT_LOG="$LOG_DIR/init.log"
if ! (
  cd "$MAIN_DIR"
  BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" init -y --stack "$STACK"
) | tee "$INIT_LOG"; then
  fatal "branchbox init failed (see $INIT_LOG)"
fi

assert_file_exists "$MAIN_DIR/.devcontainer/scripts/init-host.sh"
assert_file_exists "$MAIN_DIR/.devcontainer/scripts/setup-git.sh"
assert_file_exists "$MAIN_DIR/.devcontainer/.github-token.env"
assert_file_exists "$MAIN_DIR/.devcontainer/.git-signing-key"
assert_file_exists "$MAIN_DIR/.devcontainer/.gitconfig.env"

# Fail fast here; if these are missing, we're not testing the branch implementation.
require_file_exists "$MAIN_DIR/.devcontainer/scripts/init-host.sh"
require_file_exists "$MAIN_DIR/.devcontainer/scripts/setup-git.sh"
require_file_exists "$MAIN_DIR/.devcontainer/.github-token.env"
require_file_exists "$MAIN_DIR/.devcontainer/.git-signing-key"
require_file_exists "$MAIN_DIR/.devcontainer/.gitconfig.env"

log "Booting main devcontainer (initializeCommand + postStartCommand)"
set_devcontainer_name "$MAIN_DIR" "$MAIN_DEVCONTAINER_NAME"
MAIN_UP_LOG="$LOG_DIR/main-devcontainer-up.log"
if ! (
  cd "$MAIN_DIR"
  OP_GITHUB_REF="$OP_GITHUB_REF" OP_SIGNING_KEY_REF="$OP_SIGNING_KEY_REF" \
    devcontainer up --remove-existing-container --workspace-folder "$MAIN_DIR"
) 2>&1 | tee "$MAIN_UP_LOG"; then
  fatal "devcontainer up failed for main workspace (see $MAIN_UP_LOG)"
fi

MAIN_CONTAINER_ID="$(extract_container_id_from_log "$MAIN_UP_LOG" || true)"
if [[ -z "$MAIN_CONTAINER_ID" ]]; then
  MAIN_CONTAINER_ID="$(resolve_container_id "$MAIN_DIR" || true)"
fi
if [[ -z "$MAIN_CONTAINER_ID" ]]; then
  fatal "Failed to resolve running container for main workspace."
fi

validate_workspace_container "$MAIN_DIR" "main" "$MAIN_CONTAINER_ID"

log "Starting feature worktree"
FEATURE_START_LOG="$LOG_DIR/feature-start.log"
if ! (
  cd "$MAIN_DIR"
  BRANCHBOX_SKIP_HOST_VALIDATION=1 "$BRANCHBOX_BIN" feature start "$FEATURE_NAME"
) | tee "$FEATURE_START_LOG"; then
  fatal "branchbox feature start failed (see $FEATURE_START_LOG)"
fi

if [[ ! -d "$FEATURE_DIR" ]]; then
  fatal "Expected feature worktree at $FEATURE_DIR"
fi

log "Syncing devcontainer across worktrees"
FEATURE_SYNC_LOG="$LOG_DIR/devcontainer-sync.log"
if ! (
  cd "$MAIN_DIR"
  "$BRANCHBOX_BIN" devcontainer sync --strategy copy
) | tee "$FEATURE_SYNC_LOG"; then
  fatal "branchbox devcontainer sync failed (see $FEATURE_SYNC_LOG)"
fi

log "Booting feature devcontainer"
set_devcontainer_name "$FEATURE_DIR" "$FEATURE_DEVCONTAINER_NAME"
FEATURE_UP_LOG="$LOG_DIR/feature-devcontainer-up.log"
if ! (
  cd "$FEATURE_DIR"
  OP_GITHUB_REF="$OP_GITHUB_REF" OP_SIGNING_KEY_REF="$OP_SIGNING_KEY_REF" \
    devcontainer up --remove-existing-container --workspace-folder "$FEATURE_DIR"
) 2>&1 | tee "$FEATURE_UP_LOG"; then
  fatal "devcontainer up failed for feature workspace (see $FEATURE_UP_LOG)"
fi

FEATURE_CONTAINER_ID="$(extract_container_id_from_log "$FEATURE_UP_LOG" || true)"
if [[ -z "$FEATURE_CONTAINER_ID" ]]; then
  FEATURE_CONTAINER_ID="$(resolve_container_id "$FEATURE_DIR" || true)"
fi
if [[ -z "$FEATURE_CONTAINER_ID" ]]; then
  fatal "Failed to resolve running container for feature workspace."
fi

validate_workspace_container "$FEATURE_DIR" "feature" "$FEATURE_CONTAINER_ID"

if [[ "$CHECK_FAILURE_PATH" == "1" ]]; then
  log "Running invalid-reference failure-path smoke check"
  devcontainer down --workspace-folder "$MAIN_DIR" >/dev/null 2>&1 || true
  FAILURE_LOG="$LOG_DIR/failure-path.log"
  if ! (
    cd "$MAIN_DIR"
    OP_GITHUB_REF="op://__branchbox_nonexistent__/invalid/token" \
      OP_SIGNING_KEY_REF="op://__branchbox_nonexistent__/invalid/private key" \
      devcontainer up --remove-existing-container --workspace-folder "$MAIN_DIR"
) 2>&1 | tee "$FAILURE_LOG"; then
    record_bug "Failure-path restart with invalid refs did not complete."
  fi
  assert_log_contains "$FAILURE_LOG" "BranchBox warning"
fi

report_results

if ((${#BUGS[@]} > 0)); then
  exit 1
fi
