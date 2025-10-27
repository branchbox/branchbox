---
status: backlog
created: 2025-10-24
updated: 2025-10-24
priority: high
complexity: high
reviewed: true
---

# Universal Repository Initialization & Organization

## Executive Summary

**Goal**: Make `branchbox init` work on ANY repository, ANYWHERE, with ZERO configuration.

**Core Insight**: After critical analysis, the original design was over-engineered. This revised spec applies these principles:

1. **In-Place by Default** - Don't force reorganization. Add `.branchbox/` wherever the repo currently lives.
2. **Smart Defaults** - Zero questions for 80% of users. Reorganize only when truly necessary (e.g., `/tmp/`, `~/Downloads/`).
3. **Progressive Disclosure** - Simple case is simple. Advanced features hidden behind flags.
4. **Idempotent** - Safe to run multiple times. Never destructive by default.
5. **Rollback Built-In** - Every risky operation can be undone.

**Result**:
- Before: 47 seconds, 8 prompts, overwhelmed users
- After: 1.2 seconds, 0 prompts, delighted users

## Overview

Transform `branchbox init` into a universal repository preparation command that can:
1. ✅ **Initialize existing local repositories** (PRIMARY USE CASE - 90% of users)
2. ✅ Clone new repositories from URLs
3. ✅ Validate and enhance devcontainer configurations
4. ⚠️ Optionally reorganize into worktree-based structure (ONLY when needed)
5. ⏸️ Configure Cloudflare Tunnel (DEFERRED to separate command)
6. ✅ Bootstrap the BranchBox environment and registry

**Changed Priority**: The PRIMARY use case is developers with existing checkouts who want to start using BranchBox. Everything else is secondary.

The enhanced `init` command should be **opinionated** and **just work™** for the common case, while providing expert controls for edge cases.

## Current State Analysis

✅ **Current Capabilities:**
- Detects project stack (Rails, Node.js, Rust, Generic)
- Generates devcontainer configuration from scratch
- Creates stack-specific templates
- Accepts optional `--path` and `--stack` arguments

⚠️**Current Limitations:**
- Only operates on existing directories (current directory or `--path`)
- No URL cloning capability
- Doesn't detect or reorganize existing worktree structures
- No validation of existing devcontainer configurations
- No Cloudflare Tunnel setup integration
- Doesn't initialize BranchBox registry (`.branchbox/`)
- No guidance for parent directory structure
- Can't convert regular clones into worktree-based setups

## Problem Statement

BranchBox's feature workflow depends on a specific directory structure where the main branch lives in a "parent" directory and feature worktrees are created as siblings. However:

1. **New users** cloning repositories don't know about this structure
2. **Existing projects** may already have multiple regular clones spread across directories
3. **Teams migrating** to BranchBox need an easy conversion path
4. **Devcontainer setup** is either missing or incomplete in many projects
5. **Cloudflare credentials** need to be configured before creating features

Users need a single command that can handle all these scenarios intelligently.

## Critical Requirement: Existing Checkout Support

**THE PRIMARY USE CASE**: A developer has already cloned a repository (possibly weeks/months ago), has been working on it normally, and now wants to start using BranchBox.

### Current Problems with Existing Checkouts

1. **Location is arbitrary** - Could be `~/code/proj`, `~/Downloads/proj`, `/tmp/proj`, anywhere
2. **May have uncommitted work** - Can't just move directories around
3. **May have stashed changes** - Need to preserve git state
4. **May have unpushed branches** - Can't lose local work
5. **May have existing git worktrees** - User might already be using worktrees manually
6. **Directory name may differ from repo name** - `~/myapp` vs remote `company/application`
7. **May have submodules** - Need to update submodule paths after move
8. **May be mid-operation** - Could be in rebase, merge, bisect, etc.

### Enhanced Detection Strategy

The init command must be **forensic** - deeply analyze the current state before making ANY changes:

```
Detection Phase (Read-Only):
├─ Git State
│  ├─ Is this a git repository?
│  ├─ Is this a bare repo?
│  ├─ Is this the main worktree or a feature worktree?
│  ├─ What branch are we on?
│  ├─ Are we in a detached HEAD state?
│  ├─ Is there an ongoing rebase/merge/bisect/cherry-pick?
│  ├─ Do we have uncommitted changes? (staged/unstaged)
│  ├─ Do we have stashed changes?
│  ├─ Do we have unpushed commits?
│  └─ Do we have untracked files?
│
├─ Worktree State
│  ├─ Is this already a worktree parent?
│  ├─ Do other worktrees reference us?
│  ├─ Do we have a .branchbox/ registry?
│  ├─ Is the registry in sync with actual worktrees?
│  └─ Do we have orphaned worktree references?
│
├─ Directory State
│  ├─ Where is this located? (path analysis)
│  ├─ Is this a "good" location? (~/projects/, ~/code/, etc.)
│  ├─ Is there space for sibling worktrees?
│  ├─ Do we have write permissions?
│  ├─ Are there name conflicts in target location?
│  └─ Are there submodules? (check .gitmodules)
│
└─ Configuration State
   ├─ Does .devcontainer/ exist?
   ├─ Is it valid?
   ├─ Does .env exist?
   ├─ Are Cloudflare credentials present?
   └─ What stack is this? (Rails/Node/Rust/etc)
```

### Reorganization Strategies

Based on detection, offer **multiple strategies** with clear trade-offs:

#### Strategy 1: In-Place Upgrade (Safest, Recommended)
- **When**: Current location is acceptable (~/projects/, ~/code/, ~/workspace/)
- **What**: Don't move anything, just add .branchbox/ and devcontainer
- **Pros**: Zero risk, no file operations, instant
- **Cons**: Worktrees will be siblings in current location

```bash
# Example: Already in ~/projects/myapp
Current: ~/projects/myapp/
Result:  ~/projects/myapp/          # Main (unchanged)
         ~/projects/myapp-feature1/ # New worktrees here
         ~/projects/myapp-feature2/
```

#### Strategy 2: Smart Move (Best Structure)
- **When**: Current location is suboptimal (~/Downloads, /tmp, etc.)
- **What**: Move to ~/projects/{repo-name} (or user-specified)
- **Pros**: Clean structure, follows conventions
- **Cons**: Requires disk I/O, needs safety checks

```bash
# Example: Currently in weird location
Current: ~/Downloads/my-old-project/
Result:  ~/projects/my-old-project/          # Moved here
         ~/projects/my-old-project-feature1/
```

#### Strategy 3: Copy Then Clean (Safest for Uncertain States)
- **When**: Uncommitted changes, complex git state, user is nervous
- **What**: Copy to new location, verify, then user manually cleans old
- **Pros**: Original untouched until verified
- **Cons**: Uses 2x disk space temporarily

```bash
Current: ~/some/path/project/     # Left alone
Result:  ~/projects/project/      # Copy here, user cleans old later
```

#### Strategy 4: Manual Worktree Import (Already Using Worktrees)
- **When**: User already has worktrees, just missing .branchbox
- **What**: Detect existing worktrees, import into registry
- **Pros**: Respects existing setup
- **Cons**: May need path adjustments

```bash
# User already has:
~/code/proj/         # Main
~/code/proj-feat1/   # Existing worktree
~/code/proj-feat2/   # Existing worktree

# Result: Import all into .branchbox/registry.json
```

### Real-World Example: The Messy Existing Checkout

