use crate::{
    adapters,
    git::GitWorktree,
    modules::{self, ModuleHandle},
    naming,
    validation::{self, AppUrl},
    Error, Result,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

/// Manages feature lifecycle workflows (start/teardown) using git worktrees.
#[derive(Debug)]
pub struct FeatureWorkflow {
    repo_root: PathBuf,
    git: GitWorktree,
    state: FeatureStateStore,
}

/// Parameters for starting a feature worktree.
#[derive(Debug, Default)]
pub struct StartRequest {
    pub name: Option<String>,
    pub title: Option<String>,
    pub base_branch: Option<String>,
    pub branch_prefix: Option<String>,
    pub reuse_existing: bool,
    pub telemetry: bool,
    /// List of module names to skip during setup (e.g., "tunnel", "database")
    pub skip_modules: Vec<String>,
}

/// Result of running feature start workflow.
#[derive(Debug)]
pub struct StartSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub feature_url: Option<String>,
    pub compose_project_name: Option<String>,
    pub env_path: Option<PathBuf>,
    pub adapter: Option<AdapterSummary>,
    pub module_reports: Vec<ModuleSetupReport>,
    pub warnings: Vec<String>,
    pub color: Option<String>,
}

/// Module setup execution report.
#[derive(Debug)]
pub struct ModuleSetupReport {
    pub name: String,
    pub init_ok: bool,
    pub setup_ok: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Default)]
struct ModuleSetupOutcome {
    reports: Vec<ModuleSetupReport>,
    warnings: Vec<String>,
}

#[derive(Debug, Default)]
struct StashState {
    created: bool,
    reference: Option<String>,
}

/// Parameters for tearing down a feature worktree.
#[derive(Debug)]
pub struct TeardownRequest {
    pub work_feature: String,
    pub branch_prefix: Option<String>,
    pub delete_branch: bool,
    pub force_remove: bool,
    pub complete_spec: bool,
    pub telemetry: bool,
}

/// Result of running feature teardown workflow.
#[derive(Debug)]
pub struct TeardownSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub adapter_cleanup_warnings: Vec<String>,
    pub module_reports: Vec<ModuleTeardownReport>,
    pub warnings: Vec<String>,
}

/// Module teardown execution report.
#[derive(Debug)]
pub struct ModuleTeardownReport {
    pub name: String,
    pub teardown_ok: bool,
    pub errors: Vec<String>,
}

/// Adapter orchestration summary.
#[derive(Debug, Default)]
pub struct AdapterSummary {
    pub name: String,
    pub service_url: String,
    pub warnings: Vec<String>,
}

impl FeatureWorkflow {
    /// Create a new workflow instance from a repository path.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let candidate = path.into();
        let repo_root = resolve_repo_root(&candidate)?;
        validation::validate_git_worktree(&repo_root)?;

