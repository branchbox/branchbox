#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BRANCHBOX_BIN="${BRANCHBOX_BIN:-$REPO_ROOT/target/debug/branchbox}"
DRIVER="${BRANCHBOX_LOCAL_VM_DRIVER_PATH:-$REPO_ROOT/scripts/local-vm/branchbox-local-vm}"
TMP_ROOT=$(mktemp -d)
SIMPLE_REPO="$TMP_ROOT/simple"
STACK_REPO="$TMP_ROOT/stack"
RUNTIME_IDS=()

cleanup() {
  for runtime_id in "${RUNTIME_IDS[@]}"; do "$DRIVER" destroy "$runtime_id" >/dev/null 2>&1 || true; done
  find "$TMP_ROOT" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT

export BRANCHBOX_LOCAL_VM_DRIVER_PATH="$DRIVER"
export BRANCHBOX_SKIP_HOST_VALIDATION=1
export RUST_LOG=off

git_identity() {
  git -C "$1" init -b main
  git -C "$1" config user.email branchbox-local-vm@example.com
  git -C "$1" config user.name 'BranchBox local-vm E2E'
}

runtime_id_from() {
  jq -er '.runtime.runtime_id' "$1"
}

echo '==> Building BranchBox CLI'
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p branchbox-cli
"$DRIVER" validate

echo '==> Proving a single-container devcontainer and interactive coding CLI'
mkdir -p "$SIMPLE_REPO/.devcontainer"
git_identity "$SIMPLE_REPO"
printf '# simple local-vm fixture\n' >"$SIMPLE_REPO/README.md"
printf '#!/bin/sh\nset -eu\nprintf agent-ran > agent-proof.txt\n' >"$SIMPLE_REPO/agent-stub.sh"
chmod +x "$SIMPLE_REPO/agent-stub.sh"
printf '%s\n' '{"name":"simple","image":"mcr.microsoft.com/devcontainers/base:ubuntu","workspaceFolder":"/workspaces/simple-feature"}' \
  >"$SIMPLE_REPO/.devcontainer/devcontainer.json"
git -C "$SIMPLE_REPO" add .
git -C "$SIMPLE_REPO" commit -m 'Seed single-container fixture'
BRANCHBOX_DEFAULT_AGENT_CMD=./agent-stub.sh "$BRANCHBOX_BIN" feature start simple-feature \
  --repo "$SIMPLE_REPO" --runtime local-vm --skip-module tunnel >"$TMP_ROOT/simple-start.log"
"$BRANCHBOX_BIN" feature list --repo "$SIMPLE_REPO" --json >"$TMP_ROOT/simple-list.json"
jq -e '.[] | select(.work_feature == "simple-feature")' "$TMP_ROOT/simple-list.json" >"$TMP_ROOT/simple-start.json"
simple_runtime=$(runtime_id_from "$TMP_ROOT/simple-start.json")
RUNTIME_IDS+=("$simple_runtime")
test "$(jq -r '.runtime.version.monitor' "$TMP_ROOT/simple-start.json")" != null
test -f "$TMP_ROOT/simple-feature/agent-proof.txt"
"$BRANCHBOX_BIN" feature exec simple-feature --repo "$SIMPLE_REPO" --json -- git status --short \
  >"$TMP_ROOT/simple-exec.json"
test "$(jq -r '.exit_code' "$TMP_ROOT/simple-exec.json")" = 0

echo '==> Proving two concurrent app + Postgres + Redis stacks and host port isolation'
mkdir -p "$STACK_REPO/.devcontainer"
git_identity "$STACK_REPO"
printf '# multi-service local-vm fixture\n' >"$STACK_REPO/README.md"
printf '%s\n' '{"name":"stack","dockerComposeFile":"compose.yaml","service":"app","workspaceFolder":"/workspaces/WORKSPACE","forwardPorts":[3000],"postStartCommand":"nohup python3 -m http.server 3000 >/tmp/branchbox-http.log 2>&1 &"}' \
  >"$STACK_REPO/.devcontainer/devcontainer.json"
cat >"$STACK_REPO/.devcontainer/compose.yaml" <<'YAML'
services:
  app:
    image: mcr.microsoft.com/devcontainers/base:ubuntu
    command: sleep infinity
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
  postgres:
    image: postgres:17-alpine
    environment:
      POSTGRES_PASSWORD: branchbox-e2e
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 2s
      timeout: 2s
      retries: 30
  redis:
    image: redis:7-alpine
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 2s
      timeout: 2s
      retries: 30
YAML
git -C "$STACK_REPO" add .
git -C "$STACK_REPO" commit -m 'Seed multi-service fixture'

for feature in stack-one stack-two; do
  jq --arg workspace "/workspaces/$feature" '.workspaceFolder = $workspace' \
    "$STACK_REPO/.devcontainer/devcontainer.json" \
    >"$STACK_REPO/.devcontainer/devcontainer.json.next"
  mv "$STACK_REPO/.devcontainer/devcontainer.json.next" "$STACK_REPO/.devcontainer/devcontainer.json"
  git -C "$STACK_REPO" add .devcontainer/devcontainer.json
  git -C "$STACK_REPO" commit -m "Configure $feature workspace"
  "$BRANCHBOX_BIN" feature start "$feature" --repo "$STACK_REPO" --runtime local-vm \
    --skip-module tunnel --json >"$TMP_ROOT/$feature-start.json"
  RUNTIME_IDS+=("$(runtime_id_from "$TMP_ROOT/$feature-start.json")")
done

port_one=$(jq -er '.runtime.published_ports[] | select(.runtime == 3000) | .host' "$TMP_ROOT/stack-one-start.json")
port_two=$(jq -er '.runtime.published_ports[] | select(.runtime == 3000) | .host' "$TMP_ROOT/stack-two-start.json")
test "$port_one" != "$port_two"

for feature in stack-one stack-two; do
  "$BRANCHBOX_BIN" feature exec "$feature" --repo "$STACK_REPO" --json -- bash -lc \
    'echo >/dev/tcp/postgres/5432 && echo >/dev/tcp/redis/6379 && test ! -S /var/run/docker.sock && gateway=$(ip route | awk '\''/default/{print $3}'\'') && ! timeout 2 bash -c "echo >/dev/tcp/${gateway}/22"' \
    >"$TMP_ROOT/$feature-exec.json"
  test "$(jq -r '.exit_code' "$TMP_ROOT/$feature-exec.json")" = 0
done
curl --fail --retry 20 --retry-delay 1 "http://127.0.0.1:$port_one/README.md" | grep -q 'multi-service'
curl --fail --retry 20 --retry-delay 1 "http://127.0.0.1:$port_two/README.md" | grep -q 'multi-service'

echo '==> Proving deterministic teardown with no VM/TAP/disk/process orphan'
"$BRANCHBOX_BIN" feature teardown simple-feature --repo "$SIMPLE_REPO" --force
"$BRANCHBOX_BIN" feature teardown stack-one --repo "$STACK_REPO" --force
"$BRANCHBOX_BIN" feature teardown stack-two --repo "$STACK_REPO" --force
for runtime_id in "${RUNTIME_IDS[@]}"; do
  ! "$DRIVER" exists "$runtime_id"
  ! pgrep -af "firecracker.*$runtime_id" >/dev/null
  test ! -e "${BRANCHBOX_LOCAL_VM_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/branchbox/local-vm}/$runtime_id"
done
RUNTIME_IDS=()

echo '✅ local-vm Firecracker E2E passed.'
