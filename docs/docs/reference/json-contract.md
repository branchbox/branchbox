---
sidebar_position: 4
---

# JSON Contract

Every `branchbox` command that takes `--json` is meant to be read by programs: the BranchBox Mac app, editor integrations, CI scripts. This page describes the rules those programs can rely on, the error envelope, the stable error codes, the exit codes and where each payload is defined.

The golden examples live in the repository under `cli/tests/fixtures/contract/` (with paths normalized). The Rust tests compare real output against them, and the Mac app's tests decode them, so a change to a payload shows up in both languages.

## Machine mode {#machine-mode}

Any `--json` flag switches the process into machine mode right after argument parsing. In machine mode:

- stdout carries **exactly one JSON document**: the success payload or the error envelope;
- human-readable text (progress, banners, warnings) goes to stderr;
- nothing prompts. A step that would ask a question takes its documented non-interactive outcome, which is usually a refusal that names the flag to pass (for example `prune` without `--yes`, or a teardown of an unmerged branch).

JSON is pretty-printed and ends with a newline. Programs should parse the whole of stdout as one document.

## Error envelope {#error-envelope}

When a command fails in machine mode it prints one envelope on stdout:

```json
{
  "schema_version": 1,
  "error": {
    "code": "teardown_refused",
    "message": "Refusing to tear down 'eta'; nothing was removed. …",
    "causes": ["…"],
    "details": { "plan": { … }, "changed_anything": false, "completed_steps": [] }
  }
}
```

- `code` is stable; branch on it, never on `message`.
- `message` is the human sentence, and `causes` the error chain below it.
- `details` is an object whose shape depends on the code (see the table), or `null`.
- stderr still carries the familiar `Error: …` text, unchanged from text mode.

The envelope is printed only in `--json` mode. Older CLIs (0.13.x) print no envelope; their errors are on stderr only.

## Exit codes {#exit-codes}

| Exit | Meaning |
|---|---|
| 0 | Success, including documented partial success (for example a teardown whose worktree was removed but whose branch could not be deleted: `branch_deleted: false` with `branch_delete_error`). |
| 1 | Failure or refusal. |
| 2 | Usage error from the argument parser (unknown flag or subcommand). |
| 75 | `feature dispatch-tool`: the request file was not present before `--wait-seconds` expired. |
| 101 | Panic. In machine mode an `internal_panic` envelope is printed too. |

## In-band failures {#in-band}

A few commands report failure inside their own payload. They print the full payload, exit 1 and add no envelope. Always try to decode stdout, whatever the exit code.

| Command | Failure inside the payload |
|---|---|
| `feature exec --json` | The inner command failed: `{exit_code, stdout, stderr}` carries its exit code. |
| `devcontainer up/build/exec --json` | `"outcome": "error"`, for example `{"outcome":"error","message":"Docker is not available"}`. |
| `feature dispatch-tool --json` | The correlated response, including the not-pending outcome (exit 75). |
| `doctor --json` | A required check has `"status": "error"`. |
| `devcontainer sync --json` | A worktree's row has `"status": "failed"`. |
| `prune --yes --json` | A row has `"outcome": "failed"`. |

`devcontainer configure`, `add-tunnel` and `inject-agents --json` still answer a missing `.devcontainer/` with the older `{"error": "No .devcontainer directory found"}` object and exit 1. `devcontainer detect` also accepts root `.devcontainer.json`; when neither location is available, it returns `{"error":"No devcontainer configuration found"}` and exits 1. Recognize an envelope by `schema_version` plus an object-valued `error`.

## Error codes {#error-codes}

