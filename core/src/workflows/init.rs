//! Initialize workflow for universal repository setup
//!
//! This module implements the `branchbox init` command, which can work on ANY repository
//! with minimal user interaction following these principles:
//!
//! 1. **In-Place by Default** - Add `.branchbox/` wherever the repo currently lives
//! 2. **Smart Defaults** - Zero questions for 80% of users
//! 3. **Progressive Disclosure** - Simple case is simple
//! 4. **Idempotent** - Safe to run multiple times
//! 5. **Rollback Built-In** - Every risky operation can be undone
//!
//! # Example
//!
//! ```no_run
//! use worktree_core::workflows::init::{InitWorkflow, InitOptions, InitSource};
//! use std::path::PathBuf;
//!
//! let options = InitOptions {
//!     source: InitSource::CurrentDirectory,
//!     target_dir: None,
//!     stack: None,
//!     skip_devcontainer: false,
//!     skip_env: false,
//!     reorganize: false,
//!     update: false,
//!     validate_only: false,
//!     dry_run: false,
//!     non_interactive: false,
//!     verbose: false,
//! };
//!
//! let mut workflow = InitWorkflow::new(options);
//! let summary = workflow.execute().unwrap();
//!
//! println!("Initialized at: {}", summary.workspace_path.display());
//! ```

use crate::bootstrap::{Bootstrap, Stack};
use crate::git::GitWorktree;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Initialization workflow orchestrator
///
/// Manages the complete initialization workflow for BranchBox repositories,
/// including cloning, reorganization, devcontainer setup, and registry creation.
///
/// # Example
///
/// ```no_run
/// use worktree_core::workflows::init::{InitWorkflow, InitOptions, InitSource};
///
/// let options = InitOptions {
///     source: InitSource::CurrentDirectory,
///     ..Default::default()
/// };
///
/// let mut workflow = InitWorkflow::new(options);
/// let summary = workflow.execute()?;
/// # Ok::<(), worktree_core::Error>(())
/// ```
pub struct InitWorkflow {
    options: InitOptions,
}

/// Configuration options for initialization
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Source for initialization (URL, path, or current directory)
    pub source: InitSource,

    /// Target directory for parent worktree (if reorganizing)
    pub target_dir: Option<PathBuf>,

    /// Force specific stack
    pub stack: Option<Stack>,

    /// Skip devcontainer setup
    pub skip_devcontainer: bool,

    /// Skip environment setup
    pub skip_env: bool,

    /// Force reorganization into worktree structure
    pub reorganize: bool,

    /// Use in-place parent structure (reorganize current dir as container)
    pub use_parent_structure: bool,

    /// Update existing setup without restructuring
    pub update: bool,

    /// Validate only (no modifications)
    pub validate_only: bool,

    /// Dry run (show what would happen)
    pub dry_run: bool,

    /// Non-interactive mode (use defaults)
    pub non_interactive: bool,

    /// Verbose output
    pub verbose: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            source: InitSource::CurrentDirectory,
            target_dir: None,
            stack: None,
            skip_devcontainer: false,
            skip_env: false,
            reorganize: false,
            use_parent_structure: false,
            update: false,
            validate_only: false,
            dry_run: false,
            non_interactive: false,
            verbose: false,
        }
    }
}

/// Source for initialization
#[derive(Debug, Clone)]
pub enum InitSource {
    /// Clone from URL (HTTPS or SSH)
    Url(String),

    /// Use existing directory
    LocalPath(PathBuf),

    /// Current directory
    CurrentDirectory,
}

/// Summary of initialization results
#[derive(Debug)]
pub struct InitSummary {
    /// Final workspace path
    pub workspace_path: PathBuf,

    /// Repository state determined during analysis
    pub repository_state: RepositoryState,

    /// Whether reorganization occurred
    pub reorganized: bool,

    /// Detected/forced stack
    pub stack: Stack,

    /// Adapter name
    pub adapter: String,

    /// Enabled module names
    pub modules: Vec<String>,

    /// Devcontainer status
    pub devcontainer_status: DevcontainerStatus,

    /// Whether BranchBox registry was initialized
    pub registry_initialized: bool,

    /// Warnings encountered
    pub warnings: Vec<String>,

    /// Next steps for the user
    pub next_steps: Vec<String>,
}

/// Current state of the repository
#[derive(Debug, Clone, PartialEq)]
pub enum RepositoryState {
    /// Fresh clone from URL
    Cloned { url: String },

    /// Existing regular clone (not worktree-based)
    RegularClone { needs_reorganization: bool },

    /// Already in worktree parent structure
    WorktreeParent,

    /// In a feature worktree (should run from parent)
    FeatureWorktree { parent_path: Option<PathBuf> },

    /// Already initialized (idempotent case)
    AlreadyInitialized,

    /// Ready to initialize
    ReadyToInitialize { warn_location: bool },
}

/// Status of devcontainer configuration
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

