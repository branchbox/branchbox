BranchBox macOS App

Overview
- SwiftUI app which talks to the BranchBox agent over gRPC.
- Falls back to the BranchBox CLI if gRPC is unavailable.
- Can be packaged as a standalone .app which embeds the CLI.

Prerequisites
- macOS 13+
- Xcode command line tools (`xcode-select --install`)
- Rust toolchain installed for macOS if you plan to embed the CLI.
- Linux devcontainers cannot compile or run the app because the required Apple SDKs (`OSLog`, SwiftUI, etc.) are unavailable there—run builds/tests on macOS hardware or CI runners only.

Run from Terminal (debug)
- Start the agent inside the devcontainer:
  - `.devcontainer`: `./scripts/start-agent-local.sh`
- From macOS host:
  - `cd macos`
  - `BRANCHBOX_AGENT_GRPC_ADDR=127.0.0.1:50515 BRANCHBOX_WORKSPACE=/workspaces/milestone2 swift run BranchBoxApp`

Package a .app and embed the CLI
- `./scripts/package-macos-app.sh`
- Launch: `open macos/build/BranchBoxApp.app`
- The app will use the embedded CLI when gRPC isn’t reachable or when you select a host workspace path.

Workspace selection
- Use the “Choose…” button to select a workspace path.
- The selection persists in defaults under key `branchbox.workspace` (bundle id `dev.branchbox.app`).
- Reset with: `defaults delete dev.branchbox.app branchbox.workspace`.

CLI resolution order
1. `BRANCHBOX_CLI_PATH` environment variable
2. Embedded binary at `Contents/Resources/bin/branchbox` in the app bundle
3. `branchbox` from PATH

Notes
- When using gRPC to a devcontainer agent, the workspace path should be `/workspaces/milestone2`.
- When using the CLI on your Mac host, select the host path to the repo.