        let git = GitWorktree::new(&repo_root)?;
        let state = FeatureStateStore::new(&repo_root);
        Ok(Self {
            repo_root,
            git,
            state,
        })
    }

    /// Get absolute path to repository root.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Start a feature, creating a new git worktree and running module setup.
    pub fn start(&self, request: StartRequest) -> Result<StartSummary> {
        self.ensure_host_environment()?;

        let work_feature = self.resolve_work_feature(&request)?;
        let branch_name = build_branch_name(request.branch_prefix.as_deref(), &work_feature);
        let worktree_path = self.worktree_path(&work_feature)?;
        let mut branch_exists = self.git.branch_exists(&branch_name)?;
        let worktree_exists = worktree_path.exists();
        let base_branch = request.base_branch.clone();
        let mut warnings = Vec::new();

        if request.telemetry {
            std::env::set_var("BRANCHBOX_EMIT_TELEMETRY", "1");
        } else {
            std::env::remove_var("BRANCHBOX_EMIT_TELEMETRY");
        }

        if worktree_exists && !request.reuse_existing {
            return Err(Error::WorktreeExists(worktree_path));
        }

        if request.reuse_existing && !branch_exists {
            for remote in self.git.list_remotes()? {
                if self.git.remote_branch_exists(&remote, &branch_name)? {
                    self.git.create_branch_from_remote(&branch_name, &remote)?;
                    branch_exists = true;
                    tracing::info!(
                        "Created local branch '{}' from remote '{}' to honor reuse request",
                        branch_name,
                        remote
                    );
                    break;
                }
            }
        }

        if !request.reuse_existing && branch_exists {
            return Err(Error::BranchExists(branch_name.clone()));
        }

        if request.reuse_existing && !branch_exists {
            return Err(Error::validation(format!(
                "Cannot reuse feature branch '{}' because it does not exist locally. Fetch or create it before retrying.",
                branch_name
            )));
        }

        if worktree_exists {
            if request.reuse_existing {
                tracing::info!("Using existing worktree at {}", worktree_path.display());
            } else {
                return Err(Error::WorktreeExists(worktree_path));
            }
        } else if request.reuse_existing {
            tracing::info!(
                "Recreating missing worktree for existing branch {}",
                branch_name
            );
            self.git
                .attach_existing_branch(&worktree_path, &branch_name)?;
        } else {
            self.git
                .create(&worktree_path, &branch_name, base_branch.as_deref())?;
        }

        // Fix git worktree paths to use relative paths for devcontainer compatibility
        if let Err(err) = self.fix_git_worktree_path(&worktree_path) {
            tracing::warn!("Failed to fix git worktree path: {}", err);
            warnings.push(format!("Git worktree path fix failed: {}", err));
        }

        let stash_state = if !request.reuse_existing {
            match self.capture_stash(&work_feature) {
                Ok(state) => state,
                Err(err) => {
                    warnings.push(format!("Failed to stash local changes: {}", err));
                    StashState::default()
                }
            }
        } else {
            StashState::default()
        };

        self.ensure_spec_for_start(&work_feature, &worktree_path, &branch_name, &mut warnings);

        let adapter_copy_allowed = !request.reuse_existing;
        let adapter_summary = match self.prepare_adapter(&worktree_path, adapter_copy_allowed) {
            Ok(summary) => Some(summary),
            Err(err) => {
                warnings.push(err);
                None
            }
        };

        let modules::ModulePlan {
            handles,
            warnings: dependency_warnings,
        } = modules::detect_modules(&self.repo_root, &request.skip_modules);
        if !dependency_warnings.is_empty() {
            warnings.extend(dependency_warnings.clone());
        }

        let env_outcome = self.prepare_env(
            &worktree_path,
            &work_feature,
            &branch_name,
            request.reuse_existing,
            &mut warnings,
        )?;
        if env_outcome.skipped {
            warnings.push("Skipped .env provisioning (source file not found)".to_string());
        }

        let previous_service_url = std::env::var("SERVICE_URL").ok();
        if let Some(summary) = adapter_summary.as_ref() {
            std::env::set_var("SERVICE_URL", &summary.service_url);
        }
        let setup_outcome = self.run_module_setup(handles, &worktree_path);
        if !setup_outcome.warnings.is_empty() {
            warnings.extend(setup_outcome.warnings.clone());
        }
        match previous_service_url {
            Some(value) => std::env::set_var("SERVICE_URL", value),
            None => std::env::remove_var("SERVICE_URL"),
        }
        let module_reports = setup_outcome.reports;
        if !request.reuse_existing {
            let stash_warnings = self.apply_stash_to_worktree(&stash_state, &worktree_path);
            if !stash_warnings.is_empty() {
                warnings.extend(stash_warnings);
            }
        }

        let env_path = env_outcome.env_path.clone();
        let feature_url = env_outcome.feature_url.clone();
        let compose_project_name = env_outcome.compose_project_name.clone();

        let color = Some(generate_feature_color(&work_feature));
        let summary_color = color.clone();
        let last_commit = get_last_commit_sha(&self.repo_root, &branch_name);

        if let Err(err) = self.state.record_start(FeatureMetadata {
            work_feature: work_feature.clone(),
            branch_name: branch_name.clone(),
            worktree_path: worktree_path.clone(),
            base_branch,
            feature_url: feature_url.clone(),
            compose_project_name: compose_project_name.clone(),
            env_path: env_path.clone(),
            status: FeatureStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
            color,
            pr_number: None, // Can be populated later via gh CLI integration
            last_commit,
        }) {
            tracing::warn!("Failed to update feature registry: {}", err);
            warnings.push("Failed to update feature registry metadata".to_string());
        }

        // Set up VS Code workspace customization for visual differentiation
        if let Err(err) = self.setup_vscode_workspace(
            &worktree_path,
            &work_feature,
            &summary_color,
            feature_url.as_deref(),
        ) {
            tracing::warn!("Failed to set up VS Code workspace customization: {}", err);
            warnings.push(format!("Failed to set up VS Code workspace: {}", err));
        }

        Ok(StartSummary {
            work_feature,
            branch_name,
            worktree_path,
            feature_url,
            compose_project_name,
            env_path,
            adapter: adapter_summary,
            module_reports,
            warnings,
            color: summary_color,
        })
    }

    /// Tear down a feature worktree and optionally delete its branch.
    pub fn teardown(&self, request: TeardownRequest) -> Result<TeardownSummary> {
        self.ensure_host_environment()?;

        let work_feature = request.work_feature;
        if !naming::validate_work_feature(&work_feature) {
            return Err(Error::InvalidFeatureName(work_feature));
        }

        let branch_name = build_branch_name(request.branch_prefix.as_deref(), &work_feature);
        let worktree_path = self.worktree_path(&work_feature)?;
        if !worktree_path.exists() {
            return Err(Error::WorktreeNotFound(worktree_path.display().to_string()));
        }

        if request.telemetry {
            std::env::set_var("BRANCHBOX_EMIT_TELEMETRY", "1");
        } else {
            std::env::remove_var("BRANCHBOX_EMIT_TELEMETRY");
        }

        let previous_complete = std::env::var("BRANCHBOX_COMPLETE_SPEC").ok();
        if request.complete_spec {
            std::env::set_var("BRANCHBOX_COMPLETE_SPEC", "1");
        } else {
            std::env::remove_var("BRANCHBOX_COMPLETE_SPEC");
        }

        let mut warnings = Vec::new();
        let modules::ModulePlan {
            handles,
            warnings: dependency_warnings,
        } = modules::detect_modules(&self.repo_root, &[]); // No skip during teardown
        let (module_reports, module_warnings) = self.run_module_teardown(handles, &worktree_path);
        if !dependency_warnings.is_empty() {
            warnings.extend(dependency_warnings);
        }
        if !module_warnings.is_empty() {
            warnings.extend(module_warnings.clone());
        }
        self.handle_spec_on_teardown(
            &work_feature,
            &branch_name,
            &worktree_path,
            request.complete_spec,
            &mut warnings,
        );

        let (adapter_cleanup_warnings, adapter_detection_warning) =
            self.cleanup_adapter(&worktree_path);

        match previous_complete {
            Some(value) => std::env::set_var("BRANCHBOX_COMPLETE_SPEC", value),
            None => std::env::remove_var("BRANCHBOX_COMPLETE_SPEC"),
        }
        if let Some(message) = adapter_detection_warning {
            warnings.push(message);
        }

        let mut worktree_removed = false;
        match self.git.remove(&worktree_path, request.force_remove) {
            Ok(_) => {
                worktree_removed = true;
            }
            Err(err) => {
                warnings.push(format!("Failed to remove worktree: {}", err));
                if worktree_path.exists() {
                    match fs::remove_dir_all(&worktree_path) {
                        Ok(_) => {
                            worktree_removed = true;
                            warnings.push(
                                "Worktree directory removed manually after git removal failed"
                                    .to_string(),
                            );
                        }
                        Err(fs_err) => {
                            warnings.push(format!(
                                "Failed to remove worktree directory manually: {}",
                                fs_err
                            ));
                        }
                    }
                }
            }
        }

        let mut branch_deleted = false;
        if request.delete_branch {
            match self.git.delete_branch(&branch_name, request.force_remove) {
                Ok(_) => {
                    branch_deleted = true;
                }
                Err(err) => {
                    warnings.push(format!(
                        "Failed to delete branch '{}': {}",
                        branch_name, err
                    ));
                }
            }
        }

        if let Err(err) = self.state.record_teardown(&work_feature) {
            tracing::warn!("Failed to update feature registry: {}", err);
            warnings.push("Failed to update feature registry metadata".to_string());
        }

        Ok(TeardownSummary {
            work_feature,
            branch_name,
            worktree_removed,
            branch_deleted,
            adapter_cleanup_warnings,
            module_reports,
            warnings,
        })
    }

    /// List feature metadata from the registry, sorted by most recently updated.
    pub fn list_features(&self) -> Result<Vec<FeatureMetadata>> {
        let mut entries = self.state.list_features()?;
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(entries)
    }

    fn resolve_work_feature(&self, request: &StartRequest) -> Result<String> {
        if let Some(name) = request.name.as_ref() {
            if naming::validate_work_feature(name) {
                return Ok(name.clone());
            } else {
                return Err(Error::InvalidFeatureName(name.clone()));
            }
        }

        if let Some(title) = request.title.as_ref() {
            let generated = naming::generate_work_feature(title);
            if generated.is_empty() {
                return Err(Error::validation(
                    "Generated feature name is empty".to_string(),
                ));
            }
            return Ok(generated);
        }

        Err(Error::validation(
            "Feature name or title is required for feature start".to_string(),
        ))
    }

    fn worktree_path(&self, work_feature: &str) -> Result<PathBuf> {
        let parent = self.repo_root.parent().ok_or_else(|| {
            Error::validation("Repository root has no parent directory".to_string())
        })?;
        Ok(parent.join(work_feature))
    }

    fn resolve_app_slug(&self, env_path: &Path) -> Result<String> {
        if let Ok(vars) = validation::parse_env_file(env_path) {
            if let Some(raw) = vars
                .get("APP_NAME")
                .or_else(|| vars.get("APP_SLUG"))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                let slug = naming::generate_work_feature(raw);
                if !slug.is_empty() {
                    return Ok(slug);
                }
            }
        }

        let parent = self.repo_root.parent().ok_or_else(|| {
            Error::validation("Repository root has no parent directory".to_string())
        })?;

        let app_name = parent
            .file_name()
            .ok_or_else(|| Error::validation("Repository parent has no name".to_string()))?
            .to_string_lossy()
            .to_string();

        let slug = naming::generate_work_feature(&app_name);
        if slug.is_empty() {
            return Err(Error::validation(
                "Unable to derive application name for compose project".to_string(),
            ));
        }

        Ok(slug)
    }

    fn prepare_env(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        branch_name: &str,
        reuse_existing: bool,
        warnings: &mut Vec<String>,
    ) -> Result<EnvOutcome> {
        let source_env = self.repo_root.join(".env");
        if !source_env.exists() {
            tracing::warn!("No .env found at {}", source_env.display());
            return Ok(EnvOutcome::skipped());
        }

        let dest_env = worktree_path.join(".env");
        let (base_env, _previous_section) = split_feature_section(&source_env)?;

        if reuse_existing && dest_env.exists() {
            if let Ok((_, existing_section)) = split_feature_section(&dest_env) {
                if let Some(section) = existing_section {
                    let trimmed = section.trim();
                    if !trimmed.is_empty() {
                        warnings.push(format!(
                            "Existing feature-specific configuration replaced in {}",
                            dest_env.display()
                        ));
                    }
                } else {
                    warnings.push(format!(
                        "Existing {} will be refreshed from main .env (no branchbox block found)",
                        dest_env.display()
                    ));
                }
            }
        }

        fs::write(&dest_env, base_env)?;

        let mut feature_url = None;
        let mut compose_project_name = None;

        let app_slug = self.resolve_app_slug(&source_env)?;
        std::env::set_var("BASE_PREFIX", &app_slug);

        match AppUrl::from_env_file(&source_env) {
            Ok(app_url) => {
                let url = naming::generate_feature_url(&app_url.url, work_feature);
                let compose_name = format!("{}-{}", app_slug, work_feature);
                std::env::set_var("COMPOSE_PROJECT_NAME", &compose_name);
                std::env::set_var("DEVCONTAINER_NAME", &compose_name);
                let mut file = OpenOptions::new().append(true).open(&dest_env)?;
                ensure_trailing_newline(&mut file)?;
                writeln!(
                    file,
                    "# Feature-specific configuration (managed by branchbox)"
                )?;
                writeln!(file, "WORK_FEATURE={}", work_feature)?;
                writeln!(file, "APP_URL={}", url)?;
                writeln!(file, "COMPOSE_PROJECT_NAME={}", compose_name)?;
                writeln!(file, "DEVCONTAINER_NAME={}", compose_name)?;
                writeln!(file, "GIT_BRANCH={}", branch_name)?;

                feature_url = Some(url);
                compose_project_name = Some(compose_name);
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to parse APP_URL from {}: {}",
                    source_env.display(),
                    err
                );
            }
        }

        self.link_env_into_devcontainer(worktree_path)?;

        Ok(EnvOutcome {
            env_path: Some(dest_env),
            feature_url,
            compose_project_name,
            skipped: false,
        })
    }

    fn prepare_adapter(
        &self,
        worktree_path: &Path,
        copy_secrets: bool,
    ) -> std::result::Result<AdapterSummary, String> {
        match adapters::detect_adapter(&self.repo_root) {
            Ok(adapter) => {
                let name = adapter.name().to_string();
                tracing::info!("Detected adapter: {}", name);
                let service_url = adapter.service_url();
                let mut summary = AdapterSummary {
                    name,
                    service_url,
                    warnings: Vec::new(),
                };

                if copy_secrets {
                    if let Err(err) = adapter.copy_secrets(&self.repo_root, worktree_path) {
                        summary
                            .warnings
                            .push(format!("Failed to copy secrets: {}", err));
                    }
                }

                Ok(summary)
            }
            Err(err) => Err(format!("Failed to detect adapter: {}", err)),
        }
    }

    fn cleanup_adapter(&self, worktree_path: &Path) -> (Vec<String>, Option<String>) {
        if !worktree_path.exists() {
            return (Vec::new(), None);
        }

        match adapters::detect_adapter(worktree_path) {
            Ok(adapter) => match adapter.cleanup(worktree_path) {
                Ok(_) => (Vec::new(), None),
                Err(err) => (vec![format!("Adapter cleanup failed: {}", err)], None),
            },
            Err(err) => (
                Vec::new(),
                Some(format!("Failed to detect adapter for cleanup: {}", err)),
            ),
        }
    }

    fn capture_stash(&self, work_feature: &str) -> Result<StashState> {
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to check git status: {}", err)))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            return Err(Error::git(format!(
                "git status exited with error: {}",
                stderr.trim()
            )));
        }

        let changes = String::from_utf8_lossy(&status.stdout);
        if changes.trim().is_empty() {
            return Ok(StashState::default());
        }

        let message = format!("feature-start: changes for {}", work_feature);
        let push = Command::new("git")
            .args(["stash", "push", "-m", &message])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to invoke git stash: {}", err)))?;

        if !push.status.success() {
            let stderr = String::from_utf8_lossy(&push.stderr);
            return Err(Error::git(format!(
                "git stash push failed: {}",
                stderr.trim()
            )));
        }

        let list = Command::new("git")
            .args(["stash", "list", "-n", "1", "--format=%gd"])
            .current_dir(&self.repo_root)
            .output()
            .map_err(|err| Error::git(format!("Failed to list stashes: {}", err)))?;

        if !list.status.success() {
            let stderr = String::from_utf8_lossy(&list.stderr);
            return Err(Error::git(format!(
                "git stash list failed: {}",
                stderr.trim()
            )));
        }

        let reference = String::from_utf8_lossy(&list.stdout)
            .lines()
            .next()
            .map(|line| line.trim().to_string());

        Ok(StashState {
            created: true,
            reference,
        })
    }

    fn apply_stash_to_worktree(&self, stash: &StashState, worktree_path: &Path) -> Vec<String> {
        if !stash.created {
            return Vec::new();
        }

        match Command::new("git")
            .args(["stash", "pop"])
            .current_dir(worktree_path)
            .output()
        {
            Ok(output) if output.status.success() => Vec::new(),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let mut warning = format!(
                    "Failed to apply stashed changes to feature worktree: {}",
                    stderr.trim()
                );
                if let Some(reference) = stash.reference.as_deref() {
                    warning.push_str(&format!(" Stash {} remains available.", reference));
                } else {
                    warning.push_str(" Stash remains available.");
                }
                vec![warning]
            }
            Err(err) => {
                let mut warning = format!(
                    "Failed to apply stashed changes to feature worktree: {}",
                    err
                );
                if let Some(reference) = stash.reference.as_deref() {
                    warning.push_str(&format!(" Stash {} remains available.", reference));
                } else {
                    warning.push_str(" Stash remains available.");
                }
                vec![warning]
            }
        }
    }

    fn link_env_into_devcontainer(&self, worktree_path: &Path) -> Result<()> {
        let dev_dir = worktree_path.join(".devcontainer");
        if !dev_dir.exists() {
            fs::create_dir_all(&dev_dir)?;
        }

        let link_path = dev_dir.join(".env");
        if link_path.exists() {
            fs::remove_file(&link_path)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            if let Err(err) = symlink("../.env", &link_path) {
                tracing::warn!("Failed to create symlink for devcontainer .env: {}", err);
                fs::copy(worktree_path.join(".env"), &link_path)?;
            }
        }

        #[cfg(windows)]
        {
            if let Err(err) = std::os::windows::fs::symlink_file("..\\.env", &link_path) {
                tracing::warn!("Failed to create symlink for devcontainer .env: {}", err);
                fs::copy(worktree_path.join(".env"), &link_path)?;
            }
        }

        Ok(())
    }

    fn run_module_setup(
        &self,
        handles: Vec<ModuleHandle>,
        feature_dir: &Path,
    ) -> ModuleSetupOutcome {
        let mut reports = Vec::with_capacity(handles.len());
        let mut warnings = Vec::new();
        let mut successful: Vec<ModuleHandle> = Vec::new();
        let mut aborted = false;

        for mut handle in handles.into_iter() {
            let mut report = ModuleSetupReport {
                name: handle.name.clone(),
                init_ok: false,
                setup_ok: false,
                errors: Vec::new(),
            };

            match handle.module.init(&self.repo_root, feature_dir) {
                Ok(_) => {
                    report.init_ok = true;
                    match handle.module.setup(&self.repo_root, feature_dir) {
                        Ok(_) => {
                            report.setup_ok = true;
                            successful.push(handle);
                        }
                        Err(err) => {
                            report.errors.push(err.to_string());
                            // Attempt best-effort cleanup for the module that failed during setup.
                            if let Err(teardown_err) =
                                handle.module.teardown(&self.repo_root, feature_dir)
                            {
                                warnings.push(format!(
                                    "Rollback for module '{}' failed: {}",
                                    report.name, teardown_err
                                ));
                            }
                            aborted = true;
                            reports.push(report);
                            break;
                        }
                    }
                }
                Err(err) => {
                    report.errors.push(err.to_string());
                    aborted = true;
                    reports.push(report);
                    break;
                }
            }

            reports.push(report);
        }

        if aborted {
            while let Some(handle) = successful.pop() {
                if let Err(err) = handle.module.teardown(&self.repo_root, feature_dir) {
                    warnings.push(format!(
                        "Rollback for module '{}' failed: {}",
                        handle.name, err
                    ));
                }
            }
            if !warnings.iter().any(|w| w.contains("Module setup aborted")) {
                warnings.push("Module setup aborted due to earlier failures".to_string());
            }
        }

        ModuleSetupOutcome { reports, warnings }
    }

    fn run_module_teardown(
        &self,
        handles: Vec<ModuleHandle>,
        feature_dir: &Path,
    ) -> (Vec<ModuleTeardownReport>, Vec<String>) {
        let mut reports = Vec::with_capacity(handles.len());
        let mut warnings = Vec::new();

        for mut handle in handles.into_iter().rev() {
            let mut report = ModuleTeardownReport {
                name: handle.name.clone(),
                teardown_ok: false,
                errors: Vec::new(),
            };

            match handle.module.init(&self.repo_root, feature_dir) {
                Ok(_) => match handle.module.teardown(&self.repo_root, feature_dir) {
                    Ok(_) => {
                        report.teardown_ok = true;
                    }
                    Err(err) => {
                        let message = err.to_string();
                        report.errors.push(message.clone());
                        warnings.push(format!(
                            "Module '{}' teardown reported an error: {}",
                            report.name, message
                        ));
                    }
                },
                Err(err) => {
                    let message = err.to_string();
                    report.errors.push(message.clone());
                    warnings.push(format!(
                        "Module '{}' initialization failed during teardown: {}",
                        report.name, message
                    ));
                }
            }

            reports.push(report);
        }

        reports.reverse();
        (reports, warnings)
    }

    fn determine_feature_spec(&self, work_feature: &str) -> Result<Option<PathBuf>> {
        let features_dir = self.repo_root.join("docs/features");
        if !features_dir.exists() {
            return Ok(None);
        }

        let spec_name = format!("{}.md", work_feature);
        for group in ["in-progress", "backlog", "completed"] {
            let candidate = features_dir.join(group).join(&spec_name);
            if candidate.exists() {
                return Ok(Some(candidate));
            }
        }

        Ok(None)
    }

    fn ensure_spec_for_start(
        &self,
        work_feature: &str,
        worktree_path: &Path,
        branch_name: &str,
        warnings: &mut Vec<String>,
    ) -> Option<PathBuf> {
        let features_dir = self.repo_root.join("docs/features");
        if let Err(err) = fs::create_dir_all(features_dir.join("in-progress")) {
            warnings.push(format!("Failed to ensure specs directory exists: {}", err));
            return None;
        }

        let spec_name = format!("{}.md", work_feature);
        let in_progress = features_dir.join("in-progress").join(&spec_name);

        match self.determine_feature_spec(work_feature) {
            Ok(Some(existing)) if existing != in_progress => {
                if let Err(err) = fs::rename(&existing, &in_progress) {
                    warnings.push(format!(
                        "Failed to move spec '{}' to in-progress: {}",
                        existing.display(),
                        err
                    ));
                    return Some(existing);
                }
            }
            Ok(None) => {
                let content = format!(
                    "---\nworktree: {}\nbranch: {}\nwork_feature: {}\nstatus: in-progress\ncreated: {}\n---\n\n# {}\n\n## Overview\n\nTODO: Describe the feature scope.\n",
                    worktree_path.display(),
                    branch_name,
                    work_feature,
                    Utc::now().date_naive(),
                    feature_title_from_work_feature(work_feature)
                );

                if let Err(err) = fs::write(&in_progress, content) {
                    warnings.push(format!(
                        "Failed to create feature spec '{}': {}",
                        in_progress.display(),
                        err
                    ));
                    return None;
                }
            }
            Ok(_) => {}
            Err(err) => warnings.push(format!(
                "Failed to inspect existing feature spec for '{}': {}",
                work_feature, err
            )),
        }

        let updates = vec![
            ("worktree".to_string(), worktree_path.display().to_string()),
            ("branch".to_string(), branch_name.to_string()),
            ("work_feature".to_string(), work_feature.to_string()),
            ("status".to_string(), "in-progress".to_string()),
        ];

        if let Err(err) = update_spec_frontmatter(&in_progress, &updates) {
            warnings.push(format!(
                "Failed to update feature spec frontmatter '{}': {}",
                in_progress.display(),
                err
            ));
        }

        Some(in_progress)
    }

    fn handle_spec_on_teardown(
        &self,
        work_feature: &str,
        branch_name: &str,
        worktree_path: &Path,
        mark_complete: bool,
        warnings: &mut Vec<String>,
    ) {
        let Ok(existing) = self.determine_feature_spec(work_feature) else {
            if mark_complete {
                warnings.push(format!(
                    "Unable to locate feature spec '{}' during teardown",
                    work_feature
                ));
            }
            return;
        };

        let Some(mut spec_path) = existing else {
            if mark_complete {
                warnings.push(format!(
                    "Feature spec '{}' not found in docs/features",
                    work_feature
                ));
            }
            return;
        };

        if mark_complete {
            let features_dir = self.repo_root.join("docs/features");
            let completed_dir = features_dir.join("completed");
            if let Err(err) = fs::create_dir_all(&completed_dir) {
                warnings.push(format!(
                    "Failed to prepare completed specs directory: {}",
                    err
                ));
                return;
            }

            let target = completed_dir.join(format!("{}.md", work_feature));
            if spec_path != target {
                if let Err(err) = fs::rename(&spec_path, &target) {
                    warnings.push(format!("Failed to move feature spec to completed: {}", err));
                    return;
                }
                spec_path = target;
            }

            let mut updates = vec![
                ("status".to_string(), "completed".to_string()),
                ("branch".to_string(), branch_name.to_string()),
            ];
            updates.push(("completed".to_string(), Utc::now().date_naive().to_string()));

            if let Err(err) = update_spec_frontmatter(&spec_path, &updates) {
                warnings.push(format!(
                    "Failed to update completed spec frontmatter '{}': {}",
                    spec_path.display(),
                    err
                ));
            }
        } else if let Err(err) = update_spec_frontmatter(
            &spec_path,
            &[
                ("worktree".to_string(), worktree_path.display().to_string()),
                ("branch".to_string(), branch_name.to_string()),
                ("status".to_string(), "in-progress".to_string()),
            ],
        ) {
            warnings.push(format!(
                "Failed to refresh spec frontmatter '{}': {}",
                spec_path.display(),
                err
            ));
        }
    }

    fn ensure_host_environment(&self) -> Result<()> {
        if std::env::var("BRANCHBOX_SKIP_HOST_VALIDATION").is_ok() {
            tracing::debug!(
                "Skipping host environment validation via BRANCHBOX_SKIP_HOST_VALIDATION"
            );
            return Ok(());
        }

        validation::validate_host_environment()
    }

    /// Set up VS Code workspace customization for visual differentiation.
    fn setup_vscode_workspace(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        color: &Option<String>,
        feature_url: Option<&str>,
    ) -> Result<()> {
        let vscode_dir = worktree_path.join(".vscode");
        fs::create_dir_all(&vscode_dir)?;

        // Set up Peacock extension color and window title
        let settings_path = vscode_dir.join("settings.json");
        let mut settings = if settings_path.exists() {
            let content = fs::read_to_string(&settings_path)?;
            serde_json::from_str(&content).unwrap_or_else(|e| {
                tracing::debug!(
                    "Failed to parse existing settings.json at {}: {}. Using empty object.",
                    settings_path.display(),
                    e
                );
                serde_json::json!({})
            })
        } else {
            serde_json::json!({})
        };

        if let Some(color_value) = color {
            settings["peacock.color"] = serde_json::json!(color_value);
        }

        // Customize window title to show feature name
        settings["window.title"] = serde_json::json!(format!(
            "${{rootName}} [{}] - ${{activeEditorShort}}",
            work_feature
        ));

        let settings_json = serde_json::to_string_pretty(&settings)
            .map_err(|e| Error::config(format!("Failed to serialize VS Code settings: {}", e)))?;
        fs::write(&settings_path, settings_json)?;

        // Set up quick access task for feature URL
        if let Some(url) = feature_url {
            let tasks_path = vscode_dir.join("tasks.json");
            let tasks = serde_json::json!({
                "version": "2.0.0",
                "tasks": [
                    {
                        "label": "Open Feature URL",
                        "type": "shell",
                        "command": format!("echo 'Opening https://{}' && xdg-open 'https://{}' || open 'https://{}'", url, url, url),
                        "problemMatcher": [],
                        "presentation": {
                            "reveal": "silent",
                            "panel": "shared"
                        }
                    }
                ]
            });

            let tasks_json = serde_json::to_string_pretty(&tasks)
                .map_err(|e| Error::config(format!("Failed to serialize VS Code tasks: {}", e)))?;
            fs::write(&tasks_path, tasks_json)?;
        }

        Ok(())
    }

    /// Fix git worktree path to use relative paths for devcontainer compatibility.
    ///
    /// When a worktree is created, git stores an absolute path to the main repo's
    /// .git/worktrees/ directory. This breaks in devcontainers where paths are mounted
    /// differently. We convert absolute paths to relative paths.
    fn fix_git_worktree_path(&self, worktree_path: &Path) -> Result<()> {
        let git_file = worktree_path.join(".git");

        if !git_file.exists() {
            return Err(Error::validation(format!(
                "No .git file found in worktree at {}",
                worktree_path.display()
            )));
        }

        // Read the .git file
        let content = fs::read_to_string(&git_file)?;

        // Parse the gitdir line
        let gitdir_line = content
            .lines()
            .find(|line| line.starts_with("gitdir:"))
            .ok_or_else(|| Error::validation("No gitdir: line found in .git file".to_string()))?;

        let current_path = gitdir_line.strip_prefix("gitdir:").unwrap_or("").trim();

        // If already relative, nothing to do
        if !current_path.starts_with('/') {
            tracing::debug!("Git worktree path is already relative: {}", current_path);
            return Ok(());
        }

        // Extract the main repo name from the current path
        // Path format: /path/to/main/.git/worktrees/feature-name
        let worktree_name = worktree_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Cannot determine worktree name".to_string()))?;

        // Try to find the main repo directory
        let parent_dir = worktree_path
            .parent()
            .ok_or_else(|| Error::validation("Worktree has no parent directory".to_string()))?;

        // Determine the main repo name from the absolute path
        // Path format: /path/to/REPO_NAME/.git/worktrees/WORKTREE_NAME
        let main_name = if parent_dir.join("main").exists() {
            "main".to_string()
        } else if let Some(git_idx) = current_path.find("/.git/worktrees/") {
            // Extract REPO_NAME from the path
            let before_git = &current_path[..git_idx];
            if let Some(last_slash) = before_git.rfind('/') {
                before_git[last_slash + 1..].to_string()
            } else {
                "main".to_string()
            }
        } else {
            "main".to_string()
        };

        // Construct relative path using PathBuf for safer path manipulation
        // Path: ../MAIN_NAME/.git/worktrees/WORKTREE_NAME
        let mut relative_path = PathBuf::from("..");
        relative_path.push(&main_name);
        relative_path.push(".git");
        relative_path.push("worktrees");
        relative_path.push(worktree_name);

        // Convert to string for gitdir format (git expects forward slashes)
        let relative_path_str = relative_path
            .to_str()
            .ok_or_else(|| Error::validation("Invalid UTF-8 in worktree path".to_string()))?
            .replace('\\', "/"); // Ensure forward slashes on Windows

        // Write the fixed path
        let new_content = format!("gitdir: {}\n", relative_path_str);
        fs::write(&git_file, new_content)?;

        tracing::info!(
            "Fixed git worktree path from absolute to relative: {}",
            relative_path_str
        );

        Ok(())
    }
}

