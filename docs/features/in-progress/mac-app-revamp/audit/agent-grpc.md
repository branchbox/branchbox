# Audit area: agent-grpc

## Summary
The BranchBox agent (`branchbox-agent`, crate agent/) is a Unix-only tokio daemon. It calls worktree-core in-process. It does not shell out to the CLI. It exposes two transports. (a) A Unix-socket JSON IPC: one JSON request per connection, read to EOF, one JSON reply, with actions list_features, start_feature, teardown_feature and agent_status. (b) A tonic gRPC FeatureService with four unary RPCs (List, Start, Teardown, Status) on TCP 127.0.0.1:50515 by default. It has no TLS, no auth and no reflection. It also runs a heartbeat loop and an HTTP event drain to an optional control plane, backed by SQLite at <state_dir>/agent.db.

The binary takes no CLI flags; it does not parse argv, and `branchbox-agent --help` just starts the daemon. Configuration comes from an optional TOML file at $BRANCHBOX_AGENT_CONFIG or <state_dir>/agent.toml. The env knobs are BRANCHBOX_AGENT_DIR, BRANCHBOX_AGENT_GRPC_ADDR (used only when the TOML has no grpc_addr) and BRANCHBOX_CP_ENDPOINT, BRANCHBOX_CP_TOKEN and BRANCHBOX_CP_VERIFY_TLS. Logs go to stdout only, and at ERROR level unless RUST_LOG is set (EnvFilter default).

Users have no supported way to run it. The CLI only has `agent status`. There is no launchd plist. release.yml builds only `--package branchbox-cli`. The Homebrew formula installs branchbox, bb and branchbox-local-vm. scripts/package-macos-app.sh embeds only the CLI. So every Homebrew user runs the mac app without an agent, and the app's default "Automatic" transport tries gRPC first.

Since Nov 2025 the agent has only received compile-fix patches (new core StartRequest fields hardcoded to defaults). The proto is frozen at the v0.4 shape. Field by field, the proto Feature lacks base_branch, last_commit, last_summary_rendered_at, the whole runtime block (provider, runtime_id, published_ports host mappings, container_id, workspace_folder, container_user, config_path, in_guest, version), tunnel notes/service_url/last_updated/instructions/removed_at, module recorded_at and default_agent. StartRequest lacks runtime, runtime_manifest, devcontainer_reuse, keep_runtime_on_failure/reuse_runtime, workspace_mode (--no-worktree) and allow-container. TeardownRequest lacks force_delete_branch (hardcoded false) and keep-branch tri-state semantics. TeardownSummary lacks runtime_teardown residue evidence. StartSummary lacks runtime, module_reports and default_agent. There are no RPCs for exec, exec-provider, dispatch-tool, prune, tunnel, devcontainer, init, detect or name. The IPC path carries `runtime` but is otherwise just as lossy. Status is an untyped string, timestamps are strings, and empty string or 0 stands for None.

The most damaging behavioural drift is in List. With include_removed=false it keeps only status==Active (agent/src/ops.rs:17-19), so degraded, failed_retained and orphaned features disappear from the app. The CLI shows them by default.

Live tests (built cleanly in scratch with 0 warnings; isolated config with relative socket path; port 50615):
- `branchbox agent status --json` works over IPC.
- gRPC Status, List (main repo, read-only) and Start/Teardown (disposable repo) work.
- A crafted degraded/failed_retained registry is hidden by gRPC List but shown by the CLI.
- A 2s gRPC deadline on an 8s Start returns Cancelled, but the server finishes the Start anyway. There is no cancellation.
- 13 concurrent gRPC Starts produced 13 worktrees and branches and 13 OK replies, but only 12 registry entries. conc-04 was lost: core's registry is an unlocked, non-atomic read-modify-write.
- A concurrent gRPC Status took 7197 ms instead of about 8 ms, because the handlers run blocking core code directly on tokio workers.
- A second agent instance silently takes over the socket while its own gRPC bind fails with only a WARN.
- A socket path over SUN_LEN (104) kills the whole daemon.

On the mac side I ran the real AgentBridge in a scratch copy of the Swift package. Against a dead port, listFeatures stayed pending for 25s with no error and no CLI fallback. That matches grpc-swift defaults: waitsForConnectivity, unlimited retries, no deadline. This is the likely root cause of the "app is broken" experience. Against the live agent, gRPC worked in 0.04-0.07s. The forced CLI fallback worked in 0.05s and showed failed_retained, but tunnel status was nil. In both paths module summaries rendered "0 ok" because the app counts "ok" while core emits "success".

Recommendation: make the app CLI-JSON-first. `feature list --json` is 14 ms median, and the JSON is complete and current (runtime, ports, default_agent, runtime_teardown). Diagnostics go to stderr, so stdout parses cleanly and stderr is a ready-made progress stream. One process per operation also contains core's process-global env-var mutation (SERVICE_URL, BRANCHBOX_DEVCONTAINER_REUSE_POLICY, BRANCHBOX_EMIT_TELEMETRY), which is unsafe in a long-lived multi-request daemon. Keep or redesign the agent only for the remote/control-plane vision. If it stays, it must be shipped, give the app a way to start it (launchd), use spawn_blocking with per-repo serialization, return structured errors, stream progress, have auth, and generate its schema from core types (or carry the CLI JSON verbatim), not hand-maintain a proto. Regardless of transport, core needs a registry file lock and atomic writes.


