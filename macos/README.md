# BranchBox for Mac

A native macOS app for BranchBox: see every feature worktree across your projects, start and tear them down, open them in your editor, terminal or coding agent, and fix the ones that need attention.

The app is a front end for the `branchbox` command-line tool you already have installed. Feature operations use its [JSON contract](../docs/docs/reference/json-contract.md), with text parsing and app-side probes for legacy CLI compatibility. The app and Terminal share the same feature registry. File watching, configurable polling, and explicit refreshes pick up changes made outside the app.

The **[Mac app user guide](../docs/docs/guides/mac-app.md)** covers the full workflow, environment controls, command execution, recovery, and runtime limitations.

## Requirements

- macOS 26 (Tahoe) or later.
- The `branchbox` CLI, version 0.13.4 or later (`brew install branchbox/tap/branchbox`).
  - With the released 0.13.4 binary the app runs in **legacy mode**, using compatibility fallbacks and app-side teardown checks. Project settings are read-only, tunnel credential editing is unavailable, and Diagnostics uses host-tool probes. Some newer setup options are hidden. Development builds can report that same version with newer capabilities.
  - A CLI that answers `branchbox version --json` advertises its capabilities. Each capability enables the corresponding app feature; the version response alone does not unlock every action.
- git, plus Docker for container features. Diagnostics tells you what is missing.

## How the app finds the CLI

The app looks for `branchbox` in this order and uses the first executable it finds:

1. the `BRANCHBOX_CLI_PATH` environment variable;
2. the path chosen in **Settings › Tools › Locate…**;
3. your login shell's `PATH`, captured once from `$SHELL -l -i` (so a Finder-launched app sees the same `PATH` as Terminal);
4. `/opt/homebrew/bin`, `/usr/local/bin`, `~/.cargo/bin`, `~/.local/bin`;
5. `Contents/Helpers/branchbox` inside the app, only if the app was packaged with `--embed-cli`.

The path is used as found, without resolving symlinks, so `brew upgrade` is picked up the next time the app becomes active. The app does not embed a CLI by default: one shared CLI keeps the app and Terminal writing the same registry format.

**Diagnostics** (Window › Diagnostics) shows which CLI was chosen and why the others were rejected. It also shows the captured `PATH` and capabilities, and merges host-tool probes with `branchbox doctor` when supported (Git, Docker, the Dev Container CLI, sbx, `op`, `gh`). Checks offer applicable fixes, such as a command to copy or an app to open.

## Using the app

- **Main window.** Projects and their features are in the sidebar; the detail shows a project or a feature, and the inspector (⌥⌘I) shows the running operation and its live log. Features that need attention (degraded, failed setup, missing folder, interrupted start, an unregistered worktree) sort first and offer a fix.
- **Quick Open** (⌘K) jumps to any project or feature.
- **Start** (⌘N), **Tear Down…** (⌘⌫), **Run Command…** (⌥⌘R), **Open in Editor** (⌃⌘E), **Open in Terminal** (⌃⌘T) and **Launch Agent** (⌃⌘A) are in the Feature menu and the toolbar.
- **Teardown review.** The sheet shows reported Git changes, the files BranchBox generated, and whether the branch is merged. If the CLI refuses, the result names the files and offers **Discard N changes and tear down…** behind a confirmation. Unmerged branches default to Keep. Git-ignored files are not listed and are removed with the worktree; Keep preserves commits, not ignored files or Compose volumes.
- **Prune** removes several features one teardown at a time. Select Safe excludes reported user changes, invalid worktrees, and unsafe branch deletion under the chosen policy; an unmerged branch can remain selected with Keep. A refusal skips that feature and moves on. The app never runs `branchbox prune`.
- **Activity** (⌥⌘L) lists running and past operations with their full logs. Logs are kept in `~/Library/Logs/BranchBox/operations/`, with tokens and extra-environment values redacted.
- **Menu bar.** The icon shows one of four states: idle, working, attention (with a count) or blocked (the CLI is unavailable). Its menu lists recent activity and each project's features, with Start Feature…, Open BranchBox and Refresh. Closing the main window keeps the app running in the menu bar.
- **Quitting** while operations run asks first. **Keep Running** is the default; **Cancel and Quit** stops every running `branchbox` process and its children before the app exits.
- **Notifications** report operations that took more than 10 seconds or failed, when the main window is not in front. Click one to jump to the result. Notifications need the packaged app, so they are off under `swift run`.
- **Settings** has General, Tools (CLI location, login-shell `PATH`, extra environment), Editors & Terminal, Coding Agent, Notifications, Refresh and Advanced tabs. Each project also has its own settings (runtime, branch prefix, teardown defaults, tunnels), written through `branchbox config apply` and `branchbox tunnel credentials set`. The app never edits files in your repositories itself.
- **Environment controls** for the container runtime include start, stop, rebuild, and open a shell. Creating a Full feature prepares its workspace and configuration; start the devcontainer separately. Quick mode skips devcontainer sync by default. In current CLI source builds, Stop keeps volumes; Stop and Delete Volumes removes attached anonymous standalone volumes or owned Compose volumes. Named/shared standalone volumes remain. Open Shell uses configured user/workspace facts when the CLI supplies them, with Bash when available and `sh` otherwise. SBX state comes from the registry and Stop/Rebuild are not available yet; Local VM and in-guest environment cards are read-only.
- **Run Command** shows stdout and stderr after the command finishes, together with the exit code and duration. Use a terminal for interactive commands or live output.