/// Helper to ensure the file ends with a newline before appending.
fn ensure_trailing_newline(file: &mut File) -> io::Result<()> {
    file.write_all(b"\n")
}

fn split_feature_section(path: &Path) -> Result<(String, Option<String>)> {
    let content = fs::read_to_string(path)?;
    if let Some(pos) = content.find("# Feature-specific configuration") {
        let base = content[..pos].trim_end().to_string();
        let mut base_with_newline = base;
        if !base_with_newline.ends_with('\n') {
            base_with_newline.push('\n');
        }
        Ok((base_with_newline, Some(content[pos..].to_string())))
    } else {
        let mut base = content.trim_end().to_string();
        if !base.is_empty() && !base.ends_with('\n') {
            base.push('\n');
        }
        Ok((base, None))
    }
}

fn feature_title_from_work_feature(work_feature: &str) -> String {
    let title = work_feature
        .split('-')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    if title.is_empty() {
        work_feature.to_string()
    } else {
        title
    }
}

fn update_spec_frontmatter(path: &Path, updates: &[(String, String)]) -> Result<()> {
    let raw = fs::read_to_string(path)?;
    let trimmed = raw.trim_start();

    let (frontmatter, body_start) = if let Some(rest) = trimmed.strip_prefix("---") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some((front, remainder)) = rest.split_once("\n---") {
            let body = remainder.strip_prefix('\n').unwrap_or(remainder);
            (Some(front), Some(body))
        } else {
            (Some(rest), None)
        }
    } else {
        (None, Some(trimmed))
    };

    let mut entries = BTreeMap::new();
    if let Some(front) = frontmatter {
        for line in front.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                entries.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }

    for (key, value) in updates {
        entries.insert(key.clone(), value.clone());
    }

    let mut frontmatter_text = String::from("---\n");
    for (key, value) in &entries {
        frontmatter_text.push_str(key);
        frontmatter_text.push(':');
        frontmatter_text.push(' ');
        frontmatter_text.push_str(value);
        frontmatter_text.push('\n');
    }
    frontmatter_text.push_str("---\n");

    let mut body_text = String::new();
    if let Some(body) = body_start {
        body_text.push_str(body.trim_start_matches('\n'));
    }

    fs::write(path, format!("{}{}", frontmatter_text, body_text))?;
    Ok(())
}