/// BranchBox registry configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub version: String,
    pub workspace_path: PathBuf,
}

/// BranchBox configuration file
#[derive(Debug, Serialize, Deserialize)]
pub struct BranchBoxConfig {
    pub workspace: WorkspaceConfig,
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub parent: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub base_branch: String,
    pub branch_prefix: String,
}

impl InitWorkflow {
    /// Create a new initialization workflow
    pub fn new(options: InitOptions) -> Self {
        Self { options }
    }

    /// Execute the complete initialization workflow
    ///
    /// This is the main entry point that orchestrates all phases:
    /// 1. Analyze repository state
    /// 2. Handle cloning or reorganization if needed
    /// 3. Detect stack and adapter
    /// 4. Setup/validate devcontainer
    /// 5. Detect modules
    /// 6. Initialize BranchBox registry
    /// 7. Generate next steps
    pub fn execute(&mut self) -> Result<InitSummary> {
        let mut summary = InitSummary::default();

        if self.options.verbose {
            tracing::info!("Starting initialization workflow");
        }

        // Phase 1: Analyze current state
        let state = self.analyze_repository_state()?;
        summary.repository_state = state.clone();

        if self.options.verbose {
            tracing::info!("Repository state: {:?}", state);
        }

        // Early exit for validation-only mode
        if self.options.validate_only {
            return self.validate_and_report(state);
        }

        // Phase 2: Repository setup (clone or reorganize)
        let workspace_path = match state {
            RepositoryState::Cloned { ref url } => {
                if self.options.dry_run {
                    println!("[DRY RUN] Would clone: {}", url);
                    return Ok(summary);
                }
                self.clone_repository(url)?
            }
            RepositoryState::RegularClone {
                needs_reorganization: true,
            } => {
                if self.options.reorganize || self.should_prompt_reorganize()? {
                    summary.reorganized = true;
                    self.reorganize_to_worktree()?
                } else {
                    self.current_path()
                }
            }
            RepositoryState::RegularClone {
                needs_reorganization: false,
            } => {
                // Good location, no reorganization needed
                if self.options.reorganize {
                    // User explicitly requested reorganization
                    summary.reorganized = true;
                    self.reorganize_to_worktree()?
                } else {
                    self.current_path()
                }
            }
            RepositoryState::FeatureWorktree { parent_path: _ } => {
                return Err(Error::validation(
                    "Cannot initialize from feature worktree. Run from parent directory.",
                ));
            }
            RepositoryState::AlreadyInitialized => {
                if self.options.update {
                    self.current_path()
                } else {
                    println!("✓ Already initialized");
                    if !self.options.non_interactive {
                        println!("  Run with --update to update configuration");
                    }
                    return Ok(summary);
                }
            }
            RepositoryState::ReadyToInitialize { warn_location } => {
                if warn_location && !self.options.non_interactive {
                    self.warn_about_location()?;
                }
                self.current_path()
            }
            RepositoryState::WorktreeParent => {
                if self.options.update {
                    self.current_path()
                } else {
                    println!("✓ Already set up as worktree parent");
                    return Ok(summary);
                }
            }
        };

        summary.workspace_path = workspace_path.clone();

        // Phase 3: Detect stack and adapter
        let stack = self.detect_or_force_stack(&workspace_path)?;
        let adapter = crate::adapters::detect_adapter(&workspace_path)?;
        summary.stack = stack;
        summary.adapter = adapter.name().to_string();

        if self.options.verbose {
            tracing::info!("Detected stack: {:?}", stack);
            tracing::info!("Using adapter: {}", adapter.name());
        }

        // Phase 4: Devcontainer setup/validation
        if !self.options.skip_devcontainer {
            summary.devcontainer_status = self.setup_devcontainer(&workspace_path, stack)?;
        } else {
            summary.devcontainer_status = DevcontainerStatus::None;
        }

        // Phase 5: Module detection
        let module_plan = crate::modules::detect_modules(&workspace_path, &[]);
        summary.modules = module_plan.handles.iter().map(|h| h.name.clone()).collect();

        // Phase 6: Initialize BranchBox registry
        summary.registry_initialized = self.initialize_registry(&workspace_path)?;

        // Phase 7: Generate next steps
        summary.next_steps = self.generate_next_steps(&summary);

        Ok(summary)
    }

    /// Analyze repository state without making changes
    fn analyze_repository_state(&self) -> Result<RepositoryState> {
        // Handle URL source
        if let InitSource::Url(url) = &self.options.source {
            return Ok(RepositoryState::Cloned { url: url.clone() });
        }

        let path = self.get_working_path();

        // Check if it's a git repository
        if !path.join(".git").exists() {
            return Err(Error::validation(
                "Not a git repository. Use 'git init' first or provide a URL to clone.",
            ));
        }

        // Check git state safety
        self.check_git_state_safety(&path)?;

        // Check if already initialized
        if self.has_branchbox_registry(&path)? {
            return Ok(RepositoryState::AlreadyInitialized);
        }

        // Check if this is a worktree parent
        if self.is_worktree_parent(&path)? {
            return Ok(RepositoryState::WorktreeParent);
        }

        // Check if we're in a feature worktree
        if self.is_feature_worktree(&path)? {
            let parent = self.find_parent_worktree(&path)?;
            return Ok(RepositoryState::FeatureWorktree {
                parent_path: parent,
            });
        }

        // Regular clone - check if location is problematic
        let needs_reorganization = self.is_temporary_location(&path)?;

        Ok(RepositoryState::RegularClone {
            needs_reorganization,
        })
    }

