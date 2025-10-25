---
worktree: /workspaces/devcontainer-env-isolation
branch: feature/devcontainer-env-isolation
work_feature: devcontainer-env-isolation
url: https://rida-mbp-agentify-devcontainer-env-isolation.rida.me
status: completed
created: 2025-10-17
completed: 2025-10-25
---

# Dev Container Environment Isolation Enhancements

## Overview

Enhanced the git worktree development workflow with better environment management, visual differentiation, shared configuration handling, and module skip capabilities. These enhancements make it easier to work with multiple feature branches simultaneously while maintaining proper isolation.

## Implemented Features

### ✅ Phase 1.1: Branch from Existing Features

**Status**: Already implemented in Rust CLI

The Rust implementation already supports branching from any base branch via the `--base` flag:

```bash
# Branch from an existing feature branch
branchbox feature start --name my-sub-feature --base feature/parent-feature

# Branch from main (default)
branchbox feature start --name my-feature
```

This was a limitation of the old bash workflow that the Rust implementation has already addressed.

### ✅ Phase 3.2: Enhanced Branch Registry System

**Files Modified**:
- `core/src/workflows/feature.rs`

**Changes**:
- Added `color` field to `FeatureMetadata` for workspace color coding
- Added `pr_number` field for GitHub PR integration (populated later)
- Added `last_commit` field to track the latest commit SHA
- Implemented deterministic color generation from feature names using a predefined palette
- Added utility function `generate_feature_color()` with 12 distinguishable colors
- Added `get_last_commit_sha()` to retrieve commit information

**Registry Format**:
```json
{
  "version": 1,
  "features": [{
    "work_feature": "oauth-integration",
    "branch_name": "feature/oauth-integration",
    "worktree_path": "/path/to/oauth-integration",
    "base_branch": "main",
    "feature_url": "oauth-integration.example.com",
    "status": "active",
    "created_at": "2025-10-24T...",
    "color": "#3498db",
    "pr_number": null,
    "last_commit": "abc123..."
  }]
}
```

### ✅ Phase 2.1-2.3: Visual Differentiation

**Files Modified**:
- `core/src/workflows/feature.rs` - Added `setup_vscode_workspace()` and `setup_devcontainer_postcommand()` methods
- `.devcontainer/devcontainer.json` - Added Peacock extension
- `cli/src/commands/feature.rs` - Display workspace color in output

**Features**:

1. **Peacock Extension Integration**:
   - Automatically sets a deterministic color for each feature workspace
   - Colors are generated from feature name hash for consistency
   - 12-color palette ensures good visual distinction

2. **Window Title Customization**:
   - Format: `${rootName} [feature-name] - ${activeEditorShort}`
   - Makes it easy to identify which feature you're working on

3. **Quick URL Access Task**:
   - VS Code task: "Open Feature URL"
   - Opens the feature's URL in browser
   - Works on Linux (xdg-open) and macOS (open)

4. **Automatic Git Worktree Fix** (NEW):
   - Programmatically fixes git worktree paths immediately after creation
   - Converts absolute paths to relative paths for devcontainer compatibility
   - No user repo pollution - fix runs entirely within branchbox code

**Workspace Configuration**:

The workflow automatically creates `.vscode/settings.json`:
```json
{
  "peacock.color": "#3498db",
  "window.title": "${rootName} [oauth-integration] - ${activeEditorShort}"
}
```

And `.vscode/tasks.json`:
```json
{
  "version": "2.0.0",
  "tasks": [{
    "label": "Open Feature URL",
    "type": "shell",
    "command": "xdg-open 'https://oauth-integration.example.com' || open 'https://oauth-integration.example.com'"
  }]
}
```

**How the Git Worktree Fix Works**:

When you create a git worktree, git stores an absolute path in the `.git` file:
```
gitdir: /Users/rbarazi/projects/agentify/main/.git/worktrees/oauth-integration
```

This breaks inside devcontainers where paths are mounted differently (`/workspaces/...`).