## Architecture notes
- Process model: branchbox-agent is a single tokio multi-thread process (agent/src/main.rs:32-44 and runtime.rs). It spawns a heartbeat loop (enqueues a snapshot event every heartbeat_interval, default 30s, min 5s), an event loop (dequeues batches and POSTs them to the control plane with bearer token and backoff 2s-120s, or logs and marks delivered when log_only), an optional gRPC server, and the Unix-socket IPC server. The IPC server's lifetime is the process's lifetime.
- Agent calls worktree-core in-process: ops.rs builds core StartRequest/TeardownRequest and calls FeatureWorkflow::start/teardown/list_features synchronously. There is no shell-out to the CLI, so CLI-only behaviour is absent: prompt trimming and 2000-char truncation, --allow-container env override, interactive dirty-worktree and force-delete-branch handling, the config-driven delete_branch default, default_agent planning and launch, and JSON rendering.
- Config surface: TOML keys workspace_root, state_dir, socket_path, heartbeat_interval_secs, grpc_enabled, grpc_addr, event_flush_interval_secs, event_batch_size, event_log_only, and [control_plane] enabled/endpoint/api_token/verify_tls (agent/src/config.rs:150-170). Env: BRANCHBOX_AGENT_CONFIG (config path), BRANCHBOX_AGENT_DIR (default state dir and config location; default ~/.branchbox/agent), BRANCHBOX_AGENT_GRPC_ADDR (only if TOML lacks grpc_addr), BRANCHBOX_CP_ENDPOINT/TOKEN/VERIFY_TLS (override TOML). Defaults: socket <state_dir>/branchbox-agent.sock, gRPC 127.0.0.1:50515, state <state_dir>/agent.db (SQLite WAL; tables worktrees, events, heartbeats, control_plane_status).
- Transports compared: IPC = newline-free JSON over AF_UNIX, one request per connection, client half-closes write; actions list_features|start_feature|teardown_feature|agent_status; payloads hand-written in ipc.rs (includes runtime, last_sent_*). gRPC = 4 unary RPCs, hand-mapped in grpc.rs (no runtime, no last_sent_*). The CLI uses only IPC agent_status; the mac app uses only gRPC (TCP) plus CLI subprocesses, never the socket.
- Mac app transport stack: AgentBridge (grpc-swift 1.27 ClientConnection.insecure to BRANCHBOX_AGENT_GRPC_ADDR or 127.0.0.1:50515), FeatureListViewModel transport preference Automatic|gRPC|CLI stored in UserDefaults 'branchbox.transportPreference', and CLICompat spawning `/usr/bin/env <BRANCHBOX_CLI_PATH | Resources/bin/branchbox | branchbox> ...` synchronously with waitUntilExit. devcontainerSync and detect are CLI-only. agentStatusOrDefault runs `branchbox --help` and then `agent status --json` on every CLI-path refresh.
- Core state model assumes one operation per process: registry read-modify-write without locks, in-place truncating writes, and per-request options passed through process env vars. That fits the CLI and conflicts with a concurrent daemon. Whatever transport is chosen, a registry flock plus atomic rename in FeatureStateStore is required because the app and terminal CLI already contend.
- Recommendation: the app should go CLI-JSON-first. Spawn `branchbox feature list --json` (14 ms) and `feature start/teardown --json`. Use stdout JSON as the result and stream stderr lines as progress. Cancel by signalling the child. Read `agent status --json` only if the agent ever ships. The CLI is already installed by Homebrew and embedded by package-macos-app.sh, it is the most complete interface (runtime/ports/default_agent/runtime_teardown, all statuses, exec/prune/tunnel/devcontainer verbs), and per-process isolation matches core's design. gRPC should become optional; keep it only if a remote or control-plane surface is still on the roadmap. Unix-socket JSON IPC is not a better primary today: it has the same unshipped daemon, blocking handlers and hand-maintained lossy schema. If a daemon is wanted later for live updates (file-watching the registry and pushing changes), have it emit the exact CLI JSON schema with a schema_version field, over the Unix socket, with per-repo serialization and spawn_blocking.

## Findings

### MAC-01 [critical/bug] Mac app's default (Automatic) transport hangs indefinitely when the agent is not running; CLI fallback never triggers
AgentBridge.ensureClient builds a grpc-swift ClientConnection with default callStartBehavior .waitsForConnectivity and ConnectionBackoff retries .unlimited, and no CallOptions timeLimit. A unary call against a dead endpoint waits for connectivity forever, so the catch block that falls back to CLICompat never runs. The agent ships in no release, so this is the normal state for every user.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:294-306 (no callStartBehavior or timeLimit), :162-196 (fallback only in catch). grpc-swift 1.27.0 checkout: Sources/GRPC/ClientConnection.swift:438 `callStartBehavior: CallStartBehavior = .waitsForConnectivity`; ConnectionBackoff.swift:85 `retries: Retries = .unlimited`; CallOptions.swift:86 `timeLimit: TimeLimit = .none`; ConnectionManager.swift getHTTP2MultiplexerPatient returns readyChannelMuxPromise in .transientFailure. Live (scratch copy, real AgentBridge, port 59999): `PROBE_RESULT port=59999: STILL PENDING after 25s (no return, no throw, no CLI fallback)`. Against the live agent on 50615: `RETURNED after 0.07s transport=grpc`.

**Suggested fix:** Short term: use .fastFailure plus CallOptions(timeLimit: .timeout(.seconds(2))) for List/Status, and longer deadlines for Start/Teardown. Long term: make the CLI-JSON path primary (see REC).

**Verifier:** confirmed — Code: AgentBridge.swift:299-301 builds `ClientConnection.insecure(group:).withConnectionBackoff(maximum: .seconds(5)).connect(...)`. It sets no callStartBehavior. The generated client is created with default CallOptions (:303), and no timeLimit appears anywhere in Sources outside Generated. grpc-swift 1.27.0 (macos/.build/checkouts): ClientConnection.swift:438 `callStartBehavior: CallStartBehavior = .waitsForConnectivity`; ConnectionBackoff.swift:85 `retries: Retries = .unlimited` (withConnectionBackoff(maximum:) only changes maximumBackoff); CallOptions.swift:86 `timeLimit: TimeLimit = .none`; ConnectionManager.swift:461 `.transientFailure` returns `state.readyChannelMuxPromise.futureResult`, which stays unresolved. The CLI fallback is only in the catch (:190-196, :223-229, :259-271). The view model defaults to `.automatic` (FeatureListViewModel.swift:89-91), which maps to override nil. Live test: a scratch copy of macos/ (sources identical to the repo per diff -r) and a new XCTest that uses the real AgentBridge on dead port 59998 in Automatic mode gave `V1_RESULT dead-port=59998 automatic list: STILL PENDING after 30s`. A control on the same port with `.withCallStartBehavior(.fastFailure)` plus `CallOptions(timeLimit: .timeout(.seconds(2)))` gave `threw after 0.00s: ... Connection refused (errno: 61)`, so the suggested fix works. DIST-01 confirms no agent ships, so this hang is the default for every user. One nuance: switching the transport picker to CLI calls resetConnection(), which fails the pending call and lets the fallback run. Automatic mode still hangs indefinitely. Critical severity is justified.


### DIST-01 [high/distribution] Agent binary is not shipped and has no user-facing start/stop path
Releases build and archive only the CLI. The Homebrew formula installs branchbox, bb and branchbox-local-vm. There is no `branchbox agent start/stop`, no launchd plist, and the mac .app embeds only the CLI. The only ways to run the agent are `cargo run -p branchbox-agent` or the dev scripts. Docs advertise a nonexistent `brew install branchbox-agent`, `branchbox-agent init` and `branchbox-agent install`.

