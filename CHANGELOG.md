# Changelog

All notable changes to BranchBox will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

#### CLI and core

- Managed in-guest assignments can bind a reviewed workspace path and an explicit set of omitted
  Compose connectors. Generated configuration, the writable task bind and provider working directory
  must match the assignment; mismatches refuse startup or execution. Version-3 preloaded images are
  required. `runtime-capabilities` and `version --json` advertise these guarantees for staged binaries.
- Initialization accepts valid 1Password references with spaces in vault, item and field names,
  including `private key`, while still rejecting control characters and incomplete references.
- Devcontainer symlink sync records link ownership so unchanged BranchBox-created links no longer
  block teardown. Changed targets and links without recorded ownership remain protected; older
  symlink features need another sync to record their baseline.
- The manual agent gate now starts and tears down a private minimal feature through Unix IPC,
  then requires matching workflow events, metadata, retry delivery and a durable final control-plane
  acknowledgement. Its loopback stub uses an available port; `--ipc-only` provides a small real
  lifecycle check and prebuilt agent binaries avoid unnecessary rebuilds.
- Feature teardown removes standalone image/Dockerfile devcontainers bearing the exact worktree's
  workspace label, and discovers owned Compose resources through verified lexical and canonical
  workspace labels. Discovered Compose identities survive partial cleanup for safe retries.
  Cleanup checks command results and remaining resources; failures report an unverified receipt,
  and observed residue never reports residue-free. Remaining Compose ownership evidence is preserved.
  Failed runtime/module cleanup retains a possibly provisioned worktree and registry entry for retry;
  `--force` can override retention while the receipt still reports cleanup status. Bare features
  still work without Docker.
- Generated managed Compose project identities bind the project to its canonical workspace.
  Legacy identities restore only through matching recorded feature ownership or verified workspace
  evidence; copied or ambiguous env files refuse cleanup before containers are removed.
- Devcontainer Stop checks every exact workspace/configuration match and observed or retained
  Compose project. Stop keeps volumes by default; explicit volume deletion covers attached anonymous
  volumes for standalone containers and owned Compose volumes. Cleanup failures no longer report success.
- Devcontainer detection reports the configured user and workspace from the active devcontainer
  configuration, including root `.devcontainer.json` and unrelated nullable fields; image/Dockerfile
  projects no longer inherit service facts from unused Compose files.

- Reject Compose `provider` services in managed in-guest projects before staging CLI inputs;
  Compose would otherwise run the repository-selected provider binary on the guest host.
- Reject ambient variable interpolation left in sanitized or generated in-guest Compose inputs,
  including dependency environments, service labels, and workspace paths. Value-less dependency
  environment and build arguments can no longer inherit guest-process variables. Managed Docker
  and Dev Containers commands now receive only Docker connection settings and basic runtime paths,
  and Compose ignores a repository `.env`; signed raw project environment remains available.
- Filter disabled tunnel connectors from each sanitized in-guest Compose file's
  dependencies before Compose merges the ordered files. Later service overrides
  that only add a command no longer retain an undefined connector dependency,
  and dependencies on runnable services from earlier files remain intact.
- Allow managed in-guest Compose projects to use interpolated volume sources on secondary
  services. BranchBox now passes sanitized copies of repository Compose files to the Dev
  Containers CLI, removing repository mounts, env files, publications, discarded primary
  environment values, entire disabled connector definitions across all Compose input files,
  and manifest-replaced image/build fields before Compose's per-file interpolation while
  retaining the signed mount facade.
  Source inputs are bounded regular files inside the task worktree. For signed workspace
  consumers, generated CLI inputs live in a private assignment directory outside the writable
  worktree; unassigned builds fail closed. Stale generated copies are removed when the source
  list shrinks.
- Concurrent BranchBox processes no longer lose each other's registry updates. Every change to
  `.branchbox/registry.json` is a read-modify-write under an exclusive lock on the `.branchbox`
  directory (released when the holder exits), and the file is replaced atomically, so a reader
  never sees a partial document and a crash mid-write leaves the previous registry intact. A
  writer that waits more than 30 s fails with an error naming the lock. The devcontainer sync
  baselines under `.branchbox/devcontainer-sync/` are replaced atomically as well.
- Starting several features at once in one repository no longer fails with `fatal: failed to
  read .git/worktrees/<name>/commondir`. BranchBox now serializes its own `git worktree`
  changes and branch deletions per repository; git cannot run them concurrently.
- `tunnel open` and `tunnel remove` for a feature that is not registered now say so and name the
  registry file (`Feature '<name>' is not registered in <repo>/.branchbox/registry.json`)
  instead of reporting `Worktree not found`. When `tunnel open` cannot derive a hostname, the
  error names the `.env` file it read.
- A `feature start --runtime sbx` (or `local-vm`) that is killed after its sandbox exists no
  longer leaks the sandbox. The in-progress registry entry records the sandbox as soon as it is
  created, so `feature teardown --force` removes it instead of reporting the runtime as cleaned
  up.
- On filesystems without file locks (some NFS, SMB and FUSE mounts), registry and worktree
  changes no longer fail. BranchBox warns once and continues without the lock, as 0.13 did.
- Two concurrent starts of the same feature now refuse the second one with the usual "worktree
  already exists" error instead of a raw `git worktree add` failure.
- `feature start --json` prints only its JSON summary on stdout. The "Prompt truncated to 2000
  characters before storage." notice used to precede the JSON and break parsers; it now goes to
  stderr and is also listed in the summary's `warnings`.
- `feature teardown` without `--force` no longer deletes uncommitted work (BUG-04). When
  `git worktree remove` refused a worktree with modified or untracked files, BranchBox deleted
  the directory anyway and reported success. Teardown now lists the worktree's changes before it
  does anything and refuses, naming the files, unless `--discard-changes` (or `--force`) says to
  discard them. The `remove_dir_all` fallback is reachable only under `--force`; without it a
  failed `git worktree remove` keeps the worktree and its registry entry and names git's error.
