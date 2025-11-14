#!/usr/bin/env bash
# Boot the BranchBox agent with a local config suitable for devcontainer testing.
set -euo pipefail

WORKSPACE="${WORKSPACE:-/workspaces/milestone2}"
STATE_DIR="${STATE_DIR:-/tmp/m2-agent-state}"
SOCKET_PATH="${SOCKET_PATH:-${STATE_DIR}/branchbox-agent.sock}"
CONFIG="${CONFIG:-/tmp/agent-local.toml}"
GRPC_ADDR="${GRPC_ADDR:-0.0.0.0:50515}"
CP_ENDPOINT="${CP_ENDPOINT:-http://127.0.0.1:8787/events}"

mkdir -p "${STATE_DIR}"

cat > "${CONFIG}" <<EOF
workspace_root = "${WORKSPACE}"
state_dir = "${STATE_DIR}"
socket_path = "${SOCKET_PATH}"
heartbeat_interval_secs = 5
event_flush_interval_secs = 2
event_batch_size = 10
event_log_only = false
grpc_addr = "${GRPC_ADDR}"

[control_plane]
enabled = true
endpoint = "${CP_ENDPOINT}"
api_token = "dev"
verify_tls = false
EOF

echo "Using agent config: ${CONFIG}"
echo "Socket path: ${SOCKET_PATH}"

BRANCHBOX_AGENT_CONFIG="${CONFIG}" cargo run -p branchbox-agent