**Evidence:** .github/workflows/release.yml:189-195 (`cargo build --release ... --package branchbox-cli`), :213-216 (archive = branchbox, bb, branchbox-local-vm). /opt/homebrew/Library/Taps/branchbox/homebrew-tap/Formula/branchbox.rb:30 `bin.install "branchbox", "bb", "branchbox-local-vm"` (local tap checkout homebrew-tap/main/Formula/branchbox.rb is stale at 0.4.1). `which branchbox-agent` -> not found. `branchbox agent --help` -> only `status`. ~/Library/LaunchAgents has no branchbox plist. scripts/package-macos-app.sh:28,72-75 embeds only the CLI. docs/ARCHITECTURE.md:393-403 is fictional install text. docs/features/backlog/mac-app-polish.md lists a launchd plist only as an open question.

**Suggested fix:** Either drop the agent from the app's critical path (preferred), or ship it (release matrix plus formula), add `branchbox agent start|stop|install` with a LaunchAgent plist, and embed it in the .app.

**Verifier:** confirmed — release.yml:189-195 builds only `--package branchbox-cli`. :210-216 archives branchbox, bb (a copy) and scripts/local-vm/branchbox-local-vm. `grep -c -i agent release.yml` returns 0. The installed tap formula /opt/homebrew/Library/Taps/branchbox/homebrew-tap/Formula/branchbox.rb is version 0.13.4 and line 30 is `bin.install "branchbox", "bb", "branchbox-local-vm"`. The local checkout homebrew-tap/main/Formula/branchbox.rb is stale at `version "0.4.1"` with `bin.install "branchbox"`. `which branchbox-agent` returns `branchbox-agent not found`. `branchbox agent --help` lists only `status`. ~/Library/LaunchAgents has no branchbox entry. package-macos-app.sh:28 runs `cargo build -p branchbox-cli` and :72-74 copies only CLI_BIN into Resources/bin. docs/ARCHITECTURE.md:393,400,403 contain the nonexistent `brew install branchbox-agent`, `branchbox-agent init` and `sudo branchbox-agent install`. mac-app-polish.md:55 has the launchd plist only as an open question.


### DRIFT-01 [high/drift] Agent List hides degraded / failed_retained / orphaned features
ops::list_features keeps only FeatureStatus::Active when include_removed=false, which is what the app sends by default. The CLI default filter is `!= Removed`. So the app never shows features that need attention (degraded, failed_retained, orphaned), which are exactly the new statuses added since 0.4.

**Evidence:** agent/src/ops.rs:17-19 `entries.retain(|feature| feature.status == FeatureStatus::Active)`, compared with cli/src/commands/feature.rs:552-554 `features.retain(|feature| feature.status != FeatureStatus::Removed)`. Live: gRPC List include_removed=false -> only `probe-one active`. gRPC List include_removed=true and `branchbox feature list --json` -> `[('probe-ret','failed_retained'), ('probe-deg','degraded'), ('probe-one','active')]`. AgentBridge(grpc) against the disposable repo listed 13 active features and no probe-ret; AgentBridge(cliFallback) included `probe-ret:failed_retained`.

**Suggested fix:** Mirror CLI semantics (exclude only Removed) and add a status filter to ListRequest, or retire the gRPC list.

**Verifier:** confirmed — ops.rs:17-19 `if !include_removed { entries.retain(|feature| feature.status == FeatureStatus::Active); }`, compared with cli feature.rs:552-554 `features.retain(|feature| feature.status != FeatureStatus::Removed)`. FeatureStatus has Active/Degraded/FailedRetained/Orphaned/Removed (feature.rs:5270-5276). The app sends includeRemoved=false unless BRANCHBOX_SHOW_REMOVED=1 (AgentBridge.swift:22,166). Live test on a disposable repo with my own agent build: features va (active), vb (degraded) and vc (failed_retained). gRPC List include_removed=0 returned only `work_feature: "va" status: "active"`. include_removed=1 and `branchbox feature list --json` both returned `[('vc','failed_retained'), ('vb','degraded'), ('va','active')]`. Through the real AgentBridge after va/vb teardown: `V4_RESULT grpc transport=grpc features=[]` versus `V4_RESULT cli transport=cliFallback features=["vc:failed_retained"]`. The IPC path uses the same ops::list_features (ipc.rs:132), so the same filter applies there.


### DRIFT-02 [high/drift] Proto Feature/Start/Teardown/summary schemas are frozen at v0.4 and drop most new core data
Feature lacks base_branch, last_commit, last_summary_rendered_at, the runtime block (provider, runtime_id, published_ports {host,runtime}, container_id, workspace_folder, container_user, config_path, in_guest, version), tunnel notes/service_url/last_updated/instructions/removed_at, module recorded_at and the CLI-computed default_agent. StartRequest lacks runtime, runtime_manifest, devcontainer_reuse, keep_runtime_on_failure (reuse_runtime), workspace_mode (--no-worktree) and allow_container. ops.rs hardcodes them: devcontainer_reuse Default, keep_runtime_on_failure false, runtime None, runtime_manifest None, workspace_mode Default. TeardownRequest lacks force_delete_branch (hardcoded false) and keep-branch semantics. StartSummary lacks runtime, module_reports and default_agent/prompt_bridge_enabled. TeardownSummary lacks runtime_teardown {provider, runtime_id, verified, residue_free, residue}, which matters for sbx/local-vm residue evidence. AgentStatus lacks last_sent_batch_id/event_id/at, which IPC has, so the app sets nil. Lossy encodings: status, tunnel_status and start_mode are free strings; None becomes "" or 0 (pr_number 0, last_sync_at ""); tunnel_status defaults to "none"; mode values other than "minimal" silently become Full; ModuleSkipReason is flattened to its description string. Only mapping and dependency commits touched agent/ after 2025-11-15.

**Evidence:** agent/proto/agent.proto:21-32,38-46,52-75,77-93,120-128,142-149. core/src/workflows/feature.rs:155-179 (StartRequest), :239-257 (StartSummary), :283-307 (TeardownRequest/Summary), :5441-5502 (FeatureMetadata). core/src/runtime/mod.rs:77-102,130-133,168-176. agent/src/ops.rs:72-89,99-108. agent/src/grpc.rs:91-94,180-229,231-276. macos AgentBridge.swift:107-109. `git log -- agent/` after 77c3dc6 (0.4.0) shows only c43b654, fc14ab6, ee032a8, 7cc2438, 925d98e, bd6e398 and cb9ff12, each 1-8 lines. Live gRPC List of main returned no runtime/base_branch/last_commit; CLI JSON for the same features includes `runtime: {provider: container}`, last_commit and default_agent.

