---
status: backlog
created: 2025-10-30
updated: 2025-10-30
priority: high
complexity: medium
reviewed: true
architecture: module-based
---

# Devcontainer Propagation and Shared Tool Configuration Management

## Executive Summary

**Goal**: Ensure every feature worktree has a complete devcontainer setup AND shares tool configurations (Claude, Codex, GitHub CLI) across all worktrees for seamless development, while paving the way for per-worktree Cloudflare tunnels.

**Core Problem**: Currently, when you run `branchbox feature start`, the new worktree lacks `.devcontainer/` files, preventing VSCode/Cursor from reopening in a container. Additionally, while the shared configuration volume mounting mechanism exists in templates, it has minor inconsistencies and needs better documentation.

**Architectural Approach**: Implement as a **composable module** following the existing `Module` trait pattern, rather than hard-coding into the workflow. This enables:
- Proper dependency management
- Optional/pluggable behavior
- Sync capability for updating stale devcontainers
- Consistent lifecycle hooks (init/setup/teardown)

**Result**:
- Feature worktrees get complete devcontainer configuration automatically via `DevcontainerModule`
- All worktrees share Claude, Codex, and gh credentials (template fixes)
- Cloudflare tunnels are provision-ready for per-worktree credentials managed outside shared volumes
- Single authentication per tool across all features
- Seamless "Reopen in Container" workflow in VSCode/Cursor
- Ability to sync devcontainer updates: `branchbox devcontainer sync`

## Current State Analysis

### ✅ **What Works:**
1. **Shared config volume mounts exist** - `compose.yaml` templates have `SHARED_CONFIG_DIR` volume mounts
2. **Environment variable support** - `.env.sample` documents `SHARED_CONFIG_DIR`
3. **Default value logic** - `${SHARED_CONFIG_DIR:-../..}` correctly points to parent directory
4. **Actual mounting works** - Confirmed via `mount` command showing volumes are active

### ❌ **What's Broken:**

#### 1. **Devcontainer Not Copied to Feature Worktrees**

**Current behavior:**
```bash
cd /workspaces/branchbox
branchbox feature start "auth-feature"
cd ../branchbox-auth-feature
# VSCode cannot "Reopen in Container" - no .devcontainer/ directory!
```

**Expected behavior:**
```bash
branchbox feature start "auth-feature"
cd ../branchbox-auth-feature
ls .devcontainer/
# Output: devcontainer.json  compose.yaml  Dockerfile
# VSCode can now "Reopen in Container"
```

**Root cause**: `/workspaces/devcontainers/core/src/workflows/feature.rs:589` calls `link_env_into_devcontainer()` which creates the `.devcontainer` directory, but **does not copy the actual devcontainer configuration files** from the main worktree.

#### 2. **Template Directory Name Mismatch**

**Files affected:**
- `core/src/bootstrap/templates/rust/compose.yaml:17`
- `core/src/bootstrap/templates/rails/compose.yaml:17`
- `core/src/bootstrap/templates/nodejs/compose.yaml:17`
- `core/src/bootstrap/templates/generic/compose.yaml:17`

**Previous (incorrect) state:**
```yaml
- ${SHARED_CONFIG_DIR:-../..}/.claude-code:/home/vscode/.claude
```

**Current fix (implemented in templates and docs):**
```yaml
- ${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude
```

**Evidence**: `/home/vscode/.claude` directory exists and contains active configs (confirmed via `ls -la ~`).

**Impact**: New projects generated with `branchbox init` will mount to wrong directory name.

#### Claude Credential Path Verification

- Anthropic's CLI stores session files in `~/.claude/` (confirmed in the devcontainer: `/home/vscode/.claude/` plus `.claude.json` artifacts).
- Docs, templates, and module tests should reference that canonical path to avoid regressions.

#### 3. **Cloudflared Strategy Not Reflected in Docs**

Milestone 1 migrates tunnel provisioning to the Cloudflare APIs, generating per-worktree credentials and env files (see `docs/features/completed/rust-workflow-migration.md`). The spec still references the legacy shared-volume approach, which no longer aligns with the planned implementation.

#### 4. **Documentation Gap**

`README.md` doesn't explain:
- How devcontainers work in the BranchBox workflow
- The "Reopen in Container" process in VSCode/Cursor
- How shared configs enable single sign-on for tools
- Troubleshooting devcontainer issues

## Problem Statement

BranchBox's killer feature is **isolated feature worktrees with complete development environments**. However, this breaks down when:

1. **Feature worktrees lack devcontainer configs** - Can't use isolated Docker environments
2. **Tools require re-authentication** - Without shared configs, users must `gh auth login`, `claude login`, `codex login` in every feature worktree
3. **Workflow is unclear** - Users don't know how to open feature worktrees in containers
4. **Template inconsistencies** - Bootstrap-generated projects have wrong volume paths

