---
branch: feature/mac-app-revamp
created: 2026-10-02
status: in-progress
work_feature: mac-app-revamp
worktree: ~/projects/branchbox-suite/branchbox/mac-app-revamp
---
# Mac App Revamp

## Overview

BranchBox for Mac was a Milestone 2 preview that talked to the agent over gRPC and fell back to the CLI. It had stopped working: with no agent running (the agent is not distributed), every call hung; `swift run` crashed on the first notification; large registries deadlocked a pipe; its models had drifted from the CLI's JSON; and teardown could delete uncommitted work.

This feature rebuilds the app on a new foundation and fixes the CLI and core issues the audit found:

1. **Stabilize.** The app drives the user's installed `branchbox` through its `--json` contract. gRPC, SwiftProtobuf and NIO are gone, and the package has zero dependencies. A tested process layer handles environment capture, large output, cancellation and process groups.
2. **Feature parity.** Multi-project sidebar, feature detail with health remediation, runtime and ports, open in editor or terminal, launch a coding agent, Run Command, devcontainer up/down, tunnels, prune with preview, base-branch picker, Activity window with full logs, menu bar extra, Quick Open, notifications.
3. **Core safety.** Teardown never deletes uncommitted user work or unmerged commits without an explicit flag (`--discard-changes`, `--force-delete-branch`, `--force`), and it refuses before removing anything. BranchBox's own generated files are not counted as user changes. Registry writes are locked and atomic. Teardown uses the recorded branch name. A write-ahead start record makes interrupted starts visible. `--json` stdout is always one document, with an error envelope.
4. **Onboarding and settings.** Add project, set up BranchBox (`init`), project settings through `config get/apply`, tunnel credentials, Diagnostics with `doctor`, and app Settings.
5. **Distribution.** One script builds an ad-hoc-signed universal `BranchBox.app`; CI builds, tests and uploads it.

Out of scope for this iteration: an agent-daemon backend, Developer ID signing, notarization and a Homebrew cask, sandboxing, clone-from-URL onboarding, PR integration, JSONL progress, and decoupling `--force` from `git branch -D`.

## Design

The authoritative design is [`mac-app-revamp/DESIGN.md`](mac-app-revamp/DESIGN.md). Its "Implementation deviations" sections (waves 1-3) record where the shipped code differs from the original text, and they win where the two disagree. The audits that motivated the work are in [`mac-app-revamp/audit/`](mac-app-revamp/audit/), and the work-package briefs are in [`mac-app-revamp/work-packages.json`](mac-app-revamp/work-packages.json).

Key decisions (DESIGN §2):

- **Transport (D-1).** A `BranchBoxBackend` protocol with one conformer, `CLIBackend`, which spawns the installed CLI. The composition root is the only place it is constructed, so an agent backend can be added later.
- **Platform (D-2, D-3, D-4).** macOS 14, Swift 6 language mode, `swift-tools-version: 6.0`, no Swift 6.1+ features, untyped `throws` (every error is a `BackendError`), zero SwiftPM dependencies, warnings as errors.
- **Locating the CLI (D-8, D-9).** `BRANCHBOX_CLI_PATH`, then the Settings override, then the login-shell `PATH`, then well-known directories, then an embedded helper if one was packaged. The floor is 0.13.4. Features are gated on `branchbox version --json` capabilities, and 0.13.x runs in legacy mode.
- **Error contract (D-10).** In `--json` mode, failures print one error envelope on stdout with a stable code; exit codes are unchanged; in-band failures keep their payloads.
- **Teardown (D-11, D-13, D-27).** The first attempt never discards anything. Discard happens only as a recovery after a refusal, with consent for exactly the listed files. A content-based classifier (CLI-side on 0.14+, a Swift port on legacy CLIs) separates user changes from BranchBox-generated files. `--force` keeps its coupling to `-D` for compatibility, and the app never relies on it.
- **Prune (D-12).** The app never runs `branchbox prune`; it runs safe teardowns one at a time and skips refusals.
- **Registry (D-14, D-15, D-16).** A write-ahead `setup` record, no new `FeatureStatus` values, and per-project FIFO queueing of registry writers when the CLI lacks `registry-lock`.
- **Repository files (D-19, D-20).** The app never writes repository files itself. The Cloudflare token goes from a SecureField to the CLI's stdin.
- **Distribution (D-6).** Not sandboxed, hardened runtime, ad-hoc signed, no entitlements.

The JSON contract is documented for users in `docs/docs/reference/json-contract.md`, and the app in `macos/README.md`.

## Status

| Wave | Packages | State |
|---|---|---|
| 1 | SW-0 Swift contracts and package reset; RS-1 Rust foundations (envelope, registry lock, atomic writes, write-ahead start, `version --json`) | Landed |
| 2 | SW-1 BranchBoxCLI; SW-2 Stores; SW-3 planning and UI kit; RS-2 teardown safety; RS-3 CLI JSON surface (`detect`, `devcontainer sync`, `config`, `tunnel credentials`, `doctor`, `init --json`) | Landed and integrated |
| 3 | SW-4 app shell and menu bar; SW-5 feature surfaces; SW-6 flows and Activity; SW-7 projects, onboarding, settings, diagnostics | Landed and integrated (660 tests in 77 suites on Swift 6.2.4) |
| 4 | PK-1 packaging, dev loop, CI completion; DOC-1 docs and changelog; VER-1 integration tests and end-to-end verification | In progress |

Open before this moves to `completed/`:

- The Swift 6.0.3 / Xcode 16.2 compile (the macos-14 CI leg) has not run yet; only Swift 6.2.4 was available locally.
- The manual "Mac App ↔ CLI Loop" (`docs/docs/getting-started/manual-cli-e2e.md`) must be run on the packaged app with both 0.13.4 and a contract CLI, with results in `macos/TESTING.md` (VER-1).
- Known follow-ups are listed under "Open items" at the end of each deviations section in DESIGN.md, for example ignored files being removed with a worktree without a mention, and the missing Stores APIs for "stop after current" and "Add Anyway".