fn build_branch_name(prefix: Option<&str>, work_feature: &str) -> String {
    let prefix = prefix.unwrap_or("feature").trim_end_matches('/');
    if prefix.is_empty() {
        work_feature.to_string()
    } else {
        format!("{}/{}", prefix, work_feature)
    }
}

fn resolve_repo_root(path: &Path) -> Result<PathBuf> {
    // First attempt to ask git where the repository lives to cover nested directories.
    let cwd = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    if let Ok(output) = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(cwd)
        .output()
    {
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
    }

    // Fallback: walk up the directory tree and look for a .git entry.
    let mut current = if path.exists() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };

    if current.is_file() {
        current = current
            .parent()
            .ok_or_else(|| Error::validation("Not a git repository".to_string()))?
            .to_path_buf();
    }

    let mut cursor = Some(current);
    while let Some(dir) = cursor {
        let git_path = dir.join(".git");
        if git_path.is_dir() {
            return Ok(dir);
        }

        if git_path.is_file() {
            let git_file = fs::read_to_string(&git_path)?;
            let gitdir = git_file
                .strip_prefix("gitdir:")
                .map(|s| s.trim())
                .ok_or_else(|| Error::validation("Invalid .git file format".to_string()))?;

            let gitdir_path = if Path::new(gitdir).is_absolute() {
                PathBuf::from(gitdir)
            } else {
                dir.join(gitdir)
            };
            let canonical_gitdir = gitdir_path.canonicalize()?;
            let repo_root = canonical_gitdir
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .ok_or_else(|| {
                    Error::validation("Unable to resolve repository root".to_string())
                })?;
            return Ok(repo_root.to_path_buf());
        }

        cursor = dir.parent().map(|p| p.to_path_buf());
    }

    Err(Error::validation(format!(
        "Not a git repository: {}",
        path.display()
    )))
}

