#!/usr/bin/env bash
# Lightweight control-plane stub that acks whatever the agent sends.
set -euo pipefail

PORT="${1:-8787}"
LOG="${2:-/tmp/cp_stub.log}"

python3 - <<'PY' "$PORT" "$LOG"
import json
import sys
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = int(sys.argv[1])
LOG = sys.argv[2]

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        try:
            payload = json.loads(body.decode("utf-8"))
        except Exception:
            payload = {}
        ack_id = payload.get("cursor", {}).get("last_event_id", 0)
        ack = json.dumps({"acked_through": ack_id}).encode("utf-8")
        with open(LOG, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(payload) + "\n")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(ack)))
        self.end_headers()
        self.wfile.write(ack)

    def log_message(self, fmt, *args):
        return

HTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
PY