**Suggested fix:** Stop hand-maintaining a parallel schema. Either have the app consume CLI JSON, or make the agent return CLI-identical JSON (e.g. serialize FeatureMetadata verbatim) and generate Swift Codable models from it.

**Verifier:** confirmed — agent.proto:52-75 Feature has no base_branch, last_commit, last_summary_rendered_at or runtime, and its tunnel is flattened to provider/status/hostname. Core FeatureMetadata (feature.rs:5441-5502) has all of these, plus RuntimeMetadata (runtime/mod.rs:77-102) and FeatureTunnelState notes/service_url/instructions/last_updated/removed_at (feature.rs:5352-5369). Proto StartRequest (:21-32) lacks runtime/devcontainer_reuse/keep_runtime_on_failure/workspace_mode, and ops.rs:79-87 hardcodes them as the finding says. TeardownRequest has no force_delete_branch (ops.rs:103 hardcodes false). Proto TeardownSummary (:120-128) lacks runtime_teardown, which is present in core (feature.rs:296-306). Proto StartSummary lacks runtime and module_reports. AgentStatus (:142-149) lacks last_sent_*, which IPC returns (ipc.rs:211-213), so AgentBridge.swift:107-109 sets nil. The lossy encodings are confirmed in grpc.rs: mode `_ => StartMode::Full` (:91-94), `unwrap_or_else(|| "none")` for tunnel_status (:198-202), unwrap_or_default for pr_number/last_sync_at, and ModuleSkipReason flattened to `.description()` (:303). Git history: since 77c3dc6 only c43b654, fc14ab6, ee032a8, 7cc2438, 925d98e, bd6e398 and cb9ff12 touched agent/. All are small (ee032a8 is 5+/5-, so slightly over the stated 1-8 lines), and c43b654 only added `force_delete_branch: false`. The proto was last changed 2025-11-10. Live: the gRPC Feature for va has no runtime/last_commit/default_agent. The CLI JSON for the same feature has `runtime= {'provider': 'container'}`, last_commit e1ce796... and a default_agent object. The IPC FeatureRecord already carries `runtime: RuntimeMetadata` (ipc.rs:344,379), so gRPC lags even the agent's own IPC.


### CORE-01 [high/bug] Feature registry is an unlocked, non-atomic read-modify-write; concurrent starts lose entries
FeatureStateStore::record_start/record_teardown/update_feature do load_registry -> mutate -> save_registry. write_text_file truncates and rewrites in place, with no flock, temp file or rename. A long-running agent with concurrent requests, an app plus a terminal CLI, or two CLI processes on the same repo can lose updates or read a half-written file. AGENTS.md already lists 'registry race conditions' as known.

**Evidence:** core/src/workflows/feature.rs:5534-5575 (record_start), :5577-5606, :5666-5692 (load/save), :4910-4926 (write_text_file: OpenOptions create+truncate, no lock). grep found no flock in registry code; only core/src/runtime/in_guest.rs:2442 uses flock. Live: 13 concurrent gRPC Starts all returned OK, `git worktree list | grep -c conc-` = 13 and agent.db had 13 rows, but `registry conc entries: 12` with conc-04 missing. The feature existed on disk with a branch but was invisible to list.

**Suggested fix:** Add an exclusive flock on .branchbox/registry.lock around every load-modify-save, and write via temp file plus rename.

**Verifier:** confirmed — Code: record_start (feature.rs:5534-5575), record_teardown and update_feature all do load_registry, then mutate, then save_registry (:5666-5692). write_text_file (:4910-4926) opens with create+truncate and writes in place, with no temp file, rename or lock. The only flock in core/cli/agent is runtime/in_guest.rs:2442. AGENTS.md:202 lists 'registry race conditions'. Live repro on a disposable repo with my own agent build (post-checkout hook sleep 3 to align the critical sections): 14 concurrent gRPC Starts all returned `OK`, `git worktree list` showed 14 c* worktrees, and agent.db `worktrees` had 14 rows. registry.json had `registry c* entries: 13` with `missing: ['c07']`. The worktree repos/c07 and branch `feature/c07` exist, but gRPC List include_removed=1 returned 13 c* features, so c07 is invisible.


### AGENT-01 [high/performance] gRPC and IPC handlers run blocking core workflows directly on tokio worker threads
ops::start_feature/teardown_feature/list_features are synchronous and run git, docker and devcontainer commands for seconds to minutes. They are called inline in async handlers without spawn_blocking, so concurrent long operations stall unrelated RPCs, including Status.

**Evidence:** agent/src/grpc.rs:63-65,98,124-125; agent/src/ipc.rs:132,165,187. Live, with a post-checkout hook adding 8s to Start and 13 concurrent Starts on a 12-core host: a gRPC Status issued at +1.5s took `OK (7197 ms)`, compared with `OK (8 ms)` idle. Per-request completion times split into ~9s and ~18s groups (start-02/03/10/12/13 at ~9s, the rest at ~17.6-18.0s).

**Suggested fix:** Wrap core calls in tokio::task::spawn_blocking and serialize per repo, or move to a process-per-operation model.

**Verifier:** confirmed — grpc.rs:63-65, 98 and 124-125 and ipc.rs:132, 165 and 187 call the synchronous ops::* (git/devcontainer work) directly inside async handlers. The only spawn_blocking in agent/ is state.rs:441 (DB work). main.rs uses #[tokio::main] (multi-thread). Live on a 12-core host, with a 3s post-checkout hook: idle Status took `OK (4 ms)`. During 14 concurrent Starts, Status took `OK (2476 ms)`. The hook log shows the effect is worse than the finding describes: only 1 Start began at 19:17:01, 12 began at 19:17:05 and 1 at 19:17:08, so one blocking handler even delayed accepting other connections. Start latencies were 3.5s, ~7s (x12) and 10.4s. Even a single in-flight Start stalled a fresh Status call: `OK (2744 ms)`, `OK (2911 ms)` and `OK (3616 ms)` across 3 trials. Severity high is defensible for a daemon whose job is to serve a UI, though real-world impact is limited while the agent is unshipped (DIST-01).


### AGENT-02 [medium/architecture] Core passes per-request options through process-global env vars, unsafe in a multi-request daemon
Start sets and restores SERVICE_URL and BRANCHBOX_DEVCONTAINER_REUSE_POLICY around module setup; start and teardown set or remove BRANCHBOX_EMIT_TELEMETRY; host validation reads BRANCHBOX_SKIP_HOST_VALIDATION. In the CLI (one operation per process) this is fine. In the agent, concurrent requests for different repos cross-contaminate each other, and set_var concurrent with reads is a data race. It is `unsafe` in Rust 2024; the workspace is edition 2021.