| Code | When | `details` |
|---|---|---|
| `teardown_refused` | Teardown stopped instead of deleting work (see [teardown](#teardown)). | `{plan, changed_anything, completed_steps[]}` |
| `worktree_not_found` | The feature's worktree does not exist (including `feature teardown <unknown>`). | `{name}` or `{name, path}` |
| `feature_not_found` | The feature is not registered (tunnel, sync and prune paths). | `{name, registry}` |
| `invalid_feature_name` | The name is not a valid feature name. | `{name}` |
| `worktree_exists` | Starting a feature whose worktree already exists. | `{path}` |
| `branch_exists` | Starting a feature whose branch already exists. | `{branch}` |
| `not_a_git_repository` | The path is not inside a git repository. | `{path}` |
| `validation_failed` | Any other validation refusal (bad flag combination, unparsable patch, tunnels disabled). | `null` |
| `config_invalid` | `.branchbox/config.json` or a new value is invalid. | `{key?, line?, column?, expected?}` |
| `config_unknown_key` | `config` was given a key that does not exist. | `{key}` |
| `registry_locked` | Another BranchBox process held the registry lock for too long. | `{path, waited_secs}` |
| `confirmation_required` | `prune` without `--yes` in machine mode. | `{count}` |
| `devcontainer_source_missing` | `devcontainer sync` with worktrees to update but no main `.devcontainer/`. | `{path}` |
| `agent_unreachable` | The agent daemon could not be reached. | `null` |
| `git_failed`, `io_error`, `command_failed`, `module_failed`, `env_var_not_set`, `adapter_not_found` | The matching failure in the core library. | `null` |
| `internal`, `internal_panic` | Anything else. | `null` |

New codes may be added. Treat an unknown code as a generic failure and show `message`.

## Capabilities {#capabilities}

`branchbox version --json` tells a client what this CLI supports:

```json
{"version":"0.13.4","contract_version":1,"capabilities":["json-error-envelope","registry-lock","write-ahead-start","teardown-plan","teardown-discard-changes","teardown-unmerged-preflight","host-container-teardown-verified","prune-json","detect-json","devcontainer-sync-json","config","tunnel-credentials","doctor","init-json"]}
```

- Gate on capability strings, not on version numbers. A capability is added in the same change that implements it.
- `contract_version` changes only for a breaking change.
- A CLI older than the `version` subcommand exits 2 on it. Fall back to `branchbox --version` (`branchbox 0.13.4`) and assume no capabilities. The Mac app calls this "legacy mode".

| Capability | What it adds |
|---|---|
| `json-error-envelope` | Machine mode and the error envelope. |
| `registry-lock` | Registry writes are locked and atomic, so concurrent commands do not lose entries. |
| `write-ahead-start` | `feature list` shows a start that has not finished (see [`feature list`](#feature-list)). |
| `teardown-plan` | `feature teardown --dry-run --json`. |
| `teardown-discard-changes` | `feature teardown --discard-changes`, `--delete-branch`, `--force-delete-branch`. |
| `teardown-unmerged-preflight` | An unmerged branch is refused before anything is removed. |
| `host-container-teardown-verified` | Container teardown checks removal of exact workspace-labeled devcontainers, including standalone containers, and reports cleanup failures or residue. Failed module cleanup also invalidates the receipt; standalone volumes/custom networks are not inferred. |
| `prune-json` | `prune --dry-run --json` and `prune --yes --json`. |
| `detect-json` | `detect --json`. |
| `devcontainer-sync-json` | `devcontainer sync --json [--feature NAME]...`. |
| `config` | `config get/set/unset/apply`. |
| `tunnel-credentials` | `tunnel credentials set`. |
| `doctor` | `doctor`. |
| `init-json` | `init --json` and the `--op-*` flags. |

## Compatibility rules {#compatibility}

- Existing payloads only **gain** keys; nothing is renamed or removed. Ignore keys you do not know.
- New payloads carry `"schema_version": 1`. `version --json` is the exception and has none.
- The registry's feature `status` values are closed: `active`, `degraded`, `failed_retained`, `orphaned`, `removed`. New state goes into new optional fields (such as `setup`), never into new status values, so older CLIs can keep reading a shared registry.
- Dates are RFC 3339 strings.

## Payloads {#payloads}

### `feature list --json` {#feature-list}

An array of feature records: `work_feature`, `branch_name`, `worktree_path`, `base_branch`, `feature_url` (no scheme), `status`, `created_at`, `updated_at`, `removed_at`, `tunnel{…}`, `runtime{provider, runtime_id, published_ports[{host, runtime}], …}`, `module_outcomes[]`, `adapter{…}`, `default_agent{…}`, `devcontainer_outdated`, `last_sync_at` and more.

- A record whose start has not finished carries `"setup": {"state": "in_progress", "pid": 48211, "started_at": "…"}`. The state reads `"interrupted"` once that process is gone or the start is more than 24 hours old. The record's `status` stays `active`.
- An `active` or `failed_retained` record whose worktree folder no longer exists is reported as `orphaned`.
- Tunnel `status` is one of `pending`, `active`, `manual`, `disabled`.

Example: `cli/tests/fixtures/contract/core/list_interrupted.json`.

### `feature start --json` {#feature-start}

The start summary: the resolved name, branch, worktree path, URLs, runtime, module outcomes, tunnel and default agent. Example: `core/start_minimal_truncated_prompt.json`.

### `feature teardown` {#teardown}

**Plan** (`--dry-run --json`, always exit 0): what teardown would do, and why it would refuse.

```json
{"schema_version":1,"work_feature":"eta","registered":true,"status":"active",
 "worktree":{"path":"/r/eta","exists":true,"locked":false,"lock_reason":null},
 "changes":{"status_available":true,"truncated":false,
   "user":[{"path":"notes.txt","kind":"untracked","area":"other"}],
   "generated":[{"path":".devcontainer/.branchbox.env","rule":"reserved_name"}],
   "preserved":[{"path":"docs/features/in-progress/eta.md","destination":"docs/features/backlog/eta.md"}]},
 "branch":{"name":"feature/eta","source":"registry","exists":true,"upstream":null,"reference":"HEAD","reference_name":"main",
   "merged":false,"merged_into_head":false,"ahead":3,"action":"delete"},
 "defaults":{"delete_branch_by_default":true,"force_delete_unmerged_by_default":false},
 "runtime":{"provider":"container","runtime_id":null},"tunnel":{"status":"disabled"},
 "blockers":[{"kind":"uncommitted_changes","count":1,"message":"…","override":"--discard-changes"},
             {"kind":"unmerged_branch","branch":"feature/eta","ahead":3,"message":"…","override":"--keep-branch | --force-delete-branch"}],
 "warnings":[]}
```

| Field | Values |
|---|---|
| Blocker `kind` | `not_a_worktree` (no override), `uncommitted_changes`, `unmerged_branch`, `worktree_locked`, `status_unavailable`, `spec_not_preserved`, `worktree_removal_failed`, `runtime_cleanup_failed` |
| Change `kind` | `untracked`, `modified`, `added`, `deleted`, `typechange`, `conflicted`, `staged` |
| Change `area` | `devcontainer`, `compose`, `vscode`, `spec`, `env`, `other` |
| Generated `rule` | `reserved_name`, `devcontainer_baseline`, `derived_from_main`, `devcontainer_env_link`, `env_feature_block`, `vscode_managed_keys`, `vscode_managed_tasks` |
| `branch.source` | `explicit_prefix`, `registry`, `config_prefix` |
| `branch.action` | `keep`, `delete`, `force_delete` |

`branch` is `null` (with a warning) when the merge state cannot be read. Files git ignores are removed with the worktree and are not listed.

**Summary** (`--json`): `{work_feature, branch_name, worktree_removed, branch_deleted, branch_action, branch_delete_error, discarded_changes[], preserved[], registry_updated, module_reports[], runtime_teardown{…}, adapter_cleanup_warnings[], warnings[]}`.

`runtime_teardown` contains `{provider, runtime_id?, verified, residue_free, residue[{kind, identifiers[]}]}`. `verified` describes whether cleanup checks completed successfully; it can be `true` while observed resources make `residue_free` false. Command/probe errors, unresolved Compose ownership, or module failures invalidate the receipt. The host container check covers exact workspace-labeled containers, while the Compose module covers owned project containers, networks, and volumes; standalone volumes/custom networks are outside that container check.

In current source builds, newly generated managed Compose identity binds its project name to the canonical workspace. Legacy unbound identity requires matching recorded feature/worktree identity, exact workspace-label evidence, or valid scoped cleanup history. Copied or ambiguous managed identity does not authorize project cleanup; the module failure invalidates the receipt and can produce `runtime_cleanup_failed`, retaining the worktree unless removal is forced. A Compose filename or ambient project name is not ownership evidence.

**Refusal**: exit 1 with a `teardown_refused` envelope whose `details.plan` is the plan above. `changed_anything` is `false` for an initial safety refusal. A later refusal can report `true` after earlier teardown steps changed something; `completed_steps` names those steps, while the worktree and active registry entry remain available for retry. `runtime_cleanup_failed` blocks removal of a possibly provisioned environment unless `--force` is supplied. Force can remove the workspace with an incomplete cleanup receipt; `--keep-branch` still retains its branch.

Examples: `teardown/plan_fresh.json`, `teardown/plan_dirty_unmerged.json`, `teardown/summary_discard.json`, `teardown/refusal_envelope.json`.

### `prune --json` {#prune}

- `--dry-run --json`: `{schema_version, dry_run: true, policy{delete_branch, force_delete_branch, discard_changes, complete_spec}, candidates[{work_feature, status, branch_name, worktree_path, plan, plan_error?}], at_risk{uncommitted_changes[{work_feature, count, truncated, paths}], unmerged_commits[{work_feature, branch, ahead}]}}`.
- `--yes --json`: `{schema_version, dry_run: false, results[{work_feature, outcome: "removed"|"failed", summary, error{code, message}}], pruned, failed}`, exit 1 if any row failed.

Prune discards uncommitted changes and force-deletes branches it deletes; the dry run's `at_risk` lists what would be lost. Examples: `teardown/prune_dry_run.json`, `teardown/prune_execute.json`.

### `feature exec --json` {#feature-exec}

`{exit_code, stdout, stderr}`. Exit 1 when the inner command failed, with the payload still printed. Example: `core/exec_inband_failure.json`.

### `detect --json` {#detect}

`{schema_version, project, git_repository, initialized, stack, adapter, modules[], has_devcontainer, has_env, warnings[]}`. `initialized` means `.branchbox/registry.json` exists. Examples: `commands/detect_initialized.json`, `commands/detect_plain_folder.json`.

### `devcontainer sync --json` {#devcontainer-sync}

`{schema_version, dry_run, strategy, results[{work_feature, worktree_path, status, files[], skip_reason, error, registry_updated}], synced, failed, skipped}`. `status` is `synced`, `would_sync`, `skipped` or `failed`. Examples: `commands/sync_synced.json`, `commands/sync_dry_run.json`.

### `config` {#config}

- `config get [KEY] --json`: `{schema_version, path, exists, effective{…}, file{…}, keys[{key, type, allowed, default, value, source, description}]}`. With `KEY`, `keys` holds that key only.
- `config apply --file <PATH|-> [--dry-run] --json`: takes an RFC 7386 merge patch (`null` unsets) and returns `{schema_version, changed[{key, old, new}], effective{…}}`.

The keys are listed in the [configuration reference](./configuration.md). Examples: `commands/config_get_defaults.json`, `commands/config_apply.json`.

### `tunnel` {#tunnel}

- `tunnel open --json`: `{work_feature, state, warnings}`.
- `tunnel remove --json`: `{work_feature, previous_state, updated_state, warnings}`.
- `tunnel credentials set --account-id ID --api-token-stdin [--clear] --json`: `{schema_version, credentials_path, account_id, token_present}`. The token is read from stdin only and never appears in output.

### `doctor --json` {#doctor}

`{schema_version, cli{version, contract_version, path}, host{os, arch}, checks[{id, title, required, status, path, version, detail, remediation}], summary{ok, warn, error}}`. `status` is `ok`, `warn`, `error` or `skipped`. Exit 1 when a required check is `error`. Example: `commands/doctor_healthy.json`.

### `init --json` {#init}

`{schema_version, workspace_path, repository_state{kind, …}, reorganized, stack, adapter, modules[], devcontainer_status{kind}, registry_initialized, onepassword{status}, warnings[], next_steps[]}`. `--json` implies `--yes`; the repository moves only with `--reorganize`. Examples: `commands/init_created.json`, `commands/init_already_initialized.json`.

### `devcontainer up/down/build/exec --json` {#devcontainer}

camelCase objects: `up` → `{outcome, containerId, remoteUser, remoteWorkspaceFolder, composeProjectName}`, `down` → `{outcome, removedContainers}`, `build` → `{outcome, imageName}`, `exec` → `{outcome, exitCode, stdout, stderr}`. `devcontainer detect --json` → `{service_name, port, service_url, container_user, home_path, container_type, configured_user, workspace_folder}`.

The newer detection fields describe active configuration: `container_type` is `compose`, `dockerfile`, or `image`; `configured_user` is the explicit remote/container user or `null`; `workspace_folder` is configuration-derived, with standard basename variables expanded. `container_user` retains a historical estimate when no user is configured. Image/Dockerfile configurations do not report service facts from unused Compose files. These are configuration facts rather than a running-container probe.

In current source builds, `down` checks command results and remaining owned resources before returning success. Cleanup failures exit 1 with an error envelope. Default Down keeps volumes; `--volumes` deletes attached anonymous standalone volumes or owned Compose volumes, leaving named/shared standalone volumes untouched. Compose identity is retained within its canonical workspace for partial-failure retries and for later explicit volume cleanup.

## For contributors {#contributors}

- Print JSON with `worktree_core::output::emit_json` and human text with `humanln!` / `human!`. `println!` and `print!` are disallowed in `core` and `cli` by clippy.
- A command that adds `--json` must report it from its module's `wants_json()`, or machine mode stays off.
- Refusals keep their codes through `CliError` or `json_error::recode`.
- Add a capability string to the module's `CAPABILITIES` in the change that implements it.
- Golden fixtures are checked by the tests; after an intended payload change, regenerate them with `UPDATE_CONTRACT_FIXTURES=1 cargo nextest run -p branchbox-cli` and review the diff.
