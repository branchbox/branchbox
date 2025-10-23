---
worktree: /Users/rbarazi/projects/agentify/devcontainer-env-isolation
branch: feature/devcontainer-env-isolation
work_feature: devcontainer-env-isolation
url: https://rida-mbp-agentify-devcontainer-env-isolation.rida.me
status: in_progress
created: 2025-10-17
---

# Dev Container Environment Isolation Enhancements

## Overview

Improve the git worktree development workflow with better environment management, visual differentiation, and shared configuration handling. These enhancements will make it easier to work with multiple feature branches simultaneously while maintaining proper isolation and reducing configuration drift.

## Current State Analysis

✅ **Already Implemented (Shell Workflow):**
- Git worktree creation from main branch via `bin/feature-start`
- Branch-specific environment variables (WORK_FEATURE, APP_URL, COMPOSE_PROJECT_NAME)
- Container naming isolation per feature
- Cloudflare tunnel provisioning (manual + Cloudflare API helpers in `lib/utils/cloudflare-api.sh`)
- Claude Code persistence with branch-isolated volumes
- Feature spec lifecycle management (backlog → in-progress → completed)
- Bash-based module & adapter system orchestrated from `lib/feature-start`
- Relative path git worktree fix script (`.devcontainer/fix-git-worktree.sh`)

⚠️ **Known Issues:**
1. Shell workflow can only branch from `main`, not from existing feature branches
2. Environment variables mixed with secrets in single `.env` file
3. No visual differentiation between feature workspaces in VS Code/Cursor
4. Shared configs (Claude Code, Codex) not centralized
5. Cannot skip optional setup steps (e.g., Cloudflare tunnel)
6. No branch registry or metadata tracking system
7. Feature spec created before branch, causing commit noise
8. Rust CLI lacks parity with shell workflow (see `rust-workflow-migration` feature)

## Requirements

### Phase 1: Branching & Environment Management

#### 1.1 Branch from Existing Features
- [ ] Modify `bin/feature-start` to detect current branch
- [ ] Prompt user: "Branch from current feature or main?"
- [ ] Support creating sub-features from any existing feature branch
- [ ] Update feature spec frontmatter to include `base_branch` field
- [ ] Test creating feature → sub-feature → sub-sub-feature

#### 1.2 Environment Variable Isolation
- [ ] Create `.devcontainer/.env` as symlink or separate file for feature-specific vars
- [ ] Keep shared config/secrets in parent `.env`
- [ ] Document in CLAUDE.md: "Adding new env vars requires updating main branch"
- [ ] Update `bin/feature-start` to handle split env files
- [ ] Update compose.yaml to load both env files in correct order

**Current structure:**
```bash
# .env (contains everything)
ADMIN_EMAIL=...
OPENAI_API_KEY=...
APP_URL=...
WORK_FEATURE=feature-name  # <- Feature-specific
```

**Proposed structure:**
```bash
# .env (shared secrets - stays in main)
ADMIN_EMAIL=...
OPENAI_API_KEY=...

# .devcontainer/.env (feature-specific - symlinked)
WORK_FEATURE=feature-name
APP_URL=rida-mbp-agentify-feature-name.rida.me
COMPOSE_PROJECT_NAME=agentify-feature-name
DEVCONTAINER_NAME=agentify-feature-name
```

### Phase 2: Visual Differentiation

#### 2.1 Peacock Extension Integration
- [ ] Add `peacock.vscode-peacock` to devcontainer extensions
- [ ] Generate deterministic color from feature name hash
- [ ] Set workspace color in `.vscode/settings.json` during feature-start
- [ ] Color palette: use distinguishable hues for common feature names

#### 2.2 Window Title Customization
- [ ] Set `window.title` in workspace settings to include WORK_FEATURE
- [ ] Format: `${rootName} [${WORK_FEATURE}] - ${activeEditorShort}`
- [ ] Display APP_URL in status bar (custom extension or task)

