---
branch: feature/devcontainer-editor-experience
created: 2025-11-08
status: in-progress
work_feature: devcontainer-editor-experience
---

# Devcontainer / Editor Experience

## Overview

- Persist a “default coding agent” preference in `.branchbox/config.json` so every worktree + devcontainer session can bootstrap the same agent workflow.
- Autoconfigure Cursor/VS Code so the desired agent (or Git sidebar) is focused on attach while the auxiliary/right sidebar stays hidden.
- Smooth out the Cursor extension install experience—today every first run shows “Please reload Visual Studio Code to enable it.”

## Current Findings

### Configuration plumbing

- `BranchBoxConfig` now exposes an `editor` block:

```json
{
  "version": "1",
  "editor": {
    "default_agent": "codex",
    "auto_launch_agent_terminal": true,
    "preferred_sidebar_view": "workbench.view.scm",
    "hide_secondary_sidebar": true
  }
}
```

- These fields are optional and ship with no-op defaults, keeping older configs valid. Commands that already load `BranchBoxConfig` (init/workflows) pick up the struct automatically, so later steps can read preferences without additional file I/O.
- Next: add a `branchbox config editor` helper (or extend `branchbox init`) that prompts for the values above and writes them into `.branchbox/config.json`.

### Cursor layout automation

- Cursor exposes the same command IDs as VS Code. We can focus a sidebar or close the auxiliary pane via:
  - `workbench.view.scm` (Git)
  - `workbench.view.extension.codex` (Codex sidebar)
  - `workbench.action.closeAuxiliaryBar`
- Plan: add `.devcontainer/scripts/prime-editor-layout.sh` that reads `.branchbox/config.json`, then calls the remote CLI with `code --command ...` (Cursor bundles the `cursor` CLI, which mirrors `code`). We'll invoke it from `postAttachCommand` so it only runs when an editor attaches—not during non-interactive container starts.
- For terminals, we can create a dedicated integrated profile named `BranchBox Agent` in workspace settings. When `auto_launch_agent_terminal` is true, the script will run `code --command workbench.action.terminal.newWithProfile` to spawn that profile (profile will run `codex chat --agent <default>` or the Claude equivalent).

### Extension reload prompt

- Cursor inherits VS Code’s remote extension model: when you install an extension, it’s staged on the UI side first, then the remote server copies it into `~/.vscode-server/extensions`. The remote server must restart to pick up the new extension, hence the “Please reload” toast.
- Because we already list required extensions in `devcontainer.json → customizations.vscode.extensions`, we can avoid the reload prompt by ensuring they are preinstalled inside the image *before* Cursor attaches:
  1. Add `.devcontainer/scripts/install-extensions.sh` that runs `code --install-extension ...` for every extension ID.
  2. Call it from `postCreateCommand` (after Rust setup) so extensions are ready when the editor connects, removing the reload requirement.
- We should also document that Cursor currently lacks the “auto reload” flag VS Code added in 1.94; filing an upstream issue is worthwhile if the prompt persists even with preinstalled extensions.

## Next Steps

- [ ] Extend `branchbox init` (or add a new `branchbox config editor` subcommand) to prompt for `default_agent`, `sidebar_view`, and `auto_launch_agent_terminal`.
- [ ] Create `.devcontainer/scripts/prime-editor-layout.sh` + wire it through `postAttachCommand` to run the command sequence derived from the editor config.
- [ ] Author terminal profile scaffolding (`.vscode/settings.json`) so the profile runs `codex chat --agent <slug>` or `claude chat` automatically.
- [ ] Ship `.devcontainer/scripts/install-extensions.sh` and update `postCreateCommand` to invoke it, validating that Cursor no longer asks for a reload on a clean container.
- [ ] Document troubleshooting (how to reset layout, override default agent per-user, etc.) in `docs/DEVELOPMENT.md`.

## Implementation Plan

### Editor config helper

- Add a `branchbox config editor` subcommand under `cli/src/commands/config.rs`. It should load the existing `BranchBoxConfig`, prompt for `default_agent`, `preferred_sidebar_view`, and `auto_launch_agent_terminal`, then persist the merged struct back to `.branchbox/config.json`.
- Provide `--default-agent`, `--sidebar-view`, and `--auto-launch-agent-terminal` flags so CI or scripts can set values non-interactively.
- Teach `branchbox init` to optionally call the helper (guarded by `BRANCHBOX_ENABLE_DEVCONTAINER_MODULE`) so greenfield repos capture preferences without running two commands.

### Layout primer script

