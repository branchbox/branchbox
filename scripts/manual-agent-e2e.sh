#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
AGENT_STATE_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t branchbox-agent-state)"
AGENT_SOCKET="${AGENT_STATE_DIR}/branchbox-agent.sock"
AGENT_LOG="${AGENT_STATE_DIR}/agent.log"
AGENT_PID=""
USE_CP_STUB=0
CP_STUB_PID=""
CP_STUB_LOG=""

CLI_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --cp-stub)
      USE_CP_STUB=1
      shift
      ;;
    *)
      CLI_ARGS+=("$1")
      shift
      ;;
  esac
done
if ((${#CLI_ARGS[@]} > 0)); then
  set -- "${CLI_ARGS[@]}"
else
  set --
fi

cleanup() {
  if [[ -n "${AGENT_PID:-}" ]]; then
    kill "${AGENT_PID}" 2>/dev/null || true
    wait "${AGENT_PID}" 2>/dev/null || true
  fi
  if [[ -n "${CP_STUB_PID:-}" ]]; then
    kill "${CP_STUB_PID}" 2>/dev/null || true
    wait "${CP_STUB_PID}" 2>/dev/null || true
  fi
  if [[ -z "${KEEP_AGENT_TMP:-}" ]]; then
    rm -rf "${AGENT_STATE_DIR}"
  else
    echo "Keeping agent state/logs in ${AGENT_STATE_DIR}"
  fi
}
trap cleanup EXIT

if [[ "${USE_CP_STUB}" -eq 1 ]]; then
  CP_STUB_LOG="$(mktemp /tmp/branchbox-cp-stub-log.XXXXXX)"
  CP_STUB_PORT="${BRANCHBOX_CP_STUB_PORT:-50550}"
  export CP_STUB_PORT
  export CP_STUB_LOG
  echo "==> Starting control-plane stub on port ${CP_STUB_PORT}"
  python3 - <<'PY' >"${CP_STUB_LOG}" 2>&1 &
import http.server
import os
from datetime import datetime

port = int(os.environ["CP_STUB_PORT"])
log_path = os.environ["CP_STUB_LOG"]

class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        body = self.rfile.read(length).decode('utf-8')
        with open(log_path, 'a', encoding='utf-8') as handle:
            handle.write(f"=== {datetime.utcnow().isoformat()} ===\n")
            handle.write(body)
            handle.write("\n---\n")
        self.send_response(200)
        self.send_header('Content-Length', '2')
        self.end_headers()
        self.wfile.write(b"OK")

    def log_message(self, *args, **kwargs):
        return

http.server.HTTPServer(('127.0.0.1', port), Handler).serve_forever()
PY
  CP_STUB_PID=$!
  export BRANCHBOX_CP_ENDPOINT="http://127.0.0.1:${CP_STUB_PORT}/events"
  export BRANCHBOX_CP_TOKEN="stub-token"
  export BRANCHBOX_CP_VERIFY_TLS=0
fi

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

if [[ "${USE_CP_STUB}" -eq 1 ]]; then
  echo "==> Control-plane stub logs: ${CP_STUB_LOG}"
  python3 - <<'PY' "${AGENT_STATE_DIR}"
import sqlite3
import sys
from pathlib import Path

db_path = Path(sys.argv[1]) / "agent.db"
conn = sqlite3.connect(db_path)
cursor = conn.execute("SELECT last_ack_event_id FROM control_plane_status WHERE id = 1")
row = cursor.fetchone()
conn.close()
print(f"==> Last acked event id: {row[0] if row and row[0] is not None else 'none'}")
PY
fi
