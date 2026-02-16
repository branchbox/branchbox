#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

FAILURES=()

record_failure() {
  local message="$1"
  FAILURES+=("$message")
  printf '❌ %s\n' "$message" >&2
}

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "$cmd" >&2
    exit 2
  fi
}

check_forbidden_pattern() {
  local description="$1"
  local pattern="$2"
  shift 2
  local paths=("$@")
  local output_file
  output_file="$(mktemp)"
  if rg -n --no-heading -S "$pattern" "${paths[@]}" >"$output_file"; then
    record_failure "$description"
    sed 's/^/   /' "$output_file" >&2
  fi
  rm -f "$output_file"
}

check_required_pattern() {
  local description="$1"
  local pattern="$2"
  shift 2
  local paths=("$@")
  if ! rg -n --no-heading -S "$pattern" "${paths[@]}" >/dev/null; then
    record_failure "$description"
  fi
}

check_required_literal() {
  local description="$1"
  local literal="$2"
  local path="$3"
  if ! rg -n --no-heading -F "$literal" "$path" >/dev/null; then
    record_failure "$description"
  fi
}

check_no_harness_helper_duplication() {
  local output_file
  output_file="$(mktemp)"
  if rg -n --no-heading -S '^(function )?(detect_compose_service|read_devcontainer_service|resolve_devcontainer_service)\s*\(' \
    scripts/manual-cli-e2e.sh scripts/manual-1password-e2e.sh >"$output_file"; then
    record_failure "Duplicate devcontainer service helpers found in manual harness scripts; use scripts/lib/devcontainer-service.sh."
    sed 's/^/   /' "$output_file" >&2
  fi
  rm -f "$output_file"
}

check_no_compose_helper_duplication() {
  local output_file
  output_file="$(mktemp)"
  if rg -n --no-heading -S '^(function )?configure_compose_command\s*\(' \
    scripts/manual-cli-e2e.sh scripts/manual-1password-e2e.sh >"$output_file"; then
    record_failure "Duplicate docker compose helper found in manual harness scripts; use scripts/lib/docker-compose.sh."
    sed 's/^/   /' "$output_file" >&2
  fi
  rm -f "$output_file"
}

print_header() {
  printf '==> %s\n' "$1"
}

require_cmd rg
require_cmd diff

print_header "Checking secret-write security guardrails"
check_forbidden_pattern \
  "Found broad chmod for signing key artifacts." \
  'chmod 64[0-9].*(SIGNING_KEY|git-signing-key)|chmod 66[0-9].*(SIGNING_KEY|git-signing-key)' \
  core/src/bootstrap scripts
check_required_pattern \
  "Missing restrictive umask-based token write in init-host script." \
  'umask 077; printf .*github_token' \
  core/src/bootstrap/templates/common/init-host.sh
check_required_pattern \
  "Missing restrictive umask-based signing-key write in init-host script." \
  'umask 077; printf .*signing_key' \
  core/src/bootstrap/templates/common/init-host.sh
check_required_literal \
  "Missing empty-token preservation warning in init-host script." \
  "was empty; keeping existing token file." \
  core/src/bootstrap/templates/common/init-host.sh
check_required_literal \
  "Missing empty-signing-key preservation warning in init-host script." \
  "was empty; keeping existing key file." \
  core/src/bootstrap/templates/common/init-host.sh
check_forbidden_pattern \
  "Found raw GitHub token interpolation in setup-git credential helper." \
  'password=\$\{?github_token\}?' \
  core/src/bootstrap/templates/common/setup-git.sh

print_header "Checking env sanitization policies"
check_required_literal \
  "Missing compose-name sanitize-once flow (raw_compose_name)." \
  "let raw_compose_name = format!(\"{}-{}\", app_slug, work_feature);" \
  core/src/workflows/feature.rs
check_required_literal \
  "Missing compose-name sanitize-once flow (compose_name)." \
  "let compose_name = sanitize_compose_project_name(&raw_compose_name);" \
  core/src/workflows/feature.rs
check_forbidden_pattern \
  "Compose sanitizer still allows '.' characters." \
  "matches!\\(ch, '-' \\| '_' \\| '\\.'\\)" \
  core/src/workflows/feature.rs
check_required_literal \
  "Missing strict compose sanitizer allow-list." \
  "matches!(ch, '-' | '_')" \
  core/src/workflows/feature.rs
check_required_literal \
  "Missing strict GIT_BRANCH sanitizer allow-list." \
  "filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/'))" \
  core/src/workflows/feature.rs

print_header "Checking symlink-write hardening"
check_required_pattern \
  "Feature workflow missing symlink guard helper for writes." \
  '^fn ensure_not_symlink\(' \
  core/src/workflows/feature.rs
check_required_pattern \
  "Feature workflow missing O_NOFOLLOW hardening on managed writes." \
  'custom_flags\(libc::O_NOFOLLOW\)' \
  core/src/workflows/feature.rs
check_required_pattern \
  "Bootstrap write_if_missing missing symlink guard." \
  'Refusing to write through symlink' \
  core/src/bootstrap/mod.rs
check_required_pattern \
  "Bootstrap write_if_missing missing O_NOFOLLOW hardening." \
  'custom_flags\(libc::O_NOFOLLOW\)' \
  core/src/bootstrap/mod.rs

print_header "Checking harness portability + dedupe"
check_required_pattern \
  "manual-1password-e2e.sh missing shared devcontainer helper source." \
  'source "\$REPO_ROOT/scripts/lib/devcontainer-service\.sh"' \
  scripts/manual-1password-e2e.sh
check_required_pattern \
  "manual-cli-e2e.sh missing shared devcontainer helper source." \
  'source "\$REPO_ROOT/scripts/lib/devcontainer-service\.sh"' \
  scripts/manual-cli-e2e.sh
check_required_pattern \
  "manual-1password-e2e.sh missing shared docker-compose helper source." \
  'source "\$REPO_ROOT/scripts/lib/docker-compose\.sh"' \
  scripts/manual-1password-e2e.sh
check_required_pattern \
  "manual-cli-e2e.sh missing shared docker-compose helper source." \
  'source "\$REPO_ROOT/scripts/lib/docker-compose\.sh"' \
  scripts/manual-cli-e2e.sh
check_required_pattern \
  "Shared helper file missing resolve_devcontainer_service function." \
  '^resolve_devcontainer_service\(\)' \
  scripts/lib/devcontainer-service.sh
check_required_pattern \
  "Shared helper file missing configure_compose_command function." \
  '^configure_compose_command\(\)' \
  scripts/lib/docker-compose.sh
check_no_harness_helper_duplication
check_no_compose_helper_duplication
check_required_pattern \
  "Shared docker-compose helper missing docker-compose fallback handling." \
  'docker-compose' \
  scripts/lib/docker-compose.sh

print_header "Checking docs synchronization"
if ! diff -u scripts/manual-1password-e2e.md docs/docs/getting-started/manual-1password-e2e.md >/dev/null; then
  record_failure "scripts/manual-1password-e2e.md and docs/docs/getting-started/manual-1password-e2e.md are out of sync."
fi

if ((${#FAILURES[@]} > 0)); then
  echo
  echo "Review preflight failed with ${#FAILURES[@]} issue(s):" >&2
  for failure in "${FAILURES[@]}"; do
    echo " - $failure" >&2
  done
  exit 1
fi

echo
echo "✅ Review preflight passed."