#[derive(Clone)]
struct EnvOutcome {
    env_path: Option<PathBuf>,
    feature_url: Option<String>,
    compose_project_name: Option<String>,
    skipped: bool,
}

impl EnvOutcome {
    fn skipped() -> Self {
        Self {
            env_path: None,
            feature_url: None,
            compose_project_name: None,
            skipped: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    Active,
    Removed,
}

impl fmt::Display for FeatureStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureStatus::Active => write!(f, "active"),
            FeatureStatus::Removed => write!(f, "removed"),
        }
    }
}

impl FromStr for FeatureStatus {
    type Err = ParseFeatureStatusError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "active" => Ok(FeatureStatus::Active),
            "removed" => Ok(FeatureStatus::Removed),
            _ => Err(ParseFeatureStatusError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParseFeatureStatusError(String);

impl fmt::Display for ParseFeatureStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid feature status '{}'; expected 'active' or 'removed'",
            self.0
        )
    }
}

impl std::error::Error for ParseFeatureStatusError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureMetadata {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub base_branch: Option<String>,
    pub feature_url: Option<String>,
    pub compose_project_name: Option<String>,
    pub env_path: Option<PathBuf>,
    pub status: FeatureStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub removed_at: Option<DateTime<Utc>>,
    /// Workspace color for visual differentiation.
    ///
    /// Hex color code (e.g., "#3498db") generated deterministically from the feature name.
    /// Used by Peacock extension and VS Code to visually distinguish workspaces.
    /// Limited to 12 colors from a predefined palette, so features may share colors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// GitHub pull request number if one exists for this feature.
    ///
    /// Can be populated via `gh` CLI integration when a PR is created for this branch.
    /// Useful for tracking feature status and linking worktrees to PRs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u32>,
    /// Last commit SHA on this branch.
    ///
    /// Captured at worktree creation time using `git rev-parse <branch>`.
    /// Useful for tracking branch state and detecting stale worktrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FeatureRegistry {
    version: u32,
    features: Vec<FeatureMetadata>,
}

impl Default for FeatureRegistry {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            features: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct FeatureStateStore {
    path: PathBuf,
}

impl FeatureStateStore {
    fn new(repo_root: &Path) -> Self {
        let path = repo_root.join(".branchbox").join("feature.json");
        Self { path }
    }

