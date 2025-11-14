---
branch: feature/milestone2-control-plane
created: 2025-11-10
status: completed
updated: 2025-11-10
work_feature: milestone2
---
# Milestone 2 — Agent ↔ Control Plane Bridge & macOS Preview

## Overview

Milestone 2 connects the on-device agent daemon to the forthcoming Rails control plane and introduces a SwiftUI macOS preview app that rides on the same gRPC surface area as the CLI. The goal was to keep workflows offline-first while collecting enough telemetry, orchestration hooks, and UX coverage to validate remote management ahead of the control plane ship.

## What Shipped

- Added an authenticated HTTP drain (configured via `BRANCHBOX_CP_ENDPOINT` / `BRANCHBOX_CP_TOKEN`) that batches queued events and heartbeats with host metadata so the control plane can attribute devices without socket introspection.
- Extended the manual agent smoke harness to support the new drain (`--cp-stub`) and documented how to point it at a stub endpoint during testing.
- Bootstrapped `macos/BranchBoxApp`, a SwiftUI executable backed by `grpc-swift` + handwritten protobufs; the UI lists features, calls `FeatureService/Start` + `FeatureService/Teardown`, and falls back to `branchbox feature list --json` if the daemon is offline.
- Added CLI fallback hooks + doc updates describing how to configure the workspace path, GRPC address, and CLI binary for the preview app.
- Persisted durable control-plane acknowledgements inside `control_plane_status` (batch IDs + `last_ack_event_id`) with exponential backoff/jitter on failures, and the manual agent harness grew a `--cp-stub` flag to exercise the path automatically.
- Added `branchbox agent status` plus gRPC/Swift bindings so downstream clients (mac app, future control-plane bridge) can inspect drain configuration, last delivery/failure metadata, and mac app connectivity.
- Documented the Swift proto codegen follow-up in `docs/features/backlog/mac-app-proto-codegen.md` so we can replace handwritten stubs once the toolchain lands.
- The mac app picked up a workspace picker (`NSOpenPanel`), transport badge, module/tunnel telemetry, prompt history, skip-module chips, adapter metadata display (name/service URL/warnings), and a teardown confirmation sheet with `--force` / `--complete-spec` toggles.

## Validation

- `./scripts/manual-agent-e2e.sh --cp-stub` now exercises the drain end-to-end and surfaces telemetry in logs.
- `branchbox agent status` exposes drain connectivity, auth errors, and mac app linkage for manual verification.

## Follow-Ups

- Surface adapter metadata/control-plane health in the mac app once the gRPC surface exposes those details.
- Gate experimental Windows support behind an adapter flag once the TCP transport is proven.

