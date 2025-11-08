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
