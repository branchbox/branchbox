---
branch: backlog/control-plane-durable-acks
status: backlog
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

## Testing
1. Point the agent at a local stub that randomly fails; confirm batches retry with backoff and the cursor never advances.
2. Kill the agent mid-delivery, restart it, and ensure it resumes from the same `last_event_id`.
3. Extend `scripts/manual-agent-e2e.sh` to optionally run the stub and verify the persisted cursor after the run.

## Notes
This work blocks full control-plane orchestration. Until durable acks land, keep the drain in “best effort” mode for staging environments only.