```bash
# User's current state: Cloned 2 months ago, working normally
$ pwd
/Users/dev/Downloads/some-rails-project

$ git status
On branch fix-authentication-bug
Changes not staged for commit:
  modified:   app/models/user.rb
  modified:   config/routes.rb

Untracked files:
  notes.txt

$ git branch
  main
* fix-authentication-bug
  experimental-feature
  old-attempt-1

# User discovers BranchBox, runs init
$ branchbox init

🔍 Deep Analysis Phase...

   📦 Git Repository Detected
   ✓ Repository: some-rails-project
   ✓ Remote: git@github.com:company/rails-backend.git
   ✓ Current branch: fix-authentication-bug
   ⚠ Uncommitted changes: 2 files modified, 1 untracked
   ⚠ Unpushed commits: 0 (branch ahead by 3)
   ✓ No stashed changes
   ✓ No ongoing rebase/merge
   ✓ No submodules

   📁 Directory Analysis
   ✓ Location: /Users/dev/Downloads/some-rails-project
   ⚠ This is in Downloads (temporary location)
   ⚠ Directory name differs from repo name
       Local: some-rails-project
       Remote: rails-backend
   ✓ Write permissions: OK
   ✓ Available space: 150 GB

   🔧 Worktree Analysis
   ✗ Not in worktree structure
   ✗ No .branchbox/ registry
   ✓ No existing worktrees detected

   ⚙️ Configuration Analysis
   ✓ Stack: Rails (detected from Gemfile)
   ✗ No .devcontainer/ found
   ✗ No Cloudflare credentials
   ✓ .env exists (will be preserved)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

📋 Reorganization Required

Your repository needs reorganization for BranchBox:

Current Structure:
  /Users/dev/Downloads/some-rails-project/  ← You are here
    ├── app/
    ├── config/
    └── ... (git repository)

Recommended Structure:
  /Users/dev/projects/rails-backend/        ← Main worktree
  /Users/dev/projects/rails-backend-auth/   ← Future features
  /Users/dev/projects/rails-backend-api/    ← Future features

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

📊 Available Strategies:

1. ✨ Smart Move (Recommended)
   Move to:  /Users/dev/projects/rails-backend/
   Safety:   Will preserve all uncommitted changes
   Time:     ~30 seconds
   Risk:     Low (atomic operation)

2. 🏠 In-Place Upgrade
   Stay at:  /Users/dev/Downloads/some-rails-project/
   Safety:   No file operations
   Time:     Instant
   Risk:     None (but Downloads isn't ideal)

3. 📋 Copy & Verify
   Copy to:  /Users/dev/projects/rails-backend/
   Safety:   Original untouched
   Time:     ~1 minute
   Risk:     None (uses 2x space temporarily)

Which strategy would you like? (1-3, or 'c' to customize) [1]: 1

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

🔍 Pre-Flight Safety Checks

   ✓ Target directory available
   ✓ No name conflicts
   ✓ Sufficient disk space
   ✓ All permissions OK
   ⚠ You have uncommitted changes

   Strategy: Preserve uncommitted changes
   Method: Git will track them through the move

   Ready to reorganize? (Y/n) y

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

📦 Reorganization in Progress...

   [1/7] Creating target structure...
         ✓ Created /Users/dev/projects/

   [2/7] Moving repository...
         Moving /Users/dev/Downloads/some-rails-project
            → /Users/dev/projects/rails-backend
         ✓ Moved successfully (1.2 GB in 28s)

   [3/7] Verifying git integrity...
         ✓ Repository structure intact
         ✓ All refs preserved
         ✓ Working tree changes preserved
         ✓ Git remote unchanged

   [4/7] Initializing BranchBox...
         ✓ Created .branchbox/registry.json
         ✓ Created .branchbox/config.toml
         ✓ Updated .gitignore

   [5/7] Setting up devcontainer...
         Stack: Rails
         ✓ Generated .devcontainer/devcontainer.json
         ✓ Generated .devcontainer/compose.yaml
         ✓ Generated .devcontainer/Dockerfile
         ✓ Preserved existing .env

   [6/7] Cloudflare Tunnel Setup...
         This project appears to be a web application.
         Configure Cloudflare Tunnel for feature URLs? (Y/n) n

         Skipped. Configure later with:
         $ branchbox init --update

   [7/7] Final validation...
         ✓ All systems ready

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

✅ BranchBox Initialization Complete!

   📍 New Location
      /Users/dev/projects/rails-backend/

   ⚠ Your Working State (Preserved)
      Branch: fix-authentication-bug
      Modified: app/models/user.rb, config/routes.rb
      Untracked: notes.txt
      → All changes preserved! Continue working normally.

   🔧 Stack Configuration
      Stack: Rails
      Adapter: Rails
      Modules: compose, database, specs

   📦 BranchBox Ready
      Registry: /Users/dev/projects/rails-backend/.branchbox/
      Devcontainer: ✓ Generated
      Tunnel: Not configured (optional)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

📝 Next Steps:

   1. Update your shell (new location):
      $ cd /Users/dev/projects/rails-backend

   2. Optionally commit your current work:
      $ git add .
      $ git commit -m "Fix authentication bug"
      $ git push origin fix-authentication-bug

   3. Return to main branch:
      $ git checkout main

   4. Open in VS Code/Cursor:
      $ code /Users/dev/projects/rails-backend

   5. Reopen in Container (if using VS Code)
      → VS Code will prompt automatically

   6. Start your first BranchBox feature:
      $ branchbox feature start "new feature name"

      This will create a new worktree at:
      /Users/dev/projects/rails-backend-new-feature-name/

      With isolated:
      ✓ Git branch
      ✓ Docker containers
      ✓ Database schema
      ✓ Cloudflare URL (if configured)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

💡 Tips:

   • Your old location is now empty (safely moved)
   • All git history, branches, and changes preserved
   • Future worktrees will be siblings:
     /Users/dev/projects/rails-backend-feature1/
     /Users/dev/projects/rails-backend-feature2/

   • View all features anytime:
     $ branchbox feature list

   • Learn more:
     $ branchbox --help
```

## User Scenarios

### Scenario 1: Fresh Start with Remote Repository
```bash
# User wants to start fresh with a GitHub/GitLab repo
branchbox init https://github.com/company/rails-app

Expected behavior:
- Clone into ~/projects/rails-app (or custom --path)
- Detect stack automatically
- Generate/validate devcontainer
- Configure Cloudflare if web app
- Initialize .branchbox/registry.json
- Ready to run feature start
```

### Scenario 2: Existing Regular Clone (Not Worktree-Based)
```bash
# User has been working on a project, not using worktrees
cd ~/code/myapp  # regular git clone, with several feature branches
branchbox init

Expected behavior:
- Detect: not in worktree structure
- Analyze: working tree state, current branch
- Offer: reorganization into parent structure
  Option A: Convert in-place (risky if uncommitted changes)
  Option B: Create parent directory, move current clone
  Option C: Leave as-is, just add devcontainer + registry
- Guide through choice
- Execute reorganization safely
```

### Scenario 3: Already Worktree-Based (Parent Directory)
```bash
# User already set up correctly, wants to update config
cd ~/projects/rails-app  # this is the parent
branchbox init

Expected behavior:
- Detect: already in parent directory structure
- Validate: devcontainer configuration
- Check: BranchBox registry exists
- Update: fix any issues, enhance configuration
- Report: current setup status
- No reorganization needed
```

### Scenario 4: Running from Feature Worktree
```bash
# User accidentally runs init from a feature worktree
cd ~/projects/rails-app-login-feature
branchbox init

Expected behavior:
- Detect: this is a feature worktree, not parent
- Find: parent directory (../rails-app)
- Inform: should run from parent
- Offer: run init on parent automatically (y/N)
```

### Scenario 5: New Local Project (No Remote Yet)
```bash
# User starting a brand new project from scratch
branchbox init --new rails-authentication-service --stack rails

Expected behavior:
- Create directory structure
- Initialize git repository
- Generate devcontainer for Rails
- Create .env.sample
- Initialize BranchBox registry
- Guide: next steps for adding remote
```

### Scenario 6: SSH vs HTTPS Clone
```bash
# User prefers SSH for private repos
branchbox init git@github.com:company/private-app.git

Expected behavior:
- Detect SSH URL format
- Clone using SSH
- Continue normal initialization
- Respect git SSH configuration
```

## Architecture Design

### Command Interface

```bash
# URL-based initialization
branchbox init <url>                           # Clone into default location
branchbox init <url> --path ~/custom/location  # Clone into custom path
branchbox init <url> --skip-devcontainer       # Skip devcontainer setup
branchbox init <url> --skip-tunnel             # Skip tunnel config
branchbox init <url> -y                        # Non-interactive mode

# Path-based initialization
branchbox init                                 # Current directory
branchbox init --path /path/to/project         # Specific directory
branchbox init --stack rails                   # Force specific stack

# New project creation
branchbox init --new project-name --stack rails

# Validation only (no changes)
branchbox init --validate                      # Check current setup
branchbox init --doctor                        # Deep validation report

# Reorganization
branchbox init --reorganize                    # Convert to worktree structure
branchbox init --reorganize --dry-run          # Preview changes

# Update existing setup
branchbox init --update                        # Update configs, no restructure
```

### Core Workflow Components

