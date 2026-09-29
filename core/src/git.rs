//! Git worktree operations
//!
//! Provides functionality for creating, managing, and removing git worktrees.

use crate::{Error, Result};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve the shared Git directory, including repositories that use a linked
/// worktree or an external `--separate-git-dir` location.
pub(crate) fn repository_common_git_dir(repo_root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
            "rev-parse",
            "--git-common-dir",
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|err| {
            Error::git(format!(
                "Failed to resolve repository shared Git metadata: {err}"
            ))
        })?;
    if !output.status.success() {
        return Err(Error::git(format!(
            "Failed to resolve repository shared Git metadata: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Err(Error::validation(
            "Repository shared Git metadata path is empty".to_string(),
        ));
    }
    let common_dir = PathBuf::from(raw);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        repo_root.join(common_dir)
    };
    fs::canonicalize(&common_dir).map_err(|err| {
        Error::validation(format!(
            "Cannot validate repository shared Git metadata at '{}': {err}",
            common_dir.display()
        ))
    })
}

/// A managed checkout runs as the trusted runtime UID. Reject common Git metadata already
/// writable by another UID before checkout can consult config or info/attributes. A prior coding
/// consumer receives recursive group write access to this tree, so each managed launch requires a
/// fresh private Git directory supplied by the outer runtime.
#[cfg(unix)]
pub(crate) fn require_private_common_git_metadata(repo_root: &Path) -> Result<()> {
    let common = repository_common_git_dir(repo_root)?;
    let runtime_uid = unsafe { libc::geteuid() };
    let canonical_repo = fs::canonicalize(repo_root)?;
    if canonical_repo != repo_root {
        return Err(Error::validation(
            "Managed in-guest repository path must be canonical before Git metadata validation",
        ));
    }
    // A linked worktree or --separate-git-dir repository uses a .git *file* as the pointer to
    // its administrative tree. Validating only the resolved common directory would miss an old
    // consumer's ability to retarget that pointer before checkout.
    let git_entry = fs::symlink_metadata(canonical_repo.join(".git"))?;
    if git_entry.uid() != runtime_uid
        || !(git_entry.is_dir() || git_entry.is_file())
        || git_entry.mode() & 0o022 != 0
    {
        return Err(Error::validation(
            "Managed in-guest launch requires a private runtime-owned .git entry",
        ));
    }
    for root in [canonical_repo.as_path(), common.parent().unwrap_or(&common)] {
        for ancestor in root.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)?;
            let mode = metadata.mode();
            // A sticky parent such as /tmp protects its runtime-owned child. Every other parent
            // must be private against a different UID replacing a checked Git path.
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || (metadata.uid() != runtime_uid && metadata.uid() != 0)
                || (mode & 0o022 != 0 && mode & 0o1000 == 0)
            {
                return Err(Error::validation(format!(
                    "Managed in-guest Git metadata ancestor '{}' may be replaceable by the workspace consumer",
                    ancestor.display()
                )));
            }
        }
    }
    for entry in walkdir::WalkDir::new(&common).follow_links(false) {
        let entry = entry.map_err(|err| {
            Error::validation(format!(
                "Cannot inspect managed Git metadata below '{}': {err}",
                common.display()
            ))
        })?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.uid() != runtime_uid
            || !(metadata.is_file() || metadata.is_dir())
            || metadata.mode() & 0o022 != 0
        {
            return Err(Error::validation(format!(
                "Managed in-guest launch requires fresh runtime-owned Git metadata without consumer-writable paths; '{}' is unsafe",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn require_private_common_git_metadata(_repo_root: &Path) -> Result<()> {
    Err(Error::validation(
        "Managed Git metadata isolation requires a Unix in-guest runtime",
    ))
}

/// Git worktree manager
#[derive(Debug)]
pub struct GitWorktree {
    repo_path: PathBuf,
}

impl GitWorktree {
    /// Administrative Git commands after a managed consumer has write access to the common Git
    /// directory must not execute hooks or filesystem monitors from consumer-edited config.
    /// Callers must also avoid status-like commands that can execute configured clean filters.
    fn metadata_command(&self, consumer_writable: bool) -> Command {
        let mut command = Command::new("git");
        command.current_dir(&self.repo_path);
        if consumer_writable {
            command.arg("--no-pager");
            command.args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.attributesFile=/dev/null",
            ]);
        }
        command
    }

    /// Create a new GitWorktree instance
    pub fn new(repo_path: impl Into<PathBuf>) -> Result<Self> {
        let repo_path = repo_path.into();

        if !repo_path.exists() {
            return Err(Error::validation(format!(
                "Repository path does not exist: {}",
                repo_path.display()
            )));
        }

        // Verify it's a git repository
        let git_dir = repo_path.join(".git");
        if !git_dir.exists() {
            return Err(Error::validation(format!(
                "Not a git repository: {}",
                repo_path.display()
            )));
        }

        Ok(Self { repo_path })
    }

    /// Create a new worktree
    ///
    /// # Arguments
    ///
    /// * `path` - Path where the worktree should be created
    /// * `branch` - Branch name for the worktree
    /// * `base_branch` - Optional base branch to fork from (defaults to current branch)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use worktree_core::git::GitWorktree;
    /// use std::path::Path;
    ///
    /// let git = GitWorktree::new("/path/to/repo").unwrap();
    /// git.create(
    ///     Path::new("/path/to/worktree"),
    ///     "feature/new-feature",
    ///     Some("main")
    /// ).unwrap();
    /// ```
    pub fn create(&self, path: &Path, branch: &str, base_branch: Option<&str>) -> Result<()> {
        self.create_with_checkout_policy(path, branch, base_branch, true)
    }

    /// Create a worktree for an untrusted repository revision without running repository hooks or
    /// ambient global attributes. Callers must separately reject repository filter attributes.
    /// Checks the feature branch out in the repository itself, creating or
    /// resetting it, without adding a worktree.
    ///
    /// This is the checkout a caller wants when the clone is made for one run
    /// and discarded afterwards: there is no second worktree to isolate from, so
    /// the branch can simply live in the repository.
    ///
    /// # Errors
    ///
    /// Returns [`Error::git`] if the checkout cannot be performed.
    pub fn checkout_feature_branch(&self, branch: &str, base_branch: Option<&str>) -> Result<()> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        cmd.args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.attributesFile=/dev/null",
        ]);
        cmd.arg("checkout").arg("-B").arg(branch);
        if let Some(base) = base_branch {
            cmd.arg(base);
        }
        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to execute git checkout: {}", e)))?;
        if !output.status.success() {
            return Err(Error::git(format!(
                "Git checkout failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        tracing::info!("Checked out {} in the repository", branch);
        Ok(())
    }

    pub fn create_without_hooks(
        &self,
        path: &Path,
        branch: &str,
        base_branch: Option<&str>,
    ) -> Result<()> {
        self.create_with_checkout_policy(path, branch, base_branch, false)
    }

    fn create_with_checkout_policy(
        &self,
        path: &Path,
        branch: &str,
        base_branch: Option<&str>,
        allow_hooks: bool,
    ) -> Result<()> {
        if path.exists() {
            return Err(Error::WorktreeExists(path.to_path_buf()));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        if !allow_hooks {
            cmd.args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.attributesFile=/dev/null",
            ]);
        }
        cmd.arg("worktree").arg("add");

        // Add -B flag to create/reset branch
        cmd.arg("-B").arg(branch);

        // Add worktree path
        cmd.arg(path);

        // Add base branch if specified
        if let Some(base) = base_branch {
            cmd.arg(base);
        }

        tracing::debug!("Running: {:?}", cmd);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to execute git worktree add: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Git worktree add failed: {}", stderr)));
        }

        tracing::info!("Created worktree at {}", path.display());
        Ok(())
    }

    /// Attach an existing branch to a worktree path without resetting it.
    pub fn attach_existing_branch(&self, path: &Path, branch: &str) -> Result<()> {
        self.attach_existing_branch_with_checkout_policy(path, branch, true)
    }

    /// Attach an untrusted branch without running repository hooks or ambient global attributes.
    pub fn attach_existing_branch_without_hooks(&self, path: &Path, branch: &str) -> Result<()> {
        self.attach_existing_branch_with_checkout_policy(path, branch, false)
    }

    fn attach_existing_branch_with_checkout_policy(
        &self,
        path: &Path,
        branch: &str,
        allow_hooks: bool,
    ) -> Result<()> {
        if path.exists() {
            return Err(Error::WorktreeExists(path.to_path_buf()));
        }

        // Ensure any stale worktree registrations are removed before attempting to attach.
        if let Err(err) = self.prune_with_metadata_policy(!allow_hooks) {
            tracing::debug!("Skipping worktree prune before attach: {}", err);
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        if !allow_hooks {
            cmd.args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.attributesFile=/dev/null",
            ]);
        }
        cmd.arg("worktree").arg("add");
        cmd.arg(path);
        cmd.arg(branch);

        tracing::debug!("Running: {:?}", cmd);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to execute git worktree add: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Git worktree add failed: {}", stderr)));
        }

        tracing::info!(
            "Attached existing branch '{}' at {}",
            branch,
            path.display()
        );
        Ok(())
    }

    /// Remove a worktree
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the worktree to remove
    /// * `force` - Force removal even if worktree has uncommitted changes
    pub fn remove(&self, path: &Path, force: bool) -> Result<()> {
        self.remove_with_metadata_policy(path, force, false)
    }

    /// Forced removal avoids Git's dirty-worktree scan, which can execute a consumer-configured
    /// clean filter. Used only after managed in-guest access delegation.
    pub fn remove_consumer_writable(&self, path: &Path) -> Result<()> {
        self.remove_with_metadata_policy(path, true, true)
    }

    fn remove_with_metadata_policy(
        &self,
        path: &Path,
        force: bool,
        consumer_writable: bool,
    ) -> Result<()> {
        let mut cmd = self.metadata_command(consumer_writable);
        cmd.arg("worktree").arg("remove");

        if force {
            cmd.arg("--force");
        }

        cmd.arg(path);

        tracing::debug!("Running: {:?}", cmd);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to execute git worktree remove: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!(
                "Git worktree remove failed: {}",
                stderr
            )));
        }

        tracing::info!("Removed worktree at {}", path.display());
        Ok(())
    }

    /// Prune stale worktree registrations.
    pub fn prune(&self) -> Result<()> {
        self.prune_with_metadata_policy(false)
    }

    pub fn prune_consumer_writable(&self) -> Result<()> {
        self.prune_with_metadata_policy(true)
    }

    fn prune_with_metadata_policy(&self, consumer_writable: bool) -> Result<()> {
        let mut cmd = self.metadata_command(consumer_writable);
        cmd.arg("worktree").arg("prune");

        tracing::debug!("Running: {:?}", cmd);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to execute git worktree prune: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Git worktree prune failed: {}", stderr)));
        }

        Ok(())
    }

    /// List all worktrees
    ///
    /// Returns information about all worktrees in the repository.
    pub fn list(&self) -> Result<Vec<WorktreeInfo>> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        cmd.arg("worktree").arg("list").arg("--porcelain");

        tracing::debug!("Running: {:?}", cmd);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to execute git worktree list: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Git worktree list failed: {}", stderr)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_worktree_list(&stdout)
    }

    /// Check if a branch exists
    pub fn branch_exists(&self, branch: &str) -> Result<bool> {
        self.branch_exists_with_metadata_policy(branch, false)
    }

    pub fn branch_exists_consumer_writable(&self, branch: &str) -> Result<bool> {
        self.branch_exists_with_metadata_policy(branch, true)
    }

    fn branch_exists_with_metadata_policy(
        &self,
        branch: &str,
        consumer_writable: bool,
    ) -> Result<bool> {
        let output = self
            .metadata_command(consumer_writable)
            .args(["branch", "--list", branch])
            .output()
            .map_err(|e| Error::git(format!("Failed to check branch: {}", e)))?;

        if !output.status.success() {
            return Err(Error::git("Git branch list failed".to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }

    /// Check if a remote branch exists.
    pub fn remote_branch_exists(&self, remote: &str, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["ls-remote", "--heads", remote, branch])
            .output()
            .map_err(|e| Error::git(format!("Failed to query remote branches: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::debug!(
                "git ls-remote for {}/{} failed, assuming branch absent: {}",
                remote,
                branch,
                stderr
            );
            return Ok(false);
        }

        Ok(!output.stdout.is_empty())
    }

    /// List configured git remotes.
    pub fn list_remotes(&self) -> Result<Vec<String>> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .arg("remote")
            .output()
            .map_err(|e| Error::git(format!("Failed to list remotes: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Git remote failed: {}", stderr)));
        }

        let remotes = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        Ok(remotes)
    }

    /// Create a local branch pointing at a remote branch tip.
    pub fn create_branch_from_remote(&self, branch: &str, remote: &str) -> Result<()> {
        let remote_ref = format!("{}/{}", remote, branch);
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        cmd.args(["branch", branch, &remote_ref]);

        tracing::debug!("Running: {:?}", cmd);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to create branch from remote: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!(
                "Git branch creation from remote failed: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Delete a branch
    pub fn delete_branch(&self, branch: &str, force: bool) -> Result<()> {
        self.delete_branch_with_metadata_policy(branch, force, false)
    }

    pub fn delete_branch_consumer_writable(&self, branch: &str, force: bool) -> Result<()> {
        self.delete_branch_with_metadata_policy(branch, force, true)
    }

    fn delete_branch_with_metadata_policy(
        &self,
        branch: &str,
        force: bool,
        consumer_writable: bool,
    ) -> Result<()> {
        let mut cmd = self.metadata_command(consumer_writable);
        cmd.arg("branch");

        if force {
            cmd.arg("-D");
        } else {
            cmd.arg("-d");
        }

        cmd.arg(branch);

        let output = cmd
            .output()
            .map_err(|e| Error::git(format!("Failed to delete branch: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("Git branch delete failed: {}", stderr)));
        }

        tracing::info!("Deleted branch {}", branch);
        Ok(())
    }
}

/// Parse git worktree list --porcelain output
fn parse_worktree_list(output: &str) -> Result<Vec<WorktreeInfo>> {
    let mut worktrees = Vec::new();
    let mut current_worktree: Option<WorktreeInfo> = None;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous worktree if exists
            if let Some(wt) = current_worktree.take() {
                worktrees.push(wt);
            }

            // Start new worktree
            let path = line.strip_prefix("worktree ").unwrap_or("");
            current_worktree = Some(WorktreeInfo {
                path: PathBuf::from(path),
                branch: String::new(),
                is_bare: false,
                is_detached: false,
            });
        } else if line.starts_with("branch ") {
            if let Some(ref mut wt) = current_worktree {
                let branch = line
                    .strip_prefix("branch ")
                    .unwrap_or("")
                    .trim_start_matches("refs/heads/");
                wt.branch = branch.to_string();
            }
        } else if line == "bare" {
            if let Some(ref mut wt) = current_worktree {
                wt.is_bare = true;
            }
        } else if line == "detached" {
            if let Some(ref mut wt) = current_worktree {
                wt.is_detached = true;
            }
        }
    }

    // Add last worktree
    if let Some(wt) = current_worktree {
        worktrees.push(wt);
    }

    Ok(worktrees)
}

/// Information about a git worktree
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub is_bare: bool,
    pub is_detached: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper to create a test git repository
    fn setup_test_repo() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        // Initialize git repo
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Configure git user (required for commits)
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Create initial commit
        fs::write(repo_path.join("README.md"), "# Test Repo").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        temp_dir
    }

    #[test]
    fn test_new_git_worktree() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        assert_eq!(git.repo_path, temp_dir.path());
    }

    #[test]
    fn test_new_git_worktree_invalid_path() {
        let result = GitWorktree::new("/nonexistent/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_git_worktree_not_a_repo() {
        let temp_dir = TempDir::new().unwrap();
        let result = GitWorktree::new(temp_dir.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Not a git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn managed_checkout_rejects_previously_delegated_common_git_metadata() {
        let repo = setup_test_repo();
        let repo_path = fs::canonicalize(repo.path()).unwrap();
        require_private_common_git_metadata(&repo_path).unwrap();
        let common = repo_path.join(".git");
        fs::set_permissions(&common, fs::Permissions::from_mode(0o775)).unwrap();
        let error = require_private_common_git_metadata(&repo_path).unwrap_err();
        assert!(error
            .to_string()
            .contains("private runtime-owned .git entry"));
        fs::set_permissions(&common, fs::Permissions::from_mode(0o755)).unwrap();

        fs::set_permissions(&repo_path, fs::Permissions::from_mode(0o770)).unwrap();
        let error = require_private_common_git_metadata(&repo_path).unwrap_err();
        assert!(error.to_string().contains("ancestor"));
        fs::set_permissions(&repo_path, fs::Permissions::from_mode(0o700)).unwrap();

        let attributes = common.join("info/attributes");
        fs::write(&attributes, "*.txt filter=attacker\n").unwrap();
        fs::set_permissions(&attributes, fs::Permissions::from_mode(0o664)).unwrap();
        let error = require_private_common_git_metadata(&repo_path).unwrap_err();
        assert!(error.to_string().contains("consumer-writable paths"));

        let linked = repo_path.join("linked");
        fs::set_permissions(&attributes, fs::Permissions::from_mode(0o644)).unwrap();
        let git = GitWorktree::new(&repo_path).unwrap();
        git.create(&linked, "feature/linked", Some("main")).unwrap();
        fs::set_permissions(linked.join(".git"), fs::Permissions::from_mode(0o664)).unwrap();
        let error = require_private_common_git_metadata(&linked).unwrap_err();
        assert!(error
            .to_string()
            .contains("private runtime-owned .git entry"));
    }

    #[test]
    fn test_create_worktree() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        let worktree_path = temp_dir.path().join("feature-worktree");
        git.create(&worktree_path, "feature/test", Some("main"))
            .unwrap();

        // Verify worktree was created
        assert!(worktree_path.exists());
        assert!(worktree_path.join(".git").exists());
    }

    #[test]
    fn test_create_worktree_already_exists() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        let worktree_path = temp_dir.path().join("feature-worktree");
        fs::create_dir(&worktree_path).unwrap();

        let result = git.create(&worktree_path, "feature/test", Some("main"));
        assert!(result.is_err());
    }

    #[test]
    fn test_list_worktrees() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Should have at least one worktree (the main repo)
        let worktrees = git.list().unwrap();
        assert!(!worktrees.is_empty());
        let expected_path = temp_dir.path().canonicalize().unwrap();
        assert_eq!(worktrees[0].path, expected_path);

        // Create a new worktree
        let worktree_path = temp_dir.path().join("feature-worktree");
        git.create(&worktree_path, "feature/test", Some("main"))
            .unwrap();

        // Should now have two worktrees
        let worktrees = git.list().unwrap();
        assert_eq!(worktrees.len(), 2);
    }

    #[test]
    fn test_remove_worktree() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        let worktree_path = temp_dir.path().join("feature-worktree");
        git.create(&worktree_path, "feature/test", Some("main"))
            .unwrap();

        assert!(worktree_path.exists());

        git.remove(&worktree_path, false).unwrap();

        assert!(!worktree_path.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn consumer_writable_metadata_cleanup_never_executes_repository_commands() {
        let repo = setup_test_repo();
        let git = GitWorktree::new(repo.path()).unwrap();
        let worktree = repo.path().join("consumer-worktree");
        git.create(&worktree, "feature/consumer", Some("main"))
            .unwrap();

        // The consumer can edit both the task worktree and shared Git config after delegation.
        // A same-size tracked edit makes ordinary `git status` run the configured clean filter.
        fs::write(worktree.join(".gitattributes"), "*.md filter=attacker\n").unwrap();
        let added = Command::new("git")
            .args(["add", ".gitattributes"])
            .current_dir(&worktree)
            .status()
            .unwrap();
        assert!(added.success());
        fs::write(worktree.join("README.md"), "# Test RepO").unwrap();

        let trap = repo.path().join("git-trap.sh");
        let hook = repo.path().join("reference-transaction");
        let marker = repo.path().join("git-command-executed");
        let script =
            "#!/bin/sh\nprintf invoked >> \"$(dirname \"$0\")/git-command-executed\"\ncat\n";
        for path in [&trap, &hook] {
            fs::write(path, script).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for (key, value) in [
            ("core.fsmonitor", trap.as_os_str()),
            ("core.hooksPath", repo.path().as_os_str()),
            ("filter.attacker.clean", trap.as_os_str()),
        ] {
            let configured = Command::new("git")
                .arg("config")
                .arg(key)
                .arg(value)
                .current_dir(repo.path())
                .status()
                .unwrap();
            assert!(configured.success());
        }

        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&worktree)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            marker.exists(),
            "the adversarial Git fixture did not trigger"
        );
        fs::remove_file(&marker).unwrap();

        git.remove_consumer_writable(&worktree).unwrap();
        git.prune_consumer_writable().unwrap();
        assert!(git
            .branch_exists_consumer_writable("feature/consumer")
            .unwrap());
        git.delete_branch_consumer_writable("feature/consumer", true)
            .unwrap();
        assert!(!worktree.exists());
        assert!(
            !marker.exists(),
            "managed cleanup executed attacker Git config"
        );
    }

    #[test]
    fn test_attach_existing_branch_preserves_tip() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let git = GitWorktree::new(repo_path).unwrap();

        let worktree_path = repo_path.join("feature-worktree");
        git.create(&worktree_path, "feature/test", Some("main"))
            .unwrap();

        // Advance the branch HEAD with a new commit in the worktree.
        let feature_file = worktree_path.join("feature.txt");
        fs::write(&feature_file, "feature work").unwrap();
        Command::new("git")
            .current_dir(&worktree_path)
            .args(["add", feature_file.file_name().unwrap().to_str().unwrap()])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(&worktree_path)
            .args(["commit", "-m", "Advance feature branch"])
            .status()
            .unwrap();

        let head_before = Command::new("git")
            .current_dir(repo_path)
            .args(["rev-parse", "feature/test"])
            .output()
            .unwrap();
        let head_before = String::from_utf8(head_before.stdout).unwrap();

        // Remove the worktree directory to simulate manual cleanup.
        fs::remove_dir_all(&worktree_path).unwrap();

        // Reattach without resetting the branch.
        let restored_path = repo_path.join("feature-worktree-restored");
        git.attach_existing_branch(&restored_path, "feature/test")
            .unwrap();
        assert!(restored_path.exists());

        let head_after = Command::new("git")
            .current_dir(repo_path)
            .args(["rev-parse", "feature/test"])
            .output()
            .unwrap();
        let head_after = String::from_utf8(head_after.stdout).unwrap();

        assert_eq!(head_before, head_after);
    }

    #[test]
    fn test_branch_exists() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        assert!(git.branch_exists("main").unwrap());
        assert!(!git.branch_exists("nonexistent").unwrap());
    }

    #[test]
    fn test_delete_branch() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Create a new branch
        Command::new("git")
            .args(["branch", "test-branch"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        assert!(git.branch_exists("test-branch").unwrap());

        git.delete_branch("test-branch", false).unwrap();

        assert!(!git.branch_exists("test-branch").unwrap());
    }

    #[test]
    fn test_parse_worktree_list() {
        let output = r#"worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/feature
HEAD def456
branch refs/heads/feature/test

worktree /path/to/detached
HEAD 123abc
detached
"#;

        let worktrees = parse_worktree_list(output).unwrap();
        assert_eq!(worktrees.len(), 3);

        assert_eq!(worktrees[0].path, PathBuf::from("/path/to/repo"));
        assert_eq!(worktrees[0].branch, "main");
        assert!(!worktrees[0].is_detached);

        assert_eq!(worktrees[1].path, PathBuf::from("/path/to/feature"));
        assert_eq!(worktrees[1].branch, "feature/test");
        assert!(!worktrees[1].is_detached);

        assert_eq!(worktrees[2].path, PathBuf::from("/path/to/detached"));
        assert!(worktrees[2].is_detached);
    }

    #[test]
    fn test_parse_worktree_list_bare() {
        let output = r#"worktree /path/to/bare-repo
bare
"#;

        let worktrees = parse_worktree_list(output).unwrap();
        assert_eq!(worktrees.len(), 1);
        assert!(worktrees[0].is_bare);
    }

    #[test]
    fn test_parse_worktree_list_empty() {
        let worktrees = parse_worktree_list("").unwrap();
        assert_eq!(worktrees.len(), 0);
    }

    #[test]
    fn test_prune_worktrees() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Prune should succeed even if there are no stale worktrees
        let result = git.prune();
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_worktree_without_base_branch() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        let worktree_path = temp_dir.path().join("feature-worktree");
        git.create(&worktree_path, "feature/test", None).unwrap();

        // Verify worktree was created
        assert!(worktree_path.exists());
        assert!(worktree_path.join(".git").exists());
    }

    #[test]
    fn test_remove_worktree_with_force() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        let worktree_path = temp_dir.path().join("feature-worktree");
        git.create(&worktree_path, "feature/test", Some("main"))
            .unwrap();

        // Make uncommitted changes
        fs::write(worktree_path.join("uncommitted.txt"), "changes").unwrap();

        // Force remove should succeed even with uncommitted changes
        git.remove(&worktree_path, true).unwrap();

        assert!(!worktree_path.exists());
    }

    #[test]
    fn test_delete_branch_with_force() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Create and commit to a new branch
        let worktree_path = temp_dir.path().join("feature-worktree");
        git.create(&worktree_path, "feature/test", Some("main"))
            .unwrap();

        fs::write(worktree_path.join("test.txt"), "test").unwrap();
        Command::new("git")
            .current_dir(&worktree_path)
            .args(["add", "test.txt"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&worktree_path)
            .args(["commit", "-m", "test commit"])
            .output()
            .unwrap();

        // Remove worktree first
        git.remove(&worktree_path, true).unwrap();

        // Force delete the unmerged branch
        git.delete_branch("feature/test", true).unwrap();

        assert!(!git.branch_exists("feature/test").unwrap());
    }

    #[test]
    fn test_attach_existing_branch_path_exists() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Create a branch
        Command::new("git")
            .current_dir(temp_dir.path())
            .args(["branch", "test-branch"])
            .output()
            .unwrap();

        // Create the worktree path first
        let worktree_path = temp_dir.path().join("existing-path");
        fs::create_dir(&worktree_path).unwrap();

        // Attach should fail because path exists
        let result = git.attach_existing_branch(&worktree_path, "test-branch");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_remotes_no_remotes() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Fresh repo has no remotes
        let remotes = git.list_remotes().unwrap();
        assert_eq!(remotes.len(), 0);
    }

    #[test]
    fn test_list_remotes_with_remotes() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Add a remote
        Command::new("git")
            .current_dir(temp_dir.path())
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ])
            .output()
            .unwrap();

        let remotes = git.list_remotes().unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0], "origin");
    }

    #[test]
    fn test_remote_branch_exists_no_remote() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();

        // Query non-existent remote should return false
        let exists = git.remote_branch_exists("origin", "main").unwrap();
        assert!(!exists);
    }

    #[test]
    fn test_remote_branch_exists_with_remote() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        // Create a second repo to act as remote
        let remote_temp = TempDir::new().unwrap();
        let remote_path = remote_temp.path();

        Command::new("git")
            .args(["init", "-b", "main", "--bare"])
            .current_dir(remote_path)
            .output()
            .unwrap();

        // Add remote and push
        Command::new("git")
            .current_dir(repo_path)
            .args(["remote", "add", "origin", remote_path.to_str().unwrap()])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["push", "origin", "main"])
            .output()
            .unwrap();

        let git = GitWorktree::new(repo_path).unwrap();

        // main should exist on origin
        let exists = git.remote_branch_exists("origin", "main").unwrap();
        assert!(exists);

        // non-existent branch should not exist
        let exists = git.remote_branch_exists("origin", "nonexistent").unwrap();
        assert!(!exists);
    }

    #[test]
    fn test_create_branch_from_remote() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        // Create a second repo to act as remote
        let remote_temp = TempDir::new().unwrap();
        let remote_path = remote_temp.path();

        Command::new("git")
            .args(["init", "-b", "main", "--bare"])
            .current_dir(remote_path)
            .output()
            .unwrap();

        // Add remote and push main
        Command::new("git")
            .current_dir(repo_path)
            .args(["remote", "add", "origin", remote_path.to_str().unwrap()])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["push", "origin", "main"])
            .output()
            .unwrap();

        // Create a feature branch locally, push it, then delete it locally
        Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "feature-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["push", "origin", "feature-branch"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "-d", "feature-branch"])
            .output()
            .unwrap();

        // Fetch to get remote tracking branch
        Command::new("git")
            .current_dir(repo_path)
            .args(["fetch", "origin"])
            .output()
            .unwrap();

        let git = GitWorktree::new(repo_path).unwrap();

        // Verify branch doesn't exist locally
        assert!(!git.branch_exists("feature-branch").unwrap());

        // Create local branch from remote
        git.create_branch_from_remote("feature-branch", "origin")
            .unwrap();

        // Branch should now exist locally
        assert!(git.branch_exists("feature-branch").unwrap());
    }
}
