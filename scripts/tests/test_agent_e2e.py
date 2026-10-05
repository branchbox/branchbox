#!/usr/bin/env python3
"""Regression checks for IPC framing and workflow delivery, without Rust or Docker."""

import copy
from contextlib import closing
import importlib.util
import json
import os
from pathlib import Path
import socket
import sqlite3
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("agent_e2e", Path(__file__).parents[1] / "lib" / "agent-e2e.py")
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class AgentE2ETests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="bb-agent-test-", dir="/tmp")
        self.addCleanup(self.temp.cleanup)
        self.state = Path(self.temp.name).resolve()

    def unix_server(self, handler):
        path = self.state / "agent.sock"
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(path))
        server.listen(1)
        server.settimeout(1)
        errors = []

        def serve():
            try:
                with server:
                    stream, _ = server.accept()
                    with stream:
                        stream.settimeout(1)
                        handler(stream)
            except Exception as error:
                errors.append(error)

        thread = threading.Thread(target=serve)
        thread.start()

        def finish():
            thread.join(2)
            self.assertFalse(thread.is_alive(), "IPC regression server did not stop")
            self.assertEqual(errors, [])
        self.addCleanup(finish)
        return path

    def test_ipc_half_closes_write_and_decodes_split_response(self):
        seen = []

        def handler(stream):
            chunks = []
            while data := stream.recv(4096):
                chunks.append(data)
            seen.append(json.loads(b"".join(chunks)))
            stream.sendall(b'{"status":"success","data":')
            stream.sendall(b'{"features":[]}}')

        payload = {"action": "list_features", "repo_path": "/tmp/fixture with spaces"}
        data = helper.request(self.unix_server(handler), payload, timeout=0.5)
        self.assertEqual(data, {"features": []})
        self.assertEqual(seen, [payload])

    def test_fixture_git_overrides_do_not_redirect_to_an_existing_repo(self):
        external = self.state / "external"
        external.mkdir()
        helper.prepare(external)
        external_repo = external / "workspace" / "main"
        original_head = helper.git(external_repo, "rev-parse", "HEAD")
        original_status = helper.git(external_repo, "status", "--porcelain")
        with patch.dict(os.environ, {"GIT_DIR": str(external_repo / ".git"),
                                     "GIT_WORK_TREE": str(external_repo),
                                     "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.bare",
                                     "GIT_CONFIG_VALUE_0": "true"}):
            helper.prepare(self.state)
        repo = self.state / "workspace" / "main"
        self.assertEqual(Path(helper.git(repo, "rev-parse", "--show-toplevel")), repo)
        self.assertEqual(helper.git(external_repo, "rev-parse", "HEAD"), original_head)
        self.assertEqual(helper.git(external_repo, "status", "--porcelain"), original_status)

    def test_ipc_error_cannot_count_as_success(self):
        def handler(stream):
            while stream.recv(4096):
                pass
            stream.sendall(b'{"status":"error","error":"teardown refused"}')
        with self.assertRaisesRegex(ValueError, "teardown refused"):
            helper.request(self.unix_server(handler), {"action": "teardown_feature"}, timeout=0.5)

    def test_ipc_nonresponsive_peer_has_bounded_timeout(self):
        release = threading.Event()

        def handler(stream):
            while stream.recv(4096):
                pass
            release.wait(0.3)

        path = self.unix_server(handler)
        began = time.monotonic()
        try:
            with self.assertRaises(TimeoutError):
                helper.request(path, {"action": "agent_status"}, timeout=0.05)
            self.assertLess(time.monotonic() - began, 0.3)
        finally:
            release.set()

    def delivery_fixture(self, ack=3):
        start = {"work_feature": helper.FEATURE, "branch_name": helper.BRANCH,
                 "worktree_path": str(self.state / "workspace" / helper.FEATURE),
                 "mode": "minimal", "prompt_seed": helper.PROMPT}
        teardown = {"worktree_removed": True, "branch_deleted": True}
        helper.write_json(self.state / "ipc-start.json", start)
        helper.write_json(self.state / "ipc-teardown.json", teardown)
        events = [
            {"id": 2, "kind": "feature_start", "queued_at": "2026-10-05T12:00:00Z",
             "payload": {"event": "feature_start", "work_feature": helper.FEATURE,
                         "branch_name": helper.BRANCH, "worktree_path": start["worktree_path"],
                         "metadata": {"mode": "minimal", "prompt_seed": helper.PROMPT}}},
            {"id": 3, "kind": "feature_teardown", "queued_at": "2026-10-05T12:00:01Z",
             "payload": {"event": "feature_teardown", "work_feature": helper.FEATURE,
                         "branch_name": helper.BRANCH, **teardown}},
        ]
        body = {"workspace_root": str(self.state / "workspace" / "main"),
                "agent": {"version": "test", "hostname": "fixture", "os": "unix", "arch": "test"},
                "cursor": {"batch_id": 1, "last_event_id": 3}, "events": events}
        records = [{"http_status": 503, "body": body},
                   {"http_status": 200, "body": {**copy.deepcopy(body), "cursor": {"batch_id": 2, "last_event_id": 3}}}]
        self.log(records)
        with closing(sqlite3.connect(self.state / "agent.db")) as conn, conn:
            conn.executescript("CREATE TABLE control_plane_status(id INTEGER, last_ack_event_id INTEGER);"
                               "CREATE TABLE events(id INTEGER,event_type TEXT,payload TEXT,delivered_at TEXT);")
            conn.execute("INSERT INTO control_plane_status VALUES(1,?)", (ack,))
            conn.executemany("INSERT INTO events VALUES(?,?,?,?)",
                             [(e["id"], e["kind"], json.dumps(e["payload"]), "delivered") for e in events])
        return records

    def log(self, records):
        (self.state / "cp-stub.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))

    def test_delivered_workflows_require_durable_final_ack(self):
        self.delivery_fixture(ack=2)
        with self.assertRaisesRegex(AssertionError, "Final ack"):
            helper.delivery_snapshot(self.state)
        with closing(sqlite3.connect(self.state / "agent.db")) as conn, conn:
            conn.execute("UPDATE control_plane_status SET last_ack_event_id=3")
        self.assertEqual(helper.delivery_snapshot(self.state)["last_ack_event_id"], 3)

    def test_heartbeat_high_watermark_cannot_pass_workflow_gate(self):
        records = self.delivery_fixture(ack=999)
        heartbeat = {"id": 999, "kind": "heartbeat", "queued_at": "now", "payload": {"event": "heartbeat"}}
        records[1]["body"]["events"] = [heartbeat]
        records[1]["body"]["cursor"]["last_event_id"] = 999
        self.log(records)
        with self.assertRaisesRegex(AssertionError, "delivered feature_start"):
            helper.delivery_snapshot(self.state)

    def test_changed_workflow_metadata_and_retry_are_rejected(self):
        records = self.delivery_fixture()
        records[1]["body"]["events"][0]["payload"]["metadata"]["prompt_seed"] = "wrong feature"
        self.log(records)
        with self.assertRaises(AssertionError):
            helper.delivery_snapshot(self.state)
        records[1]["body"]["events"][0]["payload"]["metadata"]["prompt_seed"] = helper.PROMPT
        records[0]["body"]["events"][0]["payload"]["metadata"]["prompt_seed"] = "changed on retry"
        self.log(records)
        with self.assertRaisesRegex(AssertionError, "not retried intact"):
            helper.delivery_snapshot(self.state)

    def test_posted_but_database_pending_events_are_rejected(self):
        self.delivery_fixture()
        with closing(sqlite3.connect(self.state / "agent.db")) as conn, conn:
            conn.execute("UPDATE events SET delivered_at=NULL WHERE id=3")
        with self.assertRaisesRegex(AssertionError, "remain pending"):
            helper.delivery_snapshot(self.state)

    def test_missing_delivery_times_out_without_leaving_receipt(self):
        began = time.monotonic()
        with self.assertRaisesRegex(TimeoutError, "workflow delivery timed out"):
            helper.verify(self.state, timeout=0.05)
        self.assertLess(time.monotonic() - began, 0.3)
        self.assertFalse((self.state / "delivery-receipt.json").exists())


if __name__ == "__main__":
    unittest.main()