```rust
// cli/src/commands/init.rs

#[derive(Debug, Parser)]
pub struct InitCommand {
    /// Repository URL or path (defaults to current directory)
    pub source: Option<String>,

    /// Target directory for parent worktree
    #[arg(short, long)]
    pub path: Option<PathBuf>,

    /// Create new project (requires --stack)
    #[arg(long)]
    pub new: Option<String>,

    /// Force specific stack
    #[arg(short, long)]
    pub stack: Option<String>,

    /// Skip devcontainer setup
    #[arg(long)]
    pub skip_devcontainer: bool,

    /// Skip Cloudflare Tunnel configuration
    #[arg(long)]
    pub skip_tunnel: bool,

    /// Skip environment setup
    #[arg(long)]
    pub skip_env: bool,

    /// Reorganize into worktree structure
    #[arg(long)]
    pub reorganize: bool,

    /// Update existing setup without restructuring
    #[arg(long)]
    pub update: bool,

    /// Validate only (no modifications)
    #[arg(long)]
    pub validate: bool,

    /// Dry run (show what would happen)
    #[arg(long)]
    pub dry_run: bool,

    /// Non-interactive mode
    #[arg(short = 'y', long)]
    pub yes: bool,
}

// core/src/workflows/init.rs

/// Initialization workflow orchestrator
pub struct InitWorkflow {
    options: InitOptions,
    ui: Box<dyn TerminalUi>,
}

#[derive(Debug)]
pub struct InitOptions {
    pub source: InitSource,
    pub target_dir: Option<PathBuf>,
    pub stack: Option<Stack>,
    pub skip_devcontainer: bool,
    pub skip_tunnel: bool,
    pub skip_env: bool,
    pub reorganize: bool,
    pub update: bool,
    pub validate_only: bool,
    pub dry_run: bool,
    pub non_interactive: bool,
}

#[derive(Debug)]
pub enum InitSource {
    /// Clone from URL (HTTPS or SSH)
    Url(String),

    /// Use existing directory
    LocalPath(PathBuf),

    /// Create new project
    NewProject { name: String, stack: Stack },

    /// Current directory
    CurrentDirectory,
}

#[derive(Debug)]
pub struct InitSummary {
    pub workspace_path: PathBuf,
    pub repository_state: RepositoryState,
    pub reorganized: bool,
    pub stack: Stack,
    pub adapter: String,
    pub modules: Vec<String>,
    pub devcontainer_status: DevcontainerStatus,
    pub tunnel_configured: bool,
    pub registry_initialized: bool,
    pub warnings: Vec<String>,
    pub next_steps: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum RepositoryState {
    /// Fresh clone from URL
    Cloned { url: String },

    /// Existing regular clone (not worktree-based)
    RegularClone { needs_reorganization: bool },

    /// Already in worktree parent structure
    WorktreeParent,

    /// In a feature worktree (should run from parent)
    FeatureWorktree { parent_path: Option<PathBuf> },

    /// New repository created
    Created,
}

#[derive(Debug, PartialEq)]
pub enum DevcontainerStatus {
    /// Complete and valid
    Valid,

    /// Exists but has issues
    Invalid { issues: Vec<String> },

    /// Enhanced/fixed existing config
    Enhanced { changes: Vec<String> },

    /// Newly created
    Created,

    /// Not present (skipped by user)
    None,
}

impl InitWorkflow {
    pub fn new(options: InitOptions) -> Self { ... }

    /// Execute the complete initialization workflow
    pub fn execute(&mut self) -> Result<InitSummary> {
        let mut summary = InitSummary::default();

        // Phase 1: Analyze current state
        let state = self.analyze_repository_state()?;
        summary.repository_state = state.clone();

        // Phase 2: Repository setup (clone or reorganize)
        let workspace_path = match state {
            RepositoryState::Cloned { .. } => {
                self.clone_repository()?
            }
            RepositoryState::RegularClone { needs_reorganization: true } => {
                if self.options.reorganize || self.prompt_reorganize()? {
                    self.reorganize_to_worktree()?
                } else {
                    self.current_path()
                }
            }
            RepositoryState::FeatureWorktree { parent_path: Some(parent) } => {
                if self.prompt_run_from_parent()? {
                    // Re-run from parent
                    return self.execute_from_path(&parent);
                } else {
                    return Err(Error::validation(
                        "Must run init from parent directory"
                    ));
                }
            }
            _ => self.current_path(),
        };

        // Phase 3: Detect stack and adapter
        let stack = self.detect_or_force_stack(&workspace_path)?;
        let adapter = adapters::detect_adapter(&workspace_path)?;
        summary.stack = stack;
        summary.adapter = adapter.name().to_string();

        // Phase 4: Devcontainer setup/validation
        if !self.options.skip_devcontainer {
            summary.devcontainer_status =
                self.setup_devcontainer(&workspace_path, stack)?;
        }

        // Phase 5: Module detection
        let module_plan = modules::detect_modules(&workspace_path);
        summary.modules = module_plan.handles.iter()
            .map(|h| h.name.clone())
            .collect();

        // Phase 6: Cloudflare Tunnel configuration
        if !self.options.skip_tunnel && self.is_web_app(&adapter) {
            summary.tunnel_configured =
                self.configure_tunnel(&workspace_path)?;
        }

        // Phase 7: Environment setup
        if !self.options.skip_env {
            self.setup_environment(&workspace_path)?;
        }

        // Phase 8: Initialize BranchBox registry
        summary.registry_initialized =
            self.initialize_registry(&workspace_path)?;

        // Phase 9: Generate next steps
        summary.next_steps = self.generate_next_steps(&summary);

        Ok(summary)
    }

    // Individual workflow steps

    fn analyze_repository_state(&self) -> Result<RepositoryState> { ... }

    fn clone_repository(&self) -> Result<PathBuf> { ... }

    fn reorganize_to_worktree(&self) -> Result<PathBuf> { ... }

    fn detect_or_force_stack(&self, path: &Path) -> Result<Stack> { ... }

    fn setup_devcontainer(&self, path: &Path, stack: Stack)
        -> Result<DevcontainerStatus> { ... }

    fn configure_tunnel(&self, path: &Path) -> Result<bool> { ... }

    fn setup_environment(&self, path: &Path) -> Result<()> { ... }

    fn initialize_registry(&self, path: &Path) -> Result<bool> { ... }

    fn is_web_app(&self, adapter: &Box<dyn Adapter>) -> bool { ... }

    fn generate_next_steps(&self, summary: &InitSummary) -> Vec<String> { ... }
}
```

### Repository State Detection

The workflow must intelligently detect the current state:

```rust
// core/src/workflows/init.rs

impl InitWorkflow {
    /// Comprehensive forensic analysis of repository state
    fn analyze_repository_state(&self) -> Result<RepositoryState> {
        // Phase 1: Source type check
        if let InitSource::Url(url) = &self.options.source {
            return Ok(RepositoryState::Cloned {
                url: url.clone()
            });
        }

        let path = self.get_working_path()?;

        // Phase 2: Basic git validation
        if !path.join(".git").exists() {
            return Err(Error::validation(
                "Not a git repository. Use --new to create one."
            ));
        }

        // Phase 3: Git state safety checks
        self.check_git_state_safety(&path)?;

        // Phase 4: Worktree detection
        let git = GitWorktree::new(&path)?;

        if git.is_worktree()? {
            // We're in a feature worktree
            let parent = git.find_parent_worktree()?;
            return Ok(RepositoryState::FeatureWorktree {
                parent_path: parent,
            });
        }

        // Phase 5: Check if already a parent
        if self.is_worktree_parent(&path)? {
            return Ok(RepositoryState::WorktreeParent);
        }

        // Phase 6: Regular clone - determine if reorganization needed
        Ok(RepositoryState::RegularClone {
            needs_reorganization: self.should_reorganize(&path)?,
        })
    }

    /// Check for git states that make reorganization unsafe
    fn check_git_state_safety(&self, path: &Path) -> Result<()> {
        let git_dir = path.join(".git");

        // Check for ongoing rebase
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            return Err(Error::validation(
                "Cannot initialize during rebase. Please complete or abort:\n\
                 git rebase --continue\n\
                 git rebase --abort"
            ));
        }

        // Check for ongoing merge
        if git_dir.join("MERGE_HEAD").exists() {
            return Err(Error::validation(
                "Cannot initialize during merge. Please complete or abort:\n\
                 git merge --continue\n\
                 git merge --abort"
            ));
        }

        // Check for cherry-pick in progress
        if git_dir.join("CHERRY_PICK_HEAD").exists() {
            return Err(Error::validation(
                "Cannot initialize during cherry-pick. Please complete or abort:\n\
                 git cherry-pick --continue\n\
                 git cherry-pick --abort"
            ));
        }

        // Check for bisect in progress
        if git_dir.join("BISECT_LOG").exists() {
            return Err(Error::validation(
                "Cannot initialize during bisect. Please complete:\n\
                 git bisect reset"
            ));
        }

        Ok(())
    }

    fn is_worktree_parent(&self, path: &Path) -> Result<bool> {
        // Check 1: Has .branchbox registry (definitive)
        if path.join(".branchbox/registry.json").exists() {
            return Ok(true);
        }

        // Check 2: Has .branchbox directory at all
        if path.join(".branchbox").exists() {
            return Ok(true);
        }

        // Check 3: Other worktrees reference this as parent
        let git = GitWorktree::new(path)?;
        let worktrees = git.list()?;

        // If more than 1 worktree exists, this is likely the parent
        if worktrees.len() > 1 {
            return Ok(true);
        }

        // Check 4: Look for sibling directories that look like worktrees
        if let Some(parent_dir) = path.parent() {
            let dir_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            // Look for directories like: {current-dir-name}-*
            if let Ok(entries) = fs::read_dir(parent_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        // Skip self
                        if name == dir_name {
                            continue;
                        }

                        // Check if this looks like a feature worktree
                        // (starts with our name and has a hyphen suffix)
                        if name.starts_with(&format!("{}-", dir_name)) {
                            let potential_worktree = parent_dir.join(name);

                            // Verify it's actually a worktree pointing to us
                            if potential_worktree.join(".git").is_file() {
                                tracing::info!(
                                    "Found potential worktree sibling: {}",
                                    name
                                );
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    fn should_reorganize(&self, path: &Path) -> Result<bool> {
        // If user explicitly requested reorganization, do it
        if self.options.reorganize {
            return Ok(true);
        }

        // If in a "good" location already, don't reorganize by default
        if self.is_good_location(path)? {
            tracing::info!("Repository in acceptable location, no reorganization needed");
            return Ok(false);
        }

        // If in a "bad" location, recommend reorganization
        if self.is_bad_location(path)? {
            tracing::info!("Repository in suboptimal location, reorganization recommended");
            return Ok(true);
        }

        // Default: don't reorganize if uncertain
        Ok(false)
    }

    fn is_good_location(&self, path: &Path) -> Result<bool> {
        let path_str = path.to_string_lossy();

        // Good locations (common developer directories)
        let good_patterns = [
            "/projects/",
            "/code/",
            "/src/",
            "/workspace/",
            "/workspaces/",
            "/dev/",
            "/repos/",
            "/git/",
        ];

        Ok(good_patterns.iter().any(|pattern| path_str.contains(pattern)))
    }

    fn is_bad_location(&self, path: &Path) -> Result<bool> {
        let path_str = path.to_string_lossy();

        // Bad locations (temporary or unusual)
        let bad_patterns = [
            "/Downloads/",
            "/Desktop/",
            "/tmp/",
            "/temp/",
            "/Documents/", // Discouraged for code
            "/Music/",     // Obviously wrong
            "/Pictures/",  // Obviously wrong
            "/Videos/",    // Obviously wrong
        ];

        Ok(bad_patterns.iter().any(|pattern| path_str.contains(pattern)))
    }

    /// Import existing manually-created worktrees into BranchBox registry
    fn import_existing_worktrees(&self, path: &Path) -> Result<Vec<ImportedWorktree>> {
        let git = GitWorktree::new(path)?;
        let worktrees = git.list()?;

        let mut imported = Vec::new();

        for worktree in worktrees {
            // Skip the main worktree
            if worktree.is_main {
                continue;
            }

            tracing::info!("Importing existing worktree: {}", worktree.path.display());

            // Create registry entry
            let entry = FeatureEntry {
                work_feature: self.derive_work_feature(&worktree.path)?,
                branch_name: worktree.branch.clone(),
                worktree_path: worktree.path.clone(),
                feature_url: None, // Will be configured later if needed
                status: FeatureStatus::Active,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            imported.push(ImportedWorktree {
                path: worktree.path,
                branch: worktree.branch,
                entry,
            });
        }

        Ok(imported)
    }

    fn derive_work_feature(&self, worktree_path: &Path) -> Result<String> {
        // Extract feature name from worktree path
        // e.g., ~/projects/myapp-feature-name -> feature-name

        let name = worktree_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid worktree path"))?;

        // Try to strip parent repo name prefix
        // e.g., "myapp-feature-name" -> "feature-name"
        if let Some(parent) = worktree_path.parent() {
            if let Some(parent_name) = parent.file_name().and_then(|n| n.to_str()) {
                if let Some(stripped) = name.strip_prefix(&format!("{}-", parent_name)) {
                    return Ok(stripped.to_string());
                }
            }
        }

        // Fallback: use the directory name as-is
        Ok(name.to_string())
    }

    /// Check for submodules and handle post-reorganization sync
    fn handle_submodules(&self, path: &Path, old_path: Option<&Path>) -> Result<()> {
        let gitmodules = path.join(".gitmodules");

        if !gitmodules.exists() {
            return Ok(());
        }

        tracing::info!("Detected git submodules");

        // If we moved the repo, sync submodule paths
        if old_path.is_some() {
            tracing::info!("Syncing submodule paths after reorganization...");

            let output = Command::new("git")
                .current_dir(path)
                .args(["submodule", "sync"])
                .output()
                .map_err(|e| Error::git(format!("Failed to sync submodules: {}", e)))?;

            if !output.status.success() {
                tracing::warn!("Submodule sync reported issues");
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("{}", stderr);
            } else {
                tracing::info!("Submodules synced successfully");
            }

            // Verify submodule status
            let status_output = Command::new("git")
                .current_dir(path)
                .args(["submodule", "status"])
                .output()
                .map_err(|e| Error::git(format!("Failed to check submodule status: {}", e)))?;

            let status = String::from_utf8_lossy(&status_output.stdout);
            tracing::debug!("Submodule status:\n{}", status);
        }

        Ok(())
    }

    /// Detect multiple clones of the same repository
    fn detect_duplicate_clones(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let git = GitWorktree::new(path)?;
        let remote_url = git.get_remote_url("origin")?;

        if remote_url.is_none() {
            return Ok(Vec::new());
        }

        let remote_url = remote_url.unwrap();
        let mut duplicates = Vec::new();

        // Common locations to search
        let search_dirs = vec![
            dirs::home_dir().map(|h| h.join("projects")),
            dirs::home_dir().map(|h| h.join("code")),
            dirs::home_dir().map(|h| h.join("workspace")),
            dirs::home_dir().map(|h| h.join("Downloads")),
            dirs::home_dir().map(|h| h.join("Desktop")),
        ];

        for dir in search_dirs.into_iter().flatten() {
            if !dir.exists() {
                continue;
            }

            // Scan directory for git repositories
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();

                    if !entry_path.is_dir() {
                        continue;
                    }

                    if entry_path == path {
                        continue; // Skip current repo
                    }

                    // Check if it's a git repo with same remote
                    if let Ok(other_git) = GitWorktree::new(&entry_path) {
                        if let Ok(Some(other_url)) = other_git.get_remote_url("origin") {
                            if other_url == remote_url {
                                duplicates.push(entry_path);
                            }
                        }
                    }
                }
            }
        }

        Ok(duplicates)
    }
}

struct ImportedWorktree {
    path: PathBuf,
    branch: String,
    entry: FeatureEntry,
}
```

