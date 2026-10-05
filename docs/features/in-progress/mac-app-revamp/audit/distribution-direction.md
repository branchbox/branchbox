# Audit area: distribution-direction

## Summary
HOW A USER GETS THE MAC APP TODAY: they can't, except by building it from source. No release (v0.4.0 through v0.13.4) has ever included a .app, .dmg, .zip or the `branchbox-agent` binary. Homebrew has only `Formula/branchbox.rb`, which installs `branchbox`, `bb` and `branchbox-local-vm`. There is no cask. The website and the install docs advertise only the CLI. The only path is: clone the repo, install Xcode (plus Rust if you want the CLI embedded), and run `./scripts/package-macos-app.sh`. That writes `macos/build/BranchBoxApp.app` inside the repo, with an embedded CLI built from that checkout. The bundle it produces is not shippable:
- `CFBundleShortVersionString` is hard-coded to 0.1.0, while the workspace is at 0.13.4.
- No icon, no LSUIElement, and an arm64-only binary.
- Only the ad-hoc linker signature; the bundle is never sealed. `codesign --verify` fails and `spctl` rejects it.
- No hardened runtime, entitlements or notarization. There are no signing identities on this Mac, and the only GitHub secret is HOMEBREW_TAP_TOKEN.

Once launched, the app is broken for every user who has no agent running, and nobody does, because the agent is never shipped. In Automatic mode the gRPC client uses grpc-swift's default `.waitsForConnectivity` with no call deadline. I replicated AgentBridge's connect-and-list against a closed port: the call was still pending after 47s, so the CLI fallback never runs. Other problems stack on top:
- **Workspace defaults to the process cwd.** Launched from Finder that is `/`. Because `/` exists, the first-run setup prompt never shows, and the CLI then fails with "Not a git repository: /".
- **The CLI isn't found.** The app runs it with `/usr/bin/env branchbox` under launchd's PATH, which doesn't include /opt/homebrew/bin. Result: "env: branchbox: No such file or directory", exit 127.
- **Version skew.** An embedded CLI is preferred over the one Homebrew upgrades. It would also miss docker, devcontainer, op and sbx on the GUI PATH; I inferred this, I did not run it.
- **The documented `swift run` loop crashes.** It dies on the first success notification: UNUserNotificationCenter throws NSInternalInconsistencyException when there is no bundle, which I verified.

**CI.** A `macOS App (Swift)` job does exist in ci.yml and passes. It only runs a debug `swift build` and 4 unit tests. Nothing packages, uploads, signs or notarizes the app, checks for proto drift, or tests against the current CLI JSON. Milestone 3's planned `.github/workflows/macos-app.yml` was never created, and release.yml has no mac job. A release build gives 51 unique warnings, all from swift-protobuf/grpc-swift plugins and none from app code.

**Drift.** The app and agent stopped changing in Nov 2025 (app sources 2025-11-14, proto 2025-11-10). Since then the CLI has moved far ahead:
- **Missing data:** the proto has no runtime provider, published ports, default_agent, last_commit or base_branch, and no exec, prune, devcontainer or tunnel calls.
- **Missing features:** the agent's gRPC List keeps only `active` features. In my test the CLI showed active, degraded, failed_retained and orphaned, while gRPC returned only `alpha:active`.
- **Wrong data from the CLI path:** tunnel info is silently dropped (the CLI now nests it in an object), the module summary always reads "0 ok" (the CLI says success/skipped, not ok/failed), and the date format is unverified on macOS 13/14.
- **Teardown differs by path.** With "Delete branch" unchecked, gRPC keeps the branch, but the CLI path passes no flag. The CLI then deletes the branch by default and, without a terminal, errors out after the worktree is already gone.
- **The CLI doesn't use the daemon.** Its list/start/teardown agent client is dead code; only `agent status` talks to the daemon.

**Docs.** They describe things that don't exist: `brew install branchbox-agent`, `branchbox-agent init`, `sudo branchbox-agent install` (LaunchDaemon), port 50051, `config.toml`, `agent.sock`, a Unix-socket mac app, agent auto-update, Mac App Store distribution, and a `WorktreeAgent` streaming gRPC API. They give the wrong `defaults` key and domain, a dev loop that points at a deleted `/workspaces/milestone2`, and status pages that call the mac app both COMPLETE and IN PROGRESS.

**Intended direction, as the specs state it:**
- **Milestone 2:** a SwiftUI preview riding the agent's gRPC surface with CLI fallback, to prove agent orchestration before a Rails control plane exists.
- **mac-app-polish** (in-progress, Nov 2025): ship to internal users. Pillars: the obvious start/resume task first; advanced options behind disclosures; plain language ("Needs attention", not "CP pending"); and a menu bar that mirrors Home (start, reveal, teardown, tunnel state). Its open questions were whether to embed a launchd agent launcher, and TestFlight vs a notarized DMG.
- **Milestone 3** (proposed): stream agent events to the Rails control plane with durable acks; a macOS CI workflow that builds, tests, packages and uploads `.app`/zip artifacts, with notarization later; zero Swift warnings; Agent-tab "Retry now"; a menu bar tunnel card. App Store/TestFlight is explicitly out of scope.
- **Long term** (IMPLEMENTATION_STATUS): "convert the macOS preview into a true menu-bar daemon that auto-launches the Rust agent".