    fn record_start(&self, mut metadata: FeatureMetadata) -> Result<()> {
        let mut registry = self.load_registry()?;
        let now = metadata.updated_at;

        if let Some(existing) = registry
            .features
            .iter_mut()
            .find(|item| item.work_feature == metadata.work_feature)
        {
            metadata.created_at = existing.created_at;
            *existing = FeatureMetadata {
                created_at: existing.created_at,
                updated_at: now,
                status: FeatureStatus::Active,
                removed_at: None,
                ..metadata
            };
        } else {
            registry.features.push(metadata);
        }

        self.save_registry(&registry)
    }

    fn record_teardown(&self, work_feature: &str) -> Result<()> {
        let mut registry = self.load_registry()?;
        if let Some(existing) = registry
            .features
            .iter_mut()
            .find(|item| item.work_feature == work_feature)
        {
            let now = Utc::now();
            existing.status = FeatureStatus::Removed;
            existing.updated_at = now;
            existing.removed_at = Some(now);
        } else {
            tracing::debug!(
                "Feature '{}' not present in registry during teardown",
                work_feature
            );
        }

        self.save_registry(&registry)
    }

    fn list_features(&self) -> Result<Vec<FeatureMetadata>> {
        let registry = self.load_registry()?;
        Ok(registry.features)
    }

    fn load_registry(&self) -> Result<FeatureRegistry> {
        if !self.path.exists() {
            return Ok(FeatureRegistry::default());
        }

        let data = fs::read_to_string(&self.path)?;
        if data.trim().is_empty() {
            return Ok(FeatureRegistry::default());
        }

        serde_json::from_str(&data)
            .map_err(|err| Error::config(format!("Failed to parse feature registry: {}", err)))
    }

    fn save_registry(&self, registry: &FeatureRegistry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let serialized = serde_json::to_string_pretty(registry).map_err(|err| {
            Error::config(format!("Failed to serialize feature registry: {}", err))
        })?;

        fs::write(&self.path, serialized)?;
        Ok(())
    }
}

const STATE_VERSION: u32 = 1;

/// Generate a deterministic color from a feature name for visual differentiation.
///
/// Uses a hash of the feature name to select from a predefined palette of 12 distinguishable
/// colors. The same feature name will always generate the same color, making it easy to
/// identify workspaces visually in VS Code with the Peacock extension.
///
/// # Color Collisions
///
/// With only 12 colors available, projects with more than 12 concurrent features will
/// experience color collisions (multiple features sharing the same color). This is acceptable
/// for most workflows as it's unlikely to have >12 feature branches active simultaneously.
///
/// # Returns
///
/// A hex color code string in the format `#RRGGBB` (e.g., "#3498db").
///
/// # Examples
///
/// ```ignore
/// // Internal function - not exposed in public API
/// let color = generate_feature_color("oauth-integration");
/// assert_eq!(color.len(), 7); // #RRGGBB format
/// assert!(color.starts_with('#'));
///
/// // Same name always produces same color
/// let color2 = generate_feature_color("oauth-integration");
/// assert_eq!(color, color2);
/// ```
fn generate_feature_color(work_feature: &str) -> String {
    // Predefined palette of 12 distinguishable colors
    // Selected for good visual contrast and accessibility
    let palette = [
        "#3498db", // blue
        "#e74c3c", // red
        "#2ecc71", // green
        "#f39c12", // orange
        "#9b59b6", // purple
        "#1abc9c", // turquoise
        "#e67e22", // dark orange
        "#16a085", // dark turquoise
        "#27ae60", // dark green
        "#2980b9", // dark blue
        "#8e44ad", // dark purple
        "#c0392b", // dark red
    ];

    let mut hasher = DefaultHasher::new();
    work_feature.hash(&mut hasher);
    let hash = hasher.finish();
    let index = (hash % palette.len() as u64) as usize;

    palette[index].to_string()
}

/// Get the last commit SHA for a branch.
fn get_last_commit_sha(repo_root: &Path, branch: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", branch])
        .output()
        .ok()?;

    if output.status.success() {
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if sha.is_empty() {
            None
        } else {
            Some(sha)
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_build_branch_name_with_default_prefix() {
        assert_eq!(build_branch_name(None, "oauth"), "feature/oauth");
    }

    #[test]
    fn test_build_branch_name_with_custom_prefix() {
        assert_eq!(
            build_branch_name(Some("bugfix"), "issue-123"),
            "bugfix/issue-123"
        );
    }

    #[test]
    fn test_build_branch_name_with_trailing_slash() {
        assert_eq!(
            build_branch_name(Some("feature/"), "test"),
            "feature/test"
        );
    }

    #[test]
    fn test_build_branch_name_with_empty_prefix() {
        assert_eq!(build_branch_name(Some(""), "test"), "test");
    }

    #[test]
    fn test_feature_title_from_work_feature() {
        assert_eq!(
            feature_title_from_work_feature("oauth-integration"),
            "Oauth Integration"
        );
        assert_eq!(
            feature_title_from_work_feature("fix-bug-123"),
            "Fix Bug 123"
        );
        assert_eq!(feature_title_from_work_feature("a"), "A");
        assert_eq!(feature_title_from_work_feature(""), "");
    }

    #[test]
    fn test_feature_status_display() {
        assert_eq!(FeatureStatus::Active.to_string(), "active");
        assert_eq!(FeatureStatus::Removed.to_string(), "removed");
    }

    #[test]
    fn test_feature_status_from_str() {
        assert_eq!(
            FeatureStatus::from_str("active").unwrap(),
            FeatureStatus::Active
        );
        assert_eq!(
            FeatureStatus::from_str("removed").unwrap(),
            FeatureStatus::Removed
        );
        assert_eq!(
            FeatureStatus::from_str("ACTIVE").unwrap(),
            FeatureStatus::Active
        );
        assert_eq!(
            FeatureStatus::from_str(" removed ").unwrap(),
            FeatureStatus::Removed
        );
    }

    #[test]
    fn test_feature_status_from_str_invalid() {
        let result = FeatureStatus::from_str("invalid");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid"));
        assert!(err.to_string().contains("active"));
        assert!(err.to_string().contains("removed"));
    }

    #[test]
    fn test_update_spec_frontmatter_creates_new() {
        let temp = TempDir::new().unwrap();
        let spec_path = temp.path().join("spec.md");
        fs::write(&spec_path, "# Feature\n\nSome content\n").unwrap();

        let updates = vec![
            ("status".to_string(), "in-progress".to_string()),
            ("branch".to_string(), "feature/test".to_string()),
        ];
        update_spec_frontmatter(&spec_path, &updates).unwrap();

        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("branch: feature/test"));
        assert!(content.contains("status: in-progress"));
        assert!(content.contains("# Feature"));
    }

    #[test]
    fn test_update_spec_frontmatter_updates_existing() {
        let temp = TempDir::new().unwrap();
        let spec_path = temp.path().join("spec.md");
        fs::write(
            &spec_path,
            "---\nstatus: backlog\nold_key: old_value\n---\n# Feature\n",
        )
        .unwrap();

        let updates = vec![("status".to_string(), "in-progress".to_string())];
        update_spec_frontmatter(&spec_path, &updates).unwrap();

        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.contains("status: in-progress"));
        assert!(content.contains("old_key: old_value"));
    }

    #[test]
    fn test_split_feature_section_no_marker() {
        let temp = TempDir::new().unwrap();
        let env_path = temp.path().join(".env");
        fs::write(&env_path, "FOO=bar\nBAZ=qux").unwrap();

        let (base, feature) = split_feature_section(&env_path).unwrap();
        assert_eq!(base.trim(), "FOO=bar\nBAZ=qux");
        assert!(feature.is_none());
    }

    #[test]
    fn test_split_feature_section_with_marker() {
        let temp = TempDir::new().unwrap();
        let env_path = temp.path().join(".env");
        fs::write(
            &env_path,
            "FOO=bar\n# Feature-specific configuration\nWORK_FEATURE=test\n",
        )
        .unwrap();

        let (base, feature) = split_feature_section(&env_path).unwrap();
        assert_eq!(base.trim(), "FOO=bar");
        assert!(feature.is_some());
        assert!(feature.unwrap().contains("WORK_FEATURE=test"));
    }

    #[test]
    fn test_env_outcome_skipped() {
        let outcome = EnvOutcome::skipped();
        assert!(outcome.skipped);
        assert!(outcome.env_path.is_none());
        assert!(outcome.feature_url.is_none());
        assert!(outcome.compose_project_name.is_none());
    }

    #[test]
    fn test_resolve_repo_root_direct_repo() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path();

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        let resolved = resolve_repo_root(repo_path).unwrap();
        assert_eq!(resolved.canonicalize().unwrap(), repo_path.canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_repo_root_not_a_repo() {
        let temp = TempDir::new().unwrap();
        let result = resolve_repo_root(temp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not a git repository"));
    }

    #[test]
    fn test_state_store_load_empty() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());
        let registry = store.load_registry().unwrap();
        assert_eq!(registry.features.len(), 0);
    }

    #[test]
    fn test_state_store_record_start() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let metadata = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: Some("test.example.com".to_string()),
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
        };