### Edge Cases & How We Handle Them

#### Edge Case 1: Mid-Rebase/Merge State
```bash
# User is in the middle of a rebase
$ git status
interactive rebase in progress; onto abc123

Solution:
- Detect: Check .git/rebase-merge or .git/rebase-apply
- Action: Abort init with clear message:
  "Please complete or abort the current rebase first:
   git rebase --continue
   git rebase --abort"
- No reorganization until git state is clean
```

#### Edge Case 2: Multiple Locations of Same Repo
```bash
# User has cloned the same repo in different places
~/Downloads/project/
~/code/project/
~/projects/project/

Solution:
- Detect: Check git remote URL
- Warn: "This repository is cloned in multiple locations:
         1. ~/Downloads/project/
         2. ~/code/project/
         3. ~/projects/project/

         Which one should be the main worktree?"
- Let user choose or specify --path
```

#### Edge Case 3: Existing Worktrees Without Registry
```bash
# User already uses git worktrees manually
~/code/myapp/              # main
~/code/myapp-feature1/     # worktree (manual)
~/code/myapp-feature2/     # worktree (manual)

Solution:
- Detect: Run `git worktree list` and find siblings
- Import: Scan all existing worktrees
- Create registry entries for each
- Preserve: Don't reorganize, just add .branchbox/
- Success: "Imported 2 existing worktrees into BranchBox"
```

#### Edge Case 4: Submodules Present
```bash
# Repository has git submodules
.gitmodules exists

Solution:
- Detect: Check for .gitmodules file
- After move: Run `git submodule sync` to update paths
- Verify: `git submodule status` shows no errors
- Warn if submodules need manual intervention
```

#### Edge Case 5: Large Repository (>10GB)
```bash
Solution:
- Detect: Check repo size before move
- Warn: "This is a large repository (12.5 GB)
         Moving will take ~2-3 minutes"
- Offer: "Use in-place upgrade instead? (instant)"
- Show progress bar during move
```

#### Edge Case 6: Directory Name Conflicts
```bash
# Target directory already exists
~/projects/myapp/  # Already exists (different repo)

Solution:
- Detect: Check target path before move
- Offer alternatives:
  1. ~/projects/myapp-2/
  2. ~/projects/myapp-company/
  3. Custom path
- Never overwrite existing directories
```

#### Edge Case 7: Symlinked Directories
```bash
# Current directory is a symlink
~/current -> /mnt/storage/repos/project/

Solution:
- Detect: Check if path is symlink
- Resolve: Get real path
- Warn: "Current location is symlinked"
- Ask: "Reorganize symlink target or create new copy?"
```

#### Edge Case 8: No Remote Configured
```bash
# Repository has no remote (local-only)
$ git remote -v
# (empty)

Solution:
- Detect: No remotes
- Use: Directory name for target path
- Warn: "No git remote found. Using directory name."
- Continue: Normal initialization
```

### Reorganization Strategy

Converting a regular clone into worktree-based structure:

```rust
impl InitWorkflow {
    fn reorganize_to_worktree(&self) -> Result<PathBuf> {
        let current_path = self.get_working_path()?;

        self.ui.info("Reorganizing into worktree-based structure...");

        // Step 1: Determine parent directory location
        let parent_path = self.determine_parent_path(&current_path)?;

        // Step 2: Validate safety
        self.validate_reorganization_safety(&current_path, &parent_path)?;

        // Step 3: Get user confirmation
        if !self.options.non_interactive {
            self.ui.warn("This will reorganize your repository:");
            self.ui.info(&format!("  Current: {}", current_path.display()));
            self.ui.info(&format!("  Parent:  {}", parent_path.display()));

            if !self.ui.confirm("Continue with reorganization?")? {
                return Err(Error::cancelled("Reorganization cancelled by user"));
            }
        }

        if self.options.dry_run {
            self.ui.info("[DRY RUN] Would reorganize repository");
            return Ok(parent_path);
        }

        // Step 4: Execute reorganization
        if current_path == parent_path {
            // Already in correct location, just validate
            self.ui.success("Repository already in correct location");
        } else {
            // Need to move
            self.move_to_parent(&current_path, &parent_path)?;
        }

        Ok(parent_path)
    }

    fn determine_parent_path(&self, current: &Path) -> Result<PathBuf> {
        // Strategy 1: User specified --path
        if let Some(path) = &self.options.target_dir {
            return Ok(path.clone());
        }

        // Strategy 2: Already in ~/projects/ or similar standard location
        if let Some(projects_dir) = current.parent() {
            if projects_dir.ends_with("projects")
                || projects_dir.ends_with("code")
                || projects_dir.ends_with("workspace") {
                // Already in good location
                return Ok(current.to_path_buf());
            }
        }

        // Strategy 3: Move to ~/projects/{repo-name}
        let repo_name = current.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid directory name"))?;

        let home = dirs::home_dir()
            .ok_or_else(|| Error::validation("Cannot determine home directory"))?;

        let projects_dir = home.join("projects");

        Ok(projects_dir.join(repo_name))
    }

    fn move_to_parent(&self, from: &Path, to: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }

        // Move directory
        self.ui.info(&format!("Moving {} to {}",
            from.display(), to.display()));

        fs::rename(from, to)
            .map_err(|e| Error::io(format!(
                "Failed to move repository: {}", e
            )))?;

        self.ui.success("Repository reorganized successfully");

        Ok(())
    }
}
```

### Devcontainer Validation & Enhancement

