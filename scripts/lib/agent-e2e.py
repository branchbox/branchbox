#!/usr/bin/env python3
"""Private fixture, Unix IPC and drain assertions for manual-agent-e2e.sh."""

import argparse
from contextlib import closing
import http.server
import json
import os
from pathlib import Path
import socket
import signal
import sqlite3
import subprocess
import time

FEATURE = "agent-ipc-smoke"
BRANCH = "feature/" + FEATURE
PROMPT = "Disposable agent IPC delivery check"
TIMEOUT = 30.0


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def git(repo, *args):
    # -C does not override GIT_DIR/GIT_WORK_TREE, and inherited templates/config can
    # install caller-selected hooks. Seeding must use only its private repository.
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null",
               GIT_CONFIG_NOSYSTEM="1")
    return subprocess.run(["git", "-C", str(repo), *args], env=env, check=True,
                          capture_output=True, text=True, timeout=10).stdout.strip()


def prepare(state):
    repo = state / "workspace" / "main"
    repo.mkdir(parents=True)
    (repo / "README.md").write_text("# Disposable agent fixture\n", encoding="utf-8")
    (repo / ".gitignore").write_text(".branchbox/\n", encoding="utf-8")
    git(repo, "init", "-b", "main")
    git(repo, "config", "user.name", "BranchBox E2E")
    git(repo, "config", "user.email", "e2e@example.invalid")
    git(repo, "add", ".")
    git(repo, "-c", "commit.gpgsign=false", "commit", "-m", "Seed private IPC fixture")
    config = repo / ".branchbox"
    config.mkdir()
    write_json(config / "config.json", {"tunnel": {"enabled": False}})
    # JSON-quoted paths also escape TOML basic strings.
    (state / "agent.toml").write_text(
        f"workspace_root = {json.dumps(str(repo))}\n"
        f"state_dir = {json.dumps(str(state))}\n"
        f"socket_path = {json.dumps(str(state / 'agent.sock'))}\n"
        "grpc_enabled = false\nheartbeat_interval_secs = 5\n"
        "event_flush_interval_secs = 1\n", encoding="utf-8")


def request(socket_path, payload, timeout=TIMEOUT):
    # IPC reads to EOF before responding. A newline alone would deadlock both peers.
    deadline = time.monotonic() + timeout
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
        stream.settimeout(timeout)
        stream.connect(str(socket_path))
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Agent IPC connection exceeded its deadline")
        stream.settimeout(remaining)
        stream.sendall(json.dumps(payload).encode("utf-8"))
        stream.shutdown(socket.SHUT_WR)
        chunks = []
        size = 0
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Agent IPC response exceeded its deadline")
            stream.settimeout(remaining)
            chunk = stream.recv(65536)
            if not chunk:
                break
            size += len(chunk)
            if size > 4 * 1024 * 1024:
                raise ValueError("Agent IPC response exceeded 4 MiB")
            chunks.append(chunk)
    response = json.loads(b"".join(chunks))
    if response.get("status") != "success" or not isinstance(response.get("data"), dict):
        raise ValueError(f"Agent IPC request failed: {response}")
    return response["data"]


def lifecycle(state):
    repo = state / "workspace" / "main"
    socket_path = state / "agent.sock"
    expected_path = repo.parent / FEATURE
    start = request(socket_path, {
        "action": "start_feature", "repo_path": str(repo), "name": FEATURE,
        "base_branch": "main", "branch_prefix": "feature", "minimal": True,
        "skip_modules": ["devcontainer", "compose", "database", "tunnel", "specs"],
        "prompt": PROMPT,
    })["start"]
    assert start["work_feature"] == FEATURE and start["branch_name"] == BRANCH, start
    assert Path(start["worktree_path"]).resolve() == expected_path and expected_path.is_dir(), start
    assert start["mode"] == "minimal" and start["prompt_seed"] == PROMPT, start
    write_json(state / "ipc-start.json", start)
    active = request(socket_path, {"action": "list_features", "repo_path": str(repo)})["features"]
    assert [entry["work_feature"] for entry in active] == [FEATURE], active

    teardown = request(socket_path, {
        "action": "teardown_feature", "repo_path": str(repo), "name": FEATURE,
        "branch_prefix": "feature", "delete_branch": True, "force": False,
    })["teardown"]
    assert teardown["work_feature"] == FEATURE and teardown["branch_name"] == BRANCH, teardown
    assert teardown["worktree_removed"] and teardown["branch_deleted"], teardown
    assert not expected_path.exists() and not git(repo, "branch", "--list", BRANCH)
    active = request(socket_path, {"action": "list_features", "repo_path": str(repo)})["features"]
    assert active == [], active
    write_json(state / "ipc-teardown.json", teardown)


