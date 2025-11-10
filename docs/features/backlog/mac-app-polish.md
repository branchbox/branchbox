---
branch: backlog/mac-app-polish
status: in-progress
created: 2025-11-12
---

# macOS app polish + agent controls

## Problem
The SwiftUI preview in `macos/` proves that the gRPC surface works end-to-end, but it is intentionally bare-bones. We still need workflow affordances (workspace picker, launch status, tunnel health) before it can ship to internal users.

## Proposed Enhancements
- ✅ Workspace selector + persistence so the UI is not tied to the defaults entry.
- ✅ Transport status indicator that shows whether we are talking to gRPC or the CLI fallback.
- ✅ Inline telemetry (module outcomes, tunnel provider) pulled from `Feature` metadata.
- ✅ Start form improvements: branch prefix selector, module skip list, prompt seed history, reuse toggle.
- ✅ Teardown confirmation flow with `--force` + `--complete-spec` toggles.
- ✅ Surface adapter metadata (name/service URL/warnings) via gRPC + CLI fallback; control-plane health still pending until the service exposes an API.

## Testing
- Manual testing via `swift run BranchBoxApp` using the new workspace picker + CLI fallback path.
- QA to add snapshot + UI automation coverage once the UI stabilises.

## Open Questions
- Should we embed a lightweight agent launcher for mac-only installs (launchd plist)?
- How do we distribute the app (TestFlight vs. notarized DMG) while it still depends on the CLI binaries sitting in the repo?