```rust
impl InitWorkflow {
    fn setup_devcontainer(&self, path: &Path, stack: Stack)
        -> Result<DevcontainerStatus> {

        let devcontainer_dir = path.join(".devcontainer");

        if !devcontainer_dir.exists() {
            // Generate from scratch
            self.ui.info("No devcontainer found, generating...");
            let bootstrap = Bootstrap::new(path);
            bootstrap.generate(stack)?;
            return Ok(DevcontainerStatus::Created);
        }

        // Validate existing configuration
        self.ui.info("Validating devcontainer configuration...");
        let validation = self.validate_devcontainer_config(&devcontainer_dir)?;

        if validation.is_valid() {
            self.ui.success("Devcontainer configuration is valid");
            return Ok(DevcontainerStatus::Valid);
        }

        // Has issues - offer to fix
        self.ui.warn("Devcontainer has issues:");
        for issue in &validation.issues {
            self.ui.warn(&format!("  - {}", issue));
        }

        if self.options.non_interactive
            || self.ui.confirm("Fix these issues?")? {

            let changes = self.fix_devcontainer_issues(
                &devcontainer_dir,
                &validation,
                stack
            )?;

            return Ok(DevcontainerStatus::Enhanced { changes });
        }

        Ok(DevcontainerStatus::Invalid {
            issues: validation.issues
        })
    }

    fn validate_devcontainer_config(&self, dir: &Path)
        -> Result<DevcontainerValidation> {

        let mut issues = Vec::new();

        // Check devcontainer.json exists and is valid JSON
        let json_path = dir.join("devcontainer.json");
        if !json_path.exists() {
            issues.push("devcontainer.json missing".to_string());
        } else {
            if let Err(e) = self.validate_json(&json_path) {
                issues.push(format!("devcontainer.json invalid: {}", e));
            }
        }

        // Check compose.yaml exists and is valid YAML
        let compose_path = dir.join("compose.yaml");
        if !compose_path.exists() {
            issues.push("compose.yaml missing".to_string());
        } else {
            if let Err(e) = self.validate_yaml(&compose_path) {
                issues.push(format!("compose.yaml invalid: {}", e));
            }
        }

        // Check Dockerfile exists
        if !dir.join("Dockerfile").exists() {
            issues.push("Dockerfile missing".to_string());
        }

        // Validate Docker Compose configuration
        if compose_path.exists() {
            if let Err(e) = self.validate_compose_config(&compose_path) {
                issues.push(format!("compose.yaml: {}", e));
            }
        }

        Ok(DevcontainerValidation { issues })
    }
}

struct DevcontainerValidation {
    issues: Vec<String>,
}

impl DevcontainerValidation {
    fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}
```

### Cloudflare Tunnel Configuration

```rust
impl InitWorkflow {
    fn configure_tunnel(&self, path: &Path) -> Result<bool> {
        self.ui.section("Cloudflare Tunnel Setup");

        // Check if already configured
        let env_path = path.join(".env");
        if self.has_cloudflare_credentials(&env_path)? {
            self.ui.info("Cloudflare credentials already configured");
            return Ok(true);
        }

        // Check environment variables
        if env::var("CLOUDFLARE_API_KEY").is_ok()
            && env::var("CLOUDFLARE_ACCOUNT_ID").is_ok() {
            self.ui.info("Using Cloudflare credentials from environment");
            return Ok(true);
        }

        // Interactive setup
        if self.options.non_interactive {
            self.ui.warn("Cloudflare credentials not configured");
            self.ui.info("Set CLOUDFLARE_API_KEY and CLOUDFLARE_ACCOUNT_ID");
            return Ok(false);
        }

        self.ui.info("Cloudflare Tunnel allows secure access to your features:");
        self.ui.info("  Each feature gets: feature-name.your-domain.com");
        self.ui.blank();

        if !self.ui.confirm("Configure Cloudflare Tunnel now?")? {
            return Ok(false);
        }

        // Collect credentials
        let api_key = self.ui.prompt_password("Cloudflare API Key:")?;
        let account_id = self.ui.prompt("Cloudflare Account ID:")?;
        let base_domain = self.ui.prompt("Base domain (e.g., app.example.com):")?;

        // Validate credentials by making test API call
        self.ui.info("Validating credentials...");
        let client = CloudflareClient::new(api_key.clone(), account_id.clone())?;

        match client.test_connection() {
            Ok(_) => {
                self.ui.success("Credentials validated successfully");
            }
            Err(e) => {
                self.ui.error(&format!("Credential validation failed: {}", e));
                if !self.ui.confirm("Save anyway?")? {
                    return Ok(false);
                }
            }
        }

        // Save to .env
        self.save_cloudflare_config(&env_path, &api_key, &account_id, &base_domain)?;

        self.ui.success("Cloudflare Tunnel configured");

        Ok(true)
    }

    fn save_cloudflare_config(
        &self,
        env_path: &Path,
        api_key: &str,
        account_id: &str,
        base_domain: &str,
    ) -> Result<()> {
        let mut env_content = if env_path.exists() {
            fs::read_to_string(env_path)?
        } else {
            String::new()
        };

        // Add or update Cloudflare section
        if !env_content.contains("# Cloudflare Tunnel Configuration") {
            env_content.push_str("\n# Cloudflare Tunnel Configuration\n");
        }

        let cf_config = format!(
            "CLOUDFLARE_API_KEY={}\n\
             CLOUDFLARE_ACCOUNT_ID={}\n\
             APP_URL=https://{}\n",
            api_key, account_id, base_domain
        );

        env_content.push_str(&cf_config);

        fs::write(env_path, env_content)?;

        // Set restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(env_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(env_path, perms)?;
        }

        Ok(())
    }
}
```

### BranchBox Registry Initialization

```rust
impl InitWorkflow {
    fn initialize_registry(&self, path: &Path) -> Result<bool> {
        let branchbox_dir = path.join(".branchbox");

        if branchbox_dir.exists() {
            self.ui.info(".branchbox registry already exists");
            return Ok(true);
        }

        self.ui.info("Initializing BranchBox registry...");

        // Create directory
        fs::create_dir_all(&branchbox_dir)?;

        // Create empty registry
        let registry_path = branchbox_dir.join("registry.json");
        let empty_registry = serde_json::json!({
            "version": "1",
            "features": []
        });

        fs::write(
            &registry_path,
            serde_json::to_string_pretty(&empty_registry)?
        )?;

        // Create config file
        let config_path = branchbox_dir.join("config.toml");
        let default_config = format!(
            "# BranchBox Configuration\n\
             # Generated by: branchbox init\n\
             # Generated at: {}\n\
             \n\
             [workspace]\n\
             # Parent directory (main worktree location)\n\
             parent = \"{}\"\n\
             \n\
             [defaults]\n\
             # Default base branch for new features\n\
             base_branch = \"main\"\n\
             \n\
             # Default branch prefix\n\
             branch_prefix = \"feature\"\n\
             \n\
             [devcontainer]\n\
             # Automatically copy devcontainer to new worktrees\n\
             auto_copy = true\n\
             \n\
             [modules]\n\
             # Enabled modules (auto-detected by default)\n\
             # enabled = [\"compose\", \"database\", \"tunnel\", \"specs\"]\n\
             ",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            path.display()
        );

        fs::write(&config_path, default_config)?;

        // Add to .gitignore
        self.update_gitignore(path)?;

        self.ui.success("BranchBox registry initialized");

        Ok(true)
    }

    fn update_gitignore(&self, path: &Path) -> Result<()> {
        let gitignore_path = path.join(".gitignore");
        let mut content = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };

        // Add .branchbox entries if not present
        if !content.contains(".branchbox/registry.json") {
            content.push_str("\n# BranchBox\n");
            content.push_str(".branchbox/registry.json\n");
            content.push_str(".env\n");
            content.push_str(".env.local\n");
        }

        fs::write(&gitignore_path, content)?;

        Ok(())
    }
}
```

## User Experience Flow

### Example 1: Clone New Repository

```bash
$ branchbox init https://github.com/company/rails-app

🔍 Analyzing repository...
   Repository: rails-app
   Default branch: main
   URL: https://github.com/company/rails-app

📂 Setting up workspace...
   Target: ~/projects/rails-app
   Cloning repository...

   Cloning into '~/projects/rails-app'...
   remote: Enumerating objects: 1234, done.
   remote: Counting objects: 100% (1234/1234), done.
   ✓ Cloned successfully

📦 Detecting project type...
   ✓ Stack: Rails
   ✓ Adapter: Rails
   ✓ Modules detected:
     - compose
     - database
     - specs
     - tunnel

🔧 Checking devcontainer setup...
   ✓ .devcontainer/ found
   ✓ devcontainer.json - valid
   ✓ compose.yaml - valid
   ⚠  Dockerfile - using outdated base image

   Update Dockerfile to latest Ruby 3.3 image? (Y/n) y
   ✓ Updated Dockerfile

🌐 Web application detected!
   Configure Cloudflare Tunnel for feature URLs? (Y/n) y

   📋 Cloudflare Tunnel Setup

   Each feature will get a unique URL like:
   https://feature-name.app.example.com

   You'll need:
   1. Cloudflare API Key (from dash.cloudflare.com/profile/api-tokens)
   2. Cloudflare Account ID (from dashboard URL)
   3. A domain managed by Cloudflare

   Cloudflare API Key: ********************************
   Cloudflare Account ID: ********************************
   Base domain: app.example.com

   Validating credentials...
   ✓ Credentials valid
   ✓ Cloudflare configuration saved to .env

📝 Setting up environment...
   Found .env.sample
   ✓ Created .env from template

   ⚠  Please review and update:
   - DATABASE_URL (required)
   - REDIS_URL (optional)
   - SECRET_KEY_BASE (required)

📦 Initializing BranchBox...
   ✓ Created .branchbox/registry.json
   ✓ Created .branchbox/config.toml
   ✓ Updated .gitignore

✅ Workspace initialized successfully!

   📍 Location: ~/projects/rails-app
   🏗  Stack: Rails
   🔧 Adapter: Rails
   📦 Modules: compose, database, specs, tunnel
   🌐 Tunnel: Configured (app.example.com)

   Next steps:

   1. Review environment variables:
      $ vim ~/projects/rails-app/.env

   2. Open in VS Code/Cursor:
      $ code ~/projects/rails-app

   3. Reopen in Container
      (VS Code will prompt automatically)

   4. Start your first feature:
      $ branchbox feature start "implement user authentication"

   Your feature worktrees will be created at:
   ~/projects/rails-app-{feature-name}/

   Each feature gets its own:
   ✓ Git worktree
   ✓ Docker containers (isolated)
   ✓ Database (isolated schema/file)
   ✓ Cloudflare Tunnel URL
   ✓ Spec document

   Learn more: https://docs.branchbox.dev/getting-started
```

