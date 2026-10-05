//! Git worktree operations
//!
//! Provides functionality for creating, managing, and removing git worktrees.

use crate::atomic_fs::{self, StateDirLock};
use crate::workflows::teardown_plan::{parse_porcelain_z, StatusEntry};
use crate::{Error, Result};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

/// How long a worktree change waits for another BranchBox process to finish its own. Generous,
/// because the holder may be checking out a large tree or running a slow post-checkout hook.
const WORKTREE_LOCK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

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
            return Err(Error::NotAGitRepository(repo_path));
        }

        Ok(Self { repo_path })
    }

    /// Serialize the commands that change worktrees or delete branches across BranchBox
    /// processes (and threads). Git cannot run them concurrently in one repository:
    /// `worktree add` writes `.git/worktrees/<id>/` file by file, and a concurrent command that
    /// scans the worktrees (another add, a remove, `branch -D`) dies reading a file that is still
    /// empty (`failed to read .git/worktrees/<id>/commondir`). The lock is taken on the shared
    /// git directory, apart from the registry lock, so a slow checkout never holds up registry
    /// updates.
    fn lock_worktree_admin(&self) -> Result<StateDirLock> {
        let common_dir = self.common_dir()?;
        atomic_fs::lock_state_dir(&common_dir, WORKTREE_LOCK_TIMEOUT).map_err(|err| match err {
            Error::RegistryLocked { path, waited_secs } => Error::git(format!(
                "Timed out after {waited_secs}s waiting for another branchbox process to finish \
                 changing the worktrees of {}. The lock is released when that process exits; \
                 retry once it has finished.",
                path.display()
            )),
            other => other,
        })
    }

    /// The repository's shared git directory (the main `.git`, also for a linked worktree).
    fn common_dir(&self) -> Result<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(&self.repo_path)
            .output()
            .map_err(|err| Error::git(format!("Failed to execute git rev-parse: {err}")))?;
        if !output.status.success() {
            return Err(Error::git(format!(
                "Failed to resolve the shared git directory of {}: {}",
                self.repo_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let raw = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        Ok(if raw.is_absolute() {
            raw
        } else {
            self.repo_path.join(raw)
        })
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
        let _lock = self.lock_worktree_admin()?;
        // Check again under the lock: a concurrent start of the same feature may have created the
        // worktree while this one waited, and git's own refusal would lose the worktree_exists code.
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
        let _lock = self.lock_worktree_admin()?;
        // Check again under the lock: a concurrent start of the same feature may have created the
        // worktree while this one waited, and git's own refusal would lose the worktree_exists code.
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
        let level = if force {
            RemovalForce::DiscardChanges
        } else {
            RemovalForce::Clean
        };
        self.remove_worktree(path, level)
    }

    /// Remove a worktree with `git worktree remove`, passing `--force` as often as `force`
    /// says: once discards modified and untracked files, twice also removes a locked worktree.
    pub fn remove_worktree(&self, path: &Path, force: RemovalForce) -> Result<()> {
        self.remove_with_metadata_policy(path, force, false)
    }

    /// Forced removal avoids Git's dirty-worktree scan, which can execute a consumer-configured
    /// clean filter. Used only after managed in-guest access delegation.
    pub fn remove_consumer_writable(&self, path: &Path) -> Result<()> {
        self.remove_with_metadata_policy(path, RemovalForce::DiscardChanges, true)
    }

    fn remove_with_metadata_policy(
        &self,
        path: &Path,
        force: RemovalForce,
        consumer_writable: bool,
    ) -> Result<()> {
        let _lock = self.lock_worktree_admin()?;
        let mut cmd = self.metadata_command(consumer_writable);
        cmd.arg("worktree").arg("remove");

        for _ in 0..force.flag_count() {
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
        let _lock = self.lock_worktree_admin()?;
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

    /// How `path` relates to this repository's worktrees, as `git worktree list` reports
    /// them: a linked worktree (with its lock), the main working tree, or neither.
    pub fn worktree_registration(&self, path: &Path) -> Result<WorktreeRegistration> {
        let wanted = comparable_path(path);
        let listed = self.list()?;
        let Some((position, info)) = listed
            .iter()
            .enumerate()
            .find(|(_, info)| comparable_path(&info.path) == wanted)
        else {
            return Ok(WorktreeRegistration::NotListed);
        };
        // `git worktree list` prints the main working tree (or the bare repository) first.
        if position == 0 || info.is_bare {
            return Ok(WorktreeRegistration::Main);
        }
        Ok(WorktreeRegistration::Linked {
            lock: info.locked.then(|| WorktreeLock {
                reason: info.lock_reason.clone(),
            }),
        })
    }

    /// Whether the worktree at `path` is locked (`git worktree lock`), and why. A path git does
    /// not list as a worktree is not locked.
    pub fn worktree_lock(&self, path: &Path) -> Result<Option<WorktreeLock>> {
        let wanted = comparable_path(path);
        Ok(self
            .list()?
            .into_iter()
            .find(|info| comparable_path(&info.path) == wanted)
            .filter(|info| info.locked)
            .map(|info| WorktreeLock {
                reason: info.lock_reason,
            }))
    }

    /// The uncommitted changes of the worktree at `worktree`, as `git status` reports them:
    /// staged, unstaged and untracked (every untracked file listed), renames as a deletion
    /// plus an addition, and changes inside submodules. Ignored files are not reported.
    ///
    /// Read-only: `--no-optional-locks` keeps git from refreshing the index, and repository
    /// discovery stops at `worktree`, so a worktree whose `.git` file is broken fails instead
    /// of reporting the status of an enclosing repository.
    pub fn status_entries(&self, worktree: &Path) -> Result<Vec<StatusEntry>> {
        let mut cmd = Command::new("git");
        cmd.current_dir(worktree)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args([
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--no-renames",
                "--ignore-submodules=none",
            ]);
        if let Some(parent) = worktree.parent() {
            cmd.env("GIT_CEILING_DIRECTORIES", parent);
        }
        let output = cmd.output().map_err(|err| {
            Error::git(format!(
                "Failed to run git status in {}: {err}",
                worktree.display()
            ))
        })?;
        if !output.status.success() {
            return Err(Error::git(format!(
                "git status failed: {}",
                stderr_text(&output)
            )));
        }
        parse_porcelain_z(&output.stdout).map_err(|err| {
            Error::git(format!(
                "Unexpected git status output in {}: {err}",
                worktree.display()
            ))
        })
    }

    /// Whether `refs/heads/<branch>` exists. Unlike [`Self::branch_exists`] the name is never
    /// read as a pattern.
    pub fn local_branch_exists(&self, branch: &str) -> Result<bool> {
        let output = self.git_output(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(Error::git(format!(
                "git show-ref failed for branch '{branch}': {}",
                stderr_text(&output)
            ))),
        }
    }

    /// How `branch` relates to the commit `git branch -d` would compare it with: its upstream
    /// when one is configured and resolves, otherwise `HEAD` of this repository's worktree.
    /// `merged` is what decides whether `git branch -d` deletes it.
    ///
    /// Merge state comes from `git merge-base --is-ancestor`, never from parsing
    /// `git branch --merged`, whose `+` marker for branches checked out in other worktrees
    /// defeats a name comparison.
    pub fn branch_merge_state(&self, branch: &str) -> Result<BranchMergeState> {
        let head_name = self.head_name()?;
        if !self.local_branch_exists(branch)? {
            return Ok(BranchMergeState {
                exists: false,
                upstream: None,
                reference: "HEAD".to_string(),
                reference_name: head_name,
                merged: false,
                merged_into_head: false,
                ahead: 0,
            });
        }

        let branch_ref = format!("refs/heads/{branch}");
        let upstream = self.resolved_upstream(branch)?;
        let (reference, reference_name) = match &upstream {
            Some((full, short)) => (full.clone(), short.clone()),
            None => ("HEAD".to_string(), head_name),
        };
        let merged = self.is_ancestor(&branch_ref, &reference)?;
        let merged_into_head = if upstream.is_some() {
            self.is_ancestor(&branch_ref, "HEAD")?
        } else {
            merged
        };
        let ahead = self.count_commits(&format!("{reference}..{branch_ref}"))?;
        Ok(BranchMergeState {
            exists: true,
            upstream: upstream.map(|(_, short)| short),
            reference,
            reference_name,
            merged,
            merged_into_head,
            ahead,
        })
    }

    /// The branch checked out in this worktree, or `HEAD` when it is detached.
    fn head_name(&self) -> Result<String> {
        let output = self.git_output(&["symbolic-ref", "--short", "-q", "HEAD"])?;
        match output.status.code() {
            Some(0) => Ok(String::from_utf8_lossy(&output.stdout).trim().to_string()),
            Some(1) => Ok("HEAD".to_string()),
            _ => Err(Error::git(format!(
                "git symbolic-ref HEAD failed: {}",
                stderr_text(&output)
            ))),
        }
    }

    /// The upstream of `branch` as (full ref, short name), when one is configured and the
    /// ref it names exists.
    fn resolved_upstream(&self, branch: &str) -> Result<Option<(String, String)>> {
        // `<branch>@{upstream}` takes a branch name, not a full ref; a name git would read as
        // an option has no usable upstream.
        if branch.starts_with('-') {
            return Ok(None);
        }
        let spec = format!("{branch}@{{upstream}}");
        let full = self.git_output(&["rev-parse", "--symbolic-full-name", &spec])?;
        if !full.status.success() {
            return Ok(None);
        }
        let full = String::from_utf8_lossy(&full.stdout).trim().to_string();
        if full.is_empty() {
            return Ok(None);
        }
        let resolves = self.git_output(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{full}^{{commit}}"),
        ])?;
        if !resolves.status.success() {
            return Ok(None);
        }
        let short = self.git_output(&["rev-parse", "--abbrev-ref", &spec])?;
        let short = String::from_utf8_lossy(&short.stdout).trim().to_string();
        let short = if short.is_empty() {
            full.clone()
        } else {
            short
        };
        Ok(Some((full, short)))
    }

    fn is_ancestor(&self, commit: &str, of: &str) -> Result<bool> {
        let output = self.git_output(&["merge-base", "--is-ancestor", commit, of])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(Error::git(format!(
                "git merge-base --is-ancestor {commit} {of} failed: {}",
                stderr_text(&output)
            ))),
        }
    }

    fn count_commits(&self, range: &str) -> Result<u32> {
        let output = self.git_output(&["rev-list", "--count", range])?;
        if !output.status.success() {
            return Err(Error::git(format!(
                "git rev-list --count {range} failed: {}",
                stderr_text(&output)
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.trim().parse().map_err(|err| {
            Error::git(format!(
                "git rev-list --count {range} printed '{}': {err}",
                text.trim()
            ))
        })
    }

    /// Run a read-only git command in the repository and return its output, whatever its exit
    /// status.
    fn git_output(&self, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .current_dir(&self.repo_path)
            .args(args)
            .output()
            .map_err(|err| Error::git(format!("Failed to execute git {}: {err}", args.join(" "))))
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
        let _lock = self.lock_worktree_admin()?;
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
                locked: false,
                lock_reason: None,
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
        } else if line == "locked" || line.starts_with("locked ") {
            if let Some(ref mut wt) = current_worktree {
                wt.locked = true;
                wt.lock_reason = line
                    .strip_prefix("locked ")
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
                    .map(str::to_string);
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
    /// Locked with `git worktree lock`; plain removal and pruning skip it.
    pub locked: bool,
    pub lock_reason: Option<String>,
}

/// A `git worktree lock` on a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeLock {
    pub reason: Option<String>,
}

/// How a directory relates to a repository's worktrees ([`GitWorktree::worktree_registration`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeRegistration {
    /// A linked worktree (`git worktree add`), with its `git worktree lock` if it has one.
    Linked { lock: Option<WorktreeLock> },
    /// The repository's main working tree (or its bare repository).
    Main,
    /// Not listed by `git worktree list`.
    NotListed,
}

/// How hard [`GitWorktree::remove_worktree`] may push `git worktree remove`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalForce {
    /// No `--force`: git refuses a worktree with modified or untracked files, or a lock.
    Clean,
    /// `--force` once: modified and untracked files are discarded; a locked worktree is still
    /// refused.
    DiscardChanges,
    /// `--force --force`: a locked worktree is removed too.
    IncludingLocked,
}

impl RemovalForce {
    fn flag_count(self) -> usize {
        match self {
            RemovalForce::Clean => 0,
            RemovalForce::DiscardChanges => 1,
            RemovalForce::IncludingLocked => 2,
        }
    }
}

/// Where a branch stands against the commit `git branch -d` compares it with
/// ([`GitWorktree::branch_merge_state`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchMergeState {
    pub exists: bool,
    /// The configured upstream (e.g. `origin/feature/eta`), when it resolves.
    pub upstream: Option<String>,
    /// The ref the branch is compared with: the upstream's full ref, or `HEAD`.
    pub reference: String,
    /// `reference` for people: the upstream's short name, the checked-out branch, or `HEAD`
    /// when detached.
    pub reference_name: String,
    /// Every commit of the branch is in `reference`, so `git branch -d` deletes it.
    pub merged: bool,
    /// Every commit of the branch is in `HEAD`.
    pub merged_into_head: bool,
    /// Commits on the branch that `reference` does not have.
    pub ahead: u32,
}

/// A command's stderr, trimmed, for error messages.
fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// `path` in the form `git worktree list` prints it: canonical when it exists (git records
/// resolved paths, e.g. `/private/var/…` for `/var/…`), else its canonical parent joined with
/// its name, else as given.
fn comparable_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map(|parent| parent.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
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

    #[test]
    fn test_common_dir_is_shared_by_linked_worktrees() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let shared = temp_dir.path().join(".git").canonicalize().unwrap();
        assert_eq!(git.common_dir().unwrap().canonicalize().unwrap(), shared);

        let linked_path = temp_dir.path().join("linked");
        git.create(&linked_path, "feature/linked", None).unwrap();
        let linked = GitWorktree::new(&linked_path).unwrap();
        assert_eq!(linked.common_dir().unwrap().canonicalize().unwrap(), shared);
    }

    #[test]
    fn test_worktree_changes_wait_for_another_holder() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let holder = git.lock_worktree_admin().unwrap();
        // The lock leaves nothing behind in the shared git directory on Unix.
        #[cfg(unix)]
        assert!(!temp_dir.path().join(".git/.lock").exists());

        let repo = temp_dir.path().to_path_buf();
        let started = std::time::Instant::now();
        let waiter = std::thread::spawn(move || {
            let git = GitWorktree::new(&repo).unwrap();
            git.create(&repo.join("waiting"), "feature/waiting", None)
                .unwrap();
            started.elapsed()
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !temp_dir.path().join("waiting").exists(),
            "the worktree was added while another process held the lock"
        );
        drop(holder);

        assert!(waiter.join().unwrap() >= Duration::from_millis(300));
        assert!(temp_dir.path().join("waiting").exists());
    }

    /// Run git in `dir` and require success.
    fn run(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_file(dir: &Path, name: &str, contents: &str) {
        fs::write(dir.join(name), contents).unwrap();
        run(dir, &["add", name]);
        run(dir, &["commit", "-q", "-m", &format!("Add {name}")]);
    }

    #[test]
    fn merge_state_of_a_branch_without_new_commits_is_merged() {
        let temp_dir = setup_test_repo();
        run(temp_dir.path(), &["branch", "feature/done"]);
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let state = git.branch_merge_state("feature/done").unwrap();
        assert_eq!(
            state,
            BranchMergeState {
                exists: true,
                upstream: None,
                reference: "HEAD".to_string(),
                reference_name: "main".to_string(),
                merged: true,
                merged_into_head: true,
                ahead: 0,
            }
        );
    }

    #[test]
    fn merge_state_counts_commits_head_lacks() {
        let temp_dir = setup_test_repo();
        let repo = temp_dir.path();
        run(repo, &["checkout", "-q", "-b", "feature/wip"]);
        commit_file(repo, "wip.txt", "wip");
        run(repo, &["checkout", "-q", "main"]);
        let git = GitWorktree::new(repo).unwrap();
        let state = git.branch_merge_state("feature/wip").unwrap();
        assert!(state.exists);
        assert!(!state.merged && !state.merged_into_head);
        assert_eq!(state.ahead, 1);
        assert_eq!(state.reference_name, "main");
    }

    #[test]
    fn merge_state_of_a_missing_branch() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let state = git.branch_merge_state("feature/nope").unwrap();
        assert!(!state.exists && !state.merged);
        assert_eq!(state.ahead, 0);
        assert_eq!(state.reference, "HEAD");
    }

    #[test]
    fn merge_state_follows_a_pushed_upstream_like_git_branch_d() {
        let temp_dir = setup_test_repo();
        let repo = temp_dir.path();
        let remote = TempDir::new().unwrap();
        run(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        run(
            repo,
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        run(repo, &["checkout", "-q", "-b", "feature/pushed"]);
        commit_file(repo, "pushed.txt", "pushed");
        run(repo, &["push", "-q", "-u", "origin", "feature/pushed"]);
        run(repo, &["checkout", "-q", "main"]);

        let git = GitWorktree::new(repo).unwrap();
        let state = git.branch_merge_state("feature/pushed").unwrap();
        assert_eq!(state.upstream.as_deref(), Some("origin/feature/pushed"));
        assert_eq!(state.reference, "refs/remotes/origin/feature/pushed");
        assert_eq!(state.reference_name, "origin/feature/pushed");
        assert!(
            state.merged,
            "merged into its upstream, so `git branch -d` deletes it"
        );
        assert!(!state.merged_into_head);
        assert_eq!(state.ahead, 0);

        // An upstream that no longer resolves falls back to HEAD, as git does.
        run(
            repo,
            &["update-ref", "-d", "refs/remotes/origin/feature/pushed"],
        );
        let state = git.branch_merge_state("feature/pushed").unwrap();
        assert_eq!(state.upstream, None);
        assert_eq!(state.reference, "HEAD");
        assert!(!state.merged);
        assert_eq!(state.ahead, 1);
    }

    #[test]
    fn merge_state_with_a_detached_head() {
        let temp_dir = setup_test_repo();
        let repo = temp_dir.path();
        run(repo, &["branch", "feature/here"]);
        run(repo, &["checkout", "-q", "--detach", "HEAD"]);
        let git = GitWorktree::new(repo).unwrap();
        let state = git.branch_merge_state("feature/here").unwrap();
        assert_eq!(state.reference_name, "HEAD");
        assert!(state.merged);
    }

    #[test]
    fn merge_state_of_a_branch_checked_out_in_a_linked_worktree() {
        // `git branch --merged` prints such a branch as "+ feature/linked", which the 0.13 name
        // comparison never matched.
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let linked = temp_dir.path().join("linked");
        git.create(&linked, "feature/linked", None).unwrap();
        let state = git.branch_merge_state("feature/linked").unwrap();
        assert!(state.merged);
        assert_eq!(state.ahead, 0);

        commit_file(&linked, "linked.txt", "linked");
        let state = git.branch_merge_state("feature/linked").unwrap();
        assert!(!state.merged);
        assert_eq!(state.ahead, 1);
    }

    #[test]
    fn local_branch_exists_never_matches_a_pattern() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        assert!(git.local_branch_exists("main").unwrap());
        assert!(!git.local_branch_exists("ma*").unwrap());
        assert!(git.branch_exists("ma*").unwrap(), "branch --list globs");
    }

    #[test]
    fn status_entries_list_every_kind_of_change() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let linked = temp_dir.path().join("linked");
        git.create(&linked, "feature/linked", None).unwrap();
        assert!(git.status_entries(&linked).unwrap().is_empty());

        fs::write(linked.join("README.md"), "changed").unwrap();
        fs::create_dir_all(linked.join("deep/dir")).unwrap();
        fs::write(linked.join("deep/dir/new file.txt"), "new").unwrap();
        fs::write(linked.join("staged.txt"), "staged").unwrap();
        run(&linked, &["add", "staged.txt"]);
        fs::write(linked.join(".gitignore"), "ignored.log\n").unwrap();
        fs::write(linked.join("ignored.log"), "ignored").unwrap();

        let entries = git.status_entries(&linked).unwrap();
        let listed: Vec<(char, char, &str)> = entries
            .iter()
            .map(|entry| (entry.index, entry.worktree, entry.path.as_str()))
            .collect();
        assert_eq!(
            listed,
            [
                (' ', 'M', "README.md"),
                ('A', ' ', "staged.txt"),
                ('?', '?', ".gitignore"),
                ('?', '?', "deep/dir/new file.txt"),
            ]
        );
    }

    #[test]
    fn status_entries_fail_on_a_broken_worktree_instead_of_reading_its_parent() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        // The linked worktree lives inside the main repository, so without a ceiling git would
        // fall back to the main repository's status.
        let linked = temp_dir.path().join("linked");
        git.create(&linked, "feature/linked", None).unwrap();
        fs::write(
            linked.join(".git"),
            "gitdir: /nonexistent/worktrees/linked\n",
        )
        .unwrap();
        let err = git.status_entries(&linked).unwrap_err().to_string();
        assert!(err.contains("git status failed"), "{err}");
        // Git versions differ on whether they name the missing gitdir ("(null)" on some).
        assert!(err.contains("not a git repository"), "{err}");

        fs::remove_file(linked.join(".git")).unwrap();
        let err = git.status_entries(&linked).unwrap_err().to_string();
        assert!(err.contains("not a git repository"), "{err}");
    }

    #[test]
    fn worktree_registration_tells_linked_main_and_unrelated_directories_apart() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let linked = temp_dir.path().join("linked");
        git.create(&linked, "feature/linked", None).unwrap();
        assert_eq!(
            git.worktree_registration(&linked).unwrap(),
            WorktreeRegistration::Linked { lock: None }
        );
        assert_eq!(
            git.worktree_registration(temp_dir.path()).unwrap(),
            WorktreeRegistration::Main
        );
        let unrelated = temp_dir.path().join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        assert_eq!(
            git.worktree_registration(&unrelated).unwrap(),
            WorktreeRegistration::NotListed
        );

        run(temp_dir.path(), &["worktree", "lock", "linked"]);
        assert_eq!(
            git.worktree_registration(&linked).unwrap(),
            WorktreeRegistration::Linked {
                lock: Some(WorktreeLock { reason: None })
            }
        );
    }

    #[test]
    fn worktree_lock_reports_the_lock_and_its_reason() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let linked = temp_dir.path().join("linked");
        git.create(&linked, "feature/linked", None).unwrap();
        assert_eq!(git.worktree_lock(&linked).unwrap(), None);

        run(
            temp_dir.path(),
            &["worktree", "lock", "--reason", "on a usb disk", "linked"],
        );
        assert_eq!(
            git.worktree_lock(&linked).unwrap(),
            Some(WorktreeLock {
                reason: Some("on a usb disk".to_string())
            })
        );
        run(temp_dir.path(), &["worktree", "unlock", "linked"]);
        run(temp_dir.path(), &["worktree", "lock", "linked"]);
        assert_eq!(
            git.worktree_lock(&linked).unwrap(),
            Some(WorktreeLock { reason: None })
        );
        assert_eq!(
            git.worktree_lock(&temp_dir.path().join("unknown")).unwrap(),
            None
        );
    }

    #[test]
    fn removal_levels_escalate_from_clean_to_locked() {
        let temp_dir = setup_test_repo();
        let git = GitWorktree::new(temp_dir.path()).unwrap();
        let linked = temp_dir.path().join("linked");
        git.create(&linked, "feature/linked", None).unwrap();
        fs::write(linked.join("untracked.txt"), "work").unwrap();

        let err = git
            .remove_worktree(&linked, RemovalForce::Clean)
            .unwrap_err();
        assert!(err.to_string().contains("untracked"), "{err}");
        assert!(linked.join("untracked.txt").exists());

        run(temp_dir.path(), &["worktree", "lock", "linked"]);
        git.remove_worktree(&linked, RemovalForce::DiscardChanges)
            .unwrap_err();
        assert!(linked.exists(), "--force once keeps a locked worktree");

        git.remove_worktree(&linked, RemovalForce::IncludingLocked)
            .unwrap();
        assert!(!linked.exists());
    }

    #[test]
    fn test_parse_worktree_list_locks() {
        let output = "worktree /r/main\nHEAD abc\nbranch refs/heads/main\n\n\
                      worktree /r/eta\nHEAD def\nbranch refs/heads/feature/eta\nlocked on usb\n\n\
                      worktree /r/zeta\nHEAD 123\nbranch refs/heads/feature/zeta\nlocked\n";
        let worktrees = parse_worktree_list(output).unwrap();
        assert!(!worktrees[0].locked);
        assert!(worktrees[1].locked);
        assert_eq!(worktrees[1].lock_reason.as_deref(), Some("on usb"));
        assert!(worktrees[2].locked);
        assert_eq!(worktrees[2].lock_reason, None);
    }

    #[test]
    fn comparable_paths_resolve_missing_leaves_through_their_parent() {
        let temp_dir = TempDir::new().unwrap();
        let canonical = temp_dir.path().canonicalize().unwrap();
        assert_eq!(comparable_path(temp_dir.path()), canonical);
        assert_eq!(
            comparable_path(&temp_dir.path().join("missing")),
            canonical.join("missing")
        );
        assert_eq!(
            comparable_path(Path::new("/nonexistent/a/b")),
            PathBuf::from("/nonexistent/a/b")
        );
    }
}