def serve_stub(state, port):
    log_path = state / "cp-stub.jsonl"

    class Handler(http.server.BaseHTTPRequestHandler):
        failed_workflow = False

        def do_POST(self):
            if self.path != "/events" or self.headers.get("Authorization") != "Bearer stub-token":
                self.send_error(403)
                return
            payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            workflow_batch = any(event["kind"] == "feature_start" for event in payload["events"])
            status = 200
            if workflow_batch and not Handler.failed_workflow:
                Handler.failed_workflow = True
                status = 503  # Exercise a real retry without acknowledging the failed batch.
            with log_path.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps({"http_status": status, "body": payload}) + "\n")
            body = json.dumps({"acked_through": payload["cursor"]["last_event_id"]}).encode()
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    def terminate(_signal, _frame):
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, terminate)
    with http.server.HTTPServer(("127.0.0.1", port), Handler) as server:
        (state / "cp-stub.port").write_text(str(server.server_port), encoding="ascii")
        server.serve_forever()


def delivery_snapshot(state):
    start = json.loads((state / "ipc-start.json").read_text())
    teardown = json.loads((state / "ipc-teardown.json").read_text())
    repo = str(state / "workspace" / "main")
    records = []
    for line in (state / "cp-stub.jsonl").read_text().splitlines():
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            # The writer may be appending the final line; retry in the bounded poll.
            raise AssertionError("Control-plane stub log has an incomplete record")
    delivered = {}
    failed = []
    batch_ids = []
    for record in records:
        body = record["body"]
        assert body["workspace_root"] == repo, body
        assert all(isinstance(body["agent"].get(key), str) and body["agent"][key]
                   for key in ("version", "hostname", "os", "arch")), body
        ids = [event["id"] for event in body["events"]]
        assert ids and all(type(event_id) is int and event_id > 0 for event_id in ids), body
        assert ids == sorted(set(ids)) and body["cursor"]["last_event_id"] == ids[-1], body
        assert type(body["cursor"]["batch_id"]) is int and body["cursor"]["batch_id"] > 0, body
        batch_ids.append(body["cursor"]["batch_id"])
        if record["http_status"] == 503:
            failed.extend(body["events"])
        elif record["http_status"] == 200:
            for event in body["events"]:
                if event["id"] in delivered:
                    assert delivered[event["id"]] == event, "Retried event changed"
                delivered[event["id"]] = event

    assert batch_ids == sorted(set(batch_ids)), "Delivery batch cursor did not advance"

    expected = {}
    for kind in ("feature_start", "feature_teardown"):
        matches = [event for event in delivered.values() if event["kind"] == kind
                   and event["payload"].get("work_feature") == FEATURE]
        assert len(matches) == 1, f"Expected one delivered {kind}; got {len(matches)}"
        event = matches[0]
        payload = event["payload"]
        assert payload["event"] == kind and payload["branch_name"] == BRANCH, event
        assert isinstance(event["queued_at"], str) and event["queued_at"], event
        if kind == "feature_start":
            assert payload["worktree_path"] == start["worktree_path"], event
            for key in ("mode", "prompt_seed", "color", "compose_project_name", "env_path", "feature_url"):
                assert payload["metadata"].get(key) == start.get(key), (key, event)
        else:
            assert payload["worktree_removed"] == teardown["worktree_removed"] is True, event
            assert payload["branch_deleted"] == teardown["branch_deleted"] is True, event
        expected[kind] = event["id"]
    assert expected["feature_start"] < expected["feature_teardown"], expected
    assert failed and any(event["id"] == expected["feature_start"] for event in failed), "No workflow retry"
    assert all(delivered.get(event["id"]) == event for event in failed), "Failed events were not retried intact"

    with closing(sqlite3.connect((state / "agent.db").as_uri() + "?mode=ro", uri=True, timeout=1)) as conn:
        ack = conn.execute("SELECT last_ack_event_id FROM control_plane_status WHERE id=1").fetchone()[0]
        rows = conn.execute("SELECT id, event_type, payload, delivered_at FROM events WHERE id IN (?, ?)",
                            tuple(expected.values())).fetchall()
    assert type(ack) is int and ack >= expected["feature_teardown"], f"Final ack is {ack}"
    assert len(rows) == 2 and all(row[3] for row in rows), "Workflow events remain pending in the database"
    for event_id, kind, payload, _delivered_at in rows:
        assert kind == delivered[event_id]["kind"] and json.loads(payload) == delivered[event_id]["payload"]
    return {"workflow_event_ids": expected, "last_ack_event_id": ack,
            "retried_event_ids": [event["id"] for event in failed], "stub_post_count": len(records)}


def verify(state, timeout=TIMEOUT):
    deadline = time.monotonic() + timeout
    last_error = "No control-plane delivery"
    while True:
        try:
            receipt = delivery_snapshot(state)
            write_json(state / "delivery-receipt.json", receipt)
            print(f"==> Workflow events {receipt['workflow_event_ids']}; last acked event id: {receipt['last_ack_event_id']}")
            return receipt
        except (AssertionError, FileNotFoundError, sqlite3.OperationalError) as error:
            last_error = str(error)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError(f"Control-plane workflow delivery timed out: {last_error}")
        time.sleep(min(0.2, remaining))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "stub", "lifecycle", "verify"))
    parser.add_argument("state", type=Path)
    parser.add_argument("port", nargs="?", type=int, default=0)
    args = parser.parse_args()
    state = args.state.resolve(strict=True)
    if args.action == "stub":
        serve_stub(state, args.port)
    else:
        {"prepare": prepare, "lifecycle": lifecycle, "verify": verify}[args.action](state)


if __name__ == "__main__":
    main()
