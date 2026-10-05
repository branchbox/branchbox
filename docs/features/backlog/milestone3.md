---
branch: backlog/milestone3
status: proposed
created: 2025-11-14
---

# Milestone 3 – Control Plane Bridge & macOS Distribution

> **Status update (2026-10).** The macOS half of this milestone was delivered by the [mac-app-revamp](../in-progress/mac-app-revamp.md) in a different shape:
> - `.github/workflows/macos-app.yml` builds with warnings as errors, runs the tests on macOS 14 (Xcode 16.2) and macOS 15, runs integration suites against the same-commit CLI and the 0.13.4 floor, packages a universal ad-hoc-signed app and uploads it as an artifact. Notarization is still deferred.
> - The Swift warnings are gone because the app dropped `swift-protobuf`/`grpc-swift` entirely and has no dependencies.
> - The app has no Agent tab: it drives the CLI's `--json` contract, and an agent-backed transport is deferred.
>
> The control-plane bridge goals (1 and 4) are unchanged and still open.

## Problem
Milestone 2 delivered the local agent, macOS preview app, and tunnel/devcontainer UX, but everything still runs locally:
- The control plane never receives agent telemetry in real time, so remote operators cannot see tunnel/drift status.
- The macOS app can only be built/tested manually on developer machines—there is no CI artifact or notarized bundle to share.
- Swift build/test spam warnings from `swift-protobuf`/`grpc-swift` plugins, masking real regressions.
We need to wire the agent into the control plane, automate macOS builds, and tame the Swift warnings before scaling to more engineers.

## Goals
1. Stream agent events (feature lifecycle, devcontainer sync, tunnels) to the Rails control plane with durable acknowledgements.
2. Add a macOS GitHub Actions workflow that runs `swift build` / `swift test`, packages the app, and publishes artifacts per PR/release.
3. Silence or upgrade away from the deprecated SwiftPM plugin APIs so CI is warning-free.
4. Expand the Agent tab + menu bar diagnostics to show control-plane delivery state and tunnel issues (so the new Home “View log” button has real content).

## Non-goals
- Full Windows support (tracked separately).
- App Store/TestFlight distribution (this milestone only delivers notarized/zipped builds for internal testing).
- Rewriting the Rails control plane UI; we only expose new API endpoints.

## Deliverables
- `agent/src/runtime.rs`: HTTP drain writes batches to `/v1/devices/:id/events` with exponential backoff and stores the last acknowledged offset.
- Rails control-plane stub (or documented contract) describing required endpoints + authentication (bearer token).
- New GitHub Actions workflow (`.github/workflows/macos-app.yml`) that:
  - Runs `swift build` and `swift test`.
  - Invokes `./scripts/package-macos-app.sh`.
  - Uploads the `.app`/zip as build artifacts (later step notarizes).
- `macos/README.md`: section describing the CI artifacts + local notarization instructions.
- Swift dependencies bumped (or vendored patches applied) so `swift build`/`swift test` emit zero warnings.
- Agent tab UI showing last control-plane delivery/failure timestamps, plus a “Retry now” button.
- Menu bar tunnel card includes provider errors + “Copy hostname” and surfaces when no tunnel is detected.

## Milestones & Timeline
1. **Week 1** – Control plane API contract + agent HTTP drain implementation, integration tests that exercise retry/backoff.
2. **Week 2** – macOS CI workflow (build/test/package) + documentation of artifact download + signing steps.
3. **Week 3** – Dependency hygiene (bump `swift-protobuf`/`grpc-swift` or fork to silence warnings), UI updates for Agent tab/menu bar, final QA (CLI harness, macOS tests, CI green).

## Testing
- `cargo test`, `cargo clippy`, `cargo fmt --check`.
- `swift test` (mac host + CI job).
- `./scripts/manual-cli-e2e.sh` in all required stacks/modes.
- New integration test for the agent HTTP drain (hit a mock server, assert ack cursor behavior).
- GitHub Actions macOS workflow must pass on PRs before merging.

## Open Questions
1. How will we authenticate the HTTP drain in staging/prod? (Bearer token env vs. short-lived session.)
2. Do we notarize/sign the packaged `.app` in CI or leave it unsigned for now?
3. Should the control plane immediately emit tunnel/devcontainer health notifications, or do we defer alerting to Milestone 4?

## Risks
- macOS CI minutes are limited; multiple Swift builds per PR could slow the pipeline. Mitigate by caching `.build` and limiting to key branches.
- Upgrading `swift-protobuf`/`grpc-swift` might require regenerating `agent.pb.swift`; ensure scripts/tests cover this.
- Control plane endpoints may not exist yet; document stub behavior and guard the agent behind feature flags until the Rails side is live.
