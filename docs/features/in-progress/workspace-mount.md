---

branch: feature/workspace-mount
created: 2026-01-02
status: in-progress
work_feature: workspace-mount
worktree: /Users/rbarazi/projects/branchbox-suite/branchbox/workspace-mount
---
# Workspace Mount Configuration for Git Worktrees

## Problem

When using BranchBox with git worktrees, VS Code devcontainers fail to launch with the error:
```
"The terminal process failed to launch: Starting directory (cwd) '/workspaces/<worktree-name>' does not exist."
```

**Root Cause:** There's a configuration mismatch between:
- **devcontainer.json**: Uses `${localWorkspaceFolderBasename}` which dynamically resolves to the worktree name
- **docker-compose.yml**: Contains hardcoded volume mount paths like `..:/workspaces/project-name:cached`

When opening a worktree (e.g., "feature-oauth"), the devcontainer resolves to `/workspaces/feature-oauth` while Docker mounts to `/workspaces/project-name`, causing the terminal to fail since the expected directory doesn't exist in the container.

## Solution

Configure `branchbox init` to automatically update devcontainer settings for worktree compatibility:

1. **devcontainer.json**:
   - `workspaceFolder`: `/workspaces/${localWorkspaceFolderBasename}`
   - `workspaceMount`: `source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename},type=bind,consistency=cached`

2. **compose.yaml**:
   - Volume mount: `../..:/workspaces:cached` (mounts parent directory containing all worktrees)

This allows all worktrees to be accessible within `/workspaces/` and enables shared directories (`.claude`, `.codex`, etc.) to work across all worktrees.

## Implementation

### Changes

1. **Extracted `configure_workspace_settings()` function** (`core/src/modules/devcontainer.rs:450`):
   - Public function that configures devcontainer.json and compose.yaml for worktree compatibility
   - Returns `ConfigureOutcome` with details of what was modified
   - Can be called from both `branchbox init` and `branchbox feature start`

2. **Updated `InitWorkflow::setup_devcontainer()`** (`core/src/workflows/init.rs:846`):
   - For newly generated devcontainers: calls `configure_workspace_settings()` to ensure consistency
   - For existing devcontainers: calls `configure_workspace_settings()` and reports `DevcontainerStatus::Enhanced` if changes were made
   - Returns `DevcontainerStatus::Valid` if devcontainer already has correct settings

3. **Added tests**:
   - `test_init_configures_existing_devcontainer_for_worktrees`: Verifies init updates incorrect settings
   - `test_init_preserves_valid_devcontainer_settings`: Verifies init doesn't modify correct settings
   - `test_init_generated_devcontainer_has_correct_workspace_settings`: Verifies generated files are correct

### Folder Structure

After `branchbox init` (parent structure is the default), the folder structure becomes:

```
project-name/           # Parent directory (not part of git)
├── main/               # Main branch worktree
│   ├── .devcontainer/
│   │   ├── devcontainer.json
│   │   └── compose.yaml
│   ├── .branchbox/
│   │   └── registry.json
│   └── ...
├── feature-oauth/      # Feature worktree (sibling to main)
│   ├── .devcontainer/
│   │   ├── devcontainer.json  # Synced from main, configured for this worktree
│   │   └── compose.yaml
│   └── ...
└── feature-payments/   # Another feature worktree
    └── ...
```

## Testing

```bash
# Run the new tests
cargo test --package worktree-core -- test_init_configures
cargo test --package worktree-core -- test_init_preserves
cargo test --package worktree-core -- test_init_generated
```

## Related

- GitHub Issue: https://github.com/branchbox/branchbox/issues/41
- DevcontainerModule: `core/src/modules/devcontainer.rs`
- InitWorkflow: `core/src/workflows/init.rs`