The app stores its project list in `~/Library/Application Support/BranchBox/projects.json` and its preferences in the `dev.branchbox.app` defaults domain. Development builds use "BranchBox Dev" folders and the `dev.branchbox.app.dev` domain instead, so they never touch an installed app's state.

## Development

The app is a Swift package with no third-party dependencies (Swift 6 language mode, strict concurrency). Open it in Xcode with `open macos/Package.swift`, or work from Terminal:

```bash
cd macos
swift build --build-tests -Xswiftc -warnings-as-errors
swift test --parallel
swift run BranchBox
```

| Target | What it holds |
|---|---|
| `BranchBoxKit` | Foundation only: the backend protocol, models decoded from the CLI's JSON, pure planning (teardown drafts, recoveries, prune selection, remediation). |
| `BranchBoxCLI` | The process runner, login-shell environment, CLI locator and `CLIBackend`, the production backend. |
| `BranchBoxStores` | `@MainActor @Observable` state: projects, refresh, operations and their queueing. Depends on Kit only. |
| `BranchBoxPreview` | `PreviewBackend`, a scriptable fake backend for previews, tests and screenshots. |
| `BranchBoxApp` | The SwiftUI/AppKit app (product `BranchBox`), and the only place a `CLIBackend` is created. |

Tests use Swift Testing, in five test targets plus `BranchBoxTestSupport`, which holds the fixtures: real, path-scrubbed CLI 0.13.4 captures in `Tests/BranchBoxTestSupport/Fixtures/cli-0.13.4/`. The Kit tests also decode the Rust golden fixtures in `cli/tests/fixtures/contract/`.

### Dev loops

- **`swift run BranchBox`** runs the app unbundled. It shows a "DEV" badge, uses the dev defaults suite, and has notifications switched off.
  - Test a branch CLI: `BRANCHBOX_CLI_PATH=$PWD/../target/debug/branchbox swift run BranchBox`.
  - Run without any CLI on the preview backend (debug builds only): `BRANCHBOX_BACKEND=preview BRANCHBOX_PREVIEW_SCENARIO=showcase swift run BranchBox`. Other scenarios include `contract`, `legacy0134`, `emptyProject`, `cliMissing`, `interruptedSetup`, `strays` and `dirtyWorktree`.
- **`scripts/macos-dev.sh`** builds a debug `macos/build/dev/BranchBox Dev.app` (`dev.branchbox.app.dev`, ad-hoc signed) and runs it in the foreground with logs in your terminal. Notifications work in it.
  - `--open` launches it through LaunchServices, like Finder does. Use it to check what the app sees without your terminal's `PATH`.
  - `--preview NAME` runs it on the preview backend, and `--env K=V` passes environment variables.
  - `--build-only` builds the bundle and prints its path.

### Tests against the real CLI