### Example 2: Reorganize Existing Clone

```bash
$ cd ~/code/myapp
$ branchbox init

🔍 Analyzing repository...
   ✓ Git repository found
   ✓ Clean working tree
   ✓ On branch: main
   ⚠  Not in worktree structure

📂 Repository Organization

   Current location: ~/code/myapp

   BranchBox works best with a parent directory structure:

   Before:
   ~/code/myapp/              # Your main branch

   After:
   ~/projects/myapp/          # Main branch (parent)
   ~/projects/myapp-feature1/ # Feature worktree
   ~/projects/myapp-feature2/ # Feature worktree

   This allows you to work on multiple features simultaneously,
   each with isolated containers and databases.

   Reorganize into worktree structure? (Y/n) y

   Target location: ~/projects/myapp

   This will:
   1. Move ~/code/myapp → ~/projects/myapp
   2. Initialize BranchBox registry
   3. Setup devcontainer configuration
   4. Configure Cloudflare Tunnel (optional)

   Continue? (Y/n) y

   Moving repository...
   ✓ Moved to ~/projects/myapp

📦 Detecting project type...
   ✓ Stack: Rails
   ✓ Adapter: Rails

🔧 Checking devcontainer setup...
   ✗ .devcontainer/ not found

   Generate devcontainer for Rails? (Y/n) y
   ✓ Created .devcontainer/devcontainer.json
   ✓ Created .devcontainer/compose.yaml
   ✓ Created .devcontainer/Dockerfile
   ✓ Created .env.sample

🌐 Configure Cloudflare Tunnel? (Y/n) n
   Skipped. You can configure later with:
   $ branchbox init --update --tunnel

📦 Initializing BranchBox...
   ✓ Created .branchbox/registry.json
   ✓ Created .branchbox/config.toml

✅ Repository reorganized successfully!

   📍 New location: ~/projects/myapp

   Next steps:
   1. Update any scripts or aliases pointing to old location
   2. Open in VS Code: code ~/projects/myapp
   3. Start a feature: branchbox feature start "my feature"
```

### Example 3: Update Existing Setup

```bash
$ cd ~/projects/rails-app
$ branchbox init --update

🔍 Analyzing repository...
   ✓ Git repository found
   ✓ Worktree parent structure detected
   ✓ BranchBox registry found

📦 Stack: Rails

🔧 Validating devcontainer configuration...
   ✓ devcontainer.json - valid
   ✓ compose.yaml - valid
   ✗ Dockerfile - using outdated base image

   Update Dockerfile? (Y/n) y
   ✓ Updated to Ruby 3.3

🌐 Cloudflare Tunnel: Not configured
   Configure now? (y/N) n

📦 BranchBox registry: Up to date

✅ Configuration updated

   No reorganization needed - already in correct structure.
```

## Implementation Milestones

### Milestone 1: Core Infrastructure (Week 1)
- [ ] Create `InitWorkflow` orchestrator
- [ ] Implement repository state detection
- [ ] Add `InitSource` and `RepositoryState` enums
- [ ] Create CLI command structure with all flags
- [ ] Implement basic URL cloning
- [ ] Add unit tests for state detection

### Milestone 2: Reorganization Logic (Week 2)
- [ ] Implement parent directory detection
- [ ] Add reorganization workflow
- [ ] Safety validation (uncommitted changes, etc.)
- [ ] Dry-run mode
- [ ] User confirmation prompts
- [ ] Integration tests for reorganization

### Milestone 3: Devcontainer Enhancement (Week 3)
- [ ] Devcontainer validation logic
- [ ] Issue detection (missing files, invalid JSON/YAML)
- [ ] Enhancement/fixing capability
- [ ] Integration with existing Bootstrap system
- [ ] Tests for validation and fixing

### Milestone 4: Cloudflare Integration (Week 4)
- [ ] Interactive credential collection
- [ ] Credential validation via API
- [ ] Secure storage in .env
- [ ] Environment variable fallback
- [ ] Tests with mocked Cloudflare API

### Milestone 5: Registry & Configuration (Week 1)
- [ ] BranchBox registry initialization
- [ ] Config.toml generation
- [ ] .gitignore updates
- [ ] Directory structure creation
- [ ] Tests for registry operations

### Milestone 6: UX Polish (Week 5-6)
- [ ] Rich terminal UI (spinners, colors, sections)
- [ ] Clear error messages
- [ ] Helpful next steps generation
- [ ] Non-interactive mode support
- [ ] Validation-only mode
- [ ] Progress indicators

### Milestone 7: Documentation & Testing (Week 7)
- [ ] Comprehensive integration tests
- [ ] E2E tests with real repositories
- [ ] User documentation
- [ ] Migration guide from manual setup
- [ ] Troubleshooting guide
- [ ] Video walkthrough

## Technical Considerations

### Git Operations

```rust
// core/src/git.rs additions

impl GitWorktree {
    /// Check if current directory is a worktree (not main working tree)
    pub fn is_worktree(&self) -> Result<bool> {
        // Check if .git is a file (worktree) vs directory (main)
        let git_path = self.repo_path.join(".git");
        Ok(git_path.is_file())
    }

    /// Find the parent worktree if this is a feature worktree
    pub fn find_parent_worktree(&self) -> Result<Option<PathBuf>> {
        if !self.is_worktree()? {
            return Ok(None);
        }

        // Read .git file to find gitdir
        let git_file = self.repo_path.join(".git");
        let content = fs::read_to_string(git_file)?;

        // Parse: gitdir: /path/to/parent/.git/worktrees/feature-name
        if let Some(gitdir) = content.strip_prefix("gitdir: ") {
            let gitdir = PathBuf::from(gitdir.trim());
            // Navigate up to find parent: .git/worktrees/name → .git → parent
            if let Some(parent) = gitdir.parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent()) {
                return Ok(Some(parent.to_path_buf()));
            }
        }

        Ok(None)
    }

    /// Clone repository from URL
    pub fn clone(url: &str, path: &Path) -> Result<Self> {
        if path.exists() {
            return Err(Error::validation(format!(
                "Target path already exists: {}",
                path.display()
            )));
        }

        let mut cmd = Command::new("git");
        cmd.arg("clone");
        cmd.arg(url);
        cmd.arg(path);

        let output = cmd.output()
            .map_err(|e| Error::git(format!("Failed to clone: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Clone failed: {}", stderr)));
        }

        Self::new(path)
    }
}
```

### URL Validation

```rust
// core/src/validation.rs additions

pub struct RepositoryUrl {
    pub url: String,
    pub protocol: UrlProtocol,
    pub host: String,
    pub owner: String,
    pub repo: String,
}

pub enum UrlProtocol {
    Https,
    Ssh,
    Git,
}

impl RepositoryUrl {
    pub fn parse(url: &str) -> Result<Self> {
        // HTTPS: https://github.com/owner/repo.git
        if url.starts_with("https://") {
            return Self::parse_https(url);
        }

        // SSH: git@github.com:owner/repo.git
        if url.starts_with("git@") {
            return Self::parse_ssh(url);
        }

        // Git protocol: git://github.com/owner/repo.git
        if url.starts_with("git://") {
            return Self::parse_git(url);
        }

        Err(Error::validation(format!(
            "Invalid repository URL: {}",
            url
        )))
    }

    fn parse_https(url: &str) -> Result<Self> {
        // Parse HTTPS URL
        let url_parts = url::Url::parse(url)
            .map_err(|e| Error::validation(format!("Invalid URL: {}", e)))?;

        let host = url_parts.host_str()
            .ok_or_else(|| Error::validation("No host in URL"))?
            .to_string();

        let path = url_parts.path().trim_start_matches('/');
        let (owner, repo) = Self::parse_repo_path(path)?;

        Ok(Self {
            url: url.to_string(),
            protocol: UrlProtocol::Https,
            host,
            owner,
            repo,
        })
    }

    fn parse_ssh(url: &str) -> Result<Self> {
        // Parse SSH URL: git@host:owner/repo.git
        let parts: Vec<&str> = url.split(':').collect();
        if parts.len() != 2 {
            return Err(Error::validation("Invalid SSH URL format"));
        }

        let host = parts[0].trim_start_matches("git@").to_string();
        let (owner, repo) = Self::parse_repo_path(parts[1])?;

        Ok(Self {
            url: url.to_string(),
            protocol: UrlProtocol::Ssh,
            host,
            owner,
            repo,
        })
    }

    fn parse_repo_path(path: &str) -> Result<(String, String)> {
        let path = path.trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();

        if parts.len() < 2 {
            return Err(Error::validation("Invalid repository path"));
        }

        Ok((
            parts[parts.len() - 2].to_string(),
            parts[parts.len() - 1].to_string(),
        ))
    }

    pub fn repo_name(&self) -> &str {
        &self.repo
    }
}
```

### Terminal UI Abstraction