**Evidence:** core/src/workflows/feature.rs:441-443, 704-731, 1106-1110, 3143-3152; cli/src/commands/feature.rs:493-517 (HostValidationOverride env toggle). agent/Cargo.toml edition = "2021".

**Suggested fix:** Thread these options through StartRequest/TeardownRequest or a context struct; until then, keep core calls one-per-process.

### AGENT-03 [medium/missing_feature] No streaming progress, no cancellation, no client deadlines, opaque errors
All four RPCs are unary, so there is no progress for multi-minute starts. Server work cannot be cancelled: a client deadline returns Cancelled while the Start continues and records the feature. The mac app sets no deadlines. Every core error becomes Status::internal(string), so the app cannot tell connect failures from domain errors such as WorktreeDirty, and none of the CLI's dirty-worktree or force-delete-branch handling is available.

**Evidence:** agent/proto/agent.proto:5-10 (unary only); agent/src/grpc.rs:331-333 `Status::internal(err.to_string())`. Live: `PROBE_TIMEOUT_MS=2000 start ...` -> `ERR (2001 ms) code=Cancelled message=Timeout expired`, then registry showed `('probe-deadline','active','2026-10-01T22:58:10.530051Z')` and agent.db logged a feature_start event. Teardown without force -> `ERR (59 ms) code=Internal message=Worktree has module-managed changes: ... (dirty entries: [".devcontainer/"])`. Compare cli/src/commands/feature.rs:855-858,992-1000 (handle_teardown_error).

**Suggested fix:** With CLI-first: stream stderr lines as progress, cancel with SIGINT/SIGTERM on the child, and map exit codes plus JSON errors. If gRPC is kept: server-streaming progress, spawn_blocking with cooperative cancellation, and typed status codes.

### MAC-02 [high/bug] Mac app re-runs Start/Teardown through the CLI after any gRPC error, including domain errors
In Automatic mode, startFeature and teardownFeature catch every error from the gRPC call (Internal domain failures, deadline, connection drop mid-operation) and immediately run the same operation through CLICompat. A gRPC start that failed or timed out after partially running, or that is still running server-side (see AGENT-03), is executed a second time concurrently on the same registry (see CORE-01). The original gRPC error is discarded. The gRPC StartResponse/TeardownResponse are also thrown away (`_ =`), so warnings and skipped modules never reach the UI.

**Evidence:** macos/Sources/BranchBoxApp/Agent/AgentBridge.swift:222 `_ = try await client.start(request)`, :223-229 catch -> `try CLICompat.startFeature(...)`; :258 `_ = try await client.teardown(request)`, :259-271 catch -> CLICompat.teardownFeature.

**Suggested fix:** Fall back only on connectivity failures (UNAVAILABLE before the request was sent). Surface domain errors and response summaries to the UI.

**Verifier:** confirmed — AgentBridge.swift:222 `_ = try await client.start(request)` with a catch at :223-229 that, unless forceGrpc is set, calls CLICompat.startFeature for any error. Teardown does the same at :258-271. Both responses are discarded, and the CLI path also discards stdout (CLICompat.swift:39,54). Live test with the real AgentBridge in Automatic mode and BRANCHBOX_CLI_PATH pointed at a logging wrapper. (1) Domain error: gRPC Start of an existing feature returned `code=Internal message=Worktree already exists`. The bridge then ran `CLI-INVOKED: feature start vc ... --json --no-summary --minimal`, and the user-visible error was `CLI fallback failed: Error: Worktree already exists...`, so the gRPC error was replaced. (2) Concurrent re-execution is reachable without AGENT-03. With a slow start (6s hook) in flight, calling setTransportOverride(nil) at +1.5s triggered resetConnection(). The in-flight call failed at 1.60s and the bridge immediately ran the CLI `feature start vd` while the agent kept executing the original Start, which completed and left `vd active`. The app reported 'Start failed' even though the feature was created. In the UI, the Transport picker and workspace menu are not disabled while isWorking (MainAppView.swift:71-83), so a user can trigger this. One sub-case in the finding is not reachable: no deadline error can occur, because the app sets no timeLimit.


### DRIFT-03 [medium/drift] Teardown branch-deletion default differs between the gRPC path and the CLI
The CLI's default deletes the branch (config feature.teardown.delete_branch_by_default = true, with an explicit --keep-branch). The proto has a plain bool delete_branch defaulting to false, so gRPC teardown keeps branches by default, and force_delete_branch is always false. The app's CLI fallback passes --delete-branch only when the toggle is on and never --keep-branch, so with the toggle off the CLI fallback deletes the branch while gRPC keeps it.

**Evidence:** cli/src/commands/feature.rs:827-846; core/src/config.rs:154-155,176-177 (default true); agent/src/ops.rs:99-108 (force_delete_branch: false); macos CLICompat.swift:42-55. Live: gRPC teardown probe-one force=1 delete_branch=0 -> `branch_deleted: false`, branch `feature/probe-one` remains. CLI `feature teardown probe-deg --force --json` -> `"branch_deleted": true` plus a runtime_teardown block.

**Suggested fix:** Model keep/delete as a tri-state (unset = config default), or route through the CLI.

**Verifier:** confirmed — CLI teardown resolves keep_branch, then delete_branch, then config `delete_branch_by_default` (cli feature.rs:827-835). The default is true (config.rs:154-155, 176-177). gRPC passes the plain proto bool (default false) through ops.rs:99-108 with force_delete_branch hardcoded false. Core uses `force_remove || force_delete_branch` (feature.rs:1246), so a forced gRPC delete is still forced. The app's toggle defaults to false (TeardownOptions deleteBranch=false, loadTeardownDefaults uses defaults.bool). CLICompat.swift:42-55 adds --delete-branch only when the toggle is on and never passes --keep-branch. Live on a disposable repo: gRPC teardown va force=1 delete_branch=0 gave `worktree_removed: true, branch_deleted: false`, and `feature/va` remained. CLI `feature teardown vb --force --json` (app-style args, non-TTY) gave `{'branch_deleted': True, 'worktree_removed': True, 'runtime_teardown': {...}}`. The same app action therefore keeps or deletes the branch depending on transport.


### SEC-01 [medium/security] Unauthenticated plaintext gRPC can start or force-teardown worktrees in any repo path; dev script binds 0.0.0.0
The tonic server has no TLS, no interceptor and no token check, and trusts any repo_path. Default bind is 127.0.0.1:50515, but loopback TCP has no UID check, so any local user or process can call Teardown{force:true}, which removes worktrees even when dirty. Start runs repo-defined module and bootstrap logic for an arbitrary repo_path. scripts/start-agent-local.sh defaults GRPC_ADDR=0.0.0.0:50515, exposing this to the LAN, and the internals doc encourages non-loopback binds. The mac app uses ClientConnection.insecure. The IPC socket is protected by filesystem permissions (srwxr-xr-x; others lack write so cannot connect). agent.db is 0644 and contains workspace paths and prompt seeds.