        store.record_start(metadata.clone()).unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].work_feature, "test");
        assert_eq!(features[0].status, FeatureStatus::Active);
    }

    #[test]
    fn test_state_store_record_teardown() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let metadata = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: Some("test.example.com".to_string()),
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            removed_at: None,
        };

        store.record_start(metadata).unwrap();
        store.record_teardown("test").unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].status, FeatureStatus::Removed);
        assert!(features[0].removed_at.is_some());
    }

    #[test]
    fn test_state_store_record_start_updates_existing() {
        let temp = TempDir::new().unwrap();
        let store = FeatureStateStore::new(temp.path());

        let now = Utc::now();
        let metadata1 = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("main".to_string()),
            feature_url: Some("test.example.com".to_string()),
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: now,
            updated_at: now,
            removed_at: None,
        };

        store.record_start(metadata1).unwrap();

        // Record start again with updated data
        let metadata2 = FeatureMetadata {
            work_feature: "test".to_string(),
            branch_name: "feature/test".to_string(),
            worktree_path: temp.path().join("test"),
            base_branch: Some("develop".to_string()), // Changed
            feature_url: Some("new.example.com".to_string()), // Changed
            compose_project_name: Some("test".to_string()),
            env_path: Some(temp.path().join("test/.env")),
            status: FeatureStatus::Active,
            created_at: Utc::now(), // This should be ignored
            updated_at: Utc::now(),
            removed_at: None,
        };

        store.record_start(metadata2).unwrap();

        let features = store.list_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].work_feature, "test");
        assert_eq!(features[0].base_branch, Some("develop".to_string()));
        assert_eq!(features[0].feature_url, Some("new.example.com".to_string()));
        // created_at should be preserved from first record
        assert_eq!(features[0].created_at, now);
    }

    fn setup_test_repo() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .output()
            .unwrap();

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

        fs::write(repo_path.join("README.md"), "# Test Repo\n").unwrap();
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
    fn feature_start_creates_worktree_and_env() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let worktree_path = repo_path.parent().unwrap().join("test-feature");
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path).unwrap();
        }

        // Create uncommitted change to verify stash handling
        fs::write(repo_path.join("README.md"), "# Test Repo\nlocal change\n").unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("test-feature".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        assert!(summary.worktree_path.exists());
        let env_path = summary.worktree_path.join(".env");
        assert!(env_path.exists());
        let env_content = fs::read_to_string(&env_path).unwrap();
        assert!(env_content.contains("WORK_FEATURE=test-feature"));
        assert!(env_content.contains("APP_URL=dev-test-feature.example.com"));
        assert_eq!(
            summary.feature_url.as_deref(),
            Some("dev-test-feature.example.com")
        );
        let adapter = summary.adapter.as_ref().expect("adapter summary");
        assert_eq!(adapter.name, "Generic");
        assert_eq!(adapter.service_url, "http://dev:3000");
        assert!(adapter.warnings.is_empty());
        assert!(summary.warnings.is_empty());

        // Stashed README changes should be applied to the new worktree only
        let worktree_readme = fs::read_to_string(summary.worktree_path.join("README.md")).unwrap();
        assert!(worktree_readme.contains("local change"));
        let main_readme = fs::read_to_string(repo_path.join("README.md")).unwrap();
        assert_eq!(main_readme, "# Test Repo\n");

        let spec_path = repo_path.join("docs/features/in-progress/test-feature.md");
        assert!(spec_path.exists());
        let spec_content = fs::read_to_string(&spec_path).unwrap();
        assert!(spec_content.contains("status: in-progress"));
        assert!(spec_content.contains("branch: feature/test-feature"));
        assert!(spec_content.contains("worktree:"));

        let stash_list = Command::new("git")
            .args(["stash", "list"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&stash_list.stdout)
            .trim()
            .is_empty());

        let registry_path = repo_path.join(".branchbox/feature.json");
        assert!(registry_path.exists());
        let registry_data = fs::read_to_string(&registry_path).unwrap();
        let registry: Value = serde_json::from_str(&registry_data).unwrap();
        let features = registry
            .get("features")
            .and_then(|features| features.as_array())
            .expect("features is array");
        assert_eq!(features.len(), 1);
        let entry = features.first().unwrap();
        assert_eq!(entry.get("work_feature").unwrap(), "test-feature");
        assert_eq!(entry.get("status").unwrap(), "active");

        let teardown_summary = workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature.clone(),
                branch_prefix: None,
                delete_branch: true,
                force_remove: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
        assert!(teardown_summary.adapter_cleanup_warnings.is_empty());
    }

    #[test]
    fn feature_teardown_removes_worktree_and_branch() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        workflow
            .start(StartRequest {
                name: Some("cleanup".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let worktree_path = repo_path.parent().unwrap().join("cleanup");
        assert!(worktree_path.exists());

        let summary = workflow
            .teardown(TeardownRequest {
                work_feature: "cleanup".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_remove: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();

        assert!(summary.worktree_removed);
        assert!(summary.branch_deleted);
        assert!(!worktree_path.exists());
        assert!(summary.adapter_cleanup_warnings.is_empty());

        let output = Command::new("git")
            .args(["branch", "--list", "feature/cleanup"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());

        let registry_path = repo_path.join(".branchbox/feature.json");
        let registry_data = fs::read_to_string(&registry_path).unwrap();
        let registry: Value = serde_json::from_str(&registry_data).unwrap();
        let entry = registry["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item.get("work_feature").unwrap() == "cleanup")
            .unwrap();
        assert_eq!(entry.get("status").unwrap(), "removed");
    }

    #[test]
    fn list_features_returns_sorted_entries() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .start(StartRequest {
                name: Some("feature-one".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        thread::sleep(Duration::from_millis(10));

        workflow
            .start(StartRequest {
                name: Some("feature-two".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let features = workflow.list_features().unwrap();
        assert_eq!(features.len(), 2);
        assert_eq!(features[0].work_feature, "feature-two");

        workflow
            .teardown(TeardownRequest {
                work_feature: "feature-one".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_remove: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();

        let features = workflow.list_features().unwrap();
        let removed = features
            .iter()
            .find(|item| item.work_feature == "feature-one")
            .unwrap();
        assert_eq!(removed.status, FeatureStatus::Removed);

        workflow
            .teardown(TeardownRequest {
                work_feature: "feature-two".to_string(),
                branch_prefix: None,
                delete_branch: true,
                force_remove: true,
                complete_spec: false,
                telemetry: false,
            })
            .unwrap();
    }

    #[test]
    fn feature_teardown_moves_spec_to_completed_when_requested() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("complete-me".to_string()),
                ..StartRequest::default()
            })
            .unwrap();

        let in_progress_spec = repo_path.join("docs/features/in-progress/complete-me.md");
        assert!(in_progress_spec.exists());

        workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature.clone(),
                branch_prefix: None,
                delete_branch: true,
                force_remove: true,
                complete_spec: true,
                telemetry: false,
            })
            .unwrap();

        let completed_spec = repo_path.join("docs/features/completed/complete-me.md");
        assert!(completed_spec.exists());
        assert!(!in_progress_spec.exists());
        let spec_body = fs::read_to_string(completed_spec).unwrap();
        assert!(spec_body.contains("status: completed"));
        assert!(spec_body.contains("completed:"));
    }

    #[test]
    fn feature_start_reuses_existing_env_with_warning() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let worktree_path = repo_path.parent().unwrap().join("reuse-env");
        fs::create_dir_all(worktree_path.join(".devcontainer")).unwrap();
        fs::write(
            worktree_path.join(".env"),
            "FOO=bar\n# Feature-specific configuration (managed by branchbox)\nOLD_VALUE=1\n",
        )
        .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "feature/reuse-env"])
            .status()
            .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("reuse-env".to_string()),
                reuse_existing: true,
                ..StartRequest::default()
            })
            .unwrap();

        assert!(summary
            .warnings
            .iter()
            .any(|w| w.contains("Existing feature-specific configuration replaced")));

        let env_content = fs::read_to_string(summary.worktree_path.join(".env")).unwrap();
        assert!(env_content.contains("WORK_FEATURE=reuse-env"));
        assert!(!env_content.contains("OLD_VALUE"));
    }

    #[test]
    fn feature_start_reuse_requires_existing_branch() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let err = workflow
            .start(StartRequest {
                name: Some("missing-branch".to_string()),
                reuse_existing: true,
                ..StartRequest::default()
            })
            .unwrap_err();

        assert!(format!("{}", err).contains("feature/missing-branch"));
    }

    #[test]
    fn feature_start_reuse_bootstraps_branch_from_remote() {
        let temp = setup_test_repo();
        let repo_path = temp.path();
        fs::write(repo_path.join(".env"), "APP_URL=dev.example.com\n").unwrap();

        std::env::set_var("BRANCHBOX_SKIP_HOST_VALIDATION", "1");

        let remote = TempDir::new().unwrap();
        Command::new("git")
            .current_dir(remote.path())
            .args(["init", "--bare"])
            .status()
            .unwrap();

        let remote_name = "upstream";

        Command::new("git")
            .current_dir(repo_path)
            .args([
                "remote",
                "add",
                remote_name,
                remote.path().to_str().unwrap(),
            ])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["push", remote_name, "main"])
            .status()
            .unwrap();

        Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "-b", "feature/remote"])
            .status()
            .unwrap();
        fs::write(repo_path.join("REMOTE.md"), "remote branch").unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["add", "REMOTE.md"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["commit", "-m", "Remote branch state"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["push", remote_name, "feature/remote"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["checkout", "main"])
            .status()
            .unwrap();
        Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "-D", "feature/remote"])
            .status()
            .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let summary = workflow
            .start(StartRequest {
                name: Some("remote".to_string()),
                reuse_existing: true,
                ..StartRequest::default()
            })
            .unwrap();

        let branches = Command::new("git")
            .current_dir(repo_path)
            .args(["branch", "--list", "feature/remote"])
            .output()
            .unwrap();
        assert!(!branches.stdout.is_empty());
        assert_eq!(summary.branch_name, "feature/remote");
    }

    #[test]
    fn test_fix_git_worktree_path_converts_absolute() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        // Create a mock worktree directory
        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        // Write a .git file with absolute path
        let git_file = worktree_path.join(".git");
        fs::write(
            &git_file,
            format!(
                "gitdir: {}/main/.git/worktrees/feature-test\n",
                repo_path.display()
            ),
        )
        .unwrap();

        // Create the workflow and fix the path
        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        // Verify the path was converted to relative
        let content = fs::read_to_string(&git_file).unwrap();
        assert!(content.starts_with("gitdir: ../main/.git/worktrees/feature-test"));
        assert!(!content.contains(repo_path.to_str().unwrap()));
    }

    #[test]
    fn test_fix_git_worktree_path_skips_relative() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        // Write a .git file with already-relative path
        let git_file = worktree_path.join(".git");
        let original_content = "gitdir: ../main/.git/worktrees/feature-test\n";
        fs::write(&git_file, original_content).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        // Verify the path was not changed
        let content = fs::read_to_string(&git_file).unwrap();
        assert_eq!(content, original_content);
    }

    #[test]
    fn test_fix_git_worktree_path_handles_missing_git_file() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();
        // Don't create .git file

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        let result = workflow.fix_git_worktree_path(&worktree_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No .git file found"));
    }

    #[test]
    fn test_fix_git_worktree_path_handles_invalid_gitdir() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        // Write an invalid .git file (no gitdir: line)
        let git_file = worktree_path.join(".git");
        fs::write(&git_file, "invalid content\n").unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        let result = workflow.fix_git_worktree_path(&worktree_path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No gitdir: line found"));
    }

    #[test]
    fn test_fix_git_worktree_path_non_standard_main_name() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();

        // Create a worktree with a non-standard main repo name
        let worktree_path = repo_path.join("feature-test");
        fs::create_dir_all(&worktree_path).unwrap();

        // Simulate a main repo named "trunk" instead of "main"
        let git_file = worktree_path.join(".git");
        fs::write(
            &git_file,
            format!(
                "gitdir: {}/trunk/.git/worktrees/feature-test\n",
                repo_path.display()
            ),
        )
        .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow.fix_git_worktree_path(&worktree_path).unwrap();

        // Verify it extracted "trunk" from the path
        let content = fs::read_to_string(&git_file).unwrap();
        assert!(content.starts_with("gitdir: ../trunk/.git/worktrees/feature-test"));
    }

    #[test]
    fn test_generate_feature_color_deterministic() {
        // Same feature name should always generate same color
        let color1 = generate_feature_color("oauth-integration");
        let color2 = generate_feature_color("oauth-integration");
        assert_eq!(color1, color2);
    }

    #[test]
    fn test_generate_feature_color_different_names() {
        // Different names should (likely) generate different colors
        let color1 = generate_feature_color("oauth-integration");
        let color2 = generate_feature_color("payment-system");
        // Not guaranteed to be different with only 12 colors, but likely
        // This test mainly ensures the function works
        assert!(color1.starts_with('#'));
        assert!(color2.starts_with('#'));
        assert_eq!(color1.len(), 7); // #RRGGBB format
        assert_eq!(color2.len(), 7);
    }

    #[test]
    fn test_setup_vscode_workspace_creates_settings() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let color = Some("#3498db".to_string());

        workflow
            .setup_vscode_workspace(&worktree_path, "test-feature", &color, None)
            .unwrap();

        // Check that .vscode/settings.json was created
        let settings_path = worktree_path.join(".vscode/settings.json");
        assert!(settings_path.exists());

        // Verify content
        let content = fs::read_to_string(&settings_path).unwrap();
        assert!(content.contains("peacock.color"));
        assert!(content.contains("#3498db"));
        assert!(content.contains("window.title"));
        assert!(content.contains("[test-feature]"));
    }

    #[test]
    fn test_setup_vscode_workspace_creates_tasks() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .setup_vscode_workspace(
                &worktree_path,
                "test-feature",
                &None,
                Some("https://test-feature.example.com"),
            )
            .unwrap();

        // Check that .vscode/tasks.json was created
        let tasks_path = worktree_path.join(".vscode/tasks.json");
        assert!(tasks_path.exists());

        // Verify content
        let content = fs::read_to_string(&tasks_path).unwrap();
        assert!(content.contains("Open Feature URL"));
        assert!(content.contains("https://test-feature.example.com"));
    }

    #[test]
    fn test_setup_vscode_workspace_no_url() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        fs::create_dir_all(&worktree_path).unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();

        workflow
            .setup_vscode_workspace(&worktree_path, "test-feature", &None, None)
            .unwrap();

        // Should still create settings
        let settings_path = worktree_path.join(".vscode/settings.json");
        assert!(settings_path.exists());

        // But no tasks.json without URL
        let tasks_path = worktree_path.join(".vscode/tasks.json");
        assert!(!tasks_path.exists());
    }

    #[test]
    fn test_setup_vscode_workspace_preserves_existing_settings() {
        let temp_dir = setup_test_repo();
        let repo_path = temp_dir.path();
        let worktree_path = repo_path.join("test-feature");
        let vscode_dir = worktree_path.join(".vscode");
        fs::create_dir_all(&vscode_dir).unwrap();

        // Create existing settings with custom value
        let existing_settings = serde_json::json!({
            "custom.setting": "value",
            "peacock.color": "#old-color"
        });
        fs::write(
            vscode_dir.join("settings.json"),
            serde_json::to_string_pretty(&existing_settings).unwrap(),
        )
        .unwrap();

        let workflow = FeatureWorkflow::new(repo_path).unwrap();
        let color = Some("#3498db".to_string());

        workflow
            .setup_vscode_workspace(&worktree_path, "test-feature", &color, None)
            .unwrap();

        // Check that custom setting is preserved
        let content = fs::read_to_string(vscode_dir.join("settings.json")).unwrap();
        assert!(content.contains("custom.setting"));
        assert!(content.contains("value"));
        // But peacock color should be updated
        assert!(content.contains("#3498db"));
        assert!(!content.contains("#old-color"));
    }
}