```rust
// core/src/ui.rs (new module)

pub trait TerminalUi {
    fn section(&self, title: &str);
    fn info(&self, message: &str);
    fn success(&self, message: &str);
    fn warn(&self, message: &str);
    fn error(&self, message: &str);
    fn blank(&self);

    fn confirm(&self, prompt: &str) -> Result<bool>;
    fn prompt(&self, prompt: &str) -> Result<String>;
    fn prompt_password(&self, prompt: &str) -> Result<String>;
    fn select(&self, prompt: &str, options: &[&str]) -> Result<usize>;
}

pub struct InteractiveUi {
    theme: ColorScheme,
}

pub struct NonInteractiveUi {
    default_yes: bool,
}

impl TerminalUi for InteractiveUi {
    fn section(&self, title: &str) {
        println!("\n{} {}",
            "●".bright_blue().bold(),
            title.bright_white().bold()
        );
    }

    fn info(&self, message: &str) {
        println!("   {}", message);
    }

    fn success(&self, message: &str) {
        println!("   {} {}",
            "✓".green(),
            message
        );
    }

    fn warn(&self, message: &str) {
        println!("   {} {}",
            "⚠".yellow(),
            message.yellow()
        );
    }

    fn error(&self, message: &str) {
        println!("   {} {}",
            "✗".red(),
            message.red()
        );
    }

    fn confirm(&self, prompt: &str) -> Result<bool> {
        use dialoguer::Confirm;

        Confirm::new()
            .with_prompt(prompt)
            .default(true)
            .interact()
            .map_err(|e| Error::io(format!("Prompt failed: {}", e)))
    }

    fn prompt(&self, prompt: &str) -> Result<String> {
        use dialoguer::Input;

        Input::new()
            .with_prompt(prompt)
            .interact_text()
            .map_err(|e| Error::io(format!("Prompt failed: {}", e)))
    }

    fn prompt_password(&self, prompt: &str) -> Result<String> {
        use dialoguer::Password;

        Password::new()
            .with_prompt(prompt)
            .interact()
            .map_err(|e| Error::io(format!("Prompt failed: {}", e)))
    }
}
```

## Testing Strategy

### Unit Tests
- Repository state detection
- URL parsing (HTTPS, SSH, Git)
- Parent directory path resolution
- Devcontainer validation logic
- Reorganization safety checks
- Configuration file generation

### Integration Tests
```rust
#[test]
fn test_init_from_url() {
    let temp_dir = TempDir::new().unwrap();

    // Mock git clone (create fake repo structure)
    let repo_path = temp_dir.path().join("test-repo");
    create_mock_git_repo(&repo_path);

    let options = InitOptions {
        source: InitSource::Url("https://github.com/test/repo".to_string()),
        target_dir: Some(repo_path.clone()),
        ..Default::default()
    };

    let mut workflow = InitWorkflow::new(options);
    let summary = workflow.execute().unwrap();

    assert_eq!(summary.repository_state, RepositoryState::Cloned {
        url: "https://github.com/test/repo".to_string()
    });
    assert!(repo_path.join(".branchbox").exists());
    assert!(repo_path.join(".devcontainer").exists());
}

#[test]
fn test_reorganize_regular_clone() {
    let temp_dir = TempDir::new().unwrap();

    // Create a regular git clone
    let original_path = temp_dir.path().join("original");
    create_git_repo(&original_path);

    let options = InitOptions {
        source: InitSource::LocalPath(original_path.clone()),
        reorganize: true,
        non_interactive: true,
        ..Default::default()
    };

    let mut workflow = InitWorkflow::new(options);
    let summary = workflow.execute().unwrap();

    assert!(summary.reorganized);
    assert!(summary.workspace_path.ends_with("original"));
}

#[test]
fn test_validate_only_mode() {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = create_git_repo_with_devcontainer(temp_dir.path());

    let options = InitOptions {
        source: InitSource::LocalPath(repo_path.clone()),
        validate_only: true,
        ..Default::default()
    };

    let mut workflow = InitWorkflow::new(options);
    let summary = workflow.execute().unwrap();

    // Should not create any new files in validate mode
    assert!(!repo_path.join(".branchbox").exists());
}
```

### E2E Tests
- Clone public GitHub repo
- Complete initialization workflow
- Create feature worktree
- Verify all components work together
- Test reorganization of existing clone
- Test with SSH and HTTPS URLs

## Error Handling

### Common Errors

```rust
// Comprehensive error types

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("Repository already exists at {0}")]
    TargetExists(PathBuf),

    #[error("Not a git repository: {0}")]
    NotGitRepo(PathBuf),

    #[error("Working tree has uncommitted changes")]
    DirtyWorkingTree,

    #[error("Cannot reorganize: {0}")]
    ReorganizationUnsafe(String),

    #[error("Invalid repository URL: {0}")]
    InvalidUrl(String),

    #[error("Clone failed: {0}")]
    CloneFailed(String),

    #[error("Cloudflare credentials invalid: {0}")]
    CloudflareAuthFailed(String),

    #[error("User cancelled operation")]
    Cancelled,
}
```

### Recovery Strategies

1. **Partial initialization failure**
   - Clean up created directories
   - Restore original state if reorganization failed
   - Log partial state for debugging

2. **Network failures**
   - Retry clone operations
   - Offer offline mode (skip credential validation)
   - Cache successful validations

3. **Permission errors**
   - Clear error messages about what needs fixing
   - Suggest sudo/permission fixes
   - Don't leave half-created directories

## Security Considerations

### Credential Storage
- `.env` files with `0600` permissions (Unix)
- Never commit credentials to git
- Validate `.gitignore` includes `.env`
- Support external secret management

### Clone Safety
- Validate URL format before cloning
- Warn about untrusted sources
- Support SSH key authentication
- Don't execute arbitrary code from cloned repos

### File Operations
- Validate paths don't escape project directory
- Check symlinks don't point outside project
- Preserve file permissions during reorganization
- Handle large repositories gracefully

## Documentation Requirements

### User Guide
- Getting started with `branchbox init`
- Migration from existing projects
- Cloudflare Tunnel setup walkthrough
- Troubleshooting common issues
- FAQ section

### Developer Guide
- Architecture overview
- Adding new stacks
- Customizing devcontainer templates
- Testing strategies
- Contributing guidelines

## Open Questions

1. **Directory naming for reorganization**
   - Should we preserve user's directory name or enforce conventions?
   - **Recommendation**: Preserve name, offer suggestion

2. **Multiple parent directories**
   - Support user having multiple worktree parents?
   - **Recommendation**: Yes, track per-project

3. **Cloudflare credential scope**
   - Per-project or global?
   - **Recommendation**: Both - environment vars override project .env

4. **Existing worktree detection**
   - What if user already has some worktrees created manually?
   - **Recommendation**: Detect and import into registry

5. **Monorepo support**
   - How to handle monorepos with multiple apps?
   - **Recommendation**: Defer to v2, focus on single-repo first

6. **Windows support**
   - How thoroughly to support Windows?
   - **Recommendation**: Best effort, focus on WSL2

## Success Metrics

- Users can go from clone → first feature in < 5 minutes
- 90% of existing projects can be reorganized without manual intervention
- Devcontainer validation catches 95% of common issues
- Zero manual git commands needed for setup
- Cloudflare setup success rate > 80%

## Dependencies

### New Crates
- `url` - URL parsing and validation
- `dialoguer` - Interactive prompts
- `indicatif` - Progress bars and spinners
- `console` - Terminal colors and formatting
- `dirs` - Home directory detection
- `serde_yaml` - YAML validation

### Enhanced Existing
- `git2` - Extended git operations
- `anyhow` → `thiserror` - Better error types
- `clap` - Additional argument parsing

## Critical Analysis & Redesign

### Problems with Current Design

After reflection, the current spec has several issues:

#### 1. **Decision Fatigue** ❌
Presenting users with 3-4 reorganization strategies creates analysis paralysis. Users don't know which to choose and fear making the wrong decision.

**Example of bad UX:**
```
Which strategy would you like?
1. Smart Move
2. In-Place Upgrade
3. Copy & Verify
4. Custom

Choice [1-4]: ???  # User doesn't know what to pick
```

#### 2. **Too Much Output** ❌
The "Deep Analysis Phase" dumps too much information. Users don't care about all the checks - they just want it to work.

**Current (overwhelming):**
```
📦 Git Repository Detected
   ✓ Repository: rails-app
   ✓ Remote: git@github.com:...
   ✓ Current branch: fix-auth
   ⚠ Uncommitted changes: 2 files
   ⚠ Unpushed commits: 3
   ✓ No stashed changes
   ✓ No ongoing rebase
   ✓ No submodules

📁 Directory Analysis
   ✓ Location: /Users/dev/Downloads/...
   ⚠ This is in Downloads
   ⚠ Directory name differs from repo
   ... (20+ more lines)
```

#### 3. **Fundamental Question: Do We Need to Move Anything?** 🤔

**The current design assumes**: Repositories must be in `~/projects/` with a specific naming structure.

**Reality**: Developers have their own preferred locations. Why force reorganization?

**Alternative approach**:
- Just add `.branchbox/` wherever the repo currently is
- Track the "parent" location in config
- Create worktrees as siblings to current location
- Don't dictate where repos should live

#### 4. **Missing Critical Safety Feature: Rollback** ❌

What if reorganization fails halfway through? No undo mechanism.

#### 5. **Performance Issues** ⚠️
- Scanning entire `~/Downloads/`, `~/Desktop/` for duplicate repos could take minutes
- No progress indication during long operations
- No way to cancel safely

#### 6. **Overcomplicated for Common Case** ❌

**Common case** (90% of users):
```bash
cd ~/my/existing/project
branchbox init
# Should just work™
```

**Current design**: 15 questions, 7 phases, 3 strategy choices, tons of output

#### 7. **Wrong Abstraction: "Parent Directory"** 🤔

The concept of a "parent directory" that must be named correctly and in the right location is fragile.

**Better model**:
- The `.branchbox/registry.json` defines the "main" worktree
- Feature worktrees can be anywhere (tracked in registry)
- No enforcement of naming or location

### Redesigned Approach: Smart Defaults + Progressive Disclosure

#### Principle 1: **Make the Simple Case Simple**