#### 2.3 Quick Access to Feature URL
- [ ] Create VS Code task: "Open Feature URL"
- [ ] Task reads APP_URL from .env and opens in browser
- [ ] Add to `.vscode/tasks.json` during feature-start
- [ ] Keyboard shortcut suggestion: Cmd+Shift+O

### Phase 3: Shared Configuration Management

#### 3.1 Centralized Config Directory

**Three implementation approaches:**

**A. Shared bind mount (recommended for sharing):**
```json
// .devcontainer/devcontainer.json
"mounts": [
  "source=${localWorkspaceFolder}/../.shared-config/claude,target=/home/vscode/.config/claude,type=bind"
]
```

**Benefits:**
- ✅ Single authentication across all branches
- ✅ Shared conversation history and context
- ✅ Easy to backup (regular directory)
- ✅ Can reference conversations from other features
- ❌ Mixed context from different branches
- ❌ Harder to isolate feature-specific work

**B. Per-branch Docker volume (current documented approach):**
```json
// .devcontainer/devcontainer.json
"mounts": [
  "source=claude-config-${localWorkspaceFolderBasename},target=/home/vscode/.config/claude,type=volume"
]
```

**Benefits:**
- ✅ Complete isolation per feature branch
- ✅ Clean context for each feature
- ✅ Conversations clearly tied to specific work
- ❌ Re-authenticate per branch
- ❌ Can't reference other branch conversations
- ❌ More disk usage

**C. Hybrid approach (best of both worlds):**
```json
// Main branch: shared mount
"mounts": [
  "source=${localWorkspaceFolder}/.shared-config/claude,target=/home/vscode/.config/claude,type=bind"
]

// Feature branches: isolated volumes
"mounts": [
  "source=claude-config-${localWorkspaceFolderBasename},target=/home/vscode/.config/claude,type=volume"
]
```

Configure during `bin/feature-start` based on user preference.

**Tasks:**
- [ ] Decide on approach (A, B, or C)
- [ ] Create `.shared-config/` directory structure
- [ ] Add `mounts` configuration to devcontainer.json
- [ ] Update bin/feature-start to configure mounts based on choice
- [ ] Add Codex config to same strategy
- [ ] Document decision in CLAUDE.md

**Recommendation:** Start with **Option A (shared)** since Claude Code conversations are valuable cross-feature context. Can switch to Option B or C later if isolation is needed.

#### 3.2 Branch Registry System
- [ ] Create `.shared-config/branches.json` as registry
- [ ] Track: branch name, worktree path, color, URL, status, created date, PR number
- [ ] Update registry in `bin/feature-start` and `bin/feature-teardown`
- [ ] Create `bin/feature-list` to display active features with colors
- [ ] Create `bin/feature-status` to show health of all branches

**Registry format:**
```json
{
  "branches": [
    {
      "name": "feature/oauth-integration",
      "work_feature": "oauth-integration",
      "worktree_path": "/workspaces/agentify/oauth-integration",
      "base_branch": "main",
      "url": "https://rida-mbp-agentify-oauth-integration.rida.me",
      "color": "#3498db",
      "status": "active",
      "created": "2025-10-17",
      "pr_number": 123,
      "last_commit": "2025-10-18T10:30:00Z"
    }
  ]
}
```

### Phase 4: Optional Step Skipping

#### 4.1 Module System Enhancement
- [ ] Add module skip flags: `--skip-tunnel`, `--skip-database`, etc.
- [ ] Update module-interface.sh to check skip flags
- [ ] Modules gracefully handle being skipped
- [ ] Document available skip flags in feature-start-guide.md

#### 4.2 Interactive Module Selection
- [ ] If no skip flags: prompt "Configure Cloudflare tunnel? [Y/n]"
- [ ] Store preferences in `.shared-config/feature-start-preferences.json`
- [ ] Future runs: "Use previous preferences? [Y/n]"

### Phase 5: Container Image Optimization

