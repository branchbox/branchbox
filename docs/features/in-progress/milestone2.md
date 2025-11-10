---

branch: feature/milestone2
created: 2025-11-10
status: in-progress
work_feature: milestone2
worktree: /Users/rbarazi/projects/branchbox-suite/branchbox/milestone2
---
# Milestone2

## Overview

Milestone 2 connects the new agent daemon to the forthcoming Rails control plane and introduces a SwiftUI macOS preview app that rides on the same gRPC surface area. The goal is to keep the workflow offline-first while giving us enough telemetry, orchestration hooks, and UX coverage to validate remote management before we ship the control plane.

### Completed this iteration

- Added an HTTP drain that pushes queued events + heartbeats to a configurable endpoint (`BRANCHBOX_CP_ENDPOINT` / `BRANCHBOX_CP_TOKEN`). Each batch now carries agent metadata (hostname, OS, arch, version) so the control plane can attribute devices without inspecting sockets.
- Extended the manual agent smoke harness to support the new drain and documented how to point it at a stub endpoint during testing.
- Bootstrapped `macos/BranchBoxApp`, a SwiftUI executable backed by `grpc-swift` + handwritten protobufs. The UI lists features, calls `FeatureService/Start`, `FeatureService/Teardown`, and falls back to `branchbox feature list --json` if the daemon is offline.
- Added CLI fallback hooks + doc updates describing how to configure the workspace path, GRPC address, and CLI binary for the preview app.
- Durable control-plane acknowledgements now live in `control_plane_status` (batch IDs + `last_ack_event_id`) with exponential backoff/jitter on failures, and the manual agent harness grew a `--cp-stub` flag to exercise the path automatically.
- Added `branchbox agent status` + gRPC/Swift bindings so downstream clients (mac app, future control-plane bridge) can see whether the drain is configured/connected, along with last delivery/failure metadata.
- Documented the Swift proto codegen follow-up in `docs/features/backlog/mac-app-proto-codegen.md` so we can swap off handwritten stubs once the toolchain lands.
- The mac app picked up a workspace picker (`NSOpenPanel`), transport badge, module/tunnel telemetry, prompt history, skip-module chips, adapter metadata display (name/service URL/warnings), and a teardown confirmation sheet with `--force/--complete-spec` toggles.

### Still in scope

- Surface adapter metadata/control-plane health in the mac app once the gRPC surface exposes those details.
- Gate experimental Windows support behind an adapter flag once the gRPC transport is proven.