**Where the code actually went.** None of that happened. All work from v0.5 to v0.13.4 went into the CLI and core: runtime providers (container, sbx, Linux-only local-vm, Agentify in-guest), exec/exec-provider/dispatch-tool, prune, devcontainer commands and 1Password. There is no Rails control plane anywhere in branchbox-suite. ARCHITECTURE.md says it could "extend existing Agentify app", and Agentify now drives BranchBox through CLI contracts (signed manifests), not through the agent or gRPC. The specs' assumption that the agent daemon and its gRPC are the strategic API looks stale, and the product owner needs to decide that before rebuilding the app on gRPC or on the CLI's `--json` output.


## Architecture notes
- Three components: core (library), cli (`branchbox`), and agent (`branchbox-agent`, which serves tonic gRPC on 127.0.0.1:50515 and JSON IPC on ~/.branchbox/agent/branchbox-agent.sock). The agent links core in-process (agent/src/ops.rs), so it runs the same workflows as the CLI, but with defaults frozen: runtime None, no keep-runtime, default devcontainer_reuse.
- The mac app (macos/, SwiftPM executable, macOS 13+) has two transports. gRPC goes to the agent over TCP only. The CLI fallback spawns `/usr/bin/env <cli> ... --json`. Generated stubs match the current proto (regenerated 2025-11-11), but the proto itself is stale.
- Agent IPC JSON (agent/src/ipc.rs) carries runtime metadata; gRPC (agent.proto) doesn't. The CLI's own agent client for feature operations is dead code. Only `agent status` uses IPC.
- The control-plane drain posts batches to whatever BRANCHBOX_CP_ENDPOINT is set to, with bearer auth (agent/src/control_plane.rs:59-60). No receiving service exists in the suite.
- Since Nov 2025, external orchestration (Agentify) drives BranchBox through CLI contracts: `--runtime in-guest --runtime-manifest`, `feature exec-provider`, `feature dispatch-tool`. The CLI's --json output is the de facto integration API.
- local-vm (Firecracker) is x86_64 Linux/KVM-only, so a mac GUI should offer only container and sbx runtimes.
- Local main is at a00b3ee (v0.13.4). origin/main on GitHub has PR #102 (2026-09-29), which touches no mac or agent files. One open PR (#104), also not mac-related.

## Findings

### DIST-01 [critical/distribution] No distributable mac app exists anywhere (no release asset, cask, CI artifact, or install docs)
Users cannot obtain the app except by cloning and running scripts/package-macos-app.sh. GitHub releases contain only CLI archives, even v0.4.0 which RELEASING.md says shipped the 'macOS app'. The Homebrew tap has only a formula, no Casks/. The website install cards and the installation guides cover only the CLI. The package script writes the bundle into the repo (macos/build), which doesn't exist in this checkout, so the app hasn't been packaged here recently.

**Evidence:** `gh release view v0.13.4 --json assets` -> [branchbox-0.13.4-{aarch64,x86_64}-apple-darwin.tar.gz, ...linux..., windows.zip, branchbox-local-vm-image-0.13.4-x86_64.tar.gz, checksums.txt]; v0.4.0 assets likewise CLI-only. /opt/homebrew/Library/Taps/branchbox/homebrew-tap: only Formula/branchbox.rb (`ls: Casks: No such file or directory`). website/index.html:103-119 install cards = `brew install branchbox/tap/branchbox` / install.sh only. RELEASING.md:470 '0.4.0 | ... | Agent daemon, macOS app, gRPC'. scripts/package-macos-app.sh:12-13 OUT_DIR=macos/build. `ls macos/build` -> No such file or directory.

**Suggested fix:** Add a release job on macos-14 that builds a universal .app, signs it with Developer ID plus hardened runtime, notarizes and staples it, uploads a .zip/.dmg to the GitHub release, and updates a `Casks/branchbox.rb` (with `depends_on formula: "branchbox/tap/branchbox"`) in the tap.

### BUG-01 [critical/bug] Automatic transport hangs indefinitely when no agent is running; the CLI fallback never fires
AgentBridge creates a ClientConnection with backoff and calls list() with no CallOptions time limit. grpc-swift's default callStartBehavior is .waitsForConnectivity, which keeps retrying the connection while the RPC waits. With no agent (the default state, since the agent isn't distributed), loadFeatures never returns, so the CLI fallback in AgentBridge.listFeatures's catch block never runs. Start and teardown share the same pattern. This alone likely explains 'very basic and broken'.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:162-168 (list/status with no options), :294-305 (ClientConnection.insecure(...).withConnectionBackoff(maximum: .seconds(5)).connect). grpc-swift ClientConnection.swift:350-354 '.waitsForConnectivity ... may involve multiple connection attempts ... default', :438 `callStartBehavior = .waitsForConnectivity`. Scratch probe replicating that code against closed port 50998: `WATCHDOG: list() still pending after 47.2s (no fallback would occur)`.

**Suggested fix:** Use `.withCallStartBehavior(.fastFailure)` and/or `CallOptions(timeLimit: .timeout(.seconds(2)))`. Check the agent socket/port before trying gRPC, and cache the transport decision with a background re-probe.