#### 5.1 Base Image Strategy
- [ ] Create pre-built base devcontainer image
- [ ] Includes: Ruby, Node, PostgreSQL client, all extensions
- [ ] Publish to GitHub Container Registry
- [ ] Only rebuild when Dockerfile changes, not per-feature

#### 5.2 Configuration-Only Features
- [ ] Separate configuration from image build
- [ ] Use `postCreateCommand` for customization
- [ ] Feature-specific setup: database name, env vars only
- [ ] Reduces feature-start time from 5min → 30sec

### Phase 6: Workflow Improvements

#### 6.1 Feature Spec Management
- [ ] Move feature spec creation AFTER successful worktree creation
- [ ] Commit feature spec on the feature branch, not main
- [ ] Add frontmatter after worktree is confirmed working
- [ ] Reduces noise in main branch commits

#### 6.2 Stash Handling
- [ ] Current: prompts to stash changes before feature-start
- [ ] Improvement: detect if stash includes feature spec file
- [ ] Auto-exclude feature spec from stash
- [ ] Apply stash to new worktree automatically

## Testing Strategy

### Unit Tests
- [ ] Test branch detection logic
- [ ] Test color generation from feature name
- [ ] Test registry JSON read/write operations
- [ ] Test env file splitting and merging

### Integration Tests
- [ ] Test feature-start from main branch
- [ ] Test feature-start from feature branch (sub-feature)
- [ ] Test feature-start with --skip-tunnel flag
- [ ] Test feature-teardown updates registry
- [ ] Test multiple features running simultaneously
- [ ] Test Claude Code shared config access

### System Tests
- [ ] Full workflow: start → develop → teardown
- [ ] Verify visual differentiation works
- [ ] Verify branch registry accuracy
- [ ] Verify environment isolation maintained
- [ ] Verify no config drift between features

## Documentation

- [ ] Update `docs/architecture/README.md` with new features
- [ ] Update `docs/architecture/feature-start-guide.md` with skip flags
- [ ] Create `docs/architecture/branch-registry-guide.md`
- [ ] Update `CLAUDE.md` with env variable management guidelines
- [ ] Create troubleshooting guide for common issues

## Success Criteria

- ✅ Can create sub-features from any existing feature branch
- ✅ Each feature workspace has distinct visual appearance (color, title)
- ✅ Shared configs centralized and accessible from all branches
- ✅ Branch registry provides accurate status of all features
- ✅ Can skip optional setup steps without breaking workflow
- ✅ Feature creation time reduced by using base images
- ✅ Main branch stays clean (no feature spec commits)
- ✅ Environment variable management is clear and documented

## Open Questions

1. **Shared vs Isolated Claude Code config?**
   - Option A: Shared auth, shared history (centralized)
   - Option B: Per-branch auth, isolated history (current)
   - **Recommendation:** Stay with per-branch isolation for better context separation

2. **Environment file strategy?**
   - Option A: Symlink `.devcontainer/.env` → `../.env`
   - Option B: Separate files, compose loads both
   - Option C: Single `.env` with clear sections (current)
   - **Recommendation:** Option B for clearer separation

3. **Base image publishing?**
   - GitHub Container Registry (requires authentication)
   - Docker Hub (easier but public)
   - Local registry (fastest but manual)
   - **Recommendation:** Start with local, move to GHCR when stable

## Implementation Priority

**High Priority (Phase 1):**
- Branch from existing features
- Environment variable isolation
- Feature spec workflow fix

**Medium Priority (Phase 2 & 4):**
- Visual differentiation (Peacock, window title)
- Optional step skipping
- Branch registry system

**Low Priority (Phase 3 & 5):**
- Shared config directory (evaluate need first)
- Container image optimization (performance improvement, not blocker)

## Related Documentation

- [Git Worktree Architecture](../../architecture/README.md)
- [Feature Start Guide](../../architecture/feature-start-guide.md)
- [Feature Teardown Guide](../../architecture/feature-teardown-guide.md)
- [Claude Code Persistence](../../architecture/claude-code-persistence.md)