The integration suites skip themselves unless `BRANCHBOX_IT=1` is set. They run in disposable temporary git repositories and need no Docker:

```bash
cargo build -p branchbox-cli
BRANCHBOX_IT=1 BRANCHBOX_IT_CLI="$PWD/target/debug/branchbox" \
  swift test --package-path macos --filter BranchBoxIntegrationTests
```

Point `BRANCHBOX_IT_CLI` at a 0.13.4 binary to exercise legacy mode. Set `BRANCHBOX_IT_TMP` to choose where the throwaway repositories go (SwiftPM replaces `TMPDIR` for the test process). Render tests write offscreen screenshots only when `BRANCHBOX_RENDER_DIR` is set. [`TESTING.md`](TESTING.md) lists every test layer, what the integration suites cover, and the latest verification run, including the manual Mac App ↔ CLI loop.

### CI

`.github/workflows/macos-app.yml` runs on changes to `macos/`, `cli/`, `core/` and the mac scripts:

- `test`: builds with warnings as errors and runs the tests on macOS 26 with the latest stable Xcode. It also fails if gRPC, SwiftProtobuf or NIO imports come back, or if the menu bar presents a sheet or an alert.
- `integration`: builds the CLI from the same commit and runs the integration suites against it, then captures live fixtures and decodes them.
- `integration-floor`: runs the integration suites against the released 0.13.4 CLI (legacy mode).
- `package`: builds the universal app and uploads it as an artifact.

## Packaging

```bash
scripts/package-macos-app.sh --universal --zip
```

The script builds `BranchBox.app` (universal by default; `--native` builds for this Mac only), ad-hoc signs it with the hardened runtime, and verifies the signature, architectures and version. `--zip` writes `macos/build/BranchBox-<version>-<build>-<sha>.zip` plus a `.sha256` file. `--embed-cli PATH` puts a CLI in `Contents/Helpers/` (signed first); without it the app uses your installed CLI. `--sign IDENTITY` and `--notarize` are reserved for Developer ID distribution, which does not exist yet.

### Installing a CI build

Every CI run on `main` and on pull requests uploads `BranchBox-macOS-<sha>` (kept for 14 days). Unzip it and move `BranchBox.app` to `/Applications`.

The build is ad-hoc signed, not notarized, so Gatekeeper may block a downloaded build. Try to open it once, then choose **System Settings › Privacy & Security › Open Anyway**. For a build you trust, you can alternatively remove the quarantine flag:

```bash
xattr -dr com.apple.quarantine /Applications/BranchBox.app
```

On macOS 15 and later, Control-clicking Open in Finder no longer bypasses the check.

### Why the app is not sandboxed

BranchBox has to run `branchbox`, `git`, `docker` and your editor, and read and write repositories anywhere on disk. The App Sandbox allows none of that, so the app runs unsandboxed with the hardened runtime and no entitlements. It is distributed outside the Mac App Store.

## Troubleshooting

- **"BranchBox CLI not found".** Install it (`brew install branchbox/tap/branchbox`) or choose it in Settings › Tools › Locate…. Diagnostics lists every path the app tried.
- **The app finds a different CLI than Terminal.** The app captures your login shell's `PATH` at launch. Click **Re-capture** in Settings › Tools after changing your shell profile, or choose the CLI there with Locate….
- **"Some features need a newer CLI".** The selected CLI lacks the required capabilities. Check Diagnostics, then choose a compatible CLI in Settings › Tools › Locate…. Development builds can report 0.13.4 while exposing newer capabilities.
- **Docker checks fail.** Start Docker Desktop (or your Docker engine), then click **Run Checks Again** in Diagnostics.
- **A teardown was refused.** Read the result card: a safety refusal names the files or branch that blocked it and offers the matching recovery. Other failures may have partial results; inspect Activity and the refreshed feature before retrying.
- **A start was interrupted** (cancelled or the app quit). The feature shows as Interrupted with **Resume Setup** and **Tear Down…**; an unregistered worktree shows **Review Worktree…**.
- **Logs.** Activity shows every operation's log, and **Copy Report** in Diagnostics collects versions, paths and recent failures (secrets redacted). The full logs are in `~/Library/Logs/BranchBox/`.