**Evidence:** agent/src/grpc.rs:40-49 (Server::builder().add_service(...).serve(addr), no auth); agent/src/config.rs:12 DEFAULT_GRPC_ADDR "127.0.0.1:50515"; scripts/start-agent-local.sh:9 `GRPC_ADDR="${GRPC_ADDR:-0.0.0.0:50515}"`; docs/docs/internals/architecture.md:172; AgentBridge.swift:299; agent/src/state.rs:588 (prompt_seed in metadata). Live `ls -la agent-state`: `-rw-r--r-- agent.db`, `srwxr-xr-x branchbox-agent.sock`.

**Suggested fix:** Prefer the Unix socket (or no daemon) for local clients. If TCP stays, require a per-user token file (0600) in metadata, refuse non-loopback binds without TLS, and create the state dir with 0700.

**Verifier:** confirmed — grpc.rs:40-49 uses Server::builder().add_service(...).serve(addr) with no TLS, interceptor or token. config.rs:12 defaults to 127.0.0.1:50515. ops::resolve_repo accepts any repo_path. scripts/start-agent-local.sh:9 defaults `GRPC_ADDR="${GRPC_ADDR:-0.0.0.0:50515}"`. AgentBridge.swift:299 uses ClientConnection.insecure. Live: with my unauthenticated probe I started feature x1 in a second repo r2 (not the agent's workspace_root), added an untracked file and modified a tracked file, then sent Teardown force=1. The result was `OK (49 ms) worktree_removed: true`, and the directory was gone with the uncommitted work lost. State dir listing: `-rw-r--r-- agent.db`, `srwxr-xr-x a.sock` (umask 022), matching the finding. The LAN exposure is better supported than the finding states: .devcontainer/compose.yaml:26 publishes `"50515:50515"`, which Docker binds on all host interfaces, so start-agent-local.sh's 0.0.0.0 bind inside the devcontainer is reachable from the LAN. 'Encourages non-loopback binds' slightly overstates docs/docs/internals/architecture.md:170-171, which documents the option and recommends SSH/WireGuard. Medium severity is reasonable given the agent is dev-only today.


### AGENT-04 [medium/bug] Second agent instance silently takes over the IPC socket; gRPC bind failure is only a warning
IpcServer::serve unconditionally unlinks an existing socket file and binds its own. The gRPC server task logs 'listening' before binding and only warns on failure, and the process keeps running. Two daemons then split traffic: CLI and IPC go to the new one, gRPC to the old one. Both run heartbeat and event loops against the same agent.db. The socket file is also left behind on shutdown.

**Evidence:** agent/src/ipc.rs:29-36 (remove_file if exists), agent/src/grpc.rs:41 (info! before serve), agent/src/runtime.rs:80-90 (warn! on gRPC error). Live, agents A and B with the same config: B log `INFO gRPC server listening on 127.0.0.1:50615` then `WARN gRPC server exited with error: transport error: Address already in use (os error 48)`. Both stayed alive; lsof showed both PIDs on branchbox-agent.sock and only A on TCP 50615. After SIGTERM, `srwxr-xr-x branchbox-agent.sock` remained.

**Suggested fix:** Take a pidfile/flock on the state dir; treat gRPC bind failure as fatal; remove the socket on shutdown.

**Verifier:** confirmed — ipc.rs:29-36 removes an existing socket file unconditionally before binding. grpc.rs:41 logs 'listening' before serve. runtime.rs:83-86 only warn!s on a gRPC error, and the process continues. Nothing removes the socket on shutdown (no other remove_file in agent/src). Live with two agents using the same config: B logged `INFO gRPC server listening on 127.0.0.1:50715` then `WARN gRPC server exited with error: transport error: Address already in use (os error 48)` then `IPC server listening on a.sock`. Both PIDs stayed alive. The socket inode changed from 165484632 to 165531718, so B replaced A's socket. lsof showed only A on TCP 50715 and both PIDs holding a.sock. After SIGTERM to B, a.sock remained, and IPC to it failed with `CONNECT FAILED: [Errno 61] Connection refused` while A was still alive and serving gRPC. The surviving daemon is unreachable to the CLI until restart.


### AGENT-05 [low/bug] Long socket paths are fatal; no CLI flags; ERROR-only logging by default; no log file
A socket_path over SUN_LEN (104 bytes on macOS) makes UnixListener::bind fail, and the whole daemon exits, gRPC included. main() parses no argv, so `--help` starts the daemon. tracing uses EnvFilter::from_default_env, so without RUST_LOG only errors are logged, to stdout. The agent ignores BRANCHBOX_AGENT_SOCKET; only the CLI reads it, yet manual-agent-e2e.sh passes it to the agent. The IPC treats an empty repo_path string as a path, whereas gRPC falls back to workspace_root.

**Evidence:** Live: `Error: Failed to bind Unix socket /private/tmp/.../agent-state/branchbox-agent.sock` / `Caused by: path must be shorter than SUN_LEN` (path length 155), exit=1. `branchbox-agent --help` under a 2s alarm -> exit 142 with `Starting BranchBox agent` and `gRPC server listening`. agent/src/main.rs:34-44; Cargo.toml:53 tracing-subscriber features ["env-filter"]; cli/src/agent.rs:145-163 vs agent/src/config.rs:76-78; scripts/manual-agent-e2e.sh:96. IPC `{"repo_path":""}` -> `error Validation error: Not a git repository: `.

**Suggested fix:** Add clap flags (--config, --socket, --grpc-addr, --log-file), default RUST_LOG=info, and validate socket length with a clear error.

**Verifier:** confirmed — Long socket path: a config with a 154-char absolute socket_path made the agent exit 1 with `Error: Failed to bind Unix socket .../stC/branchbox-agent.sock / Caused by: path must be shorter than SUN_LEN`. It had already logged `gRPC server listening on 127.0.0.1:50716`, so the whole daemon died. No argv parsing: main.rs:34-44. `branchbox-agent --help` under a 3s alarm gave exit=142 and created agent.db and d.sock, so the daemon started. Without RUST_LOG it printed 0 bytes on stdout and stderr. With RUST_LOG=info it printed `INFO branchbox_agent: Starting BranchBox agent` to stdout. This matches fmt::try_init using EnvFilter::from_default_env (tracing-subscriber 0.3.23 fmt/mod.rs:1200-1204, env-filter feature at Cargo.toml:53). BRANCHBOX_AGENT_SOCKET is read only by cli/src/agent.rs:146, and the agent's socket comes from the config file or state_dir (config.rs:76-78). manual-agent-e2e.sh:96 passes it, though harmlessly, because the script also sets BRANCHBOX_AGENT_DIR and AGENT_SOCKET equals ${AGENT_STATE_DIR}/branchbox-agent.sock (lines 7-8). IPC `{"action":"list_features","repo_path":""}` returned `Validation error: Not a git repository: `, whereas grpc.rs:164-170 maps an empty string to None (workspace_root). Low severity is appropriate. The default socket path ~/.branchbox/agent/branchbox-agent.sock is well under 104 bytes, so the SUN_LEN failure needs a custom long path.


### AGENT-06 [low/architecture] Agent's own SQLite view of worktrees is incomplete and unbounded
The agent's worktrees table is keyed by work_feature only, with no repo column, so features with the same name in different repos collide. It records only features started or torn down through the agent, missing everything created by the CLI, yet heartbeats ship this snapshot to the control plane. Delivered events are never deleted, and each heartbeat stores a full snapshot.

**Evidence:** agent/src/state.rs:453 `work_feature TEXT PRIMARY KEY`, :39-85, :134-166; grep DELETE in agent/src/state.rs -> 0. Live agent.db after the session: `heartbeat|69`, `feature_start|15`, `feature_teardown|16`. The worktrees table had probe-one and probe-deadline but not the CLI-created probe-deg/probe-ret.

**Suggested fix:** Derive snapshots from core registries per repo, key by (repo, feature), and prune delivered events.

### MAC-03 [medium/ux] Module health always renders '0 ok'; CLI fallback drops tunnel status
FeatureViewData.moduleSummary counts statuses equal to "ok", but core emits success, skipped and failed. CLICompat.FeatureRecord expects flat tunnel_status/tunnel_provider/tunnel_hostname, but CLI JSON nests them under `tunnel{provider,status,notes,last_updated}`, so they decode as nil. The Swift unit-test fixture uses a flat tunnel_status and second-precision timestamps, so tests pass against a shape the CLI no longer emits.

**Evidence:** macos/Sources/BranchBoxApp/Agent/FeatureModels.swift:39-51; core/src/workflows/feature.rs:59-67 ("success"); CLICompat.swift:150-167; macos/Tests/BranchBoxAppTests/BranchBoxAppTests.swift fixture. Live AgentBridge output for main: `prine:active:tunnel=disabled:modules=0 ok` (prine has 3 success and 1 skipped). CLI fallback: `probe-ret:failed_retained:tunnel=nil:modules=0 ok`.

**Suggested fix:** Generate Codable models from real `feature list --json` output and add a golden-file test.

### TEST-01 [medium/test_gap] No tests exercise gRPC/IPC mapping; the 'agent' e2e harness never routes feature ops through the agent
The agent has 5 unit tests (state cursor and CP payload) and none for grpc.rs, ipc.rs or ops.rs. scripts/manual-agent-e2e.sh starts the agent and then runs manual-cli-e2e.sh. The CLI never talks to the agent for feature commands: AgentClient list/start/teardown are dead code under #![allow(dead_code)], and BRANCHBOX_CLI_DIRECT is never read. The 'via agent' run only checks the drain. CHANGELOG and docs claim the CLI and mac app use the socket and gRPC.

**Evidence:** `grep -rn '#[test]|#[tokio::test]' agent/src | wc -l` -> 5; cli/src/agent.rs:1,33-90; cli/src/commands/agent.rs:25-27 (only agent_status); scripts/manual-agent-e2e.sh:116-121; docs/IMPLEMENTATION_STATUS.md:237; CHANGELOG.md:357; docs/docs/internals/architecture.md:161-166.

**Suggested fix:** If the agent is kept, add contract tests asserting proto/IPC payloads equal CLI JSON for the same registry. Otherwise delete the dead client paths and correct the docs.

### DOC-01 [low/doc_gap] PROTOCOL.md and ARCHITECTURE.md describe a different, unimplemented agent
PROTOCOL.md specifies package worktree.agent.v1 with WorktreeAgent (StartFeature/ListWorktrees/StreamState, an enum status, etc.), none of which exists. ARCHITECTURE.md describes brew install branchbox-agent, branchbox-agent init/install as a LaunchDaemon, ~/.branchbox/agent/config.toml with device tokens, port 50051, Tailscale and bidirectional streaming. The internals doc claims 'All APIs return structured errors'. The manual-e2e doc uses defaults key `workspace`, but the app reads `branchbox.workspace`.

**Evidence:** docs/PROTOCOL.md:3-80,391-403; docs/ARCHITECTURE.md:101-113,207-208,314-336,388-403; docs/docs/internals/architecture.md:174; docs/docs/getting-started/manual-cli-e2e.md:106 vs AgentBridge.swift:20 and FeatureListViewModel.swift:73.

**Suggested fix:** Mark these docs as aspirational or rewrite them to match agent/proto/agent.proto and the chosen transport.

## Experiments

- [pass] **Build agent into scratch target** — `CARGO_TARGET_DIR=<scratch>/agent-grpc/target cargo build -p branchbox-agent`
  - Finished `dev` profile in 3m 03s; `grep -c ^warning build.log` = 0. Repo `git status --porcelain` stayed empty.

- [fail] **Agent with long socket path (default-style absolute path under scratch, 155 bytes)** — `BRANCHBOX_AGENT_CONFIG=<scratch>/agent.toml perl -e 'alarm 6; exec @ARGV' branchbox-agent`
  - Error: Failed to bind Unix socket .../agent-state/branchbox-agent.sock / Caused by: path must be shorter than SUN_LEN; exit=1 (whole daemon exits, gRPC included)

- [pass] **Agent isolated run (relative socket, grpc 127.0.0.1:50615, RUST_LOG=info)** — `cd agent-state && BRANCHBOX_AGENT_CONFIG=... BRANCHBOX_AGENT_DIR=... RUST_LOG=info branchbox-agent &`
  - gRPC server listening on 127.0.0.1:50615; IPC server listening on branchbox-agent.sock; lsof: TCP 127.0.0.1:50615 (LISTEN). Stopped later with SIGTERM: 'Agent runtime stopped'.

- [pass] **CLI agent status over IPC** — `BRANCHBOX_AGENT_SOCKET=branchbox-agent.sock branchbox agent status --json`
  - {"control_plane_configured": false, "control_plane_connected": false, "last_delivery_at": null, ...}; without the env var: 'failed to connect to BranchBox agent at ~/.branchbox/agent/branchbox-agent.sock'

- [skipped] **grpcurl availability** — `which grpcurl grpc_cli protoc; python3 -c 'import grpc'`
  - grpcurl not found, grpc_cli not found, protoc not found; no python grpc. Wrote a scratch tonic client (grpc-probe) compiled offline from agent/proto/agent.proto.

- [pass] **gRPC Status and List on main repo (read-only)** — `grpc-probe status; grpc-probe list ~/projects/branchbox-suite/branchbox/main 0`
  - Status OK (8 ms). List OK (44 ms): prine and remotion with status active, tunnel_status disabled, pr_number 0, last_sync_at "", timestamps '+00:00'; no runtime, base_branch, last_commit or default_agent fields.

- [pass] **gRPC Start on disposable repo (uninitialized)** — `PROBE_SKIP=compose,database,tunnel grpc-probe start <scratch>/repos/disposable probe-one minimal`
  - OK (821 ms), worktree repos/probe-one, branch feature/probe-one, warnings ['Skipped .env provisioning (source file not found)', ...], no runtime in summary

- [fail] **Status filtering: gRPC vs CLI vs IPC** — `Set registry statuses probe-deg=degraded, probe-ret=failed_retained; grpc-probe list R 0 / 1; branchbox feature list --json; IPC list_features`
  - gRPC include_removed=false -> only probe-one active; include_removed=true -> all 3; CLI default -> [failed_retained, degraded, active]; IPC -> only probe-one (with runtime {provider: container}).

- [fail] **Deadline / cancellation** — `post-checkout hook sleep 8; PROBE_TIMEOUT_MS=2000 grpc-probe start R probe-deadline minimal`
  - ERR (2001 ms) code=Cancelled message=Timeout expired; afterwards the registry shows probe-deadline active at 22:58:10.53 and agent.db has a feature_start event, so the server finished anyway.

- [fail] **Concurrency: 13 parallel Starts plus a Status probe** — `13x grpc-probe start R conc-NN minimal & ; at +1.5s grpc-probe status; IPC agent_status`
  - All 13 OK (5 at ~9 s, 8 at ~17.6-18.0 s). Status took 7197 ms (idle 8 ms). IPC status 41 ms. Registry has 12 conc entries; conc-04 missing although its worktree and branch exist and agent.db has 13 rows (lost update).

- [partial] **Teardown semantics** — `grpc-probe teardown R probe-one 0 0; grpc-probe teardown R probe-one 1 0; branchbox feature teardown probe-deg --force --json`
  - Unforced gRPC -> code=Internal 'Worktree has module-managed changes ... [".devcontainer/"]'. Forced gRPC -> OK, branch_deleted false, branch kept. CLI non-tty unforced -> exit 1 'rerun with --force'. CLI forced -> branch_deleted true plus runtime_teardown {verified, residue_free}.

- [fail] **Two agent instances same config** — `start agent A, then agent B with identical config`
  - B: 'gRPC server listening on 127.0.0.1:50615' then 'WARN gRPC server exited with error: ... Address already in use'. Both alive, both hold branchbox-agent.sock (B rebound it), A holds TCP. Both stopped by SIGTERM.

- [fail] **Agent ignores CLI args** — `perl -e 'alarm 2; exec @ARGV' branchbox-agent --help`
  - exit=142 (killed by alarm); log shows 'Starting BranchBox agent' and listeners, so --help was ignored

- [fail] **Mac AgentBridge against dead gRPC port (scratch copy of macos/, swift test)** — `PROBE_ENABLED=1 PROBE_GRPC_PORT=59999 swift test --skip-build --filter TransportProbeTests`
  - PROBE_RESULT port=59999: STILL PENDING after 25s (no return, no throw, no CLI fallback)

- [partial] **Mac AgentBridge against live agent** — `PROBE_GRPC_PORT=50615 PROBE_WORKSPACE=main / disposable swift test --filter TransportProbeTests`
  - RETURNED after 0.07s transport=grpc features=[prine:active:tunnel=disabled:modules=0 ok, remotion:...]; for disposable, probe-ret (failed_retained) absent

- [partial] **Mac AgentBridge forced CLI fallback** — `PROBE_TRANSPORT=cli ... swift test --filter TransportProbeTests`
  - RETURNED after 0.05s transport=cliFallback, includes probe-ret:failed_retained, but tunnel=nil and modules=0 ok for all

- [partial] **CLI-fallback JSON decode on this macOS (Swift 6.2, macOS 26)** — `swift decode.swift feature-list.json (verbatim CLICompat.FeatureRecord, .iso8601 strategy)`
  - DECODED 2 records; first tunnelStatus=nil (fractional-second timestamps accepted on this OS)

- [pass] **Latency: CLI JSON vs gRPC** — `7 runs each: branchbox feature list --json --repo main; grpc-probe list main 0`
  - CLI median 14 ms; gRPC probe (incl. process spawn) median 15 ms; `agent status --json` with dead socket 6 ms

- [pass] **Cleanup** — `gRPC teardown force=1 delete_branch=1 for all conc-*, probe-*; kill -TERM my agent PIDs; git status in main`
  - All teardowns OK (conc-04 too, recovered without registry entry); `git worktree list` only disposable main; my agent processes gone; main repo `git status --porcelain` empty; ~/.branchbox not created

## Open questions
- Does the CLI-fallback JSONDecoder `.iso8601` strategy reject chrono's fractional-second timestamps (e.g. 2026-03-17T03:37:57.979509Z) on macOS 13/14, the app's minimum target? On macOS 26 / Swift 6.2 it decoded fine (verified). Older Foundation's ISO8601DateFormatter without .withFractionalSeconds would fail the whole list decode. Not verified on an older OS.
- How does the CLI behave on SIGINT/SIGTERM mid `feature start` (partial worktree, registry consistency)? This matters for a CLI-first app that cancels by signalling the child. Not tested.
- Is the remote control-plane/Tailscale vision (docs/ARCHITECTURE.md, PROTOCOL.md, milestone3) still on the roadmap? If so, the agent could be redesigned for that purpose instead of being deleted. If not, the gRPC stack, the proto codegen script and the generated Swift stubs could be retired.
- Why did 13 concurrent starts finish in two waves (~9 s and ~18 s)? Likely tokio LIFO-slot pinning or git-level locking; not investigated beyond the observed timings.
- Other sibling auditors appeared to be building or running their own agent (processes under scratchpad/runtime-e2e were seen). Their state was not touched, and port 50615 was used to avoid colliding with the default 50515.