This undermines the "zero context-switching" promise.

## User Scenarios

### Scenario 1: Starting a New Feature (Primary Use Case)

```bash
# User is in main worktree (opened in devcontainer)
cd /workspaces/branchbox
branchbox feature start "Add OAuth Integration"

# Output shows worktree created
cd ../branchbox-oauth-integration

# Current (broken):
code .  # VSCode opens, but "Reopen in Container" is unavailable

# Expected (fixed):
code .  # VSCode opens
# → VSCode detects .devcontainer/
# → Prompts: "Reopen in Container?"
# → User clicks "Reopen in Container"
# → Container starts with same config as main
# → User's gh/claude/codex credentials work immediately
```

### Scenario 2: Switching Between Multiple Features

```bash
# User has 3 active features
ls /workspaces/
# branchbox/               # main (container running)
# branchbox-oauth/         # feature 1 (container running)
# branchbox-api-refactor/  # feature 2 (no container yet)

cd /workspaces/branchbox-api-refactor
code .
# Expected: VSCode prompts "Reopen in Container"
# → Clicking "Yes" starts isolated container
# → Shares credentials with other containers
# → All 3 containers use same gh token, claude session, etc.
```

### Scenario 3: Collaborator Onboarding

```bash
# New team member clones main repo
git clone https://github.com/company/project
cd project

# Opens existing feature worktree from PR
branchbox feature start --reuse oauth-integration

cd ../project-oauth-integration
code .
# Expected: VSCode prompts "Reopen in Container"
# → Container uses project's devcontainer config
# → Prompts for gh auth (first time only)
# → gh token saved to ~/projects/.gh/hosts.yml
# → All future worktrees use same token
```

### Scenario 4: Bootstrap New Project

```bash
mkdir myapp && cd myapp
git init
branchbox init --stack rails

# Expected: .devcontainer/ created with correct volume mounts
cat .devcontainer/compose.yaml | grep claude
# Output: - ${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude

# Start using immediately
code .  # Reopen in Container works
gh auth login  # Credentials saved to ~/projects/.gh/
branchbox feature start "first feature"
cd ../myapp-first-feature
code .  # Reopen in Container works, gh already authenticated
```

## Critical Design Review

### Issues with Original Proposal

