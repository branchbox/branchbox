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

## 2025-01 UX Critique
- The Home tab still mirrors internal telemetry (control-plane drain, devcontainer syncs) that most app users neither understand nor need to touch. It pushes the primary job—start or teardown a feature—below status badges.
- Advanced knobs (branch prefixes, module skip list, prompt history) are always visible in the Features tab, which overloads first-time users who just want to launch worktrees.
- Terminology such as “control plane” and “devcontainer strategy” leaks agent internals into the UI. Users expect “Connect to BranchBox” and “Update workspace” wording instead.
- Workspace switching hides in the toolbar even though it is the first action a new install must take.
- Menu bar quick actions exist but only expose devcontainer sync choices. They should also let users start/stop features and view tunnel health without opening the main window.

## Updated UX Plan

### Experience pillars
1. **Do the obvious thing quickly**: the Home tab is a dashboard with two tasks—start a feature or resume one—and links to tunnels if configured.
2. **Keep advanced workflows discoverable but out of the way**: advanced options live behind “More options” drawers or the Settings tab.
3. **Plain language**: surface “Workspace”, “Agent connection”, “Tunnels” instead of control-plane jargon.
4. **Menu bar parity**: the status item mirrors the Home quick actions (start, reveal, teardown) and eventually exposes tunnel toggles.

### Surface breakdown
- **Toolbar**: Workspace picker, transport selector (Automatic/gRPC/CLI), “Refresh” button, and status lights. Advanced toggles move to Settings.
- **Home tab**: Two-column grid with (a) Quick Start + Recent Activity; (b) Active Feature card + simple “Workspace status” card that includes a “Fix it” button (opens Agent tab or kicks off devcontainer sync) plus a tunnels card that shows provider/status and lets people copy the hostname.
- **Features tab**: List and detail remain, but the inline start form defaults to minimal fields with a “Show advanced options” disclosure for branch prefixes, prompt seeds, module skip list, etc.
- **Agent tab**: Diagnostics, logs, drains, and devcontainer strategies live here, targeting operators.
- **Settings tab**: CLI path overrides, telemetry opt-in, default branch prefixes, tunnel defaults.
- **Menu bar extra**: Quick Start / Teardown actions, “Open BranchBox…”, “Sync workspace”, and a planned tunnel indicator.

### Implementation outline
1. **Simplify Home** (done in this branch) and add clear CTAs for workspace selection + tunnels.
2. **Hide advanced start controls** behind a disclosure in Features tab.
3. **Rename badges** to user-facing language (“Needs attention” vs. “CP pending”).
4. **Extend menu bar quick actions** with Start/Teardown/Workspace selectors + tunnel state (future work).
5. **Documentation**: update README + manual guide once UX stabilises.

## Testing
- Manual testing via `swift run BranchBoxApp` using the new workspace picker + CLI fallback path.
- QA to add snapshot + UI automation coverage once the UI stabilises.

## Open Questions
- Should we embed a lightweight agent launcher for mac-only installs (launchd plist)?
- How do we distribute the app (TestFlight vs. notarized DMG) while it still depends on the CLI binaries sitting in the repo?