- Create `.devcontainer/scripts/prime-editor-layout.sh` with jq + `cursor` CLI dependencies baked into the devcontainer image.
- Script flow:
  1. Read `.branchbox/config.json` (fall back to no-op if missing/malformed).
  2. Derive the sidebar command (default `workbench.view.scm`).
  3. Run `cursor --remote=devcontainer --command ...` to focus the view.
  4. If `hide_secondary_sidebar` is true, call `workbench.action.closeAuxiliaryBar`.
  5. For `auto_launch_agent_terminal`, dispatch `workbench.action.terminal.newWithProfile` targeting the `BranchBox Agent` profile.
- Hook into `devcontainer.json → postAttachCommand` so it triggers only when an editor session starts.

### Terminal profile scaffolding

- Extend `.vscode/settings.json` (under `devcontainer.json/customizations.vscode.settings`) with:
  - `terminal.integrated.profiles.linux.BranchBox Agent` that shells into `codex chat --agent <slug>` or `claude chat` depending on config/env.
  - `terminal.integrated.defaultProfile.linux` conditionally set via `${branchbox:editor.autoLaunchAgentTerminal}` once VS Code exposes profile variables; until then, the layout script triggers the command explicitly.
- Document how users can override the profile locally via `~/Library/Application Support/Cursor/User/settings.json` without fighting workspace defaults.

### Extension preinstall script

- Add `.devcontainer/scripts/install-extensions.sh` that parses the extension list from `devcontainer.json` (using `jq '.customizations.vscode.extensions[]'`) and runs `cursor --install-extension` for each entry.
- Invoke the script from `postCreateCommand` right after dependency bootstrap so the remote server already hosts the extensions before the first UI attach.
- Cache the installed marker (`/home/vscode/.cache/branchbox/extensions.hash`) to skip reinstallation when the extension list hasn't changed; include the hash in telemetry for debugging.

### Telemetry & validation

- Emit `module.devcontainer.editor_layout` spans from the layout script via `branchbox tracing emit ...` (or a lightweight Rust helper) with attributes for `sidebar_view`, `auto_launch_agent_terminal`, and exit status.
- Update the validation runbook to include:
  - `branchbox devcontainer sync --strategy copy --dry-run` to ensure `.devcontainer/scripts` propagate to worktrees.
  - Manual attach test in Cursor verifying the sidebar focus + terminal spawn behavior.
  - Clean devcontainer rebuild confirming the “Please reload” toast no longer appears.

## Open Questions

- Should `default_agent` support per-user overrides via `${SHARED_CONFIG_DIR}/branchbox/config.local.json`, similar to how `.env` works today?
- If users prefer VS Code instead of Cursor, do we need a per-editor flag to avoid launching Cursor-specific command IDs (or detect via `$TERM_PROGRAM` during `postAttachCommand`)?
- How do we surface failures from `prime-editor-layout.sh` back to the CLI? Option: exit non-zero so `postAttachCommand` surfaces an inline toast, or log to `~/.branchbox/logs`.

## Proposed CLI UX

```
$ branchbox config editor --default-agent codex --sidebar-view workbench.view.extension.codex --auto-launch-agent-terminal
✔ Using config at /repo/.branchbox/config.json
? Hide the auxiliary sidebar on attach? (y/N) › y
? Launch terminal profile "BranchBox Agent" on attach? (Y/n) › Y
? Preferred sidebar view (workbench.view.scm) › workbench.view.extension.codex
Updated editor preferences:
{
  "default_agent": "codex",
  "auto_launch_agent_terminal": true,
  "preferred_sidebar_view": "workbench.view.extension.codex",
  "hide_secondary_sidebar": true
}
```

- Non-interactive mode should skip prompts and print a diff-style summary for scripts (`--quiet` suppresses all output except errors).
- If `.branchbox/config.json` is missing, the command should seed `BranchBoxConfig::default()` before applying overrides.
- Extend `branchbox config view --json` to include editor settings so operators can double-check effective values.

## Script Pseudocode

