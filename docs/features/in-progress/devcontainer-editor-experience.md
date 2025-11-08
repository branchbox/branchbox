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
