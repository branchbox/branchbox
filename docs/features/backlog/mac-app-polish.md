---
branch: backlog/mac-app-polish
status: backlog
created: 2025-11-12
---

# macOS app polish + agent controls

## Problem
The SwiftUI preview in `macos/` proves that the gRPC surface works end-to-end, but it is intentionally bare-bones. We still need workflow affordances (workspace picker, launch status, tunnel health) before it can ship to internal users.

## Proposed Enhancements
- Workspace selector + persistence so the UI is not tied to the defaults entry.
- Transport status indicator that shows whether we are talking to gRPC, the Unix socket, or the CLI fallback.
- Inline telemetry (module outcomes, adapter name, tunnel provider) pulled from `Feature` metadata.
- Start form improvements: branch prefix selector, module skip list, prompt seed history.
- Teardown confirmation flow with `--force` + `--complete-spec` toggles.
- Control-plane indicator that surfaces whether the HTTP drain is healthy (read from the registry once the ack backlog project lands).

## Testing
- Snapshot tests for the SwiftUI views (macOS only) using sample feature data.
- UIAutomation shim that runs against the agent smoke harness to verify the start/teardown buttons work repeatedly.

## Open Questions
- Should we embed a lightweight agent launcher for mac-only installs (launchd plist)?
- How do we distribute the app (TestFlight vs. notarized DMG) while it still depends on the CLI binaries sitting in the repo?