```bash
#!/usr/bin/env bash
set -euo pipefail

CONFIG_PATH="${BRANCHBOX_CONFIG_PATH:-/workspaces/repo/.branchbox/config.json}"
JQ=${JQ_BIN:-jq}
CURSOR_BIN=${CURSOR_BIN:-cursor}

editor_val() {
  local key="$1"
  "$JQ" -r --arg key "$key" '.editor[$key] // empty' "$CONFIG_PATH" 2>/dev/null || true
}

sidebar_cmd="$(editor_val preferred_sidebar_view)"
sidebar_cmd="${sidebar_cmd:-workbench.view.scm}"
"$CURSOR_BIN" --remote=dev-container --command "$sidebar_cmd"

if [[ "$(editor_val hide_secondary_sidebar)" == "true" ]]; then
  "$CURSOR_BIN" --remote=dev-container --command workbench.action.closeAuxiliaryBar
fi

if [[ "$(editor_val auto_launch_agent_terminal)" == "true" ]]; then
  "$CURSOR_BIN" --remote=dev-container --command workbench.action.terminal.newWithProfile --command-argument 'BranchBox Agent'
fi
```

- Guard the script with `if ! command -v cursor` to fail fast when the CLI is missing; emit actionable log.
- On failure, write a JSON status blob to `/workspace/.branchbox/logs/editor-layout.log` for debugging.

## Milestone Breakdown

1. **Config plumbing (done)** – `EditorSettings` struct & defaults.
2. **CLI helper** – shipping interactive + flag-driven UX, unit tests covering config persistence.
3. **Devcontainer assets** – add layout + extension scripts, update `devcontainer.json`, test via `devcontainer up`.
4. **Telemetry + docs** – wire spans, update `docs/DEVELOPMENT.md`, add troubleshooting appendix.
5. **Agent integration** – once CLI pieces harden, have the agent trigger the scripts during `DevcontainerModule::setup`.

## Validation Matrix

| Scenario | Steps | Expected result |
| --- | --- | --- |
| Fresh repo, defaults | Run `branchbox init` with feature flag off | `.branchbox/config.json` contains only `version` + `tunnel`, no `editor` block |
| Editor config via CLI | `branchbox config editor --default-agent codex --hide-secondary-sidebar` | Config file shows the overrides; rerun with `--default-agent claude` updates value |
| Layout script happy path | Set `preferred_sidebar_view=workbench.view.extension.codex`, attach Cursor | Activity sidebar switches to Codex, auxiliary bar hidden, terminal opens |
| Layout script missing config | Remove `.branchbox/config.json`, attach | Script logs warning and exits zero without running commands |
| Extension preinstall | Delete `~/.vscode-server/extensions`, rebuild devcontainer | Required extensions present before first attach; no reload toast |
| Telemetry capture | `branchbox devcontainer sync` with scripts enabled | `branchbox.log` includes `module.devcontainer.editor_layout` span with strategy + duration |
| Failure handling | Force `cursor` CLI to fail (rename binary) | Script logs error, sets non-zero exit causing devcontainer toast, `devcontainer_outdated` flag set |

## Risks & Mitigations

- **Cursor-specific logic breaks VS Code users**: Detect `$TERM_PROGRAM` inside `postAttachCommand` (Cursor sets `TERM_PROGRAM=cursor`) and skip Cursor-only commands when absent.
- **Scripts slow down attach times**: Cache computed config (hash + timestamp) and skip rerunning commands when preferences haven't changed since last attach.
- **Per-user overrides stomped by sync**: Encourage operators to keep personal overrides in `${SHARED_CONFIG_DIR}`; doc a `BRANCHBOX_EDITOR_CONFIG_OVERRIDE` env var that points to a private path excluded from sync.
- **Extension install flakiness**: Wrap installation loop with retries (`cursor --install-extension` can fail due to network). Store partial progress and continue on next `postCreateCommand`.
- **Telemetry noise**: Rate-limit span emission to once per attach (ignore re-entrant postAttach triggers) and redact user-specific data (extension IDs are fine).

## Outstanding Tasks

- [ ] Define schema for `.branchbox/config.local.json` overrides and merge precedence rules.
- [ ] Decide whether to store agent terminal command templates in config (`codex chat --agent {default}`) or hardcode script logic.
- [ ] Add integration tests under `core/tests/devcontainer_module.rs` to simulate sync → attach pipeline.
- [ ] Coordinate with docs team on a troubleshooting flowchart (where to look when the layout script fails).

## Telemetry Schema Sketch

```json
{
  "name": "module.devcontainer.editor_layout",
  "attributes": {
    "workspace_id": "UUID",
    "worktree_path": "/repo/.worktrees/feature/foo",
    "strategy": "copy",
    "sidebar_view": "workbench.view.extension.codex",
    "auto_launch_agent_terminal": true,
    "hide_secondary_sidebar": true,
    "default_agent": "codex",
    "status": "success|failed|skipped",
    "duration_ms": 1823,
    "error_type": "command_failed",
    "error_message": "cursor: command not found"
  }
}
```