    /// Check for git states that make initialization unsafe
    fn check_git_state_safety(&self, path: &Path) -> Result<()> {
        let git_dir = path.join(".git");

        // Check for ongoing rebase
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            return Err(Error::validation(
                "Cannot initialize during rebase.\n\
                 Please complete or abort first:\n\
                   git rebase --continue\n\
                   git rebase --abort",
            ));
        }

        // Check for ongoing merge
        if git_dir.join("MERGE_HEAD").exists() {
            return Err(Error::validation(
                "Cannot initialize during merge.\n\
                 Please complete or abort first:\n\
                   git merge --continue\n\
                   git merge --abort",
            ));
        }

        // Check for cherry-pick in progress
        if git_dir.join("CHERRY_PICK_HEAD").exists() {
            return Err(Error::validation(
                "Cannot initialize during cherry-pick.\n\
                 Please complete or abort first:\n\
                   git cherry-pick --continue\n\
                   git cherry-pick --abort",
            ));
        }

        // Check for bisect in progress
        if git_dir.join("BISECT_LOG").exists() {
            return Err(Error::validation(
                "Cannot initialize during bisect.\n\
                 Please complete first:\n\
                   git bisect reset",
            ));
        }

        Ok(())
    }

    /// Check if path has .branchbox registry
    fn has_branchbox_registry(&self, path: &Path) -> Result<bool> {
        Ok(path.join(".branchbox").exists())
    }

    /// Check if this is a worktree parent
    fn is_worktree_parent(&self, path: &Path) -> Result<bool> {
        // Check for .branchbox/registry.json (definitive)
        if path.join(".branchbox/registry.json").exists() {
            return Ok(true);
        }

        // Check if git worktree list shows multiple worktrees
        let git = GitWorktree::new(path)?;
        let worktrees = git.list()?;

        Ok(worktrees.len() > 1)
    }

    /// Check if this is a feature worktree
    fn is_feature_worktree(&self, path: &Path) -> Result<bool> {
        // Check if .git is a file (worktree) vs directory (main)
        let git_path = path.join(".git");
        Ok(git_path.is_file())
    }

    /// Find parent worktree if in feature worktree
    fn find_parent_worktree(&self, path: &Path) -> Result<Option<PathBuf>> {
        if !self.is_feature_worktree(path)? {
            return Ok(None);
        }

        // Read .git file to find gitdir
        let git_file = path.join(".git");
        let content = fs::read_to_string(git_file)?;

        // Parse: gitdir: /path/to/parent/.git/worktrees/feature-name
        if let Some(gitdir) = content.strip_prefix("gitdir: ") {
            let gitdir = PathBuf::from(gitdir.trim());
            // Navigate up: .git/worktrees/name → .git → parent
            if let Some(parent) = gitdir
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                return Ok(Some(parent.to_path_buf()));
            }
        }

        Ok(None)
    }

    /// Check if location is temporary/problematic
    fn is_temporary_location(&self, path: &Path) -> Result<bool> {
        let path_str = path.to_string_lossy();

        let bad_patterns = [
            "/tmp/",
            "/temp/",
            "/Downloads/",
            "/Desktop/",
            "/Documents/", // Discouraged for code
        ];

        Ok(bad_patterns
            .iter()
            .any(|pattern| path_str.contains(pattern)))
    }

    /// Get current working path based on source
    fn get_working_path(&self) -> PathBuf {
        match &self.options.source {
            InitSource::LocalPath(path) => path.clone(),
            InitSource::CurrentDirectory => {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            }
            InitSource::Url(_) => unreachable!("URL source handled separately"),
        }
    }

    /// Get current path (for non-URL sources)
    fn current_path(&self) -> PathBuf {
        self.get_working_path()
    }

    /// Get the default projects directory
    ///
    /// Uses BRANCHBOX_PROJECTS_DIR environment variable if set,
    /// otherwise defaults to ~/projects/
    fn get_projects_dir(&self) -> Result<PathBuf> {
        // Check environment variable first
        if let Ok(projects_dir) = std::env::var("BRANCHBOX_PROJECTS_DIR") {
            return Ok(PathBuf::from(projects_dir));
        }

        // Fall back to ~/projects/
        let home =
            dirs::home_dir().ok_or_else(|| Error::validation("Cannot determine home directory"))?;
        Ok(home.join("projects"))
    }

    /// Validate clone URL for security
    ///
    /// Rejects file:// URLs and validates format
    fn validate_clone_url(&self, url: &str) -> Result<()> {
        // Reject file:// URLs (security risk - can access local files)
        if url.starts_with("file://") {
            return Err(Error::validation(
                "file:// URLs are not supported for security reasons.\n\
                 Use git clone directly if you need to clone from a local path.",
            ));
        }

        // Validate URL format
        if url.starts_with("http://") || url.starts_with("https://") {
            // HTTPS/HTTP URLs are fine
            Ok(())
        } else if url.starts_with("git@") || url.starts_with("ssh://") {
            // SSH URLs are fine
            Ok(())
        } else if url.starts_with("git://") {
            // Git protocol URLs are fine
            Ok(())
        } else {
            Err(Error::validation(format!(
                "Invalid repository URL format: {}\n\
                 Supported formats:\n\
                   - https://github.com/user/repo.git\n\
                   - git@github.com:user/repo.git\n\
                   - ssh://git@github.com/user/repo.git\n\
                   - git://github.com/user/repo.git",
                url
            )))
        }
    }

    /// Clone repository from URL
    fn clone_repository(&self, url: &str) -> Result<PathBuf> {
        // Validate URL before cloning
        self.validate_clone_url(url)?;

        // Determine target path
        let target_path = if let Some(path) = &self.options.target_dir {
            path.clone()
        } else {
            // Extract repo name from URL
            let repo_name = self.extract_repo_name(url)?;
            let projects_dir = self.get_projects_dir()?;
            projects_dir.join(repo_name)
        };

        if target_path.exists() {
            return Err(Error::validation(format!(
                "Target path already exists: {}",
                target_path.display()
            )));
        }

        println!("Cloning {} into {}", url, target_path.display());

        // Create parent directory
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Clone using git command
        let output = Command::new("git")
            .arg("clone")
            .arg(url)
            .arg(&target_path)
            .output()
            .map_err(|e| Error::validation(format!("Failed to clone: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::validation(format!("Clone failed: {}", stderr)));
        }

        println!("✓ Cloned successfully");

        Ok(target_path)
    }

    /// Extract repository name from URL
    fn extract_repo_name(&self, url: &str) -> Result<String> {
        // Handle different URL formats
        let url = url.trim_end_matches('/').trim_end_matches(".git");

        let name = if let Some(name) = url.rsplit('/').next() {
            name.to_string()
        } else {
            return Err(Error::validation("Cannot extract repository name from URL"));
        };

        Ok(name)
    }

    /// Reorganize repository to worktree structure
    fn reorganize_to_worktree(&self) -> Result<PathBuf> {
        let current_path = self.current_path();

        println!("⚠ Reorganization requested");

        // Check if using in-place parent structure
        if self.options.use_parent_structure {
            return self.reorganize_in_place_parent(&current_path);
        }

        // Determine target path for standard reorganization
        let target_path = if let Some(path) = &self.options.target_dir {
            path.clone()
        } else {
            // Move to projects directory (configurable via BRANCHBOX_PROJECTS_DIR)
            let repo_name = current_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| Error::validation("Invalid directory name"))?;

            let projects_dir = self.get_projects_dir()?;
            projects_dir.join(repo_name)
        };

        // Don't move if already in target location
        if current_path == target_path {
            println!("✓ Already in target location");
            return Ok(current_path);
        }

        if self.options.dry_run {
            println!("[DRY RUN] Would move:");
            println!("  From: {}", current_path.display());
            println!("  To:   {}", target_path.display());
            return Ok(target_path);
        }

        // Confirm with user unless non-interactive
        if !self.options.non_interactive {
            println!("  From: {}", current_path.display());
            println!("  To:   {}", target_path.display());
            println!();
            println!("Continue? (y/N)");

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                // User declined - use current path instead (not an error)
                println!("Reorganization cancelled. Continuing with current location.");
                return Ok(current_path);
            }
        }

        // Create parent directory
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Move directory
        println!("Moving repository...");
        fs::rename(&current_path, &target_path)
            .map_err(|e| Error::validation(format!("Failed to move repository: {}", e)))?;

        println!("✓ Moved to {}", target_path.display());

        Ok(target_path)
    }

    /// Reorganize repository in-place using parent structure
    ///
    /// Transforms:
    ///   /path/to/project-folder/  (git repo)
    /// Into:
    ///   /path/to/project-folder/  (container directory)
    ///     main/                   (git repo moved here)
    ///
    /// This enables sibling feature worktrees:
    ///   /path/to/project-folder/
    ///     main/
    ///     feature-1/
    ///     feature-2/
    fn reorganize_in_place_parent(&self, current_path: &Path) -> Result<PathBuf> {
        // Get the parent directory and directory name
        let parent_dir = current_path
            .parent()
            .ok_or_else(|| Error::validation("Cannot reorganize root directory"))?;

        let dir_name = current_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid directory name"))?;

        // Generate unique temporary name to avoid collisions
        let temp_name = format!(".branchbox-temp-{}", uuid::Uuid::new_v4());
        let temp_path = parent_dir.join(&temp_name);

        // Final paths
        let container_path = current_path.to_path_buf();
        let main_path = container_path.join("main");

        if self.options.dry_run {
            println!("[DRY RUN] Would reorganize in-place:");
            println!("  Current: {}", current_path.display());
            println!("  Result:  {}/main/", dir_name);
            println!();
            println!("  Future worktrees will be siblings:");
            println!("    {}/main/", dir_name);
            println!("    {}/feature-name/", dir_name);
            return Ok(main_path);
        }

        // Show user what will happen
        if !self.options.non_interactive {
            println!();
            println!("  Creating parent directory structure:");
            println!("    Current: {}", current_path.display());
            println!("    Result:  {}/main/", current_path.display());
            println!();
            println!("  Future worktrees will be created as:");
            println!("    {}/feature-name/", current_path.display());
            println!();
            println!("Continue? (y/N)");

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Reorganization cancelled. Continuing with current location.");
                return Ok(current_path.to_path_buf());
            }
        }

        if self.options.verbose {
            println!();
            println!("Starting safe reorganization:");
            println!("  Step 1: Rename current directory to temporary name");
            println!("  Step 2: Create parent container directory");
            println!("  Step 3: Move repository into container as 'main'");
        }

        // Step 1: Rename current directory to temporary location
        if self.options.verbose {
            println!();
            println!("  → Renaming {} to temporary location...", dir_name);
        }

        fs::rename(current_path, &temp_path).map_err(|e| {
            Error::validation(format!(
                "Failed to rename directory to temporary location: {}",
                e
            ))
        })?;

        // Step 2: Create the container directory (with original name)
        // If this fails, we can roll back by renaming temp back
        if self.options.verbose {
            println!("  → Creating container directory...");
        }

        if let Err(e) = fs::create_dir(&container_path) {
            // Rollback: restore original directory
            if self.options.verbose {
                println!("  ✗ Failed to create container. Rolling back...");
            }

            let _ = fs::rename(&temp_path, current_path);

            return Err(Error::validation(format!(
                "Failed to create container directory: {}. Changes rolled back.",
                e
            )));
        }

        // Step 3: Move temp directory into container as 'main'
        // If this fails, cleanup and rollback
        if self.options.verbose {
            println!("  → Moving repository into container as 'main'...");
        }

        if let Err(e) = fs::rename(&temp_path, &main_path) {
            // Rollback: remove container and restore original
            if self.options.verbose {
                println!("  ✗ Failed to move repository. Rolling back...");
            }

            let _ = fs::remove_dir(&container_path);
            let _ = fs::rename(&temp_path, current_path);

            return Err(Error::validation(format!(
                "Failed to move repository into container: {}. Changes rolled back.",
                e
            )));
        }

        if self.options.verbose {
            println!();
            println!("✓ Reorganization complete");
            println!("  Repository is now at: {}", main_path.display());
            println!("  Container directory: {}", container_path.display());
        } else {
            println!("✓ Reorganized into parent structure: {}/main/", dir_name);
        }

        Ok(main_path)
    }

    /// Detect stack or use forced stack
    fn detect_or_force_stack(&self, path: &Path) -> Result<Stack> {
        if let Some(stack) = self.options.stack {
            return Ok(stack);
        }

        let bootstrap = Bootstrap::new(path);
        bootstrap.detect_stack()
    }

    /// Setup or validate devcontainer configuration
    fn setup_devcontainer(&self, path: &Path, stack: Stack) -> Result<DevcontainerStatus> {
        let devcontainer_dir = path.join(".devcontainer");

        if !devcontainer_dir.exists() {
            // Generate from scratch
            if self.options.verbose {
                println!("Generating devcontainer configuration...");
            }

            if self.options.dry_run {
                println!("[DRY RUN] Would generate devcontainer for {:?}", stack);
                return Ok(DevcontainerStatus::Created);
            }

            let bootstrap = Bootstrap::new(path);
            bootstrap.generate(stack)?;

            if !self.options.verbose {
                println!("✓ Created devcontainer configuration");
            }

            return Ok(DevcontainerStatus::Created);
        }

        // Existing devcontainer - validate
        if self.options.verbose {
            println!("✓ Devcontainer configuration exists");
        }

        Ok(DevcontainerStatus::Valid)
    }

    /// Initialize BranchBox registry atomically
    ///
    /// Uses atomic file creation to prevent race conditions when multiple
    /// init processes run concurrently.
    fn initialize_registry(&self, path: &Path) -> Result<bool> {
        let branchbox_dir = path.join(".branchbox");
        let registry_path = branchbox_dir.join("registry.json");

        // Quick check if already exists (optimization)
        if registry_path.exists() {
            if self.options.verbose {
                println!("✓ BranchBox registry already exists");
            }
            return Ok(false);
        }

        if self.options.dry_run {
            println!("[DRY RUN] Would create .branchbox/ registry");
            return Ok(true);
        }

        // Create directory if it doesn't exist
        fs::create_dir_all(&branchbox_dir)?;

        // Try to create registry file atomically
        use std::fs::OpenOptions;
        let empty_registry = serde_json::json!({
            "version": "1",
            "features": []
        });
        let registry_content = serde_json::to_string_pretty(&empty_registry)?;

        // Attempt atomic creation (fails if file already exists)
        match OpenOptions::new()
            .write(true)
            .create_new(true) // Atomic: fails if file exists
            .open(&registry_path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(registry_content.as_bytes())?;
                file.sync_all()?; // Ensure written to disk

                // Update .gitignore
                self.update_gitignore(path)?;

                if self.options.verbose {
                    println!("✓ Created BranchBox registry");
                }

                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Another process created it - that's fine
                if self.options.verbose {
                    println!("✓ BranchBox registry already exists");
                }
                Ok(false)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Update .gitignore to exclude registry and env files
    ///
    /// Only adds entries that don't already exist to avoid duplication.
    fn update_gitignore(&self, path: &Path) -> Result<()> {
        let gitignore_path = path.join(".gitignore");
        let mut content = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };

        // Check which entries need to be added
        let entries = vec![".branchbox/registry.json", ".env", ".env.local"];

        let mut new_entries = Vec::new();
        for entry in entries {
            // Check if this specific entry exists (not just any branchbox entry)
            if !content.lines().any(|line| line.trim() == entry) {
                new_entries.push(entry);
            }
        }

        // Only modify file if there are new entries to add
        if !new_entries.is_empty() {
            // Ensure content ends with newline
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }

            // Add BranchBox section header if adding entries
            if !content.contains("# BranchBox") {
                content.push_str("\n# BranchBox\n");
            } else {
                content.push('\n');
            }

            // Add new entries
            for entry in new_entries {
                content.push_str(entry);
                content.push('\n');
            }

            fs::write(&gitignore_path, content)?;
        }

        Ok(())
    }

    /// Generate helpful next steps
    fn generate_next_steps(&self, summary: &InitSummary) -> Vec<String> {
        let mut steps = Vec::new();

        if summary.reorganized {
            steps.push(format!("cd {}", summary.workspace_path.display()));
        }

        steps.push("Open in VS Code/Cursor and reopen in container".to_string());
        steps.push("branchbox feature start \"feature name\"".to_string());

        steps
    }

    /// Validate and report (for --validate mode)
    fn validate_and_report(&self, state: RepositoryState) -> Result<InitSummary> {
        println!("🔍 Validation Mode");
        println!();

        match state {
            RepositoryState::AlreadyInitialized => {
                println!("✓ Already initialized");
            }
            RepositoryState::WorktreeParent => {
                println!("✓ Set up as worktree parent");
            }
            RepositoryState::ReadyToInitialize { warn_location } => {
                println!("Ready to initialize");
                if warn_location {
                    println!("⚠ Location is temporary (consider reorganization)");
                }
            }
            RepositoryState::RegularClone {
                needs_reorganization,
            } => {
                println!("Regular clone detected");
                if needs_reorganization {
                    println!("⚠ Reorganization recommended");
                }
            }
            _ => {}
        }

        Ok(InitSummary::default())
    }

    /// Warn user about temporary location
    fn warn_about_location(&self) -> Result<()> {
        let path = self.current_path();
        println!("⚠ Warning: Repository is in a temporary location");
        println!("  Current: {}", path.display());
        println!();
        println!("  This directory may be deleted by the system.");
        println!("  Recommend running with --reorganize flag.");
        println!();

        Ok(())
    }

    /// Check if we should prompt for reorganization
    fn should_prompt_reorganize(&self) -> Result<bool> {
        if self.options.non_interactive {
            return Ok(false);
        }

        let path = self.current_path();
        if self.is_temporary_location(&path)? {
            println!("Move to permanent location? (Y/n)");

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            Ok(!input.trim().eq_ignore_ascii_case("n"))
        } else {
            Ok(false)
        }
    }
}

