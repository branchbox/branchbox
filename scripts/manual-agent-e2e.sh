#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
AGENT_STATE_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t branchbox-agent-state)"
AGENT_SOCKET="${AGENT_STATE_DIR}/branchbox-agent.sock"
AGENT_LOG="${AGENT_STATE_DIR}/agent.log"
AGENT_PID=""

cleanup() {
  if [[ -n "${AGENT_PID:-}" ]]; then
    kill "${AGENT_PID}" 2>/dev/null || true
    wait "${AGENT_PID}" 2>/dev/null || true
  fi
  if [[ -z "${KEEP_AGENT_TMP:-}" ]]; then
    rm -rf "${AGENT_STATE_DIR}"
  else
    echo "Keeping agent state/logs in ${AGENT_STATE_DIR}"
  fi
}
trap cleanup EXIT

echo "==> Building BranchBox agent (release)"
cargo build -p branchbox-agent --release >/dev/null

echo "==> Starting BranchBox agent (socket: ${AGENT_SOCKET})"
(
  cd "${REPO_ROOT}" && \
  BRANCHBOX_AGENT_DIR="${AGENT_STATE_DIR}" \
  BRANCHBOX_AGENT_SOCKET="${AGENT_SOCKET}" \
  BRANCHBOX_SKIP_HOST_VALIDATION=1 \
  cargo run -p branchbox-agent --release
) >"${AGENT_LOG}" 2>&1 &
AGENT_PID=$!

for _ in {1..150}; do
  if [[ -S "${AGENT_SOCKET}" ]]; then
    break
  fi
  sleep 0.2
done

if [[ ! -S "${AGENT_SOCKET}" ]]; then
  echo "❌ Agent socket ${AGENT_SOCKET} not available (logs: ${AGENT_LOG})"
  exit 1
fi

echo "==> Agent started (pid ${AGENT_PID}); tail -f ${AGENT_LOG} for details"

export BRANCHBOX_AGENT_SOCKET="${AGENT_SOCKET}"
export BRANCHBOX_AGENT_DIR="${AGENT_STATE_DIR}"
unset BRANCHBOX_CLI_DIRECT

echo "==> Running manual CLI e2e via agent"
"${SCRIPT_DIR}/manual-cli-e2e.sh" "$@"