- `status=skipped` when `.branchbox/config.json` omits editor preferences or feature flag disabled.
- `error_type` should map to the error matrix (permission_denied, command_failed, parse_error).
- Emit complementary metrics:
  - Counter `devcontainer.editor_layout.sync_total{status,sidebar_view}`.
  - Histogram `devcontainer.editor_layout.duration_ms`.

## Troubleshooting Snippets

```
# 1. Verify config file
cat .branchbox/config.json | jq '.editor'

# 2. Dry-run layout script (outside postAttach)
DEVCONTAINER=true .devcontainer/scripts/prime-editor-layout.sh --dry-run

# 3. Watch logs
tail -f ~/.branchbox/logs/editor-layout.log

# 4. Check extension state
ls ~/.vscode-server/extensions | grep codex
```

- Document common fixes (e.g., reinstall `cursor` CLI, reset workspace settings, wipe `~/.vscode-server/extensions`).

## Future Enhancements

- Allow per-feature overrides (spec files can declare `editor.default_agent` to auto-scope agents per backlog item).
- Teach control plane UI to surface latest editor sync timestamp and highlight stale worktrees.
- Add `branchbox devcontainer doctor` command that runs layout + extension checks and reports actionable diagnostics.
- Explore supporting JetBrains Gateway by translating editor preferences into Gateway scripts (long-term).

## Devcontainer JSON changes (draft)

```jsonc
{
  "name": "BranchBox Devcontainer",
  "postCreateCommand": ".devcontainer/scripts/install-extensions.sh && just bootstrap",
  "postAttachCommand": [
    "if [ -f .devcontainer/scripts/prime-editor-layout.sh ]; then .devcontainer/scripts/prime-editor-layout.sh; fi"
  ],
  "customizations": {
    "vscode": {
      "settings": {
        "terminal.integrated.profiles.linux": {
          "BranchBox Agent": {
            "path": "/bin/bash",
            "args": [
              "-lc",
              "BRANCHBOX_DEFAULT_AGENT=${BRANCHBOX_DEFAULT_AGENT:-codex} codex chat --agent ${BRANCHBOX_DEFAULT_AGENT:-codex}"
            ]
          }
        },
        "terminal.integrated.defaultProfile.linux": "BranchBox Agent"
      },
      "extensions": [
        "github.copilot-chat",
        "codex.codex",
        "ms-vscode.git"
      ]
    }
  }
}
```

- `BRANCHBOX_DEFAULT_AGENT` should be injected via `.devcontainer/docker-compose` env to keep layout script + terminal profile aligned.
- `postAttachCommand` must remain idempotent; wrap in `bash -lc` to ensure env vars resolve.
- Consider gating the terminal profile override behind config so vanilla users keep their previous default.

## Sample `.devcontainer/scripts/install-extensions.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

EXTENSIONS_JSON=".devcontainer/devcontainer.json"
STATE_HASH_FILE="${HOME}/.cache/branchbox/extensions.hash"
INSTALLER="${CURSOR_BIN:-cursor}"

desired_hash="$(jq -r '.customizations.vscode.extensions | sort | join(\"\\n\")' \"$EXTENSIONS_JSON\" | sha256sum | cut -d' ' -f1)"

if [[ -f \"$STATE_HASH_FILE\" ]] && [[ \"$(cat \"$STATE_HASH_FILE\")\" == \"$desired_hash\" ]]; then
  echo "Extensions up-to-date; skipping install."
  exit 0
fi

jq -r '.customizations.vscode.extensions[]' \"$EXTENSIONS_JSON\" | while read -r ext; do
  echo \"Installing $ext\"
  \"$INSTALLER\" --install-extension \"$ext\" --force || {
    echo \"Failed to install $ext\" >&2
    exit 1
  }
done

echo \"$desired_hash\" > \"$STATE_HASH_FILE\"
```

- Add exponential backoff around the install loop if network hiccups become frequent.
- Ensure script runs as `vscode` user so extensions land in the right homedir.

## Control Plane Integration Ideas

- Extend `/v1/worktrees/:id` payload with:

```json
{
  "editor": {
    "last_sync_at": "2025-11-10T03:22:12Z",
    "sync_status": "success|failed|stale",
    "default_agent": "codex",
    "preferred_sidebar_view": "workbench.view.extension.codex"
  }
}
```

- Surface a banner when `sync_status=failed` with CTA “Run `branchbox devcontainer sync`”.
- Allow operators to push overrides via control plane UI that write to `.branchbox/config.json` (respecting precedence rules discussed above).
