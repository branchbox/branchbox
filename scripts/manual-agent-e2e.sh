#!/usr/bin/env bash
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
HELPER="${SCRIPT_DIR}/lib/agent-e2e.py"
USE_CP_STUB=0
IPC_ONLY=0
CLI_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --cp-stub) USE_CP_STUB=1 ;;
    --ipc-only) IPC_ONLY=1 ;;
    -h|--help)
      echo "Usage: manual-agent-e2e.sh [--cp-stub] [--ipc-only] [manual-cli-e2e.sh arguments]"
      echo "--ipc-only runs a real disposable agent lifecycle without the Docker CLI harness."
      exit 0 ;;
    *) CLI_ARGS+=("$1") ;;
  esac
  shift
done

# Stay below macOS's Unix socket path limit even when TMPDIR is long.
AGENT_STATE_DIR="$(mktemp -d /tmp/branchbox-agent-e2e.XXXXXX)"
AGENT_SOCKET="${AGENT_STATE_DIR}/agent.sock"
AGENT_LOG="${AGENT_STATE_DIR}/agent.log"
AGENT_PID=""
CP_STUB_PID=""

stop_owned_process() {
  local pid="$1"
  if [[ -n "$pid" ]]; then
    kill "$pid" 2>/dev/null || true
    for _ in {1..25}; do
      if ! kill -0 "$pid" 2>/dev/null; then break; fi
      sleep 0.2
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
  fi
}
cleanup() {
  local result=$?
  stop_owned_process "${AGENT_PID}"
  stop_owned_process "${CP_STUB_PID}"
  rm -f "${AGENT_SOCKET}"
  if [[ "${KEEP_AGENT_TMP:-0}" == "1" || "$result" -ne 0 ]]; then
    echo "Keeping private agent receipts/logs in ${AGENT_STATE_DIR}"
  else
    rm -rf "${AGENT_STATE_DIR}"
  fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

python3 -I "${HELPER}" prepare "${AGENT_STATE_DIR}"
export BRANCHBOX_AGENT_CONFIG="${AGENT_STATE_DIR}/agent.toml"
export BRANCHBOX_AGENT_DIR="${AGENT_STATE_DIR}"
export BRANCHBOX_AGENT_SOCKET="${AGENT_SOCKET}"

if [[ "${USE_CP_STUB}" -eq 1 ]]; then
  echo "==> Starting private loopback control-plane stub"
  python3 -I "${HELPER}" stub "${AGENT_STATE_DIR}" "${BRANCHBOX_CP_STUB_PORT:-0}" \
    >"${AGENT_STATE_DIR}/cp-stub.log" 2>&1 &
  CP_STUB_PID=$!
  for _ in {1..100}; do
    if [[ -s "${AGENT_STATE_DIR}/cp-stub.port" ]]; then break; fi
    if ! kill -0 "${CP_STUB_PID}" 2>/dev/null; then break; fi
    sleep 0.1
  done
  if [[ ! -s "${AGENT_STATE_DIR}/cp-stub.port" ]]; then
    echo "Control-plane stub failed to start (see ${AGENT_STATE_DIR}/cp-stub.log)" >&2
    exit 1
  fi
  CP_STUB_PORT="$(cat "${AGENT_STATE_DIR}/cp-stub.port")"
  export BRANCHBOX_CP_ENDPOINT="http://127.0.0.1:${CP_STUB_PORT}/events"
  export BRANCHBOX_CP_TOKEN="stub-token"
  export BRANCHBOX_CP_VERIFY_TLS=0
else
  # A local fixture must not send its events to the caller's real control plane.
  unset BRANCHBOX_CP_ENDPOINT BRANCHBOX_CP_TOKEN BRANCHBOX_CP_VERIFY_TLS
fi

if [[ -z "${BRANCHBOX_AGENT_BIN:-}" ]]; then
  echo "==> Building BranchBox agent (release)"
  cargo build --manifest-path "${REPO_ROOT}/Cargo.toml" -p branchbox-agent --release >/dev/null
  BRANCHBOX_AGENT_BIN="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/release/branchbox-agent"
fi
if [[ ! -x "${BRANCHBOX_AGENT_BIN}" ]]; then
  echo "Agent binary is not executable: ${BRANCHBOX_AGENT_BIN}" >&2
  exit 1
fi
# Validate and resolve in the same caller directory, before the daemon changes cwd.
BRANCHBOX_AGENT_BIN="$(python3 -I -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "${BRANCHBOX_AGENT_BIN}")"

echo "==> Starting BranchBox agent (socket: ${AGENT_SOCKET})"
(
  cd "${REPO_ROOT}"
  for git_variable in "${!GIT_@}"; do unset "$git_variable"; done
  unset FEATURES_DIR
  export BRANCHBOX_SKIP_HOST_VALIDATION=1 BRANCHBOX_POLICY_ENFORCED_MODULES=""
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1
  exec "${BRANCHBOX_AGENT_BIN}"
) >"${AGENT_LOG}" 2>&1 &
AGENT_PID=$!
printf '{"agent_pid": %s, "cp_stub_pid": %s}\n' \
  "${AGENT_PID}" "${CP_STUB_PID:-0}" >"${AGENT_STATE_DIR}/processes.json"
for _ in {1..150}; do
  if [[ -S "${AGENT_SOCKET}" ]]; then break; fi
  if ! kill -0 "${AGENT_PID}" 2>/dev/null; then break; fi
  sleep 0.2
done
if [[ ! -S "${AGENT_SOCKET}" ]]; then
  echo "Agent socket not available (logs: ${AGENT_LOG})" >&2
  exit 1
fi

echo "==> Starting and tearing down a disposable minimal feature through agent IPC"
python3 -I "${HELPER}" lifecycle "${AGENT_STATE_DIR}"
if [[ "${USE_CP_STUB}" -eq 1 ]]; then
  echo "==> Verifying workflow delivery, retry, metadata and durable final acknowledgement"
  python3 -I "${HELPER}" verify "${AGENT_STATE_DIR}"
fi
if [[ "${IPC_ONLY}" -eq 0 ]]; then
  echo "==> Running the separate direct-CLI Docker lifecycle harness"
  if ((${#CLI_ARGS[@]} > 0)); then
    "${SCRIPT_DIR}/manual-cli-e2e.sh" "${CLI_ARGS[@]}"
  else
    "${SCRIPT_DIR}/manual-cli-e2e.sh"
  fi
fi
echo "==> Agent IPC lifecycle passed"