### DIST-02 [critical/distribution] Agent daemon is never shipped: no binary in releases or Homebrew, no launchd plist, no init/install commands
The release workflow builds only `--package branchbox-cli`, and the formula installs branchbox, bb and branchbox-local-vm. The agent binary has no subcommands (no init/install) and nothing in the repo installs a LaunchAgent. Yet README advertises 'An always-on BranchBox Agent', and the app's primary transport assumes one. The only documented way to run it is `cargo run -p branchbox-agent`.

**Evidence:** .github/workflows/release.yml:194 `cargo build --release --target ... --package branchbox-cli`; tap Formula/branchbox.rb:30 `bin.install "branchbox", "bb", "branchbox-local-vm"`; `which branchbox-agent` -> not found; `ls ~/.branchbox` -> No such file or directory; `branchbox agent status --json` (per scouting) -> 'failed to connect ... branchbox-agent.sock: No such file or directory'; agent/src/main.rs:32-44 (no CLI args); grep for launchd/LaunchAgent/SMAppService in *.rs/*.sh/*.swift/*.yml -> only docs hits; README.md:92 'An always-on BranchBox Agent tracks'.

**Suggested fix:** Decide whether the agent is strategic (see open questions). If yes, ship `branchbox-agent` in the release tarballs and formula with a `service do` block (brew services) or an app-embedded LaunchAgent registered via SMAppService. If no, drop gRPC from the app and talk to the CLI's --json directly.

### DIST-03 [high/distribution] No Developer ID signing, hardened runtime, entitlements or notarization; the bundle fails codesign verification
The package script never calls codesign. The binary carries only the linker's ad-hoc signature, the bundle isn't sealed, the signing identifier ('BranchBoxApp') doesn't match the bundle id, Info.plist isn't bound, and there's no hardened-runtime flag. Gatekeeper rejects it. Any downloaded copy would be quarantined and blocked, and since macOS 15 Control-click Open no longer bypasses this. The embedded CLI is also ad-hoc and would need re-signing inside a notarized app. No signing identities exist on this Mac and the repo has no Apple secrets.

**Evidence:** scripts/package-macos-app.sh:15-79 (no codesign/notarytool). Scratch replica bundle: `codesign --verify --deep --strict` -> 'code has no resources but signature indicates they must be present'; `spctl --assess --type execute` -> same rejection; `codesign -dv` -> 'flags=0x20002(adhoc,linker-signed) ... Signature=adhoc ... Info.plist=not bound', Identifier=BranchBoxApp; embedded CLI 'Signature=adhoc TeamIdentifier=not set'. `security find-identity -v -p codesigning` -> '0 identities found'. `gh secret list` -> only HOMEBREW_TAP_TOKEN. No *.entitlements files in repo.

**Suggested fix:** Obtain a Developer ID Application cert and add APPLE_* secrets (certificate p12, notarytool API key). Sign nested binaries first, then the bundle, with `--options runtime --timestamp`. Notarize with `xcrun notarytool submit --wait`, then `stapler staple`.

### DIST-04 [high/distribution] Bundle metadata is placeholder-grade and the binary is arm64-only
Info.plist hard-codes version 0.1.0/build 1 (the workspace is at 0.13.4). CFBundleName is 'BranchBoxApp', and there's no CFBundleIconFile, LSApplicationCategoryType, LSUIElement or NSHumanReadableCopyright. `swift build -c release` produces a thin arm64 binary while the CLI ships x86_64-apple-darwin, so Intel Macs can't run a packaged app.

**Evidence:** scripts/package-macos-app.sh:41-68 (CFBundleShortVersionString 0.1.0, CFBundleVersion 1); `plutil -p` of replica bundle shows only those 10 keys; Contents/Resources contains only `bin`; `file .../release/BranchBoxApp` -> 'Mach-O 64-bit executable arm64'; Cargo.toml workspace version 0.13.4.

**Suggested fix:** Generate the plist from the workspace version and git SHA, add an AppIcon .icns, display name 'BranchBox' and category. Build with `swift build -c release --arch arm64 --arch x86_64`, or move to an Xcode project or xcodebuild archive.

### DIST-05 [high/distribution] The app can't find the Homebrew CLI when launched from Finder; the embedded CLI is preferred and drifts from it
CLI resolution order is BRANCHBOX_CLI_PATH env (impractical for GUI launches), then the embedded Resources/bin/branchbox, then `/usr/bin/env branchbox` on PATH. GUI apps get launchd's minimal PATH, which doesn't include /opt/homebrew/bin, so the Homebrew CLI is never found. When a CLI is embedded, it wins over the Homebrew CLI the user upgrades, so the app and the terminal run different CLI versions against the same .branchbox/registry.json. The CLI it spawns inherits the same minimal PATH, so its docker/devcontainer/op/sbx lookups would fail (inferred, not executed). Settings has no CLI path field, which mac-app-polish promised.

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:104-108 (`/usr/bin/env` + cliBinary), :132-148 (resolution order). `launchctl print gui/501` environment = { SSH_AUTH_SOCK } only (no PATH). `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox --version` -> 'env: branchbox: No such file or directory' exit=127. Tool locations: docker=/usr/local/bin, devcontainer=~/.nvm/..., op/sbx=/opt/homebrew/bin. Views/SettingsView.swift:10-40 (no CLI path setting); docs/features/backlog/mac-app-polish.md:40 'Settings tab: CLI path overrides'.

**Suggested fix:** Resolve the CLI by probing known locations (/opt/homebrew/bin, /usr/local/bin, ~/.local/bin, ~/.cargo/bin) or a login-shell `command -v`. Prefer the user's installed CLI and show its version with a mismatch warning. Pass an augmented PATH to child processes, and add a CLI path field plus a 'Doctor' check in Settings.

### UX-01 [high/ux] First-run workspace defaults to '/' when launched from Finder, and the onboarding prompt never appears
AgentConfiguration falls back to FileManager.currentDirectoryPath, which is '/' for Finder/Dock launches. workspaceNeedsSetup only checks that the path exists, so '/' passes and the 'Choose workspace' card never shows. The CLI then fails on every refresh.

**Evidence:** AgentBridge.swift:19-21 (`?? FileManager.default.currentDirectoryPath`); FeatureListViewModel.swift:84,190-192 (`!FileManager.default.fileExists(atPath:)`); HomeView.swift:20. `branchbox feature list --json --repo /` -> 'Error: Validation error: Not a git repository: /' exit=1.

**Suggested fix:** Treat 'no stored workspace' or 'not a git repo with .branchbox' as needing setup. Support several workspaces (recents) and validate them with `branchbox detect`.

### BUG-02 [high/bug] The documented `swift run BranchBoxApp` dev loop crashes on the first success notification
AppDelegate only guards the authorization request with `Bundle.main.bundleIdentifier != nil`. LocalNotifier.notify calls UNUserNotificationCenter.current() unconditionally after every successful start, sync or teardown, and outside a bundle that throws. README and manual-cli-e2e.md both tell developers to use `swift run`.

**Evidence:** Services/LocalNotifier.swift:6-12; calls at FeatureListViewModel.swift:249,275,311; guard only at App/BranchBoxMacApp.swift:13. Scratch probe calling UNUserNotificationCenter.current() from a non-bundled binary: "*** Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil ...'". Docs: macos/README.md:19, docs/docs/getting-started/manual-cli-e2e.md:109.

**Suggested fix:** Guard LocalNotifier on bundleIdentifier, and provide a dev script that wraps the debug build in a minimal .app.

### DRIFT-01 [high/drift] Agent gRPC surface frozen at Nov 2025; List hides degraded, failed_retained and orphaned features
agent.proto is unchanged since 2025-11-10. It has no runtime provider or id, published ports, default_agent, last_commit or base_branch, and no RPCs for exec, prune, devcontainer up/down/build, tunnels or progress streaming. The agent's IPC JSON was updated to carry runtime metadata, but gRPC was not. ops::list_features keeps only Active entries unless include_removed is set, while the CLI shows retained and orphaned features by default. The agent's start request hard-codes runtime None, keep_runtime_on_failure false and the default devcontainer_reuse.

**Evidence:** agent/proto/agent.proto:52-75 (Feature fields), :5-10 (4 unary RPCs); `git log -1 -- agent/proto/agent.proto` -> 05e7231 2025-11-10; agent/src/ipc.rs:344,379 (runtime in IPC payload); agent/src/ops.rs:17-19 (`retain(... == FeatureStatus::Active)`), :79-87. Experiment on synthetic registry: CLI default -> [alpha active, beta degraded, gamma failed_retained, delta orphaned]; gRPC probe -> `features=["alpha:active"]`.

**Suggested fix:** If gRPC stays: version the proto (v1), add runtime/ports/default_agent/health fields and streaming Start/Teardown progress, return all non-removed statuses, and add a CI check that regenerates the Swift stubs and diffs them. Otherwise, retire it.

### DRIFT-02 [high/drift] CLI-fallback decoding no longer matches `feature list --json`
The CLI now emits tunnel as a nested object {provider,status,notes,last_updated}. The app decodes flat tunnelStatus/tunnelProvider/tunnelHostname, so tunnel info is silently nil and the Home/menu tunnel cards always say 'No tunnel detected'. moduleSummary counts 'ok'/'failed', but the CLI emits 'success'/'skipped', so every feature shows '0 ok'. runtime, default_agent, last_commit, base_branch and color are ignored. Fractional-second timestamps decode on macOS 26, but this is unverified on the declared minimum, macOS 13/14. The unit-test fixture uses the old flat shape, so CI can't catch any of this.

**Evidence:** Real output: `"tunnel": {"provider": "cloudflared", "status": "disabled", ...}`, module_outcomes statuses 'success'/'skipped', dates '2026-03-17T03:37:57.979509Z'. Swift replica decode of real JSON: `OK prine active ... tunnelStatus= nil`. CLICompat.swift:150-167; FeatureModels.swift:45-50; Tests/BranchBoxAppTests/BranchBoxAppTests.swift:6-19 (flat `tunnel_status`, no fractional seconds).

**Suggested fix:** Generate Swift Codable models from a JSON schema exported by the CLI (or snapshot-test against `branchbox feature list --json` fixtures captured in CI), and decode dates with a fractional-seconds-tolerant strategy.

### BUG-03 [high/bug] Teardown 'Delete branch' toggle means opposite things over gRPC and CLI; CLI path can fail after removing the worktree
The app defaults deleteBranch=false and on the CLI path passes no flag. Since v0.4.1 the CLI defaults delete_branch_by_default=true. Without a terminal it refuses to force-delete an unmerged branch and bails after the worktree is already removed, so the app reports 'Teardown failed' for a half-completed teardown. The gRPC path sends delete_branch=false and keeps the branch. The CLICompat comment that teardown has no --json is stale; it now has deterministic --json.

**Evidence:** FeatureListViewModel.swift:390-395,442-446 (deleteBranch default false); CLICompat.swift:42-55 (only adds --delete-branch; no --keep-branch); cli/src/commands/feature.rs:828-836 (default_delete_branch), :860-870 (non-TTY bail "could not be deleted without force"); core/src/config.rs:176-178 (`true`); `branchbox feature teardown --help` -> '--keep-branch ... (default is to delete it)', '--json Emit deterministic teardown and residue evidence as JSON'; commit c43b654 'make non-interactive teardown fail on unmerged branch unless forced'.

**Suggested fix:** Always pass --keep-branch or --delete-branch explicitly (plus --force-delete-branch when the user confirms), use teardown --json for the result, and show the residue evidence.

### BUG-04 [medium/bug] CLICompat.run can deadlock on large CLI output (waits for exit before draining pipes)
The process waits for exit before reading stdout or stderr. If the child writes more than the pipe buffer (about 64KB), for example feature start module or Compose diagnostics on stderr, it blocks forever and so does the app's operation. Long operations also give no streamed progress. runDetect calls the synchronous CLI on the MainActor, which freezes the UI. All of this is from reading the code; I didn't run it.

**Evidence:** macos/Sources/BranchBoxApp/Agent/CLICompat.swift:116-124 (`process.waitUntilExit()` then `readDataToEndOfFile()`); FeatureListViewModel.swift:352-365 (`CLICompat.detectProject` inside a MainActor Task).

**Suggested fix:** Read both pipes asynchronously (readabilityHandler or AsyncBytes) while the process runs, stream lines to a log pane, and move every subprocess call off the main actor.

### CI-01 [medium/test_gap] macOS CI job only does debug build + 4 unit tests; no packaging, artifacts, signing, release job, or contract tests
ci.yml has a macos_swift job that passed on the latest main run. It runs `swift build -v` (debug) and `swift test` (4 trivial tests) only. Milestone 3's `.github/workflows/macos-app.yml` (build, test, package, upload, later notarize) doesn't exist, and release.yml has no mac app job. Nothing checks that the generated Swift stubs match agent.proto, and no test runs the app's decoders against a real CLI build. 51 unique warnings in a release build all come from swift-protobuf/grpc-swift plugins (Milestone 3 goal 3 is still open).

**Evidence:** .github/workflows/ci.yml:264-282; `gh run view 36597980282` -> 'macOS App (Swift)' success with steps Build macOS app / Run Swift tests; `ls .github/workflows` (no macos-app.yml); docs/features/backlog/milestone3.md:30-35; scratch `swift build -c release`: 51 unique warnings, 0 in Sources/BranchBoxApp (e.g. "swift-protobuf: 'path' is deprecated: renamed to 'url'").

**Suggested fix:** Add a workflow that runs scripts/package-macos-app.sh (universal), uploads the .app.zip per PR, and adds sign/notarize on tags. Add a job that builds the CLI, captures `feature list --json` and teardown --json fixtures, and runs Swift decode tests against them. Add a proto-regeneration diff check.

### ARCH-01 [medium/architecture] The CLI doesn't use the agent daemon for any feature operation; the daemon's only consumers are the mac app and `agent status`
The CLI's AgentClient list/start/teardown methods are dead code (the module is marked allow(dead_code)), and only `branchbox agent status` connects. All the new capability (runtimes, exec, prune, devcontainer) is CLI-only and runs in-process. So the gRPC-first app design depends on a component that isn't distributed, isn't in the CLI's path, and has had only maintenance commits since Nov 2025.

**Evidence:** cli/src/agent.rs:1 `#![allow(dead_code)]`, :33-89 (list/start/teardown IPC); grep AgentClient -> only cli/src/commands/agent.rs:26; `git log -- agent/src` since 2025-11: runtime/maintenance commits (fc14ab6, 925d98e, bd6e398, cb9ff12) but no new agent RPCs; IMPLEMENTATION_STATUS.md:236 claims 'Agent bridge with BRANCHBOX_AGENT_SOCKET/BRANCHBOX_CLI_DIRECT fallbacks'.

**Suggested fix:** Make an explicit decision. Either (a) the CLI --json contract is the app's API (the app spawns the user's CLI and the daemon is optional), or (b) the daemon becomes a real, shipped LaunchAgent that the CLI also routes through, with an API kept at parity with the CLI.

### DOC-01 [high/doc_gap] Architecture/protocol docs describe a nonexistent agent install path, config and API
Published docs (the Docusaurus internals page and docs/ARCHITECTURE.md) tell users to `brew install branchbox-agent`, `branchbox-agent init` and `sudo branchbox-agent install` (as a root LaunchDaemon, although state lives per-user in ~/.branchbox). They also give `~/.branchbox/agent/config.toml` with [agent]/[tailscale]/[local] sections and listen_addr 127.0.0.1:50051 and agent.sock. They claim the mac app talks over a Unix socket, the agent auto-updates, and the app ships via the Mac App Store, which requires sandboxing incompatible with spawning docker/CLI. PROTOCOL.md defines `worktree.agent.v1.WorktreeAgent` with StreamState and other RPCs that don't exist. The agent's HTTP drain posts to whatever endpoint is configured, not `/v1/devices/:id/events`.

**Evidence:** docs/docs/internals/architecture.md:106-116, :98 ('Auto-update capability'), :151; docs/ARCHITECTURE.md:155,159-161,388-424; actual: agent/src/config.rs:11-12 (`branchbox-agent.sock`, `127.0.0.1:50515`), :150-161 (FileConfig keys), :177 (`agent.toml`); agent/src/main.rs (no subcommands); AgentBridge.swift:299-301 (TCP only); docs/PROTOCOL.md:12-29 vs agent/proto/agent.proto:3-10; agent/src/control_plane.rs:59-60 (`.post(&self.config.endpoint)`); grep for auto-update in agent/src -> none.

**Suggested fix:** Mark these sections 'Planned' or delete them, and generate the config and protocol reference from code (agent.toml keys, the proto).

### DOC-02 [medium/drift] Mac app dev-loop docs are wrong (defaults key/domain, dead workspace path, Docker host check)
manual-cli-e2e.md says `defaults write dev.branchbox.app workspace`, but the code reads `branchbox.workspace`, and under `swift run` the defaults domain is `BranchBoxApp`, not dev.branchbox.app. On this machine the two domains hold different workspaces, and the packaged one points at a deleted worktree. macos/README and start-agent-local.sh hard-code /workspaces/milestone2 and run the agent inside the devcontainer. Core's host check rejects feature start and teardown when /.dockerenv exists, and the script doesn't set BRANCHBOX_SKIP_HOST_VALIDATION, so that loop could only ever list. This is inferred from code; I didn't run it.

**Evidence:** docs/docs/getting-started/manual-cli-e2e.md:106-107; FeatureListViewModel.swift:73 (`branchbox.workspace`); `defaults read BranchBoxApp branchbox.workspace` -> .../branchbox/main; `defaults read dev.branchbox.app branchbox.workspace` -> .../branchbox/milestone2; `ls .../milestone2` -> No such file or directory; macos/README.md:16-19,37; scripts/start-agent-local.sh:5,9; core/src/workflows/feature.rs:377-378,1049-1050,3143-3151; core/src/validation.rs:79-95.

**Suggested fix:** Rewrite the loop around a host-run agent (or the CLI-only mode), document the real keys, and stop shipping 'milestone2' defaults.

### DOC-03 [medium/doc_gap] Status docs contradict each other and the code
IMPLEMENTATION_STATUS lists 'Phase 6: Mac App (COMPLETE)' alongside two 'Phase 4' sections marked IN PROGRESS. It keeps the codegen follow-up unchecked although codegen landed, and claims CI uses Xcode 15.4 and the devcontainer ships Swift 5.10.1. milestone2.md exists in both completed/ and in-progress/. mac-app-polish.md is 'status: in-progress' but sits in backlog/, and its plan steps 3 (rename jargon badges) and 5 (docs) aren't done. AGENTS.md says the workspace ships two members with the agent commented out. The website's CLI card describes `agent status` as showing 'telemetry for the background agent' that users can't install. None of these docs has been touched since 2025-11-14.

**Evidence:** docs/IMPLEMENTATION_STATUS.md:8,202-206,218,251-258; ci.yml:276 `xcode-version: latest-stable`; .devcontainer/devcontainer.json:51 ('SwiftUI builds run on macOS hosts; skipping swift package resolve'); docs/features/{completed,in-progress}/milestone2.md; docs/features/backlog/mac-app-polish.md:3,46-48; FeatureListViewModel.swift:178-180 ('CP connected'/'CP pending'); AGENTS.md:7 vs Cargo.toml:2-6; website/index.html:405-406; `git log -1 -- docs/IMPLEMENTATION_STATUS.md docs/ARCHITECTURE.md` -> 2025-11-14.

**Suggested fix:** Collapse to a single mac-app spec with a real status, and move the stale milestone docs to archive.

### DOC-04 [medium/doc_gap] No user-facing documentation for the mac app at all
The installation guides, quick-start, website and README never say how to get, install or use the app. Only contributor material exists (macos/README.md and a manual-test section). README just says 'macOS app integration over gRPC'.

**Evidence:** grep mac app/agent in docs/INSTALLATION.md, docs/docs/getting-started/installation.md, docs/HOMEBREW_SETUP.md -> no hits; website/index.html (no 'mac app'/'menu bar'/'download'); README.md:100.

**Suggested fix:** After distribution exists, add a 'BranchBox for Mac' page (install via cask, prerequisites, how it finds the CLI, troubleshooting) and a website section.

### MISSING-01 [medium/missing_feature] App exposes none of the post-Nov-2025 feature set the in-progress specs imply a GUI should surface
The app has no:
- runtime provider choice (container or sbx; local-vm is Linux/KVM-only and shouldn't be offered on macOS)
- health and recovery for degraded, failed_retained and orphaned features (--keep-runtime-on-failure, --reuse-runtime, prune)
- `feature exec`, or opening a shell in the devcontainer
- devcontainer up/down/build
- 'Open in Cursor/VS Code attached to the devcontainer' (devcontainer-editor-experience) or 'Open in Codex Desktop' (issue #76)
- default-agent status or launch
- published ports or a browser link for feature_url
- --devcontainer-reuse conflict policy
- init or 1Password onboarding
- --base branch
- periodic or live refresh
- launch at login or update checks
The start sheet still offers only the 4 modules from 2025.

**Evidence:** CHANGELOG.md:120-200 (0.11.0–0.12.1 runtime/exec/retained features); `branchbox feature --help` (exec, exec-provider, dispatch-tool, prune); `branchbox feature start --help` (--runtime, --devcontainer-reuse, --keep-runtime-on-failure, --reuse-runtime, --base); FeatureListViewModel.swift:79 (`availableModules = ["compose","database","tunnel","specs"]`); grep Timer/SMAppService/Sparkle in macos/Sources -> none; core/src/runtime/mod.rs:77-102 (RuntimeMetadata published_ports, container_id...); docs/features/in-progress/devcontainer-editor-experience.md:11-14; gh issue #76 'Add Codex Desktop Remote SSH adapter'.

**Suggested fix:** Re-scope the app around the CLI JSON contract: a feature list with health badges, per-feature actions (open editor, open URL/ports, exec shell, launch agent, teardown/prune), runtime picker, and streaming logs.

### DIST-06 [low/distribution] No launch-at-login, auto-update, or Dock/menu-bar policy
There's no SMAppService login item, no Sparkle or other update channel, and no LSUIElement or activation policy. The app always shows a Dock icon plus a MenuBarExtra and a WindowGroup, and AppDelegate force-activates on launch. The local homebrew-tap working copy is stale (formula 0.4.1, last commit 2025-12-15), while the installed tap is at 0.13.4, so any cask work must start from the remote.

**Evidence:** grep -E 'SMAppService|Sparkle|LSUIElement|setActivationPolicy' macos/Sources -> none; App/BranchBoxMacApp.swift:10,29-66; ~/projects/branchbox-suite/homebrew-tap/main/Formula/branchbox.rb:4 `version "0.4.1"` vs /opt/homebrew/Library/Taps/branchbox/homebrew-tap/Formula/branchbox.rb:4 `version "0.13.4"`.

**Suggested fix:** Choose menu-bar-first (LSUIElement plus an optional window) or window-first. Add SMAppService.mainApp for login, and Sparkle or a brew cask `auto_updates` strategy.

### ARCH-02 [medium/architecture] The Rails control plane the app and agent were built for doesn't exist; Agentify drives BranchBox via the CLI instead
branchbox-suite contains only branchbox and homebrew-tap. Specs say the control plane lives in `control-plane/` 'or extend existing Agentify app'. Since Aug 2026, Agentify integration has gone through CLI contracts: in-guest runtime with signed manifests, exec-provider, dispatch-tool. The app's 'Agent & Control Plane' tab, 'CP pending' badges and drain diagnostics target a backend nobody runs. 'Agent' is also overloaded between the daemon and coding agents: `default_agent`, and the planned `branchbox agent codex connect` in #76 lives in the same `agent` namespace as the daemon's `agent status`.

**Evidence:** `ls ~/projects/branchbox-suite` -> branchbox homebrew-tap; docs/ARCHITECTURE.md:163-165,436,479; docs/features/in-progress/in-guest-devcontainer-provider.md:1-12; CHANGELOG.md 0.11.0 'Verified a real Agentify Compose/devcontainer stack inside Docker SBX'; Views/AgentStatusView.swift:11 'Agent & Control Plane'; gh issue #76 body 'Add commands under the existing `branchbox agent` namespace'.

**Suggested fix:** Remove control-plane UI from the default experience until a backend exists, and rename the daemon surface (for example 'BranchBox service') to avoid colliding with coding agents.

## Experiments

- [pass] **Release assets inventory** — `gh release view v0.13.4 / v0.4.0 --repo branchbox/branchbox --json assets`
  - v0.13.4: branchbox-0.13.4-{aarch64,x86_64}-apple-darwin.tar.gz, linux x2, windows zip, branchbox-local-vm-image, checksums.txt. v0.4.0: CLI archives + checksums only. No .app/.dmg/agent.

- [pass] **Installed Homebrew tap contents** — `ls/cat /opt/homebrew/Library/Taps/branchbox/homebrew-tap/{Formula,Casks}`
  - Formula/branchbox.rb v0.13.4 installs branchbox, bb, branchbox-local-vm; 'ls: Casks: No such file or directory'

- [pass] **Agent binary presence** — `which branchbox-agent; ls ~/.branchbox; ls ~/Library/LaunchAgents | grep -i branch`
  - 'branchbox-agent not found'; '~/.branchbox: No such file or directory'; no LaunchAgents

- [pass] **CI macOS Swift job status** — `gh run view 36597980282 --json jobs`
  - 'macOS App (Swift)' success; steps: Select latest Xcode, Print Swift version, Build macOS app (swift build -v), Run Swift tests

- [pass] **GitHub secrets for signing** — `gh secret list --repo branchbox/branchbox`
  - HOMEBREW_TAP_TOKEN (only)

- [pass] **Local codesigning identities / notary tools** — `security find-identity -v -p codesigning; xcrun --find notarytool`
  - '0 identities found'; notarytool and stapler present in Xcode

- [pass] **Release build of mac app in scratch copy** — `swift build -c release --scratch-path <scratch>/swift-build (in rsync'd copy of macos/)`
  - Build complete! (330.74s); 51 unique warnings, all from swift-protobuf/grpc-swift plugin deprecations, 0 from Sources/BranchBoxApp; binary 'Mach-O 64-bit executable arm64', 'flags=0x20002(adhoc,linker-signed)'

- [fail] **Replica bundle signing/Gatekeeper assessment** — `assemble BranchBoxApp.app per package-macos-app.sh Info.plist; codesign --verify --deep --strict; spctl --assess --type execute`
  - 'code has no resources but signature indicates they must be present' for both; Signature=adhoc; Info.plist=not bound; Identifier=BranchBoxApp; no hardened runtime flag

- [fail] **gRPC connect behavior with no agent (replica of AgentBridge)** — `<scratch>/swift-build/release/GrpcProbe 50998 (ClientConnection.insecure + withConnectionBackoff(max 5s), list() without CallOptions)`
  - 'WATCHDOG: list() still pending after 47.2s (no fallback would occur)'

- [fail] **gRPC List vs CLI list status filtering** — `scratch agent (BRANCHBOX_AGENT_CONFIG, grpc 127.0.0.1:50997) + synthetic registry in disposable repo; branchbox feature list --json vs GrpcProbe 50997 <repo>`
  - CLI default: [alpha active, beta degraded, gamma failed_retained, delta orphaned]; gRPC: features=["alpha:active"]. Agent stopped afterward ('Agent runtime stopped').

- [pass] **CLI IPC agent status against scratch agent** — `BRANCHBOX_AGENT_SOCKET=s.sock branchbox agent status --json`
  - {control_plane_configured:false, control_plane_connected:false, last_*:null ...}; text mode 'Control plane: disabled'

- [partial] **Agent socket path length** — `branchbox-agent with BRANCHBOX_AGENT_DIR under long scratch path`
  - 'Failed to bind Unix socket ... path must be shorter than SUN_LEN' (agent exits); worked with relative socket_path

- [partial] **CLI JSON decode with app's FeatureRecord** — `swiftc replica of CLICompat.FeatureRecord decoding real `branchbox feature list --json` output`
  - 'OK prine active Optional(2026-03-17 03:37:57 +0000) tunnelStatus= nil' (decode succeeds on macOS 26; tunnel data lost)

- [fail] **UNUserNotificationCenter outside bundle (swift run path)** — `swiftc probe calling UNUserNotificationCenter.current() unbundled`
  - "Terminating app due to uncaught exception 'NSInternalInconsistencyException', reason: 'bundleProxyForCurrentProcess is nil ...'"

- [fail] **CLI lookup under GUI-like PATH** — `env -i HOME=$HOME PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/bin/env branchbox --version; launchctl print gui/501 environment`
  - 'env: branchbox: No such file or directory' exit=127; gui/501 environment = { SSH_AUTH_SOCK } (no PATH)

- [fail] **CLI with Finder-default workspace** — `cd / && branchbox feature list --json --repo /`
  - 'Error: Validation error: Not a git repository: /' exit=1

- [partial] **Persisted app defaults** — `defaults read BranchBoxApp branchbox.workspace; defaults read dev.branchbox.app branchbox.workspace`
  - .../branchbox/main vs .../branchbox/milestone2 (latter path does not exist)

- [pass] **Unmerged mac/agent work** — `git log --all --oneline --not main -- macos agent/proto scripts/package-macos-app.sh`
  - only f8470a7 on feat/move-changes-prompt (PR #69 closed unmerged) adding `bool move_changes = 11;` to StartRequest; no unmerged macos/ commits

- [pass] **Repo cleanliness after builds** — `git status --porcelain (before/after CARGO_TARGET_DIR=<scratch> cargo build --locked -p branchbox-agent)`
  - empty both times; no leftover GrpcProbe/branchbox-agent processes

## Open questions
- Is the agent daemon still the strategic API? If yes, it needs to ship (formula/cask plus a LaunchAgent), reach CLI parity, and get a proto version bump. If no, the app should talk to the user's installed CLI `--json` and drop gRPC and grpc-swift.
- Is the Rails control plane alive, or has Agentify replaced it? Should the 'Agent & Control Plane' tab, the CP badges and the HTTP drain stay in a user-facing app at all?
- Who are the target users: the owner only, internal engineers, or public Homebrew users? This decides between ad-hoc local builds, notarized zips via a cask, and a DMG/website download. App Store is effectively ruled out because the app spawns unsandboxed processes.
- Menu-bar-first (LSUIElement, quick start/teardown/open) or window-first dashboard? Specs lean toward 'menu-bar daemon that auto-launches the agent'.
- Should the app embed its own CLI, or require and detect the Homebrew CLI? Embedding guarantees availability but risks version skew against the shared .branchbox/registry.json.
- Which post-0.4 capabilities belong in the GUI? Candidates: runtime picker (container/sbx), retained/orphaned recovery, exec shell, open in Cursor/VS Code/Codex Desktop (issue #76), default-agent launch, ports and URLs, prune, init and 1Password onboarding.
- Should the app support multiple workspaces/repos (recent list), or stay with a single persisted workspace?
- Who owns Apple Developer ID credentials for CI signing and notarization, and is there budget for macOS runner minutes on every PR?
- Should 'agent' be renamed for the daemon, given `branchbox agent codex connect` (#76) and default_agent use the same word for coding agents?
- Does the app need to work on macOS 13/14, its declared minimum? The JSONDecoder `.iso8601` handling of the CLI's fractional-second timestamps was verified only on macOS 26.