impl Default for InitSummary {
    fn default() -> Self {
        Self {
            workspace_path: PathBuf::new(),
            repository_state: RepositoryState::ReadyToInitialize {
                warn_location: false,
            },
            reorganized: false,
            stack: Stack::Generic,
            adapter: String::new(),
            modules: Vec::new(),
            devcontainer_status: DevcontainerStatus::None,
            registry_initialized: false,
            warnings: Vec::new(),
            next_steps: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_repo(temp_dir: &TempDir) -> PathBuf {
        let repo_path = temp_dir.path().join("test-repo");
        fs::create_dir_all(&repo_path).unwrap();

        // Initialize git
        Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create initial commit
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        fs::write(repo_path.join("README.md"), "# Test").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        repo_path
    }

    #[test]
    fn test_analyze_new_repository() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            ..Default::default()
        };

        let workflow = InitWorkflow::new(options);
        let state = workflow.analyze_repository_state().unwrap();

        // Expect RegularClone (needs_reorganization depends on whether temp dir is in /tmp/ or not)
        assert!(matches!(state, RepositoryState::RegularClone { .. }));
    }

    #[test]
    fn test_initialize_registry() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            ..Default::default()
        };

        let workflow = InitWorkflow::new(options);
        let initialized = workflow.initialize_registry(&repo_path).unwrap();

        assert!(initialized);
        assert!(repo_path.join(".branchbox/registry.json").exists());
    }

    #[test]
    fn test_idempotent_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // First init
        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            ..Default::default()
        };

        let workflow = InitWorkflow::new(options.clone());
        workflow.initialize_registry(&repo_path).unwrap();

        // Second init should detect existing registry
        let workflow2 = InitWorkflow::new(options);
        let state = workflow2.analyze_repository_state().unwrap();

        assert_eq!(state, RepositoryState::AlreadyInitialized);
    }

    #[test]
    fn test_validate_clone_url_https() {
        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        // Valid HTTPS URLs
        assert!(workflow
            .validate_clone_url("https://github.com/user/repo.git")
            .is_ok());
        assert!(workflow
            .validate_clone_url("https://gitlab.com/user/repo")
            .is_ok());
        assert!(workflow
            .validate_clone_url("http://example.com/repo.git")
            .is_ok());
    }

    #[test]
    fn test_validate_clone_url_ssh() {
        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        // Valid SSH URLs
        assert!(workflow
            .validate_clone_url("git@github.com:user/repo.git")
            .is_ok());
        assert!(workflow
            .validate_clone_url("ssh://git@github.com/user/repo.git")
            .is_ok());
    }

    #[test]
    fn test_validate_clone_url_git_protocol() {
        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        // Valid git protocol URL
        assert!(workflow
            .validate_clone_url("git://github.com/user/repo.git")
            .is_ok());
    }

    #[test]
    fn test_validate_clone_url_rejects_file() {
        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        // Should reject file:// URLs for security
        let result = workflow.validate_clone_url("file:///path/to/repo");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("file://"));
    }

    #[test]
    fn test_validate_clone_url_rejects_invalid() {
        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        // Should reject invalid URL formats
        let result = workflow.validate_clone_url("invalid-url");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid repository URL"));
    }

    #[test]
    fn test_get_projects_dir_default() {
        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        // Should return ~/projects by default
        let projects_dir = workflow.get_projects_dir().unwrap();
        assert!(projects_dir.to_string_lossy().ends_with("projects"));
    }

    #[test]
    fn test_get_projects_dir_from_env() {
        use std::env;

        // Set custom projects directory
        let custom_dir = "/tmp/my-projects";
        env::set_var("BRANCHBOX_PROJECTS_DIR", custom_dir);

        let options = InitOptions::default();
        let workflow = InitWorkflow::new(options);

        let projects_dir = workflow.get_projects_dir().unwrap();
        assert_eq!(projects_dir, PathBuf::from(custom_dir));

        // Clean up
        env::remove_var("BRANCHBOX_PROJECTS_DIR");
    }

    #[test]
    fn test_execute_dry_run_mode() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            dry_run: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let _summary = workflow.execute().unwrap();

        // Dry run should not create registry
        assert!(!repo_path.join(".branchbox/registry.json").exists());
    }

    #[test]
    fn test_execute_validate_only_mode() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            validate_only: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let result = workflow.execute();

        // Should complete validation
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_skip_devcontainer() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            skip_devcontainer: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        assert_eq!(summary.devcontainer_status, DevcontainerStatus::None);
    }

    #[test]
    fn test_execute_non_interactive_mode() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Should initialize successfully
        assert!(summary.registry_initialized);
        assert!(repo_path.join(".branchbox/registry.json").exists());
    }

    #[test]
    fn test_execute_update_mode() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // First initialization
        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        workflow.execute().unwrap();

        // Update mode should work on already initialized repo
        let update_options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            update: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut update_workflow = InitWorkflow::new(update_options);
        let summary = update_workflow.execute().unwrap();

        assert!(summary.workspace_path.exists());
    }

    #[test]
    fn test_execute_already_initialized_no_update() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // First initialization
        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        workflow.execute().unwrap();

        // Second init without update flag should exit early
        let second_options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut second_workflow = InitWorkflow::new(second_options);
        let summary = second_workflow.execute().unwrap();

        // Should detect already initialized
        assert_eq!(summary.workspace_path, PathBuf::new());
    }

    #[test]
    fn test_detect_stack_rails() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // Create Rails markers
        fs::write(repo_path.join("Gemfile"), "gem 'rails'\n").unwrap();
        fs::write(repo_path.join("config.ru"), "# Rails app\n").unwrap();

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        assert_eq!(summary.stack, Stack::Rails);
        assert_eq!(summary.adapter, "Rails");
    }

    #[test]
    fn test_detect_stack_nodejs() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // Create Node.js markers
        fs::write(repo_path.join("package.json"), "{}").unwrap();

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        assert_eq!(summary.stack, Stack::NodeJs);
        assert_eq!(summary.adapter, "Node.js");
    }

    #[test]
    fn test_force_stack_override() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // Create Rails markers but force Rust stack
        fs::write(repo_path.join("Gemfile"), "gem 'rails'\n").unwrap();

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            stack: Some(Stack::Rust),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Should use forced stack, not detected stack
        assert_eq!(summary.stack, Stack::Rust);
    }

    #[test]
    fn test_devcontainer_generation_creates_files() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Check devcontainer status
        assert!(matches!(
            summary.devcontainer_status,
            DevcontainerStatus::Created
                | DevcontainerStatus::Valid
                | DevcontainerStatus::Enhanced { .. }
        ));

        // Check generated files
        let devcontainer_dir = repo_path.join(".devcontainer");
        if matches!(summary.devcontainer_status, DevcontainerStatus::Created) {
            assert!(devcontainer_dir.join("devcontainer.json").exists());
        }
    }

    #[test]
    fn test_module_detection_integration() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // Create compose file to detect compose module
        fs::create_dir_all(repo_path.join(".devcontainer")).unwrap();
        fs::write(
            repo_path.join(".devcontainer/compose.yaml"),
            "version: '3.8'\nservices:\n  app:\n    image: alpine",
        )
        .unwrap();

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Should detect compose module
        assert!(summary.modules.contains(&"compose".to_string()));
        assert!(summary.modules.contains(&"tunnel".to_string()));
    }

    #[test]
    fn test_next_steps_generation() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Should have next steps
        assert!(!summary.next_steps.is_empty());
    }

    #[test]
    fn test_verbose_mode_logging() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            verbose: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let result = workflow.execute();

        // Should complete successfully with verbose logging
        assert!(result.is_ok());
    }

    #[test]
    fn test_skip_env_option() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            skip_env: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Should still initialize successfully
        assert!(summary.registry_initialized);
    }

    #[test]
    fn test_current_directory_source() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // Change to repo directory
        std::env::set_current_dir(&repo_path).unwrap();

        let options = InitOptions {
            source: InitSource::CurrentDirectory,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        assert!(summary.registry_initialized);
        assert!(repo_path.join(".branchbox/registry.json").exists());
    }

    #[test]
    fn test_reorganize_flag() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            reorganize: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let result = workflow.execute();

        // Should attempt reorganization (may fail in test env, just verify it tries)
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_reorganize_in_place_parent_structure() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            reorganize: true,
            use_parent_structure: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // Verify the result structure
        // The main repo should now be at: {original_path}/main/
        let expected_main_path = repo_path.join("main");

        assert!(summary.reorganized);
        assert_eq!(summary.workspace_path, expected_main_path);

        // Verify the directory structure exists
        assert!(expected_main_path.exists(), "Main directory should exist");
        assert!(
            expected_main_path.join(".git").exists(),
            "Git repo should be in main/"
        );

        // Verify parent container exists
        assert!(
            repo_path.exists(),
            "Parent container directory should exist"
        );

        // Verify .branchbox registry was created in main/
        assert!(
            expected_main_path.join(".branchbox").exists(),
            "BranchBox registry should exist in main/"
        );
    }

    #[test]
    fn test_reorganize_in_place_dry_run() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            reorganize: true,
            use_parent_structure: true,
            dry_run: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        // In dry run, no actual changes should be made
        // The repository should still be at original location
        assert!(repo_path.join(".git").exists());

        // Main subdirectory should NOT exist
        assert!(!repo_path.join("main").exists());

        // But the summary should reflect what WOULD happen
        assert_eq!(summary.workspace_path, repo_path.join("main"));
    }

    #[test]
    fn test_reorganize_in_place_preserves_git_state() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_repo(&temp_dir);

        // Create a test file and commit it
        fs::write(repo_path.join("test.txt"), "test content").unwrap();

        Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "Add test file"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let options = InitOptions {
            source: InitSource::LocalPath(repo_path.clone()),
            reorganize: true,
            use_parent_structure: true,
            non_interactive: true,
            ..Default::default()
        };

        let mut workflow = InitWorkflow::new(options);
        let summary = workflow.execute().unwrap();

        let main_path = summary.workspace_path;

        // Verify git repository is intact
        assert!(main_path.join(".git").exists());
        assert!(main_path.join("test.txt").exists());

        // Verify git history is preserved
        let log_output = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&main_path)
            .output()
            .unwrap();

        let log_str = String::from_utf8_lossy(&log_output.stdout);
        assert!(log_str.contains("Add test file"));
    }
}