**Branchbox automatically fixes this** by:
1. Reading the `.git` file in the worktree immediately after creation
2. Extracting the main repo name from the absolute path
3. Rewriting with a relative path:
   ```
   gitdir: ../main/.git/worktrees/oauth-integration
   ```
4. No modification to user's `devcontainer.json` - it's all done programmatically!

This happens in `core/src/workflows/feature.rs:1141` via the `fix_git_worktree_path()` method.

### ✅ Phase 4.1: Module Skip Flags

**Files Modified**:
- `core/src/workflows/feature.rs` - Added `skip_modules` to `StartRequest`
- `core/src/modules/mod.rs` - Updated `detect_modules()` signature
- `cli/src/commands/feature.rs` - Added `--skip-module` CLI flag

**Usage**:

```bash
# Skip the tunnel module during feature start
branchbox feature start --name my-feature --skip-module tunnel

# Skip multiple modules
branchbox feature start --name my-feature --skip-module tunnel --skip-module database
```

**Available Modules**:
- `compose` - Docker Compose configuration management
- `database` - Database isolation and setup
- `tunnel` - Cloudflare tunnel provisioning
- `specs` - Feature specification lifecycle tracking

**Implementation Details**:
- `detect_modules()` now accepts a skip list and filters out modules by name
- Skip list is passed from CLI → StartRequest → module detection
- During teardown, no modules are skipped (empty list)
- New test `test_skip_modules()` verifies functionality

## Testing

All existing tests pass, plus new test coverage:

- `test_skip_modules()` - Verifies module skip functionality
- Existing integration tests verify color generation and VS Code setup
- 78 tests passing in total

## Usage Examples

### Creating a Feature with Visual Differentiation

```bash
$ branchbox feature start --name oauth-integration

🚀 Feature workspace ready
  Worktree: /workspaces/oauth-integration
  Branch: feature/oauth-integration
  Workspace color: #3498db
  Feature URL: https://oauth-integration.example.com
  Compose project: agentify-oauth-integration
  .env copied to: /workspaces/oauth-integration/.env
  Adapter: Rails
  Service URL: http://web:3000

Modules:
  - compose (ok)
  - database (ok)
  - tunnel (ok)
  - specs (ok)
```

### Branching from Another Feature

```bash
# Create a sub-feature from an existing feature
$ branchbox feature start --name oauth-google --base feature/oauth-integration

🚀 Feature workspace ready
  Worktree: /workspaces/oauth-google
  Branch: feature/oauth-google
  Workspace color: #e74c3c
  ...
```

### Skipping Optional Modules

```bash
# Start a feature without Cloudflare tunnel setup
$ branchbox feature start --name quick-test --skip-module tunnel

🚀 Feature workspace ready
  Worktree: /workspaces/quick-test
  Branch: feature/quick-test
  Workspace color: #2ecc71
  ...

Modules:
  - compose (ok)
  - database (ok)
  - specs (ok)
```

## Known Limitations

1. **PR Number**: Currently set to `null` during feature creation. Can be populated later via `gh` CLI integration.

2. **Color Palette**: Limited to 12 colors. With more features, colors will repeat (deterministically based on hash).

3. **VS Code Settings**: The workspace settings are created/updated during feature start. Manual changes to `.vscode/settings.json` will be overwritten on next feature start.

## Future Enhancements

From the original spec, these items were not implemented:

- **Phase 1.2**: Environment variable isolation (`.devcontainer/.env` separate from root `.env`)
- **Phase 3.1**: Shared configuration directory (centralized Claude Code/Codex config)
- **Phase 4.2**: Interactive module selection with preferences
- **Phase 5**: Container image optimization
- **Phase 6**: Improved feature spec management

These can be implemented in future iterations as needed.

## Migration Notes

This feature is fully backward compatible:

- Existing features in the registry will have `color`, `pr_number`, and `last_commit` as `null`
- The registry version remains at 1
- Old workflows continue to work as before
- New fields are optional and skipped during serialization if null

## Related Documentation

- [Git Worktree Architecture](../../architecture/README.md)
- [Feature Start Guide](../../architecture/feature-start-guide.md)
- [Module System](../../architecture/modules.md)