**❌ Wrong Abstraction Level**: The original draft proposed adding 118 lines of devcontainer copying logic directly to `core/src/workflows/feature.rs`. This violates the established module architecture and creates:
- Tight coupling between workflow orchestration and devcontainer management
- No dependency management (can't ensure compose module runs first)
- No reusability (can't sync updates independently)
- Harder to test in isolation

**❌ Staleness Problem**: One-time copy during `feature start` means:
- Feature worktrees become stale when main repo's `.devcontainer/` is updated
- No mechanism to propagate devcontainer improvements to existing features
- Manual copying required for updates

**❌ Code Duplication**: The proposed `copy_dir_recursive()` helper reinvents existing Rust ecosystem solutions (`fs_extra`, `walkdir`)

**❌ Hardcoded Assumptions**:
- Assumes `.devcontainer/` is always at repo root (breaks monorepos)
- Hardcodes file list instead of discovering files
- No configuration for copy vs symlink strategy
- No exclusion patterns (unnecessarily copies `.env` when it's already symlinked)

### ✅ Better Approach: DevcontainerModule

Following the existing `Module` trait pattern used by `SpecsModule`, `ComposeModule`, etc:

```rust
// core/src/modules/devcontainer.rs
pub struct DevcontainerModule {
    source_dir: PathBuf,
    strategy: SyncStrategy,  // Copy or Symlink
    exclude_patterns: Vec<String>,
}

impl Module for DevcontainerModule {
    fn name(&self) -> &str { "devcontainer" }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join(".devcontainer").exists()
    }

    fn init(&mut self, main_dir: &Path, _feature_dir: &Path) -> Result<()> {
        self.source_dir = main_dir.join(".devcontainer");
        // Discover what files exist, validate structure
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        // Sync devcontainer files to feature worktree
        self.sync_to(feature_dir)
    }

    fn teardown(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        // Optional: cleanup devcontainer in feature worktree
        Ok(())
    }

    fn dependencies(&self) -> &[&str] {
        &[]  // No dependencies, runs early
    }
}
```

**Benefits**:
- **Composable**: Automatically detected and executed in module pipeline
- **Testable**: Easy to unit test in isolation
- **Reusable**: Can be called independently for sync operations
- **Configurable**: Strategy pattern for copy vs symlink
- **Discoverable**: Scans actual files instead of hardcoding list
- **Safe**: Respects exclude patterns (don't duplicate `.env`)

## Architecture Design

### Component 1: DevcontainerModule Implementation

**Location**: `core/src/modules/devcontainer.rs` (new file)

**Elegant, minimal implementation (~60 lines vs 118)**:

```rust
//! Devcontainer Module
//!
//! Manages devcontainer configuration synchronization between main repo and feature worktrees

use super::Module;
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy)]
pub enum SyncStrategy {
    /// Copy files (default - allows per-feature customization)
    Copy,
    /// Symlink files (updates propagate automatically but no customization)
    Symlink,
}

pub struct DevcontainerModule {
    source_dir: PathBuf,
    strategy: SyncStrategy,
    /// Files to exclude (e.g., .env is already symlinked separately)
    exclude: Vec<String>,
}

impl DevcontainerModule {
    pub fn new() -> Self {
        Self {
            source_dir: PathBuf::new(),
            strategy: SyncStrategy::Copy,
            exclude: vec![".env".to_string(), ".gitignore".to_string()],
        }
    }

    /// Sync devcontainer files to target directory
    pub fn sync_to(&self, target_dir: &Path) -> Result<SyncOutcome> {
        let dest = target_dir.join(".devcontainer");
        if !dest.exists() {
            std::fs::create_dir_all(&dest)?;
        }

        let mut synced_files = Vec::new();

        // Walk source directory, sync all files except excluded ones
        for entry in WalkDir::new(&self.source_dir)
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| !self.is_excluded(e.path()))
        {
            let entry = entry.map_err(|e| Error::validation(format!("Failed to walk: {}", e)))?;
            let rel_path = entry
                .path()
                .strip_prefix(&self.source_dir)
                .map_err(|e| Error::validation(format!("Path strip failed: {}", e)))?;
            let dest_path = dest.join(rel_path);

            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&dest_path)?;
            } else {
                match self.strategy {
                    SyncStrategy::Copy => {
                        std::fs::copy(entry.path(), &dest_path)?;
                    }
                    SyncStrategy::Symlink => {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::symlink;
                            if dest_path.exists() {
                                std::fs::remove_file(&dest_path)?;
                            }
                            let rel_source = pathdiff::diff_paths(
                                entry.path(),
                                dest_path.parent().unwrap()
                            ).ok_or_else(|| Error::validation("Path diff failed"))?;
                            symlink(rel_source, &dest_path)?;
                        }
                        #[cfg(not(unix))]
                        {
                            std::fs::copy(entry.path(), &dest_path)?;
                        }
                    }
                }
                synced_files.push(rel_path.display().to_string());
                tracing::debug!("Synced .devcontainer/{}", rel_path.display());
            }
        }

        Ok(SyncOutcome {
            synced_files,
            strategy: self.strategy,
        })
    }

    fn is_excluded(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| self.exclude.iter().any(|e| n == e))
            .unwrap_or(false)
    }
}

impl Default for DevcontainerModule {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for DevcontainerModule {
    fn name(&self) -> &str {
        "devcontainer"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join(".devcontainer").exists()
    }

    fn init(&mut self, main_dir: &Path, _feature_dir: &Path) -> Result<()> {
        self.source_dir = main_dir.join(".devcontainer");

        if !self.source_dir.exists() {
            return Err(Error::validation(format!(
                "Devcontainer directory not found: {}",
                self.source_dir.display()
            )));
        }

        // Check for strategy override via env var
        if let Ok(strategy) = std::env::var("BRANCHBOX_DEVCONTAINER_STRATEGY") {
            self.strategy = match strategy.to_lowercase().as_str() {
                "symlink" => SyncStrategy::Symlink,
                _ => SyncStrategy::Copy,
            };
        }

        tracing::info!(
            "Devcontainer module initialized (strategy: {:?})",
            self.strategy
        );
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Syncing devcontainer configuration...");
        let outcome = self.sync_to(feature_dir)?;
        tracing::info!(
            "Synced {} devcontainer files ({:?})",
            outcome.synced_files.len(),
            outcome.strategy
        );
        Ok(())
    }

    fn teardown(&self, _main_dir: &Path, _feature_dir: &Path) -> Result<()> {
        // No cleanup needed - devcontainer removed with worktree
        Ok(())
    }

    fn validate(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        let devcontainer = feature_dir.join(".devcontainer");
        if !devcontainer.exists() {
            return Err(Error::validation(
                "Feature worktree missing .devcontainer directory".to_string()
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SyncOutcome {
    pub synced_files: Vec<String>,
    pub strategy: SyncStrategy,
}
```

**Dependencies to add** (`Cargo.toml`):
```toml
walkdir = "2.4"      # Recursive directory walking
pathdiff = "0.2"     # Relative path calculation for symlinks
```

**Registration** (`core/src/modules/mod.rs`):
```rust
pub mod devcontainer;
pub use devcontainer::{DevcontainerModule, SyncStrategy};

pub fn all_modules() -> Vec<Box<dyn Module>> {
    vec![
        Box::new(DevcontainerModule::new()),  // Add this
        Box::new(ComposeModule::new()),
        Box::new(DatabaseModule::new()),
        Box::new(TunnelModule::new()),
        Box::new(SpecsModule::new()),
    ]
}
```

### Component 2: Shared Config Mounts - Data-Driven Approach

**Problem**: Currently, shared config volume mounts are duplicated across 4 template files. Every adjustment (like fixing the Claude directory) requires editing 4 files. This doesn't scale.

**Better approach**: Extract to a constant/config that's reused across templates.

**Create shared volume mount definition** (`core/src/bootstrap/templates/mod.rs`):

```rust
pub const SHARED_CONFIG_MOUNTS: &[(&str, &str)] = &[
    (".codex", "/home/vscode/.codex"),
    (".claude", "/home/vscode/.claude"),
    (".gh", "/home/vscode/.config/gh"),
];

pub fn render_shared_volumes() -> String {
    SHARED_CONFIG_MOUNTS
        .iter()
        .map(|(host_dir, container_path)| {
            format!("    - ${{SHARED_CONFIG_DIR:-../..}}/{}:{}", host_dir, container_path)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

**Update templates to use helper** (all 4 templates):
```yaml
volumes:
  - ../..:/workspaces:cached
  - dind-data:/var/lib/docker
  # Shared tool configurations - managed via SHARED_CONFIG_DIR
{{ render_shared_volumes() }}
```

**Immediate fixes** (applied in this iteration):
- [x] `core/src/bootstrap/templates/rust/compose.yaml:17` - `.claude-code` → `.claude`
- [x] `core/src/bootstrap/templates/rails/compose.yaml:17` - `.claude-code` → `.claude`
- [x] `core/src/bootstrap/templates/nodejs/compose.yaml:17` - `.claude-code` → `.claude`
- [x] `core/src/bootstrap/templates/generic/compose.yaml:17` - `.claude-code` → `.claude`
- [x] Confirm all shared-volume mounts reference the canonical Claude directory (`~/.claude`)

**Future**: Migrate to template engine (handlebars, tera) for proper code generation.

### Component 3: Fix Current Project's compose.yaml

**File**: `.devcontainer/compose.yaml`

**Changes applied**:
- Update shared-volume mounts to use `.claude` (not `.claude-code`)
- Leave Cloudflare tunnel credentials to the per-worktree provisioning flow (see Milestone 1 design notes)

### Component 4: Documentation in README.md

Add new section after "Usage Examples" and before "Installation":

## Devcontainer Workflow

BranchBox features work seamlessly with VS Code/Cursor devcontainers, giving each feature its own isolated Docker environment while sharing tool credentials.

### Opening a Feature in a Container

When you start a new feature, BranchBox automatically copies the `.devcontainer/` configuration from your main repository:

```bash
# In main repo
branchbox feature start "Add OAuth"

# Navigate to new worktree
cd ../myapp-oauth/

# Open in VS Code/Cursor
code .
```

**VS Code/Cursor will prompt**: "Reopen in Container?"

Click **"Reopen in Container"** and your feature will run in an isolated Docker environment with:
- ✓ Separate Docker network (no port conflicts)
- ✓ Isolated database (for Rails/Node.js projects)
- ✓ Same development environment as main repo
- ✓ Shared tool credentials (see below)

### Shared Tool Credentials

All feature worktrees share authentication for common development tools, so you only need to log in once:

**Supported tools:**
- **GitHub CLI** (`gh`) - Credentials stored in `~/.config/gh/`
- **Claude Code** (`claude`) - Session stored in `~/.claude/` (Anthropic CLI default)
- **Codex** (`codex`) - Config stored in `~/.codex/`

**Upcoming**:
- **Cloudflare tunnels** - Provisioned via API per worktree (no shared volume). See `docs/features/completed/rust-workflow-migration.md` for tunnel background.

**How it works:**

1. **First time** - Authenticate in your main worktree:
   ```bash
   cd ~/projects/myapp  # main worktree
   code .  # Reopen in Container
   gh auth login  # Authenticate once
   claude login  # Authenticate once
   ```

2. **All features inherit** - Open any feature worktree:
   ```bash
   branchbox feature start "new feature"
   cd ../myapp-new-feature
   code .  # Reopen in Container
   gh repo view  # Already authenticated!
   claude chat  # Already authenticated!
   ```

3. **Credentials persist** - Stored in parent directory (`~/projects/`), mounted read-write to all containers via `SHARED_CONFIG_DIR` environment variable.

### How Shared Configs Work

The `.devcontainer/compose.yaml` file contains volume mounts that share tool configurations:

```yaml
volumes:
  - ${SHARED_CONFIG_DIR:-../..}/.gh:/home/vscode/.config/gh
  - ${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude
  - ${SHARED_CONFIG_DIR:-../..}/.codex:/home/vscode/.codex
```

**Directory structure:**
```
~/projects/
├── .gh/              # Shared GitHub CLI credentials
├── .claude/          # Shared Claude session
├── .codex/           # Shared Codex config
├── myapp/            # Main worktree devcontainer mounts these
├── myapp-feature1/   # Feature 1 devcontainer mounts same directories
└── myapp-feature2/   # Feature 2 devcontainer mounts same directories
```

**Override location** (optional):
```bash
# .env file
SHARED_CONFIG_DIR=/custom/path  # Change where configs are mounted from
```

**Default**: `../..` (parent directory of your project)

### Troubleshooting Devcontainers

**Problem**: "Reopen in Container" option not available

**Solution**: Check that `.devcontainer/` exists in feature worktree:
```bash
ls .devcontainer/
# Should show: devcontainer.json  compose.yaml  Dockerfile
```

If missing, manually copy from main repo:
```bash
cp -r ../myapp/.devcontainer .
```

**Problem**: Tools require re-authentication in each container

**Solution**: Verify shared config mounts are active:
```bash
# Inside container
mount | grep -E '(gh|claude|codex)'
# Should show volume mounts from host
```

Check `SHARED_CONFIG_DIR` in `.env`:
```bash
grep SHARED_CONFIG_DIR .env
```

**Problem**: Container fails to start

**Solution**: Validate devcontainer configuration:
```bash
branchbox init --validate  # (future feature)
```

Manually check:
```bash
# Check JSON syntax
cat .devcontainer/devcontainer.json | jq .

# Check YAML syntax
cat .devcontainer/compose.yaml | yq .
```

## Implementation Plan (Module-Based Approach)

### Phase 1: Core Module Implementation (2-3 days)
- [ ] Create `core/src/modules/devcontainer.rs` with `DevcontainerModule`
- [ ] Implement `Module` trait: `detect()`, `init()`, `setup()`, `teardown()`, `validate()`
- [ ] Add `SyncStrategy` enum (Copy/Symlink) with env var configuration
- [ ] Implement `sync_to()` method using `walkdir`
- [ ] Add exclusion logic for `.env` and `.gitignore`
- [ ] Add dependencies: `walkdir = "2.4"`, `pathdiff = "0.2"` to `Cargo.toml`
- [ ] Register module in `core/src/modules/mod.rs::all_modules()`
- [ ] Unit tests for `DevcontainerModule` (detection, sync, exclusion)

### Phase 2: Template Fixes (1 day)
- [x] Fix `.claude-code` → `.claude` in all 4 template `compose.yaml` files
- [x] Update comments and docs to reference the Claude default directory (`~/.claude`)
- [x] Fix current project's `.devcontainer/compose.yaml` to use `.claude`
- [x] Update `.env.sample` to document `BRANCHBOX_DEVCONTAINER_STRATEGY`
- [ ] Add test: verify bootstrap generates correct volume mounts

### Phase 3: Sync Command (1-2 days)
- [x] Add `branchbox devcontainer sync` CLI command
- [x] Implement: iterate all feature worktrees, call `DevcontainerModule::sync_to()`
- [x] Add `--strategy` flag to override copy/symlink behavior
- [x] Add `--dry-run` flag to preview changes
- [x] Integration test: sync updates existing feature worktrees

- [x] Add "Devcontainer Workflow" section to `README.md`
- [x] Document `BRANCHBOX_DEVCONTAINER_STRATEGY` env var
- [x] Document `branchbox devcontainer sync` command
- [x] Add troubleshooting guide for "Reopen in Container" issues
- [x] Update `docs/DEVELOPMENT.md` with module architecture notes

### Phase 5: Testing & Validation (1-2 days)
- [ ] Integration test: Create feature, verify `.devcontainer/` exists
- [ ] Integration test: Verify excluded files (`.env`) not duplicated
- [ ] Integration test: Test both Copy and Symlink strategies
- [ ] Manual: Create feature, open in VS Code, verify "Reopen in Container" works
- [ ] Manual: Authenticate `gh` in main, verify token works in feature
- [ ] Manual: Update main `.devcontainer/`, run `sync`, verify features updated
- [ ] Test on Linux, macOS, Windows/WSL2

**Total estimate**: 6-9 days (vs original 10-15 days)

**Complexity reduction**:
- ~60 lines of module code vs 118+ lines of workflow code
- Reuses existing `Module` infrastructure (no new patterns)
- Uses battle-tested crates (`walkdir`) vs custom recursion
- Naturally testable via module isolation

## Testing Strategy

### Unit Tests

**File**: `core/src/modules/devcontainer.rs` (in `#[cfg(test)]` module)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect_with_devcontainer() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".devcontainer")).unwrap();

        let module = DevcontainerModule::new();
        assert!(module.detect(temp.path()));
    }

    #[test]
    fn test_detect_without_devcontainer() {
        let temp = TempDir::new().unwrap();
        let module = DevcontainerModule::new();
        assert!(!module.detect(temp.path()));
    }

    #[test]
    fn test_sync_to_copies_files() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        // Create source devcontainer
        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(
            main.join(".devcontainer/devcontainer.json"),
            r#"{"name": "test"}"#
        ).unwrap();
        std::fs::write(
            main.join(".devcontainer/compose.yaml"),
            "services: {}"
        ).unwrap();

        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        let outcome = module.sync_to(&feature).unwrap();

        // Verify files synced
        assert!(feature.join(".devcontainer/devcontainer.json").exists());
        assert!(feature.join(".devcontainer/compose.yaml").exists());
        assert_eq!(outcome.synced_files.len(), 2);
        assert!(matches!(outcome.strategy, SyncStrategy::Copy));
    }

    #[test]
    fn test_sync_excludes_env_file() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::write(main.join(".devcontainer/.env"), "SECRET=123").unwrap();
        std::fs::write(main.join(".devcontainer/devcontainer.json"), "{}").unwrap();
        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        let outcome = module.sync_to(&feature).unwrap();

        // .env should be excluded
        assert!(!feature.join(".devcontainer/.env").exists());
        // But other files should sync
        assert!(feature.join(".devcontainer/devcontainer.json").exists());
        assert_eq!(outcome.synced_files.len(), 1);
    }

    #[test]
    fn test_sync_with_subdirectories() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer/scripts")).unwrap();
        std::fs::write(
            main.join(".devcontainer/scripts/setup.sh"),
            "#!/bin/bash"
        ).unwrap();
        std::fs::create_dir_all(&feature).unwrap();

        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        module.sync_to(&feature).unwrap();

        // Verify subdirectory synced
        assert!(feature.join(".devcontainer/scripts/setup.sh").exists());
    }

    #[test]
    fn test_strategy_from_env_var() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        let feature = temp.path().join("feature");

        std::fs::create_dir_all(main.join(".devcontainer")).unwrap();
        std::fs::create_dir_all(&feature).unwrap();

        std::env::set_var("BRANCHBOX_DEVCONTAINER_STRATEGY", "symlink");
        let mut module = DevcontainerModule::new();
        module.init(&main, &feature).unwrap();
        std::env::remove_var("BRANCHBOX_DEVCONTAINER_STRATEGY");

        assert!(matches!(module.strategy, SyncStrategy::Symlink));
    }
}
```

### Integration Tests

**File**: `cli/tests/feature_commands.rs` (add to existing test suite)

```rust
#[test]
fn test_devcontainer_module_syncs_to_feature() {
    let temp = setup_test_repo();
    let repo_path = temp.path();

    // Create .devcontainer in main repo
    std::fs::create_dir_all(repo_path.join(".devcontainer")).unwrap();
    std::fs::write(
        repo_path.join(".devcontainer/devcontainer.json"),
        r#"{"name": "Test"}"#
    ).unwrap();
    std::fs::write(
        repo_path.join(".devcontainer/compose.yaml"),
        "services: {}"
    ).unwrap();
    std::fs::write(repo_path.join(".env"), "APP_URL=dev.test\n").unwrap();

    std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

    let workflow = FeatureWorkflow::new(repo_path);
    let summary = workflow.start(StartRequest {
        work_feature: "test-sync".to_string(),
        base_branch: None,
        reuse_existing: false,
        skip_modules: vec![],
        branch_prefix: None,
        telemetry: false,
    }).unwrap();

    // Verify devcontainer module ran and synced files
    let feature_devcontainer = summary.worktree_path.join(".devcontainer");
    assert!(feature_devcontainer.join("devcontainer.json").exists());
    assert!(feature_devcontainer.join("compose.yaml").exists());

    // Verify .env is symlinked (not copied)
    assert!(feature_devcontainer.join(".env").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink_metadata;
        let metadata = symlink_metadata(feature_devcontainer.join(".env")).unwrap();
        assert!(metadata.file_type().is_symlink());
    }

    // Verify module reports
    assert!(summary.module_reports.iter().any(|r| r.name == "devcontainer"));
}
```

### Manual Testing Checklist

- [ ] Start main repo in devcontainer
- [ ] Run `branchbox feature start "test"`
- [ ] Navigate to feature worktree
- [ ] Run `ls .devcontainer/` - verify files exist
- [ ] Open in VS Code - verify "Reopen in Container" prompt appears
- [ ] Reopen in container - verify container starts successfully
- [ ] Run `gh auth login` in main container
- [ ] Open feature container - run `gh auth status` - verify already authenticated
- [ ] Repeat test with Cursor IDE
- [ ] Test on all platforms (Linux, macOS, Windows/WSL2)

## Success Metrics

- ✅ **100% devcontainer propagation** - All feature worktrees get `.devcontainer/` files
- ✅ **Zero re-authentication** - Tools authenticated in main work in all features
- ✅ **Template correctness** - Bootstrap generates valid volume mounts
- ✅ **Documentation clarity** - New users understand workflow within 5 minutes
- ✅ **Test coverage** - 90%+ line coverage for new code

## Security Considerations

### Credential Isolation

**Risk**: Shared credentials mean compromise in one container affects all.

**Mitigation**:
- Credentials stored in parent directory (outside any single worktree)
- Docker containers run as `vscode` user (not root)
- Volume mounts are per-user (not system-wide)
- Users can override `SHARED_CONFIG_DIR` for isolation if needed

### Volume Mount Safety

**Risk**: Incorrect paths could expose unintended files.

**Mitigation**:
- Volume mounts use explicit paths (no wildcards)
- Default `SHARED_CONFIG_DIR=../..` is relative, not absolute
- Mounts are read-write only for specific directories
- Docker-in-Docker isolation prevents container-to-host escapes

### .env File Security

**Risk**: `.env` files contain secrets, symlinked into devcontainers.

**Mitigation**:
- `.env` already in `.gitignore` (set by feature workflow)
- Symlinks preserve Unix permissions (0600 recommended)
- Feature-specific `.env` sections isolated via branchbox markers
- Never commit `.devcontainer/.env` symlink to git

## Open Questions

### 1. Should we copy or symlink devcontainer files?

**Current approach**: Copy files

**Alternative**: Symlink (like `.env`)

**Decision**: **COPY** is correct because:
- Each feature may need customizations (different ports, env vars)
- Breaking symlink (delete main repo) would break all features
- Copies allow evolution of devcontainer config per feature

### 2. What if main repo has no .devcontainer?

**Current approach**: Warn but continue (feature workflow skips devcontainer copy)

**Alternative**: Error and force user to run `branchbox init` first

**Decision**: **WARN** is correct because:
- User might be using non-containerized workflow
- Shouldn't block feature creation
- Can add devcontainer later

### 3. Should we validate devcontainer files before copying?

**Current approach**: Copy blindly, let Docker/VS Code validate

**Alternative**: Parse JSON/YAML and validate before copy

**Decision**: **VALIDATE** optionally (future enhancement):
- Phase 1: Copy without validation (simple, fast)
- Phase 2: Add `--validate` flag for paranoid users
- Phase 3: Add `branchbox config validate` command

### 4. How to handle devcontainer config drift?

**Scenario**: Main repo updates `.devcontainer/`, feature worktrees now outdated.

**Proposed solution** (future enhancement):
```bash
branchbox config sync  # Update all feature devcontainers from main
```

**Decision**: Defer to future milestone (not blocking for MVP)

## Dependencies

### Existing Crates (no new dependencies)
- `std::fs` - File operations (copy, create dirs)
- `anyhow` - Error handling
- `tracing` - Logging

### Future Crates (for validation enhancement)
- `serde_json` - Validate `devcontainer.json` (already used elsewhere)
- `serde_yaml` - Validate `compose.yaml` (already used elsewhere)

## References

- Devcontainer specification: https://containers.dev/
- Git worktree docs: https://git-scm.com/docs/git-worktree
- Docker Compose spec: https://docs.docker.com/compose/compose-file/
- VS Code devcontainer: https://code.visualstudio.com/docs/devcontainers/containers
- Current implementation: `core/src/workflows/feature.rs:589` (`link_env_into_devcontainer`)
- Bootstrap templates: `core/src/bootstrap/templates/*/compose.yaml`

## Timeline (Revised)

- **Day 1-3**: Core `DevcontainerModule` implementation + unit tests
- **Day 4**: Template fixes (`.claude-code` → `.claude`, doc updates for Claude path)
- **Day 5-6**: `branchbox devcontainer sync` command + integration tests
- **Day 7**: Documentation (README, DEVELOPMENT.md, env vars)
- **Day 8-9**: Manual testing across platforms, bug fixes

**Total**: 6-9 days to production-ready (vs original 10-15 days)

## Summary of Improvements

### Architectural Improvements ✅

| Aspect | Original Proposal | Improved Design |
|--------|------------------|----------------|
| **Abstraction** | Hard-coded in workflow | Composable `Module` |
| **Lines of code** | 118+ lines | ~60 lines |
| **Reusability** | None - copy only on start | Reusable via `sync` command |
| **Testability** | Coupled to workflow | Isolated unit tests |
| **Dependencies** | Manual ordering | Automatic via `Module` trait |
| **File discovery** | Hardcoded list | Dynamic via `walkdir` |
| **Exclusions** | None - copies everything | Configurable exclude list |
| **Strategy** | Copy only | Copy or Symlink (configurable) |
| **Staleness** | No solution | `branchbox devcontainer sync` |
| **Extensibility** | Requires workflow changes | Add to module list |

### Code Quality Improvements ✅

1. **No reinventing the wheel**: Uses `walkdir` instead of custom recursive copy
2. **No hardcoded assumptions**: Discovers files dynamically
3. **Configurable behavior**: `BRANCHBOX_DEVCONTAINER_STRATEGY` env var
4. **Follows existing patterns**: Mirrors `SpecsModule` architecture
5. **Better error handling**: Leverages `Module` trait's `Result<()>` contract
6. **Proper separation of concerns**: Workflow orchestrates, module executes

### User Experience Improvements ✅

1. **Automatic sync**: Just run `branchbox feature start`, devcontainer appears
2. **Manual sync**: Run `branchbox devcontainer sync` to update all features
3. **Strategy choice**: Users can choose copy vs symlink per project
4. **Dry run**: Preview changes with `--dry-run` flag
5. **Debugging**: Clear module reports show what was synced

### Timeline Improvements ✅

- **40% faster**: 6-9 days vs 10-15 days
- **Lower complexity**: Reuses existing infrastructure
- **Better maintainability**: Module isolation reduces coupling
- **Easier testing**: Unit tests don't need full workflow setup

## Related Features

- Universal Init Workflow (depends on this for devcontainer copying)
- Cloudflare Tunnel Provisioning (per-worktree credentials via Cloudflare API - Milestone 1)
- Agent Daemon (will need access to shared configs - Milestone 1)

## Migration Plan

- **Inventory active worktrees**: Extend `branchbox feature list --json` so the CLI can feed the devcontainer sync command with an authoritative set of worktrees.
- **One-time bulk sync**: Ship a helper script (`scripts/devcontainer-sync.sh`) invoked by maintainers after upgrading BranchBox to the module-based release. Script runs `branchbox devcontainer sync --strategy copy` and captures a report per worktree for auditability.
- **Workspace bootstrap**: Update onboarding docs to instruct new contributors to run `branchbox devcontainer sync` after cloning older repositories lacking `.devcontainer/`.
- **Graceful fallback**: If sync fails (e.g., corrupted `.devcontainer`), log a warning, mark the worktree in the registry with a `devcontainer_outdated` flag, and skip blocking the workflow. CLI surfaces this flag in `branchbox feature list`.

## Rollout Checklist

- Feature flag the module with `BRANCHBOX_ENABLE_DEVCONTAINER_MODULE=1` for canary testing.
- Validate on a real project with 3+ parallel worktrees, including at least one Rails and one Rust repository.
- Confirm VS Code and Cursor both prompt "Reopen in Container" post-sync.
- Ensure the shared credential directories remain untouched when running `branchbox feature teardown`.
- Publish an internal changelog entry outlining new commands, env vars, and troubleshooting guidance.

## Telemetry & Observability

- Emit `module.devcontainer.sync` tracing spans with attributes: `strategy`, `synced_files`, `duration_ms`, `outcome`.
- Add a lightweight metrics exporter in the future daemon to count sync successes vs. failures.
- Record module status in the workflow summary JSON (already exposed), allowing the control plane to surface stale worktrees in dashboards.
- Capture CLI exit codes for `branchbox devcontainer sync` and wire them into existing analytics (Milestone 1 agent work).

## Risks & Mitigations

- **Risk**: Symlink strategy on macOS requires developer to grant additional permissions.
  - **Mitigation**: Default to copy, document symlink caveats, and gate symlink with explicit opt-in.
- **Risk**: Large devcontainer directories inflate sync time.
  - **Mitigation**: Add `.branchboxignore` support in `.devcontainer/` to skip heavy assets (e.g., prebuilt language servers).
- **Risk**: Shared credentials leak across teams sharing a workstation.
  - **Mitigation**: Document per-user `SHARED_CONFIG_DIR` overrides and encourage `branchbox devcontainer sync --strategy symlink` only for trusted environments.
- **Risk**: Module errors block feature workflows.
  - **Mitigation**: Treat failures as soft errors with actionable warnings, and surface remediation steps directly in CLI output.

## Future Enhancements

- Integrate JSON/YAML schema validation (`jsonschema`, `schemars`) to catch malformed devcontainer files before sync completes.
- Allow per-feature overrides by honoring `.devcontainer/.branchbox-local.toml` with exclusion/inclusion lists.
- Teach the module to diff existing files and show a concise changelog when running with `--dry-run`.
- Coordinate with the upcoming agent daemon to trigger background syncs whenever main `.devcontainer/` changes (file watcher integration).
- Explore selective sync for multi-root repos, mapping `.devcontainer` directories per package or workspace member.
