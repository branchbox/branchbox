//! Git worktree operations
//!
//! Provides functionality for creating, managing, and removing git worktrees.

use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Git worktree manager
#[derive(Debug)]
pub struct GitWorktree {
    repo_path: PathBuf,
}

impl GitWorktree {
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
        if path.exists() {
            return Err(Error::WorktreeExists(path.to_path_buf()));
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
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
        if path.exists() {
            return Err(Error::WorktreeExists(path.to_path_buf()));
        }

        // Ensure any stale worktree registrations are removed before attempting to attach.
        if let Err(err) = self.prune() {
            tracing::debug!("Skipping worktree prune before attach: {}", err);
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
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
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
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
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
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
        let output = Command::new("git")
            .current_dir(&self.repo_path)
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
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
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
}
