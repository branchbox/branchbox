---
branch: backlog/control-plane-durable-acks
status: completed
created: 2025-11-12
---

# Control-plane durable acknowledgements

## Problem
The Milestone 2 HTTP drain posts batches of queued events to the control plane, but delivery tracking ends once the HTTP request succeeds. If the Rails endpoint crashes between persistence and acking (or we need to replay a subset of events), the agent has no notion of what the server actually processed. We also want better observability around dropped batches.

## Proposal
- Include a monotonically increasing batch identifier + queue cursor (`last_event_id`) with every POST.
- Persist the remote ack (or rejection) in the SQLite store so we can resume from the last known good cursor after an agent restart.
- Surface delivery metrics (`sync_success`, `sync_failed`) via the tracing layer so the control plane dashboard can flag unhealthy devices.
- Add exponential backoff + jitter when the endpoint keeps failing instead of retrying every `event_flush_interval`.

## Implementation Notes
- `control_plane_status` now tracks `next_batch_id` and `last_ack_event_id` so the agent resumes from the last acked cursor after restarts.
- `ControlPlaneClient::send_events` includes `cursor.batch_id` + `cursor.last_event_id` in every POST and persists the ack returned by the server (or the batch cursor for legacy stubs).
- The event loop backs off with jittered exponential delays whenever delivery fails, and `scripts/manual-agent-e2e.sh --cp-stub` spins up a local HTTP stub, captures batches, and prints the persisted cursor on exit.

## Testing
1. `scripts/manual-agent-e2e.sh --cp-stub` (default stack) – confirms queued events reach the stub and `last_ack_event_id` is updated.
2. Manual python stub with intermittent failures (see README) – logs show retries honoring the backoff window.
3. `cargo test --package branchbox-agent` – covers the new sqlite helpers (`next_batch_id`, `update_control_plane_ack`).

## Notes
This work blocks full control-plane orchestration. Until durable acks land, keep the drain in “best effort” mode for staging environments only.