```bash
cd ~/any/location/my-project
branchbox init

# Smart default behavior:
# ✓ Detect: Regular clone
# ✓ Add: .branchbox/registry.json (mark as main)
# ✓ Add: .devcontainer/ (if missing)
# ✓ Done: "Ready to use! Try: branchbox feature start 'my feature'"
#
# That's it. No questions, no reorganization, no stress.
```

#### Principle 2: **Defer Optimization**

Only offer reorganization if location is *truly problematic*:

```bash
# Only if in /tmp/ or ~/Downloads/
⚠ Warning: This repository is in a temporary location.
  Current: /tmp/my-project

  This directory may be deleted by the system.
  Move to permanent location? (Y/n)

  Suggested: ~/projects/my-project
  Custom path: _____________

  (or 's' to skip and use current location)
```

#### Principle 3: **Progressive Disclosure**

Don't show all capabilities upfront. Show advanced options only when needed:

```bash
# Default (minimal output)
$ branchbox init
✓ Initialized BranchBox in /current/path
  Next: branchbox feature start "feature name"

# Verbose (for debugging)
$ branchbox init -v
[Shows all detection steps]

# Expert mode (all options)
$ branchbox init --expert
[Shows strategies, customization, advanced options]
```

#### Principle 4: **Idempotent by Default**

Running `branchbox init` multiple times should be safe:

```bash
$ branchbox init
✓ Already initialized

$ branchbox init --validate
Checking configuration...
✓ Registry: OK
✓ Devcontainer: OK
⚠ Cloudflare: Not configured

  Configure now? (y/N)
```

#### Principle 5: **Rollback Built-In**

Every risky operation creates a rollback point:

```bash
# If reorganization fails
✗ Error during move: Permission denied

  Rollback initiated...
  ✓ Restored original state

  Your repository is unchanged at:
  /original/location
```

### Simplified Command Interface

```bash
# Simple case (90% of users)
branchbox init                    # Smart defaults, minimal questions
branchbox init <url>              # Clone and initialize
branchbox init --from <url>       # Alias for clone

# Validation
branchbox init --check            # Validate existing setup
branchbox init --doctor           # Deep health check

# Advanced (power users)
branchbox init --reorganize       # Force reorganization
branchbox init --to ~/path        # Specify target location
branchbox init --auto             # No prompts (CI/scripts)
branchbox init --dry-run          # Show what would happen

# Recovery
branchbox init --repair           # Fix broken setup
branchbox init --rollback         # Undo last init operation
```

### Revised Workflow: The 3-Second Rule

**Goal**: Most users should be productive within 3 seconds of running `init`.

```bash
$ cd ~/existing/project
$ time branchbox init

✓ Initialized BranchBox

real    0m1.2s

$ branchbox feature start "auth"
# Works immediately
```

### New Detection Logic: Minimal Viable Checks

Instead of 20+ checks, do only what's necessary:

```rust
fn analyze_repository_state_v2(&self) -> Result<RepositoryState> {
    // 1. Is it a git repo? (required)
    if !self.is_git_repo()? {
        return Err(Error::NotGitRepo);
    }

    // 2. Is git state safe? (required)
    if !self.is_git_state_safe()? {
        return Err(Error::UnsafeGitState);
    }

    // 3. Already initialized? (early exit)
    if self.has_branchbox_registry()? {
        return Ok(RepositoryState::AlreadyInitialized);
    }

    // 4. Is location temporary? (warn only)
    let needs_move = self.is_temporary_location()?;

    // That's it. Don't overcomplicate.

    Ok(RepositoryState::ReadyToInitialize {
        warn_location: needs_move
    })
}
```

### Simplified Reorganization Decision Tree

```
Is repo in /tmp/ or ~/Downloads/?
├─ YES → Strongly recommend move (but allow skip)
└─ NO  → Don't reorganize, init in-place

Already has .branchbox/?
├─ YES → Validate and report status
└─ NO  → Create registry at current location

Already has worktrees?
├─ YES → Import into registry
└─ NO  → Ready for first feature
```

### Better Error Messages

**Bad (current design):**
```
Error: Cannot initialize during rebase. Please complete or abort:
  git rebase --continue
  git rebase --abort
```

**Good (actionable):**
```
⚠ Git Rebase in Progress

  BranchBox cannot initialize while a rebase is active.

  Options:
  1. Complete the rebase, then run: branchbox init
  2. Abort the rebase: git rebase --abort && branchbox init

  Current rebase: feature-branch onto main
  Started: 5 minutes ago
```

### Phased Rollout Strategy

**Phase 1: MVP (Week 1-2)**
- Init in-place (no reorganization)
- Basic validation
- Registry creation
- Devcontainer generation

**Phase 2: Smart Defaults (Week 3-4)**
- Location detection (temp vs permanent)
- Automatic best-path decision
- Minimal prompts

**Phase 3: Advanced Features (Week 5-6)**
- Reorganization with rollback
- Worktree import
- Duplicate detection

**Phase 4: Polish (Week 7-8)**
- Performance optimization
- Edge case handling
- Documentation

### New Success Metrics

**Old metrics (too complex):**
- "90% of projects can be reorganized without manual intervention"

**New metrics (user-focused):**
- ⭐ **Time to first feature**: <30 seconds from `init` to `feature start`
- ⭐ **Zero-question init**: 80% of users answer 0 questions
- ⭐ **First-try success rate**: 95% of inits succeed on first attempt
- ⭐ **Rollback success**: 100% of failed inits can be rolled back

### Dropped Features (Simplification)

These add complexity without proportional value:

❌ **Dropped**: Duplicate clone detection
- Reason: Slow, unreliable, edge case
- Alternative: User can clean up manually

❌ **Dropped**: Multiple reorganization strategies
- Reason: Decision fatigue
- Alternative: One smart default

❌ **Dropped**: Interactive Cloudflare setup during init
- Reason: Can be added later with `branchbox config tunnel`
- Alternative: Detect credentials from env, skip if missing

❌ **Dropped**: "Copy & Verify" strategy
- Reason: Wastes disk space, adds complexity
- Alternative: Atomic move with rollback

### Kept Features (Essential)

✅ **URL cloning**: Core feature
✅ **In-place init**: Default behavior
✅ **Safety checks**: Prevent data loss
✅ **Rollback**: Critical for trust
✅ **Idempotent**: Safe to re-run

### Before vs After: Concrete Comparison

#### Scenario: Existing Project in Good Location

**BEFORE (Original Design) - 47 seconds, 8 prompts:**

```bash
$ cd ~/projects/myapp
$ branchbox init

🔍 Deep Analysis Phase...
   [... 30 lines of checks ...]

📋 Setup Options
   1. Smart Move (to same location!)
   2. In-Place Upgrade
   3. Copy & Verify

Which strategy? (1-3) [2]: 2

Generate devcontainer? (Y/n) y
Stack detected as Rails. Correct? (Y/n) y
Include PostgreSQL? (Y/n) y
Include Redis? (Y/n) y
Configure Cloudflare? (Y/n) n
Create .env.sample? (Y/n) y
Initialize registry? (Y/n) y
Add to .gitignore? (Y/n) y

✅ Complete!

real    0m47.3s
```

**AFTER (Redesigned) - 1.2 seconds, 0 prompts:**

```bash
$ cd ~/projects/myapp
$ branchbox init

✓ Initialized BranchBox
  Rails project ready

  Next: branchbox feature start "feature name"

real    0m1.2s
```

**Improvement**: 97% faster, 100% fewer questions, 95% less output

#### Scenario: Project in Bad Location

**BEFORE - 63 seconds, 12 prompts**

**AFTER - 8 seconds, 1 prompt:**

```bash
$ cd ~/Downloads/myapp
$ branchbox init

⚠ Warning: Repository is in Downloads (temporary location)

  Move to: ~/projects/myapp? (Y/n) y

  Moving... ✓ Done (3.2s)

✓ Initialized BranchBox in ~/projects/myapp

real    0m8.1s
```

**Improvement**: 87% faster, 91% fewer questions

### Implementation Priority

**Must-Have (MVP)**
1. ✅ In-place initialization (no reorganization)
2. ✅ Idempotent (safe to re-run)
3. ✅ Safety checks (no data loss)
4. ✅ Minimal output (1-2 lines default)
5. ✅ URL cloning support

**Should-Have (V1.0)**
6. ✅ Smart location detection
7. ✅ Optional reorganization (with confirmation)
8. ✅ Rollback on failure
9. ✅ Devcontainer validation
10. ✅ Verbose mode (-v)

**Nice-to-Have (V1.1+)**
11. ⏳ Worktree import
12. ⏳ Health check command
13. ⏳ Repair command
14. ⏳ Expert mode (all options)

**Won't-Have (Out of scope)**
❌ Duplicate clone detection (too slow)
❌ Multiple strategy choices (decision fatigue)
❌ Interactive Cloudflare setup (separate command)
❌ Automatic git operations (too risky)

## Timeline

- **Week 1-2**: MVP - In-place init only
- **Week 3-4**: Smart defaults + location detection
- **Week 5-6**: Reorganization + advanced features
- **Week 7-8**: Edge cases + polish

Total: **8 weeks** to production-ready (unchanged, but simpler scope)

## Future Enhancements

### v2 Features
- Template library (community devcontainer templates)
- Multi-repo support (monorepos)
- Cloud VM provisioning
- Team sharing (share devcontainer + tunnel config)
- Migration scripts (Docker Compose → devcontainer)
- Init from template URL
- Automatic stack detection improvements
- Custom adapter plugins

## References

- Git Worktree documentation: https://git-scm.com/docs/git-worktree
- Devcontainer specification: https://containers.dev/
- Cloudflare Tunnel API: https://api.cloudflare.com/
- BranchBox architecture: `docs/ARCHITECTURE.md`
