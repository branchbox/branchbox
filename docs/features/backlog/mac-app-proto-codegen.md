---
branch: backlog/mac-app-proto-codegen
status: backlog
created: 2025-11-10
---

# macOS Swift proto codegen plan

## Problem
The macOS preview app currently relies on handwritten SwiftProtobuf stubs. This was acceptable for the initial Milestone 2 bring-up, but it will become unmaintainable as soon as the proto surface grows (control-plane status, tunnel health, future agent RPCs). We also need parity across CI/devcontainers so new contributors aren’t hand-editing generated files.

## Plan
1. Add protoc + `protoc-gen-swift`/`protoc-gen-grpc-swift` to the devcontainer/toolchain.
2. Check the generated Swift sources into `macos/Sources/BranchBoxApp/Generated` and hook them up to the SwiftPM target.
3. Replace the handwritten `ProtoMessages.swift` + `FeatureServiceClient.swift` scaffolding with generated types, keeping only thin helpers for bridging to SwiftUI models.
4. Add a `swift generate-protos` script (invoked by CI + developers) that regenerates the bindings whenever `agent/proto/agent.proto` changes.
5. Document the workflow in README + AGENTS so future contributors know to rerun the generator rather than editing Swift files by hand.

## Timeline
Target early Milestone 3 (after the control-plane Rails work lands) so the necessary tooling can be added to the devcontainer/CI images in the same change that introduces the Rails proto surface.

## Open Questions
- Should we ship the generated files in the repo or fetch them via SwiftPM? (Initial plan: check them in to avoid requiring protoc on mac testers.)
- Do we want a shared Rust→Swift descriptor step (e.g., `tonic-build` emitting JSON) to keep CLI + mac app in sync?