- A freshly started feature tears down without `--force`, also in a repository with no
  `.gitignore` (UX-13). Files BranchBox wrote itself (`.devcontainer/.branchbox.env` and the
  other reserved names, the `.devcontainer/.env` link, the feature block appended to `.env`, the
  Peacock and window-title settings in `.vscode/settings.json`, the "Open Feature URL" task,
  `.devcontainer` files still matching their sync baseline, and files identical to the main
  worktree's) are recognized by their content and no longer count as changes. Editing any of
  them, for example adding a setting to a tracked `.vscode/settings.json`, makes it a user change
  again.
- Teardown deletes the branch the feature was started on (DRIFT-07). A feature started with
  `--branch-prefix spike` had its `spike/<name>` branch left behind while teardown tried
  `feature/<name>`. The branch now comes from `--branch-prefix` when given, else from the
  registry, else from the configured prefix. `prune` uses the registry too. A branch that does
  not exist is skipped with a warning.
- Whether a branch is merged now follows `git branch -d` (its upstream when one is set, else
  `HEAD`) instead of parsing `git branch --merged`, which missed branches checked out in another
  worktree.
- `feature start` keeps the settings of an existing `.vscode/settings.json` that has comments or
  trailing commas; it used to replace them with only BranchBox's keys.
- `feature teardown <name>` refuses when the folder at the feature's path is not a linked worktree
  of the repository, with or without `--force`. `teardown main` (the standard `<project>/main`
  layout) used to run the specs module and the adapter cleanup against the main worktree, deleting
  its `tmp/` and `.cache/` contents, and `--force` then deleted the whole main repository; an
  unrelated sibling repository or folder was deleted the same way. The plan reports a
  `not_a_worktree` blocker that no flag overrides. The folder a half-finished forced teardown
  leaves behind (registered, with no `.git` link) still needs `--force`.
- The feature spec teardown promises to keep is no longer lost when moving it to the main
  worktree fails (for example a read-only `docs/features/backlog/`): teardown stops with a
  `spec_not_preserved` blocker before the runtime goes, also under `--discard-changes`; only
  `--force` removes it anyway. Only the one spec teardown moves (the first of `in-progress/`,
  `backlog/` and `completed/`) counts as preserved; another copy is a user change. An in-guest
  teardown moves no spec, so an edited spec there is a user change too.
- The adapter cleanup (`tmp/`, `.cache/`, `build/`, `dist/` and the other stack caches) runs after
  the runtime is stopped and the worktree passed its last check, right before removal. Files
  written there meanwhile now stop the teardown instead of being deleted unseen, and a teardown
  that stops keeps those directories.
- `branchbox init` writes `.branchbox/config.json` atomically under the `.branchbox` lock, so a
  concurrent reader never sees a partly written file. `init --update` (and any init over an
  existing `config.json`) now edits only the settings it changes, keeping keys it does not know
  and the file's formatting; it used to rewrite the file from scratch and drop them.
- `branchbox init` treats a project as already set up only when `.branchbox/registry.json`
  exists, the rule `detect --json` and `doctor` use. A `.branchbox/` holding only `config.json`
  (from `config set`, `tunnel credentials set`, or committed configuration in a fresh clone) used
  to make `init` report `already_initialized` and do nothing; it now sets the project up and
  keeps that configuration.
- `config set`, `config apply` and `tunnel credentials set` accept a `config.json` whose
  `tunnel.providers.cloudflared` is `null` (what init writes when tunnels are declined) and
  replace the null with the provider settings; they used to refuse it as "not an object".
- `init --validate --json` reports the detected `stack`, `adapter` and `modules` instead of the
  `generic` default.
- `detect` and `doctor --repo` run from a subfolder report the main worktree's paths plainly
  (`<main>/.branchbox/config.json`, not `<main>/sub/../.branchbox/config.json`).
- The `config apply` refusals for an oversized or unparsable patch no longer contain a run of
  spaces.
- The Cloudflare token file written by `branchbox init` (`.branchbox/secure/cloudflared.env`) is
  now owner-only (0600) from creation, in an owner-only (0700) directory, and replaced
  atomically; a file an earlier version wrote world-readable is narrowed on the next write.
  Lines BranchBox does not manage are kept.

#### macOS app

- Stray-worktree removal rechecks the exact paths confirmed for discard and refuses new changes.
  A second confirmation retains earlier consent. Prune blocks truncated change lists; ordinary
  Tear Down requires explicit confirmation before removing changes omitted from a truncated list.
- Removed-feature branch actions refresh when the project, feature or branch changes and ignore
  cancelled lookups. Empty-project descriptions respect the window height so the sidebar and
  Quick Open remain visible after closing Activity.
- Open Shell uses the configured container user and workspace, and falls back from Bash to `sh`
  for images that do not provide Bash.
- The macOS app finds the `branchbox` CLI when it is launched from Finder or the Dock, and the CLI it runs finds
  `docker`, `git`, the devcontainer CLI (nvm), `op` and `gh`. GUI apps inherit launchd's
  `PATH=/usr/bin:/bin:/usr/sbin:/sbin`; the app now captures your login shell's environment once
  (`$SHELL -l -i -c`, falling back to `-l -c`, then to its own environment), searches that PATH for the CLI
  itself, and gives every command it runs that PATH plus the Homebrew, cargo, `~/.local/bin` and Docker
  Desktop directories. The CLI path is used as found (`/opt/homebrew/bin/branchbox`, not the Cellar path), so
  a `brew upgrade` is picked up. Only the PATH is remembered between launches, in
  `~/Library/Application Support/BranchBox/environment-cache.json`.
- The macOS app no longer freezes when a command prints more than about 64 KB (for example
  `feature list --all` with many features). Output is read while the command runs, never after it exits.
- Cancelling an operation in the macOS app stops everything the command started: SIGINT to its process
  group, then SIGTERM after 5 s and SIGKILL after 3 more, and the app waits for the group to exit before
  reporting the cancellation. A cancellation that arrives before the command has started means it never
  starts. Quitting the app stops running commands the same way, within 10 s.
- Tearing down a feature from the macOS app no longer deletes uncommitted work. Right before running
  `feature teardown`, the app reads the worktree with git and separates your changes from the files BranchBox
  generated (its `.devcontainer` env files, the managed `.env` block, Peacock colours in `.vscode/settings.json`,
  copies of main's files). If any change is yours, it refuses and names the files without running anything. It
  goes ahead only once you confirm discarding exactly those files, and it refuses again if new changes appeared
  after you confirmed. With CLI 0.13.x it never passes `--force` without that confirmation.
- With CLI 0.13.x the app now always runs teardown with `--keep-branch` and deletes the branch itself afterwards:
  `git branch -d` for "Delete if merged", `-D` only for "Force-delete". If git refuses to delete the branch, the
  teardown still counts as done and the app shows git's reason. "Merged" follows `git branch -d`: the branch's
  upstream if it has one, otherwise main's `HEAD`. A branch checked out in another worktree is no longer
  misread (the app no longer parses `git branch --merged`). The derived `--branch-prefix` is passed, so a
  `spike/zeta` branch is found.
- On its first launch the macOS app waits for the login-shell capture (about 2 s) before locating the CLI, so a
  `branchbox` earlier on your PATH is used rather than the one in `/opt/homebrew/bin`. Later launches search the
  remembered PATH straight away.
- Opening a feature worktree (or the folder that holds `main/`) as a project opens its main worktree. With CLI
  0.13.4, `feature list` run from a feature worktree lists nothing.
- Errors in the macOS app show the CLI's own cause: the JSON error envelope on 0.14, or the `Error:` line and
  its "Caused by" list on 0.13.x, never the last log line. A corrupt `.branchbox/registry.json` is reported as
  such, and known refusals (worktree exists, not a git repository, `sbx login` needed, no main `.devcontainer`,
  and others) name their cause.
- `devcontainer sync` with CLI 0.13.x: a feature whose sync failed is shown as failed even though the CLI exits 0.
- `devcontainer up/down/build` with Docker stopped shows "Docker is not available" instead of a decode error, and
  `feature exec` shows a failing command's exit code and output instead of an error.

### Changed

- Release readiness now uses named user outcomes and E2E evidence, with refusal, recovery,
  cancellation, persistence and cleanup paths where applicable. Contributor and release guidance
  retain Rust line coverage as a diagnostic instead of a numeric threshold, and require explicit
  evidence scope and accepted gaps. Published 0.13.4 fixes are recorded under that version.

#### CLI and core

- `feature start` registers the feature as soon as its worktree exists. Until the start
  completes, the entry carries `setup: {state, pid, started_at}`; `feature list --json` reports
  `state: "interrupted"` once that process has exited or the start is more than 24 hours old.
  A start that fails or is killed midway therefore stays listed instead of leaving an
  unregistered worktree behind. Older CLIs ignore the new key.
- `feature list` reports an active or retained feature whose worktree directory no longer exists
  as `orphaned`.
- `branchbox init` adds `.branchbox/devcontainer-sync/`, `.branchbox/.registry.*.tmp` (left
  behind only if a write crashes) and `.branchbox/.lock` (the lock file on platforms that cannot
  lock the directory) to `.gitignore`.
- A command that waits for another BranchBox process to release the registry or worktree lock
  logs one line saying so on stderr.
- Any `--json` flag now implies a non-interactive run. Nothing prompts; each prompt takes its
  non-interactive outcome instead (usually a refusal that names the flag to pass), as it already
  did without a terminal. Prompts also require stdin to be a terminal, not only stdout, and
  `branchbox init` no longer reads its reorganization answers from piped stdin: without a
  terminal it keeps the repository where it is and says to rerun with `--yes`.
- In `--json` mode, human-readable text goes to stderr, so stdout carries exactly one JSON
  document. Text mode prints the same text to stdout as before.
- Log lines on stderr are coloured only when stderr is a terminal and `NO_COLOR` is not set; they
  no longer carry ANSI escape codes when redirected to a file or pipe.
- `branchbox name validate` reports an invalid name as an error (`Error: Invalid feature name:
  <name>`, exit 1) after its existing explanation, like every other failing command.
- Without a terminal (or with `--json`), a teardown that would delete an unmerged branch now
  refuses before it changes anything, naming `--keep-branch` and `--force-delete-branch`, unless
  `feature.teardown.force_delete_unmerged_by_default` is set. It used to remove the worktree and
  mark the feature removed first, then fail on the branch. Interactive teardown still asks.
- Teardown without `--force` no longer discards changes silently, for every caller: the CLI and
  the agent's IPC and gRPC teardown now get a `teardown_refused` error for a worktree with
  uncommitted changes. Callers that relied on the silent discard must pass `--force` (as the
  repository's harnesses and tests do) or `--discard-changes`. `prune` is unchanged: it still
  discards changes and force-deletes branches, as documented.
- A teardown that refuses changes nothing: the tunnel, modules, spec, adapter, runtime, worktree,
  branch and registry entry are all left as they were. If user changes appear while teardown is
  stopping the runtime, it stops before removing the worktree, keeps the worktree and the
  registry entry, and lists what it already did. The registry entry is marked removed only once
  the worktree is gone.
- `--force` still force-deletes the branch (`git branch -D`) when deleting it. When that deletes
  unmerged commits, the summary warns: `Force-deleted unmerged branch <branch> (<n> commits); use
  --discard-changes to discard files without deleting unmerged commits`. The `--force` help says
  what it does.
- `feature teardown --json` reports a refusal as the `teardown_refused` envelope whose
  `details` are `{plan, changed_anything, completed_steps}`. Before, a refusal over module files
  carried `plan: null` and a `files` list.
- A failed branch delete after the worktree is gone is reported and the teardown still succeeds
  (exit 0, `branch_deleted: false`, `branch_delete_error`).
- In machine mode `prune` without `--yes` fails with `confirmation_required`, and a `--feature`
  that is not an active or retained feature fails with `feature_not_found` before anything is
  pruned.
- `branchbox devcontainer sync` exits 1 when any worktree failed to sync, in text and JSON mode,
  after printing the full report. It used to print the failures and exit 0. Text mode still
  prints each worktree's row as it finishes.
- A successful `devcontainer sync` records the worktree's devcontainer baseline, so a later
  teardown recognises the synced files as BranchBox's own.
- `branchbox init` without a terminal names `--op-github-ref` and `--skip-1password` in its
  "1Password credential references not configured" warning. An unknown `--stack` is refused with
  `validation_failed` (the message is unchanged).

Text-mode output and the manual harness:

- The refusal banner's first line is unchanged when devcontainer or compose files changed
  (`⚠️  Detected devcontainer/compose changes inside <path>:`), and the `Error:` line still starts
  with `Devcontainer/compose changes detected; rerun this command with --force to proceed.`, now
  followed by a `Caused by:` line naming every changed file and `--discard-changes`. Other
  changes print `⚠️  Detected uncommitted changes inside <path>:`. Each listed file now shows its
  kind (`• README.md (modified)`), and the last banner line reads `(BranchBox refuses to discard
  them without --discard-changes or --force)`.
- `scripts/manual-cli-e2e.sh` needs no change: its dirty-teardown step still fails first and
  succeeds on the scripted `--force` retry, and the phrases it checks ("Detected
  devcontainer/compose changes", "Tunnel descriptor missing") are still printed. Its two clean
  teardowns (`--delete-branch --complete-spec`) succeed without `--force` as before.
- A missing branch is reported as `Branch '<branch>' not found; nothing to delete` instead of a
  `Failed to delete branch` warning carrying git's error.

#### macOS app

- The macOS app requires macOS 26 or later (was macOS 13) and builds with Xcode 26.
- The macOS app is rebuilt on a new, dependency-free Swift package (`macos/Package.swift`, Swift 6 language
  mode). It is split into `BranchBoxKit` (backend contract, CLI JSON models, process contract),
  `BranchBoxCLI`, `BranchBoxStores`, `BranchBoxPreview` and the `BranchBoxApp` executable. The product is now
  `BranchBox`: run it with `swift run --package-path macos BranchBox` instead of `swift run BranchBoxApp`.
  Debug builds can run entirely on a scripted preview backend with `BRANCHBOX_BACKEND=preview`.
- macOS app CI moves to `.github/workflows/macos-app.yml`. It builds with warnings as errors and runs the
  tests on macOS 26 with the latest stable Xcode. The old `macos_swift` job is gone
  from `ci.yml`.
- On first launch the app carries over the 0.13 app's project (`branchbox.workspace`) and recent prompts. It
  deletes the saved teardown choices (Force, Delete branch, Complete spec), the transport preference and the
  devcontainer strategy, so an old Force setting can never pre-arm a teardown.
- A development build (`swift run`, `swift test`) keeps its projects and logs in "BranchBox Dev" folders, as it
  already keeps its preferences in the `dev.branchbox.app.dev` suite.
- Quitting while operations run asks first; Cancel and Quit stops them and their processes.
  Closing the window keeps BranchBox in the menu bar, and the Dock icon reopens it.
- Empty and blocking states keep their buttons at their natural width and stack them when space
  is short. Paths are listed one per line instead of wrapping mid-path.
- Waiting operations show a clock instead of a spinner, prune progress shows "n of m" beside a
  wider bar, and setup checklists no longer show "0 ms".
- An error banner offers Retry only for errors that can pass on their own. A refused teardown
  offers Show Changes and "Discard N changes and tear down…" (confirmed first) instead.
- The macOS app no longer uses gRPC. The generated SwiftProtobuf/gRPC stubs, the agent bridge and the
  grpc-swift, swift-protobuf and swift-nio dependencies are gone, along with `macos/Package.resolved`. The app
  drives the `branchbox` CLI directly through its JSON output. `scripts/generate-swift-protos.sh` is removed.
- `macos/.swiftpm/` is no longer tracked; it is now ignored.
- `scripts/package-macos-app.sh` is rewritten. It builds a universal (arm64 and x86_64) `BranchBox.app`
  in `macos/build/` (was `BranchBoxApp.app`) with the Cargo workspace version, the git build number and
  commit, bundle ID `dev.branchbox.app`, macOS 26 or later and the app icon, signs it with the hardened
  runtime (ad hoc unless `--sign IDENTITY`) and fails unless `codesign --verify --deep --strict`, the
  architectures and the version check pass. `--zip` adds `BranchBox-<version>-<build>-<sha>.zip` and its
  `.sha256`; `--native`, `--configuration debug`, `--out DIR`, `--scratch-path` and `--jobs` are also
  available. It no longer builds the Rust CLI: the app uses the `branchbox` you have installed, and
  `--embed-cli PATH` bundles a prebuilt one at `Contents/Helpers/branchbox` instead of
  `Contents/Resources/bin/branchbox`. `--notarize` is not available yet and says what it needs (a
  Developer ID identity and `NOTARY_PROFILE`).
- `scripts/macos-dev.sh` builds `BranchBox Dev.app` from the same Info.plist template as the release app,
  so it carries the workspace version, git build number and commit, and the app icon.

### Added

#### CLI and core

- `branchbox version --json` prints `{"version", "contract_version", "capabilities"}`, so tools
  can tell which machine-readable features this CLI supports (`json-error-envelope`,
  `registry-lock` and `write-ahead-start` so far). `branchbox version` prints the same text as
  `branchbox --version`.
- In `--json` mode a failing command prints an error envelope on stdout,
  `{"schema_version": 1, "error": {"code", "message", "causes", "details"}}`, with a stable
  `code` such as `not_a_git_repository`, `worktree_not_found`, `feature_not_found`,
  `teardown_refused`, `registry_locked` or `agent_unreachable`. stderr keeps the same `Error: …`
  report, and the exit code is unchanged. Invalid arguments such as `feature list --status
  bogus` report `validation_failed`. A panic prints an `internal_panic` envelope and still
  exits 101. Commands whose JSON payload already reports the failure (`feature exec --json`,
  `devcontainer up/down/build/exec --json`) print only that payload.
- `feature teardown --discard-changes` discards the worktree's uncommitted changes without
  force-deleting the branch.
- `feature teardown --dry-run` prints the teardown plan and changes nothing; with `--json` it is
  the plan document: the worktree and its lock, the user changes (path, kind, area), the
  BranchBox-generated files and the rule that recognized each, the spec teardown moves to the
  main worktree, the branch with its source, merge state and action, the project's teardown
  defaults, and the blockers (`uncommitted_changes`, `unmerged_branch`, `worktree_locked`,
  `status_unavailable`, `not_a_worktree`, `spec_not_preserved`), each with a cause-naming message
  and the flag that overrides it. It accepts the same flags as a real teardown and exits 0 for
  any valid feature name (a missing worktree is a plan with `worktree.exists: false`; the text
  output says that a real teardown needs `--force`).
- `feature teardown --json` summaries gain `branch_action` (`keep`, `delete`, `force_delete`),
  `branch_delete_error`, `discarded_changes`, `preserved` (the spec moved to the main worktree)
  and `registry_updated`.
- `prune --dry-run --json` lists each candidate with its plan and an `at_risk` summary of the
  uncommitted changes and unmerged commits the forced prune would destroy; `prune --yes --json`
  reports each feature's result (`removed` or `failed`, with its summary or error) and exits 1
  if any failed. `--feature <name>` (repeatable) limits prune to the named features. The text
  listing notes what each feature would lose.
- `branchbox version --json` lists `teardown-plan`, `teardown-discard-changes`,
  `teardown-unmerged-preflight` and `prune-json`.
- `branchbox detect --json` prints `{"schema_version": 1, "project", "git_repository",
  "initialized", "stack", "adapter", "modules", "has_devcontainer", "has_env", "warnings"}`.
  `stack` and `adapter` are lowercase ids (`rust`, `nodejs`, `generic`), and `initialized` means
  the repository's main worktree has `.branchbox/registry.json`. The text output is unchanged;
  a project folder that does not exist is now refused instead of being reported as `Generic`.
- `branchbox devcontainer sync --json` reports every worktree it looked at:
  `{"schema_version": 1, "dry_run", "strategy", "results": [{"work_feature", "worktree_path",
  "status": "synced|would_sync|skipped|failed", "files", "skip_reason", "error",
  "registry_updated"}], "synced", "failed", "skipped"}`. `--feature NAME` (repeatable) syncs only
  the named features, in any status but `removed`; an unknown or removed name is refused with
  `feature_not_found` before anything is synced. Without a `.devcontainer` in the main worktree,
  a sync now fails with `devcontainer_source_missing` naming the missing path.
- `branchbox tunnel credentials set --account-id ID --api-token-stdin [--repo R] [--json]` stores
  the Cloudflare API token, read from standard input (or a hidden prompt on a terminal) and never
  accepted as an argument, in `.branchbox/secure/cloudflared.env`, then points
  `tunnel.providers.cloudflared` at it (`account_id`, `api_token_path`,
  `manual_instructions: false`) so tunnels provision automatically. Other lines of the file and
  every other key, and the formatting, of `.branchbox/config.json` are kept. An empty or
  whitespace token, an invalid account ID, or a `config.json` with comments is refused with
  nothing changed. The config change goes through the same checks as `branchbox config`.
  `--clear` removes the stored token and turns manual tunnel instructions back
  on. The token never appears in any output.
- `branchbox doctor [--repo R] [--check-auth] [--json]` checks git, the Docker CLI, daemon and
  Compose, the Dev Container CLI, Docker Sandboxes (including sign-in), the local-vm driver, the
  1Password and GitHub CLIs, whether it runs on the host, and `PATH`; with `--repo`, also the
  repository, its BranchBox setup, `config.json`, `registry.json` and `.gitignore` entries. Each
  check has a status (`ok`, `warn`, `error`, `skipped`), a cause and a remediation. A tool that
  does not answer within 3 s is killed and reported as timed out; a check that runs a tool
  twice (Compose and its `docker-compose` fallback, a CLI's version then its sign-in) shares
  those 3 s, so the whole report takes about 3 s at most. `op` and `gh` sign-in is
  checked only with `--check-auth`. The command exits 1 when a required check fails; with
  `--json` the report is printed either way.
- `branchbox config get [KEY] [--json]`, `config set KEY VALUE`, `config unset KEY` and
  `config apply --file <PATH|-> [--dry-run] [--json]` read and change `.branchbox/config.json`.
  `get --json` prints `{"schema_version": 1, "path", "exists", "effective", "file", "keys"}`, where
  each key row has its `type`, `allowed` values, `default`, effective `value`, `source`
  (`file` or `default`) and `description`. `apply` takes an RFC 7386 JSON merge patch (`null`
  unsets a key) and prints `{"schema_version": 1, "changed": [{"key", "old", "new"}],
  "effective"}`. Values are checked before anything is written: an unknown key is refused with
  `config_unknown_key`, an invalid value with `config_invalid` naming the key and the accepted
  values (`feature.branch_prefix` must make `<prefix>/<name>` a valid git branch). Edits keep the
  file's formatting, permissions and unknown keys, are written atomically under the `.branchbox`
  lock, and refuse a file with comments, naming the line and column. The supported keys are
  documented in the new configuration reference (`docs/docs/reference/configuration.md`),
  generated from the same key registry.
- `branchbox init --json` prints `{"schema_version": 1, "workspace_path", "repository_state":
  {"kind", …}, "reorganized", "stack", "adapter", "modules", "devcontainer_status": {"kind", …},
  "registry_initialized", "onepassword": {"status": "configured|skipped|not_configured"},
  "warnings", "next_steps"}` and never prompts (it implies `--yes`); progress goes to stderr.
- `branchbox init --op-github-ref op://… [--op-signing-key-ref op://…]` records the 1Password
  references in `.devcontainer/.env` without the interactive questions. Each reference is
  checked with `op read` before init changes anything, and an unreadable one is refused naming
  the flag, the reference and the `op` error; `--no-verify-op-refs` skips the check.
  `--skip-1password` records that the project does not use 1Password.
- `branchbox version --json` lists the `detect-json`, `devcontainer-sync-json`, `config`,
  `tunnel-credentials`, `doctor` and `init-json` capabilities.

#### macOS app

- The macOS app reads the CLI's version and capabilities from `branchbox version --json`, and from
  `branchbox --version` on 0.13.x, and refuses CLIs older than 0.13.4 with a message naming the version it
  found. The result is cached per CLI binary and re-checked when the binary changes.
- The macOS app lists worktrees that are in BranchBox's layout but missing from the registry, for example after
  an interrupted start, as "Unregistered worktree", and can remove them; a dirty one only after you confirm
  discarding its changes. Your own worktrees elsewhere, or on other branches, are never listed.
- The app's doctor checks git, the Docker CLI and daemon, Docker Compose, the Dev Container CLI, Docker Sandboxes
  (`sbx ls`, including sign-in), the 1Password CLI and the GitHub CLI, with a 5 s limit each. With CLI 0.14 it
  is merged with `branchbox doctor --json`.
- "Copy as Command" gives a shell-ready command line, with `--prompt` text, your extra environment values and
  tokens redacted.
- The macOS app keeps your projects in `~/Library/Application Support/BranchBox/projects.json`, pinned ones first,
  then the most recently opened. Adding a feature worktree adds its main worktree instead and says so, as does
  adding a folder that holds BranchBox worktrees; the same repository is never added twice, even through a symlink.
  A project whose folder is gone stays listed so you can locate or remove it, and removing a project never touches
  its files.
- Projects refresh by themselves:
  - when `.branchbox/registry.json` changes on disk, whichever way the CLI wrote it, also while the app is in the
    background;
  - when the app becomes active and the data is more than 5 seconds old;
  - every minute for the selected project while the app is active, and every 5 minutes for all projects (both
    configurable, or off);
  - after every operation, including failed and cancelled ones.

  A refresh requested while one is running waits for one more pass instead of restarting it, a failed refresh
  keeps showing the last good list, and at most two `feature list` processes run at a time.
- Operations queue instead of colliding. A feature runs one change at a time; a second one is refused with the name
  of the one in progress. Prune, Update All Workspaces, Set Up, project settings and tunnel credentials wait for
  the project's other changes, and later changes wait for them. With a CLI older than 0.14 (no registry lock),
  every change to a project's registry runs in the order you started it, shown as "Waiting for …". Running a
  command in a feature is never blocked.
- Prune tears the selected features down one at a time, exactly as selected. A refused teardown is recorded and
  skipped, and Stop ends the prune before the next feature.
- Each operation's full log is written to `~/Library/Logs/BranchBox/operations/` (the newest 100 are kept, as set
  in Settings), with Settings' extra environment values and tunnel tokens redacted. The last 100 operations are
  remembered across launches.
- An operation that fails, or takes longer than 10 seconds, while you are not looking at BranchBox posts a
  notification ("oauth is ready", "Couldn't start oauth", "Teardown of oauth needs attention"), as Settings allow.
  A start or teardown stopped on a CLI without the registry lock says what it may have left behind.
- The macOS app has a UI-free planning layer (`BranchBoxKit/Planning`) that decides what the app offers
  before anything runs, with table-driven tests:
  - Tear Down never discards changes or forces removal on the first attempt. When the CLI or the app's
    preflight refuses because of uncommitted changes, the app offers "Discard N changes and tear down…",
    which asks for confirmation listing those files, and tears down only if the uncommitted changes are
    still exactly those files (new changes stop it again). An unmerged branch is kept by default, even when the project config deletes merged branches;
    Force-delete is never preselected, and "Delete if merged" on an unmerged branch is blocked with the
    commit count.
  - Every refusal and partial failure gets only the recoveries that match it: Keep or Force-delete for an
    unmerged branch, forced removal (always keeping the branch) for a locked or unreadable worktree, Reuse
    for a folder that already exists, `sbx login` in Terminal for a Docker Sandbox that needs signing in,
    and Locate or a Homebrew command for a missing or outdated CLI. Other failures offer the log and the
    Diagnostics window.
  - Prune preselects only features whose teardown loses nothing and explains every row it leaves out (for
    example uncommitted changes, a locked worktree, or a setup that is still running).
  - Start Feature applies the CLI's own naming rules: a title becomes the slug the CLI would choose, and
    only that slug is sent. It blocks prompts over 2,000 characters (counted as the CLI counts them), a
    name or folder that is already in use, and "keep the sandbox on failure" for runtimes other than
    Docker Sandboxes.
  - Health remediation for interrupted setups, failed modules, a missing folder, degraded, retained,
    orphaned and unrecognised statuses, with the callout text for each. Resume, Recreate and Re-run Setup
    are offered only for runtimes the app can start, and Retry with the kept runtime only for Docker
    Sandboxes. A start that is still running is never offered a cleanup. Feature names starting with "-"
    are refused.
  - Opening a feature in VS Code, Cursor or another editor, in its dev container, in Terminal or iTerm, or
    with a coding agent (the project's `editor.default_agent`, then the app setting) with the feature's
    prompt. Terminal scripts single-quote every path and prompt, and the built-in agents get the prompt
    after `--`. The project's `editor.default_agent` comes from the repository, so it is only ever an agent
    name (`claude`, `codex`, or one bare executable name); anything else is ignored in favour of the app
    setting and never runs as shell text. Terminals and agents for Docker Sandboxes
    are disabled for now and offer `sbx exec <id> bash` to copy instead, and every host action is disabled
    while the feature's folder is missing.
- The macOS app has a shared SwiftUI kit for the screens that follow: status, runtime and attention
  badges that never rely on colour alone, colour swatches, the module checklist, port links, result
  cards with confirmed destructive recoveries, error banners, a live log view (level icons, timestamps,
  Find, a warnings filter and auto-scroll that pauses when you scroll up), operation rows and progress
  views whose Stop confirmation warns about partial worktrees and registry damage on older CLIs, empty
  states, and a copy button with a VoiceOver announcement.
- "Copy diagnostic report" produces redacted Markdown with the app version and Git SHA, the CLI's path,
  source, version and capabilities, the child PATH, the redacted command line, the exit status and the
  last 50 stderr lines. Tokens, prompts, extra environment values and the home folder are removed.
- The Mac app now runs on the BranchBox CLI you have installed. It finds `branchbox` on your
  login-shell PATH or in the usual Homebrew and Cargo folders (or where Settings points it), and
  lists, starts and tears down features through it.
- A new main window: a sidebar with one group per project, listing features with their colour,
  status (or what needs attention), runtime, a "Quick" tag and a spinner while an operation runs.
  Unregistered worktrees, starts in progress, loading, empty and failed-refresh states have rows
  of their own, and "Show removed" lists torn-down features. Double-click opens a feature in your
  editor; search filters every project. The selection is remembered per window.
- When the CLI is missing, too old or unusable, the window says so and offers Locate…, the
  Homebrew command and Re-detect. A 0.13.x CLI gets a dismissible banner naming what needs a
  newer one.
- Menus and shortcuts for every feature and project action (⌘N Start Feature, ⌘O Add Project,
  ⌘R Refresh, ⌘K Quick Open, ⌘⌫ Tear Down, ⌃⌘E/T/A/O/R, ⌥⌘R, ⌥⌘C, ⇧⌥⌘C, ⌥⌘I, ⇧⌘., ⌥⌘L).
  Unavailable commands are disabled and their help tag says why.
- Quick Open (⌘K): search features, projects and commands and act on them from the keyboard.
- A menu bar menu with a status summary, recent activity, each project's features and their
  actions, and Start Feature. Its icon shows a dot while work runs, a badge and count when
  something needs attention, and a mark when the CLI is unavailable.
- Notifications when an operation finishes while you are away (bundled app only), with Show and
  Open in Editor actions. While BranchBox is in front they appear as a message in the window.
- `scripts/macos-dev.sh` builds a signed `BranchBox Dev.app` for development: run it in the
  foreground, `--open` it like Finder does (`--background` keeps it behind other apps), or
  `--build-only` to print the bundle path. `--env K=V` and `--preview <scenario>` pass settings
  to the app.
- The macOS app's feature detail is rebuilt around what you do with a feature:
  - A header with the feature's colour, name, status (and derived attention such as "Interrupted" or
    "Folder missing"), runtime, Quick mode and "branch from base · created …", with the everyday actions
    next to it: Open in your editor (a split button with VS Code, Cursor and Open in Dev Container),
    Terminal, Launch Agent (the project's `editor.default_agent` or your App Settings agent) and Open URL.
  - A health callout explains what happened and offers exactly the fixes BranchBox can run: Resume Setup
    for an interrupted start, Retry for a kept or stopped sandbox (plus the `sbx exec` inspect command),
    Recreate Runtime for an orphaned feature, Clean Up for a missing folder (keeps the branch), Re-run Setup
    for failed setup steps, Run Doctor for an unknown status, and Tear Down. None of them discards changes;
    the buttons wait while another operation runs on the feature.
  - Cards for links (feature URL, tunnel and published ports open in the browser; addresses that only work
    inside the container are copy-only), the dev container (Running / Stopped / Not created, Start, Stop,
    Stop and Delete Volumes…, Rebuild…, Open Shell, "Config out of date" → Update All Workspaces…),
    sharing (Share via Tunnel, Stop Sharing… and, when the provider refuses, Remove Anyway… behind a second
    confirmation; manual setup steps as a numbered list), overview, coding agent and prompt (Show All, Copy,
    Launch with Prompt), setup checklist, runtime, adapter warnings and pull request. They sit in two
    columns when the window is wide enough.
  - Torn-down features are read-only and offer Delete Branch… while the branch still exists.
- One feature actions menu is shared by the sidebar, the menu bar and the toolbar's More menu. Disabled
  items say why (for example a missing folder, or a Docker Sandbox shell, which offers Copy Shell Command).
- The Run Command window runs a command in a feature's runtime or its running dev container, through
  `/bin/sh -lc` by default, with per-feature history and per-project Quick Commands. It shows output and
  errors, the exit code and the duration; a non-zero exit is a result, not an error.
- The macOS app's Start Feature sheet derives the feature's name, branch and folder from the title as you type
  (checked with the CLI 250 ms after the last keystroke), with Edit Name to choose the slug yourself. Only the
  resolved slug is sent, never the title. It blocks a name or folder already in use and prompts over 2,000
  characters, and notes a branch that already exists or a removed feature with the same name. It offers a
  searchable base branch (Current HEAD by default), Container with Docker's state, Docker Sandboxes when
  installed (with Sign In… when signed out), Local VM as Linux-only, Full or Quick setup, recent prompts and
  the default prompt (Quick only). Advanced options (branch prefix, skipped modules, reuse, keep the sandbox on
  failure, verbose logs) are remembered per project once a start runs; cancelling remembers nothing.
- A start runs in the sheet with its live log, [Run in Background] and [Stop…]. The result shows the name
  the CLI actually used, the setup checklist, skipped modules, links and ports, and every warning, with a
  stash warning called out; [Open in Editor] is the default action, and the coding agent launches on its own
  when the project's config asks for it. A failed start shows its cause with [Edit and Retry], and
  [Show Feature] when it left a registry entry.
- The Teardown sheet says what will happen before anything runs: your uncommitted changes ("will be
  permanently deleted"), BranchBox-generated files, the spec that is kept, unmerged commits, the runtime and
  tunnel that go with it, and a folder that is already gone. The first attempt never discards anything; a
  refusal shows in the same sheet with "Discard N changes and tear down…", confirmed with the file list.
  Force-delete asks first with the commit count. The result says whether the worktree is gone (checked on
  disk), what happened to the branch (with [Force-Delete Branch…] only for an unmerged one), whether the
  runtime cleanup was verified or left resources behind (with [Copy Cleanup Commands], copied and never run),
  module reports, warnings, and a red flag when CLI 0.13 deleted the folder by hand.
- The Prune sheet checks every feature for unsaved work (four at a time), preselects the safe ones and says
  why each other row is left out, with [Select Safe], [All] and [None] and a branch policy (Keep, Delete if
  merged, Force-delete after a confirmation listing the branches). Checking a feature with uncommitted
  changes asks first and discards only the files it listed. Features are torn down one after another; a
  refused one is skipped and reported ("2 torn down · 0 partial · 1 refused · 0 failed") with its own
  recoveries. "Nothing to prune" when every feature is gone.
- The Unregistered Worktree sheet shows the folder, branch and commit with [Reveal], [Open in Terminal] and
  [Remove Worktree…]. A worktree with uncommitted changes is refused and offers to discard exactly those
  files; the branch can be deleted afterwards when it is merged.
- Activity: the main window's inspector lists the selected feature's or project's operations with their live
  logs; the Activity window lists every operation (this session's and earlier ones) with project and state
  filters, [Reveal Log File] and [Copy Diagnostic Report]; the toolbar popover shows what is running and the
  latest results. Viewing a failed or partial operation clears its attention badge. Stopping an operation
  always asks first, and on a CLI without registry locking the question warns about a partial worktree and a
  damaged registry.
- BranchBox for Mac has a Welcome checklist for first launch: install the branchbox tool (with the
  Homebrew command, Locate… and Check Again), check Git, Docker and the optional tools (each problem
  offers its fix: Open Docker Desktop, Sign In to Docker Sandboxes, copy an install command, allow
  notifications), then add the first project. It is reachable again from Diagnostics and Settings.
- Add Project accepts a folder from an open panel or a drop. A feature folder or a container folder
  adds the project's main folder and says so; a repository without BranchBox offers Set Up
  BranchBox…; a folder that is not a Git repository explains why.
- Set Up BranchBox runs `branchbox init -y` from a form: project type (prefilled from detection),
  dev container, `.env` and coding-agent support, tunnels (off by default), 1Password references,
  and where the repository lives. The repository stays where it is unless you pick "Move it into a
  parent folder" and confirm. Preview shows the dry run's log; the result lists the stack, modules,
  warnings and next steps with Start Your First Feature…. Repair and Check Setup use `--update` and
  `--validate`.
- Project detail shows the project's path, detection chips, default runtime and branch prefix,
  status counts and a sortable feature table, with Start Feature, Prune, Update All Workspaces,
  Project Settings, Repair, Check Setup and Remove from Sidebar (which never deletes files).
- Update All Workspaces previews and then copies (or links) main's dev container setup to every
  active feature, with a result row per feature; a failed feature is shown even when the CLI exits 0.
- Project Settings edits `.branchbox/config.json` through `branchbox config apply`: Features,
  Teardown, Runtime, Sharing and Coding Agent tabs built from the CLI's key table, a review of the
  changes before applying, and errors shown next to the setting the CLI rejected. The Cloudflare API
  token goes to `branchbox tunnel credentials set` on stdin and is never stored by the app. With
  CLI 0.13.x the settings are read-only, with Open config.json.
- App Settings has General, Tools (which CLI is used, Locate…/Use Automatic/Check Again, the shell
  PATH with Re-capture, extra environment variables), Editors & Terminal, Coding Agent,
  Notifications, Refresh and Advanced tabs. Changes take effect without relaunching.
- The Diagnostics window lists the CLI (path, version, contract version, capabilities, skipped
  copies), the shell environment, every tool check with its fix, the runtimes, each project's health
  and the recent operations, with Run Checks Again, Copy Report (redacted) and Show Logs in Finder.
- The Mac app has an icon (the BranchBox logo, from `macos/Packaging/AppIcon.iconset`;
  `macos/Packaging/make-iconset.sh` regenerates it from `assets/icons`).
- `scripts/macos-capture-fixtures.sh <cli> <outdir>` records the JSON a `branchbox` CLI prints for the
  commands the app runs (version, list, start, exec, teardown plan and teardown, a missing feature), from a
  throwaway git repo, with paths scrubbed and a `manifest.json` describing each file.
- macOS app CI runs the integration suites against the CLI built from the same commit and against the
  0.13.4 release (the oldest CLI the app supports), decodes freshly captured CLI output, and uploads a
  universal, ad-hoc-signed `BranchBox-macOS-<sha>` app zip with every run (kept for 14 days).
- The app's integration suites (`BRANCHBOX_IT=1`) now cover the whole lifecycle against a real CLI in both
  legacy (0.13.x) and contract mode: exec exit codes, the dirty-teardown refusal and its discard recovery,
  each branch policy and a custom branch prefix, duplicate starts, the generated-files recovery, a 40-feature
  registry (over 64 KiB of JSON, 20 concurrent readers), cancelling a start (stray removal, and Interrupted →
  Resume on contract CLIs), concurrent registry writers, prune planning, config round trips and the registry
  watcher. Every Rust golden fixture in `cli/tests/fixtures/contract/` is decoded by the app's models in the
  default test run, and `LiveFixtureDecodeTests` reports keys a CLI prints that the app does not read.
  `macos/TESTING.md` records the latest verification run and the manual Mac App ↔ CLI loop.

#### Documentation

- A JSON contract reference (`docs/docs/reference/json-contract.md`) documents machine mode, the
  error envelope, the stable error codes, exit codes, capabilities and every `--json` payload.
- The CLI reference is regenerated from the `--help` of every subcommand, including `version`,
  `doctor`, `config` and `tunnel credentials`, with notes on teardown safety and the `--force`
  semantics.
- `macos/README.md` is rewritten for the CLI-backed app (requirements, how the CLI is found,
  dev loops, packaging, installing a CI build past Gatekeeper, troubleshooting), and the manual
  E2E guide's "Mac App ↔ CLI Loop" replaces the old agent loop.

## [0.13.4] - 2026-09-10

### Fixed

- Create the in-guest tool-request replay ledger with its owner-only mode in one step. Two
  dispatchers opening the ledger at the same moment could have the second find the directory
  before the first had set its permissions, and refuse it as not owner-only.
- Build on non-Unix targets again: the consumer-readable check on a `provider-credential`
  source reads Unix file modes and is now gated to Unix like the private-file check beside it.

## [0.13.3] - 2026-09-09

Consolidates the same-day 0.13.0, 0.13.1, 0.13.2 and 0.13.3 releases.

### Fixed

- A `provider-credential` source is admitted and inspected as readable by its consumer (0644)
  rather than private to the runtime: a bind preserves the source's owner and mode, and the
  provider runs inside the container as a different user, so a 0600 credential was delivered
  unreadable. The file still lives in the run's owner-only materializations directory.
- Inspect a `provider-credential` source as the private regular file it was admitted as. The
  post-start inspection classified only directories and sockets, so an admitted credential file
  was reported as "source type changed after assignment validation" and the run failed after
  the devcontainer had started.

### Added

- Add a `provider-credential` lease scope that binds one signed credential file at the path its
  provider reads, rather than below the lease root every other file target is pinned to. The file
  stays read-only and digest-bound and is inspected on the running container like any other signed
  bind; only the directory holding it belongs to the container. This is what a provider needs when
  it keeps state beside its credential: Codex authenticates from `auth.json` in `CODEX_HOME` and
  opens a sqlite state database in the same directory, so a credential home delivered as a
  read-only directory let it read its credential and then fail to start.
- Add an Agentify-oriented `in-guest` runtime that reconciles a BranchBox worktree and explicit
  devcontainer facade inside an already-owned Firecracker guest, validates opaque lease file paths
  and digests, publishes loopback-only ports, exposes correlated container/runtime identity, and
  returns deterministic teardown residue evidence.
- Add `feature exec-provider` for the fixed Codex executable with name-only
  `OPENAI_API_KEY` inheritance into the configured devcontainer user/workspace.
- Deliver one digest-verified, canonical project-environment materialization only to the primary
  Compose service through a raw env file, without mounting or serializing its values.
- Add run-owned shared-directory and Unix tool-endpoint leases with exact read-only primary-container
  bind evidence and residue-checked directory/socket cleanup.
- Add provider-neutral `tool-request` leases and `feature dispatch-tool`: coding-container users can
  submit bounded capability-bound requests through a per-run volume while the owner-only trusted
  Unix endpoint and its underlying credentials remain outside the container.
- Bind the Firecracker kernel config into the image manifest, require built-in virtio-vsock
  support, and prove a trusted guest-to-host transfer without exposing `/dev/vsock` to coding
  devcontainers.
- Publish `vmlinux`, `kernel.config`, `rootfs.tar.gz`, and `manifest.json` as a portable, versioned
  local-VM guest-base contract with built-in legacy IPv4 xtables support for chain, conntrack,
  UID-owner, TCP-reset rejection, bridge, and NAT policy. CI verifies every published digest and the
  complete fixed kernel minimum before upload; a disposable trusted network namespace proves the
  rules without granting `NET_ADMIN` to coding devcontainers.

### Security

- Allow managed in-guest assignments to bind every runnable Compose service to a preloaded,
  digest-pinned image; the generated facade removes repository builds, disables pulls and
  devcontainer image derivation, verifies each exact image locally, and fails closed when a binding
  or preloaded image is missing. Published ports in this mode also require an immutable preloaded
  proxy image launched outside Compose with Docker pulls disabled.
- Force the managed in-guest primary devcontainer onto Docker's built-in seccomp profile so direct
  `AF_VSOCK` access is denied while signed shared-directory leases remain portable to non-root
  container users.
- Keep linked trusted-tool sockets at strict owner-only permissions across guest/container UID
  mismatches; exact run/lease/consumer binding, immutable spool descriptors, single-use replay
  claims, bounded relay framing, and residue-checked teardown replace direct socket mounts.
- Restore a non-writable `0022` file-creation mask for every in-guest runtime and managed-provider
  command, even when the nested container exec implementation supplies an unsafe ambient mask.
- The in-guest facade runs before host-side repository lifecycle behavior, disables checkout hooks
  and filter drivers, strips ambient host env/mount authority plus outside/in/from-Docker feature
  aliases, replaces repository volume and port publication declarations with the exact canonical
  task-worktree/Git projection and primary-container-only signed loopback proxies, starts only the
  primary service and its validated dependency closure, omits repository tunnel sidecars, rejects dangerous
  Compose/build/extends/secondary-service authority and local or remote
  Docker/containerd/Podman/BuildKit endpoints, and verifies the resolved and running container have
  no supervisor socket, credential-directory mount, privileged host namespace/device/capability,
  disabled confinement, persisted daemon endpoint, or model key.

### Fixed

- Preserve Dev Container environment semantics across image, Dockerfile, and Compose runtimes:
  `containerEnv` is applied at container creation, configured plus explicit `remoteEnv` reaches only
  lifecycle and exec processes, concurrent exec arguments stay invocation-local, and Docker
  diagnostics no longer include environment values. Existing containers whose static environment
  no longer matches now require an explicit recreate. Public container labels bind only canonical
  environment variable names, never value-derived digests that could expose low-entropy secrets to
  offline guessing.

- Make trusted `tool-request` delivery recoverable across staged-read, relay-response, and consumer
  acknowledgement loss when its signed provider-neutral lease opts into
  `exact-digest-replay-v1`: durable capability-stripped request fingerprints, serialized attempts,
  cached correlated responses, and exact-only retries prevent duplicate endpoint effects while
  existing manifests continue to deny replay by default.

- The portable Firecracker netfilter proof now exercises conntrack and UID-owner matching as
  independent rules, with the owner-scoped rule in a user-defined chain attached to `OUTPUT`.
  Consumers can compose exact destination policy without relying on a non-portable combined
  conntrack/owner match.
- Compose-backed in-guest devcontainers now receive the mandatory seccomp option from one generated
  facade only, avoiding duplicate `security_opt` entries from Dev Containers CLI overlays.
- In-guest devcontainer failures now carry stable content-free diagnostic codes for orchestration
  feedback without exposing repository output or credential-bearing startup logs.
- Active in-guest worktrees now use BranchBox's validated `/workspaces/main` Git projection without
  repository-specific hooks; restoration and normal teardown resolve Git's shared common directory
  when BranchBox itself is invoked from a linked worktree.
- In-guest failed starts now pre-record and recover Dev Containers/Compose ownership, remove
  dependency-only containers, networks, volumes, materializations, worktrees, and task branches,
  and bypass repository modules/adapters during no-registry cleanup.
- In-guest coding containers now receive a private, bounded 1 GiB shared-memory allocation while
  repository shared-memory overrides and host IPC remain disabled.
- Release changelog generation now authenticates git-cliff GitHub API requests with the scoped
  workflow token, avoiding anonymous API rate-limit failures during publication.

## [0.12.1] - 2026-08-20

### Fixed

- CLI diagnostics now use stderr so `--json` stdout remains valid machine-readable JSON even when
  `RUST_LOG` enables warnings or informational events.

## [0.12.0] - 2026-08-20

### Added

- SBX failed-start diagnostics can retain the sandbox and nested build cache with
  `--keep-runtime-on-failure`, retry it with `--reuse-runtime`, report retained/degraded/orphaned
  health in `feature list`, and remove retained runtimes through normal teardown/prune lifecycle.
- The account-free `local-vm` provider now runs repository devcontainers and Compose stacks inside
  fresh jailed Firecracker VMs on x86_64 Linux/KVM, including captured/interactive execution,
  collision-safe ports and TAP networks, explicit scoped credential injection, digest-addressed
  guest artifacts, workspace synchronization, restricted host/private-network access, and
  deterministic teardown.

### Fixed

- SBX execution now reconciles the devcontainer after sandbox resume, refreshes port proxies when
  the container identity changes, and runs commands through the devcontainer user's login shell so
  Mise/asdf/nvm-managed tools receive the same environment as an interactive terminal.
- Copy-mode `feature start --reuse` now detects feature-local devcontainer divergence against the
  last BranchBox sync baseline and requires an explicit fail/preserve/overwrite/inspect decision.
- SBX can exclude incompatible optional Compose services through `runtime.sbx.run_services`,
  preflights `/dev/net/tun` usage, and keeps required Compose dependencies intact.
- SBX feature startup now provisions a required Cloudflare tunnel environment before creating the
  sandbox or starting the devcontainer, and fails its credential preflight before an expensive
  sandbox build when that required environment cannot be populated.
- Worktree pointer repair now derives and validates the relative target from authoritative Git
  metadata instead of trusting an unrelated sibling directory named `main`.

### Security

- SBX devcontainer startup failures now discard rendered Compose configuration, structurally redact environment assignments and expanded values, and return only a bounded actionable error tail with the exit status. This prevents arbitrary env-file credentials from reaching terminal, JSON, agent, CI, or telemetry logs.

- Upgrade the agent gRPC stack to remove the `h2` version affected by
  RUSTSEC-2026-0258.

## [0.11.1] - 2026-08-12

### Fixed
- `branchbox feature teardown` now discovers the Compose project created by the devcontainer CLI from the worktree's exact Docker label, restores BranchBox's persisted Compose identity across CLI processes, removes containers, networks, and volumes, and reports an error if owned resources remain.

## [0.11.0] - 2026-08-12

### Added
- Pluggable workspace runtime providers with the existing account-free container workflow as the default and experimental Docker SBX support for microVM-isolated workspaces.
- `branchbox feature start --runtime <container|sbx|local-vm>` runtime selection through the CLI and `.branchbox/config.json` defaults.
- `branchbox feature exec` for captured or interactive command execution through the runtime recorded for an active feature.
- Runtime provider, runtime identity, and resolved host-port mappings in feature registry records, CLI summaries, JSON output, and agent IPC payloads.

### Changed
- Docker SBX workspaces now start the repository devcontainer inside the sandbox, execute coding agents in that devcontainer, bridge nested Compose services to collision-safe host ports, recover mappings after sandbox restart, and remove provider-owned state during teardown.
- Worktree `.git` pointers are restored to portable relative paths after repository lifecycle hooks run inside a devcontainer.

### Fixed
- Upgraded the shared HTTP client to reqwest 0.12 and patched Rustls dependencies, resolving the `rustls-webpki` certificate-validation and CRL vulnerabilities reported by the repository security audit.
- Fixed the agent E2E harness on macOS Bash 3 when `--cp-stub` is used without additional CLI arguments.

### Documentation
- Documented the distinction between devcontainers and outer isolation runtimes, experimental SBX requirements, runtime configuration and command execution, and the account-free local VM direction using Colima/Lima.

### Testing
- Added a fake-SBX full CLI lifecycle test covering provisioning, port publication, devcontainer startup, agent execution, and teardown.
- Verified a real Agentify Compose/devcontainer stack inside Docker SBX, including Codex execution, Rails/PostgreSQL access, host-port bridging, restart/reuse, and cleanup.
- Kept the disposable multi-feature CLI and agent E2E harness portable across macOS Bash and stack-specific Compose service names.

## [0.10.1] - 2026-03-19

### Fixed
- `branchbox feature teardown` and `branchbox feature prune` now accept `--allow-container` (alias `--no-host-check`), matching `feature start`. Previously, features started inside a container could not be torn down from the same environment.

## [0.10.0] - 2026-03-16

### Added
- `branchbox feature start --allow-container` (alias `--no-host-check`) allows running feature start from inside a containerized environment (e.g., devcontainers with Docker socket mounted), enabling programmatic workspace provisioning for coding agent orchestration (#65).

## [0.9.3] - 2026-03-05

### Added
- `branchbox prune` and `branchbox feature prune` to tear down all active feature worktrees in one command, with `--dry-run` and `--yes` support for safe automation.
- Added a `features` alias for the `feature` command group so `branchbox features list` works.

### Changed
- Installer now creates a `bb` alias symlink to `branchbox` and warns when another `bb` command already exists in `PATH`.
- Release packaging now ships `bb`/`bb.exe` alongside `branchbox`, and Homebrew formula automation updates install lines to include both binaries.

## [0.9.0] - 2026-02-27

### Added
- `branchbox init` now interactively prompts for 1Password credential references (`OP_GITHUB_REF`, `OP_SIGNING_KEY_REF`) and persists them to `.devcontainer/.env`, eliminating the need to manually export environment variables.
- References are validated live via `op read` during setup; users can re-run `branchbox init --update` to reconfigure.
- `init-host.sh` now sources persisted references from `.devcontainer/.env` with automatic fallback to the main worktree's copy for feature worktrees.
- BranchBox's own devcontainer now includes the 1Password bootstrap flow (`initializeCommand`, `postStartCommand`, secret mounts) matching the stack templates.
- `GITHUB_TOKEN` is now injected via compose `env_file` so it is available to all container processes, not just interactive login shells.

### Fixed
- Pre-built devcontainer images for Rails and Node.js no longer have a broken `PATH` caused by `${containerEnv:PATH}` being baked literally into the image instead of resolved at runtime (#58).
- `write_op_env` now preserves existing non-BranchBox keys in `.devcontainer/.env` instead of truncating the file.
- Worktree fallback path for 1Password references now correctly resolves the main worktree name from `BRANCHBOX_MAIN_NAME` in `.branchbox.env`.
- Windows compilation fixed: Unix-specific file permission code gated behind `#[cfg(unix)]`.

## [0.8.0] - 2026-02-17

### Added
- `branchbox init` now scaffolds 1Password bootstrap assets in `.devcontainer/` (`scripts/init-host.sh`, `scripts/setup-git.sh`, `.github-token.env`, `.git-signing-key`, `.gitconfig.env`) and wires each stack template to run them during devcontainer startup.
- Compose templates for Rust, Generic, Rails, and Node now mount the generated 1Password credential files into the container.
- Added a focused manual 1Password regression harness (`scripts/manual-1password-e2e.sh`) plus runbook (`scripts/manual-1password-e2e.md`) to validate PAT + SSH-signing setup end-to-end.
- Added `scripts/review-preflight.sh` plus CI wiring to enforce security/sanitizer/harness-doc-sync guardrails before deeper test jobs run.

### Changed
- Devcontainer compose templates no longer pin top-level compose project names or `container_name`, preventing collisions across parallel worktrees.

### Fixed
- `branchbox feature start` now consistently derives `COMPOSE_PROJECT_NAME` / `DEVCONTAINER_NAME` from app slug + feature name (including when the source repo has no `.env`), while still writing `.devcontainer/.branchbox.env`.
- Feature-start stash handling now ignores untracked files and applies the exact stash reference, eliminating false “failed to apply stashed changes” warnings in common workflows.
- 1Password host bootstrap now preserves previously fetched token/signing files when `op read` fails and writes signing keys with owner-only permissions.
- 1Password host bootstrap now surfaces the final `op read` error output after retries so secret-fetch failures are diagnosable in `initializeCommand` logs.
- Devcontainer git credential bootstrapping now stores GitHub credentials via `git credential approve` + `store --file` (no shell helper interpolation of token content).
- Feature/bootstrap file generation now rejects symlink targets for managed writes and uses `O_NOFOLLOW`/file-handle permission hardening on Unix to prevent unintended host file overwrite via malicious repository links.
- Feature env generation now applies context-specific sanitization before writing `.env` files (`APP_URL` keeps URL-safe delimiters and is single-quoted when emitted, `GIT_BRANCH` is allow-listed to env-safe branch characters, and `COMPOSE_PROJECT_NAME` is normalized to Docker Compose-safe lowercase chars), and generated feature env files are written with owner-only permissions.
- VS Code feature URL tasks now use process-style launchers across platforms (`xdg-open`/`open`/`explorer`) instead of `cmd /C start` shell invocation.
- Compose lifecycle operations now fall back from `docker compose` to `docker-compose` when plugin-style compose is unavailable.
- `scripts/manual-cli-e2e.sh` now resolves devcontainer services via `devcontainer read-configuration` first (with JSONC/compose fallbacks), avoiding brittle JSONC parsing.
- `scripts/manual-1password-e2e.sh` now supports `docker compose`/`docker-compose` fallback and resolves devcontainer services via `devcontainer read-configuration` first (with JSONC/compose fallbacks).

### Documentation
- Updated manual E2E docs and release guidance to include the 1Password-specific harness and required environment inputs for issue #45 style validation.
## [0.7.0] - 2026-01-14

### Added
- New devcontainer CLI commands (`branchbox devcontainer up`, `exec`, `down`, `build`) for direct container management without entering the devcontainer environment.
- `.ai-agents/` directory structure for consolidated AI agent configurations (Claude Code, GitHub CLI, Codex) with automatic initialization during bootstrap.
- Release skill for guided version releases with automated quality checks and documentation updates.

### Changed
- AI agent configuration directories (`.claude/`, `.gh/`, `.codex/`) are now organized under `.ai-agents/` for cleaner workspace structure.
- Devcontainer builds for arm64 now only run on pushes to main branch to optimize CI performance.

### Fixed
- Handle empty `SHARED_CONFIG_DIR` environment variable gracefully in devcontainer configurations.
- Ensure `.ai-agents/` directory structure is created before Docker mount operations during bootstrap and devcontainer setup.

### Testing
- Added verification for `.claude.json` mount in feature worktree tests.
- Added comprehensive mount tests for root user scenarios in devcontainer module.

## [0.6.0] - 2026-01-13

### Added
- Official pre-built devcontainer images for all stacks (Rust, Rails, Node.js, Generic) published to GHCR at `ghcr.io/branchbox/branchbox/devcontainer-<stack>:latest`.
- `branchbox init` now generates compose.yaml files that use pre-built images by default with automatic fallback to local Dockerfile builds.
- New environment variables for devcontainer image control: `DEVCONTAINER_IMAGE` (custom image override) and `DEVCONTAINER_PULL_POLICY` (missing/always/build).
- GitHub Actions workflow (`devcontainer-build.yml`) that automatically builds and publishes all stack images when `.devcontainer/` or template Dockerfiles change on `main`.

### Changed
- Rails and Node.js devcontainer templates now use `mcr.microsoft.com/devcontainers/base:debian` with mise for runtime version management, reading `.ruby-version`, `.nvmrc`, `.node-version`, and `.tool-versions` files.
- All stack compose.yaml templates now include `init: true` and `ipc: host` for better container behavior.

### Upgrade Guide for Existing Projects

**New projects** created with `branchbox init` automatically use pre-built images.

**Existing projects** initialized before this release need manual updates to benefit from pre-built images:

1. **Update your compose.yaml** to reference the pre-built image:

   ```yaml
   services:
     your-service:
       image: ${DEVCONTAINER_IMAGE:-ghcr.io/branchbox/branchbox/devcontainer-<stack>:latest}
       build:
         context: ..
         dockerfile: .devcontainer/Dockerfile
       pull_policy: ${DEVCONTAINER_PULL_POLICY:-missing}
   ```

   Replace `<stack>` with your stack: `rust`, `rails`, `nodejs`, or `generic`.

2. **Optionally add to your `.env`** for customization:

   ```bash
   # Override image (optional)
   # DEVCONTAINER_IMAGE=my-custom-image:tag

   # Control pull behavior: missing (default), always, build
   # DEVCONTAINER_PULL_POLICY=missing
   ```

3. **Rails/Node.js users**: The new templates use mise for runtime version management. If you want to adopt the new approach, re-run `branchbox init` to regenerate your `.devcontainer/` files, or manually update your Dockerfile to use the base image with mise:

   ```dockerfile
   FROM mcr.microsoft.com/devcontainers/base:debian
   # mise will be installed and read .ruby-version, .nvmrc, etc.
   ```

**Feature worktrees** will automatically use the updated configuration from your main worktree when you run `branchbox feature start`.
## [0.5.0] - 2026-01-07

### Added
- `branchbox init` now automatically configures devcontainer.json and compose.yaml for git worktree compatibility, ensuring `workspaceFolder` uses dynamic `${localWorkspaceFolderBasename}` and compose mounts use `../..:/workspaces:cached`.
- Compose templates now mount `.claude.json` file for Claude Code authentication alongside the existing `.claude/` directory mount, ensuring proper authentication across worktrees.
- `branchbox init` generates `docs/BRANCHBOX.md` quickstart guide for new projects.
- Init next steps now suggest committing the BranchBox configuration with a ready-to-use git command.
- `branchbox init` automatically adds cloudflared tunnel service to compose file when tunnels are enabled, with `.cloudflared.env` template for configuration.
- `branchbox init` can now provision the tunnel for `main` immediately when API credentials are provided, populating `.cloudflared.env` with the actual `TUNNEL_TOKEN`.
- Compose file name detection now reads `dockerComposeFile` from devcontainer.json and falls back to common names (`compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml`).

### Changed
- Parent structure (`use_parent_structure`) is now the default for `branchbox init`, creating worktrees as siblings (project/main/, project/feature-x/). Use `--no-parent-structure` to opt out.

### Fixed
- Fixed duplicate volume mount entries in compose.yaml when transforming `..:/workspaces:cached` to `../..:/workspaces:cached`.

## [0.4.1] - 2025-12-15

### Fixed
- `branchbox init --update` now always repairs `.gitignore` entries for `.branchbox/` and devcontainer env/tunnel files.
- `branchbox feature teardown` now force-removes worktrees when you accept the interactive `--force` prompt.
- Cloudflare API errors no longer fail JSON decode when error payloads omit `result`; BranchBox surfaces the real Cloudflare message instead.

### Added
- `branchbox tunnel open` and `branchbox tunnel remove` for provisioning/removing tunnels on existing features.
- `.branchbox/config.json` `feature.*` defaults for branch prefix and teardown branch-delete policy.

## [0.4.0] - 2025-11-15

### Added
- Introduced the BranchBox agent daemon (`branchbox-agent`) with its own crate, control-plane HTTP drain, durable ack tracking, and a CLI bridge (`branchbox agent status`) so long-running workflows can keep syncing even when the CLI exits.
- Added a gRPC surface consumed by both the CLI and a redesigned SwiftUI macOS preview app; the app now shows adapter metadata, control-plane diagnostics, tunnel health, and one-click feature actions from the home dashboard and menu bar.
- Demo/devcontainer tooling now forwards agent/control plane ports inside the devcontainer, includes a teaser harness for quick recordings, and keeps macOS packaging reproducible even when the Rust toolchain is unavailable.

### Changed
- Refreshed README, architecture docs, and milestone plans to highlight the agent milestone, macOS app loop, and end-to-end telemetry expectations before tagging releases.
- The macOS experience received a full visual overhaul (shell, active cards, error states, background sync indicators) so testers can validate the agent/control-plane loop without diving into logs.

### Fixed
- `branchbox feature start` no longer rewrites devcontainer configs when nothing changed and the demo harness copies fallback assets when `rsync` is missing.
- Agent + macOS IPC defaults now correctly gate Unix-only mechanisms on Windows, trim helper output, and keep CLI fallbacks optional so the UI keeps running even when the CLI binary is absent.
- Devcontainer + tunnel scripts gained better permission handling (rsync fallback, Cloudflared defaults) and we fixed multiple regressions surfaced by the teaser/demo harness runs.

### Documentation
- Added a macOS developer README plus packaging instructions, refreshed the release guide with milestone expectations, and documented the 60s teaser workflow for future recordings.
- Expanded manual CLI E2E docs with verbose/pretend modes across stacks, clarified the PATH refresh requirement after install, and noted default agent scope for Unix platforms.

### Testing
- Added SwiftUI view helper tests that cover devcontainer status fallback logic and extended CI to install the Swift toolchain/macOS targets so the preview app keeps building in pull requests.
- Demo harness scripts now run under CI with Dracula-themed VHS recordings to confirm CLI output remains stable.

## [0.3.0] - 2025-11-09

### Added
- Default agent hand-off: `branchbox feature start` now surfaces and (optionally) auto-launches the command defined in `BRANCHBOX_DEFAULT_AGENT_CMD`, propagates the label via `BRANCHBOX_DEFAULT_AGENT_NAME`, and reports readiness in both the checklist and JSON summary so automation can react immediately.
- `.branchbox/config.json` picks up an `editor` block that tracks preferred agent slugs, sidebar focus, and terminal auto-launch hints, letting future agent daemons stamp consistent workspace preferences.
- Specs automation now discovers backlog entries in `docs/features/backlog/`, promotes them into feature worktrees, generates stubs (with frontmatter) when a spec is missing, and honors `FEATURES_DIR` overrides across start/teardown.

### Changed
- `branchbox feature start` and `feature list` ship richer summaries: new checklist rows for prompt seeds, module health, and default agents plus list output that shows start mode, prompt status, and module outcome counts at a glance.
- The manual CLI harness (`scripts/manual-cli-e2e.sh`) now drives every mode (regular/verbose/pretend) across Rust, generic, Rails, and Node stacks so releases validate adapters and tunnel permutations consistently.

### Fixed
- Feature teardown refuses to delete worktrees when devcontainer/module-managed files are dirty unless `--force` or `BRANCHBOX_FORCE_REMOVE_MODULES=1`, preventing accidental loss of template edits.
- Registry reconciliation no longer trips over git porcelain parsing, and backlog specs stay in sync when moving between in-progress and completed folders.

### Documentation
- Expanded devcontainer docs with telemetry, Cloudflared wiring, and troubleshooting guidance plus refreshed README sections covering default agent auto-launch.
- Added a detailed manual CLI E2E guide (modes + stack matrix) and release runbook updates so maintainers know exactly which commands to run before tagging.

### Testing
- Introduced targeted CLI unit tests for prompt/default-agent summaries, dirty teardown guards, and specs promotion logic.
- Beefed up the manual CLI harness to assert Cloudflared/manual tunnel flows, `branchbox devcontainer sync` dry-runs, registry JSON output, and dirty teardown retries.

## [0.2.2] - 2025-11-08

### Added
- Seed `.devcontainer/.branchbox.env` from bootstrap so new repositories ship a template for per-worktree overrides and agent-driven env injection *(bootstrap)*

### Changed
- `branchbox feature start` now copies and customizes `.branchbox.env` for every worktree, keeping secrets isolated and ready for devcontainer sync *(workflows)*

### Bug Fixes
- Align devcontainer workspace/git mounts across strategies and skip syncing env overlays to avoid clobbering per-worktree state *(modules/devcontainer)*
- Ensure release automation keeps Cloudflared tunnel jobs enabled by default *(workflows)*

### Documentation
- Rebuilt the documentation site on Docusaurus with refreshed architecture, getting started, and reference sections *(docs)*

### Testing
- Added regression coverage for Docker/devcontainer sync paths, including per-workflow env propagation *(tests)*

## [0.2.1] - 2025-11-03

### Bug Fixes
- Accept JSONC devcontainer configs so comment-preserving scaffolds sync cleanly *(modules)*

### Miscellaneous
- Streamline Homebrew formula updates to keep tap automation stable *(release)*

## [0.2.0] - 2025-11-03

### Added
- Introduced the devcontainer module with sync tracking and a `branchbox devcontainer sync` command supporting dry-run and strategy overrides to keep feature worktrees aligned.
- Implemented Cloudflared tunnel automation, including provisioning, DNS updates, credential management, and manual fallbacks driven by `.branchbox/config.json`.
- Expanded feature lifecycle workflows with richer registry metadata, improved list output (status filters, JSON), and env linking for devcontainer compatibility.

### CI/CD
- Reworked primary and legacy pipelines, promoted `llvm-cov`, and added a documentation deploy workflow to publish the mdBook site.

### Documentation
- Published an mdBook-powered documentation site with refreshed theming, CLI reference generation, devcontainer rollout guidance, and Cloudflared integration specs.

### Testing
- Added devcontainer and Cloudflared smoke fixtures, sample workspaces, and supporting scripts to exercise new automation paths under coverage.

## [0.1.0-alpha.1] - 2025-10-27

### Features
- Add automated release workflow with cross-platform builds
- Add cargo-release configuration for version management
- Add git-cliff for automated changelog generation

### CI/CD
- Create release workflow for GitHub Actions
- Support Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64) builds
- Implement binary packaging with tar.gz (Unix) and zip (Windows)
- Generate SHA256 checksums for all release artifacts
- Support pre-release tags (beta, alpha, rc)

### Documentation
- Add RELEASING.md with comprehensive maintainer release guide
- Add installation instructions to README with badges
- Create feature specs for Homebrew tap and install scripts

### Fixed
- Correct repository URL in Cargo.toml
- Update author metadata from placeholder

## [0.1.0] - 2025-10-27

### Features
- Implement core workflow orchestration for feature worktrees in Rust
- Add `branchbox feature start/teardown/list` commands with full lifecycle management
- Migrate from bash scripts to Rust-based implementation

<!-- generated by git-cliff -->
