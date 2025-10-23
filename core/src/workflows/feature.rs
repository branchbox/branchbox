use crate::{
    git::GitWorktree,
    modules, naming,
    validation::{self, AppUrl},
    Error, Result,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
    pub module_reports: Vec<ModuleSetupReport>,
    pub warnings: Vec<String>,
}

/// Module setup execution report.
#[derive(Debug)]
pub struct ModuleSetupReport {
    pub name: String,
    pub init_ok: bool,
    pub setup_ok: bool,
    pub errors: Vec<String>,
}

/// Parameters for tearing down a feature worktree.
#[derive(Debug)]
pub struct TeardownRequest {
    pub work_feature: String,
    pub branch_prefix: Option<String>,
    pub delete_branch: bool,
    pub force_remove: bool,
    pub complete_spec: bool,
}

/// Result of running feature teardown workflow.
#[derive(Debug)]
pub struct TeardownSummary {
    pub work_feature: String,
    pub branch_name: String,
    pub worktree_removed: bool,
    pub branch_deleted: bool,
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
        let base_branch = request.base_branch.clone();
        let mut warnings = Vec::new();

        if worktree_path.exists() && !request.reuse_existing {
            return Err(Error::WorktreeExists(worktree_path));
        }

        if !request.reuse_existing && self.git.branch_exists(&branch_name)? {
            return Err(Error::BranchExists(branch_name));
        }

        if worktree_path.exists() && request.reuse_existing {
            tracing::info!("Using existing worktree at {}", worktree_path.display());
        } else {
            self.git
                .create(&worktree_path, &branch_name, request.base_branch.as_deref())?;
        }

        self.write_git_fix_script(&worktree_path)?;

        let env_outcome = self.prepare_env(&worktree_path, &work_feature, &branch_name)?;
        if env_outcome.skipped {
            warnings.push("Skipped .env provisioning (source file not found)".to_string());
        }

        let module_reports = self.run_module_setup(&worktree_path);

        let env_path = env_outcome.env_path.clone();
        let feature_url = env_outcome.feature_url.clone();
        let compose_project_name = env_outcome.compose_project_name.clone();

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
        }) {
            tracing::warn!("Failed to update feature registry: {}", err);
            warnings.push("Failed to update feature registry metadata".to_string());
        }

        Ok(StartSummary {
            work_feature,
            branch_name,
            worktree_path,
            feature_url,
            compose_project_name,
            env_path,
            module_reports,
            warnings,
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

        let previous_complete = std::env::var("BRANCHBOX_COMPLETE_SPEC").ok();
        if request.complete_spec {
            std::env::set_var("BRANCHBOX_COMPLETE_SPEC", "1");
        } else {
            std::env::remove_var("BRANCHBOX_COMPLETE_SPEC");
        }

        let module_reports = self.run_module_teardown(&worktree_path);

        match previous_complete {
            Some(value) => std::env::set_var("BRANCHBOX_COMPLETE_SPEC", value),
            None => std::env::remove_var("BRANCHBOX_COMPLETE_SPEC"),
        }
        let mut warnings = Vec::new();

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

    fn prepare_env(
        &self,
        worktree_path: &Path,
        work_feature: &str,
        branch_name: &str,
    ) -> Result<EnvOutcome> {
        let source_env = self.repo_root.join(".env");
        if !source_env.exists() {
            tracing::warn!("No .env found at {}", source_env.display());
            return Ok(EnvOutcome::skipped());
        }

        let content = fs::read_to_string(&source_env)?;
        let stripped = strip_feature_section(&content);
        let dest_env = worktree_path.join(".env");
        fs::write(&dest_env, stripped)?;

        let mut feature_url = None;
        let mut compose_project_name = None;

        match AppUrl::from_env_file(&source_env) {
            Ok(app_url) => {
                let url = naming::generate_feature_url(&app_url.url, work_feature);
                let compose_name = format!("{}-{}", app_url.base_prefix, work_feature);
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

    fn write_git_fix_script(&self, worktree_path: &Path) -> Result<()> {
        let dev_dir = worktree_path.join(".devcontainer");
        fs::create_dir_all(&dev_dir)?;
        let script_path = dev_dir.join("fix-git-worktree.sh");
        fs::write(&script_path, FIX_GIT_SCRIPT)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms)?;
        }

        Ok(())
    }

    fn run_module_setup(&self, feature_dir: &Path) -> Vec<ModuleSetupReport> {
        modules::detect_modules(&self.repo_root)
            .into_iter()
            .map(|mut module| {
                let name = module.name().to_string();
                let mut report = ModuleSetupReport {
                    name,
                    init_ok: false,
                    setup_ok: false,
                    errors: Vec::new(),
                };

                match module.init(&self.repo_root, feature_dir) {
                    Ok(_) => {
                        report.init_ok = true;
                        match module.setup(&self.repo_root, feature_dir) {
                            Ok(_) => {
                                report.setup_ok = true;
                            }
                            Err(err) => {
                                report.errors.push(err.to_string());
                            }
                        }
                    }
                    Err(err) => {
                        report.errors.push(err.to_string());
                    }
                }

                report
            })
            .collect()
    }

    fn run_module_teardown(&self, feature_dir: &Path) -> Vec<ModuleTeardownReport> {
        modules::detect_modules(&self.repo_root)
            .into_iter()
            .map(|mut module| {
                let name = module.name().to_string();
                let mut report = ModuleTeardownReport {
                    name,
                    teardown_ok: false,
                    errors: Vec::new(),
                };

                match module.init(&self.repo_root, feature_dir) {
                    Ok(_) => match module.teardown(&self.repo_root, feature_dir) {
                        Ok(_) => {
                            report.teardown_ok = true;
                        }
                        Err(err) => {
                            report.errors.push(err.to_string());
                        }
                    },
                    Err(err) => {
                        report.errors.push(err.to_string());
                    }
                }

                report
            })
            .collect()
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
}

/// Helper to ensure the file ends with a newline before appending.
fn ensure_trailing_newline(file: &mut File) -> io::Result<()> {
    file.write_all(b"\n")
}

fn strip_feature_section(content: &str) -> String {
    if let Some(pos) = content.find("# Feature-specific configuration") {
        content[..pos].trim_end().to_string()
    } else {
        content.trim_end().to_string()
    }
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
    let abs = if path.exists() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };

    let git_path = abs.join(".git");
    if git_path.is_dir() {
        return Ok(abs);
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
            abs.join(gitdir)
        };
        let canonical_gitdir = gitdir_path.canonicalize()?;
        let main_git_dir = canonical_gitdir
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| Error::validation("Unable to resolve main git directory".to_string()))?;
        return main_git_dir
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| Error::validation("Unable to resolve repository root".to_string()));
    }

    Err(Error::validation(format!(
        "Not a git repository: {}",
        abs.display()
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

const FIX_GIT_SCRIPT: &str = r#"#!/bin/bash
# Fix git worktree path for devcontainer
# This script is automatically created by branchbox

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKTREE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKTREE_NAME=$(basename "$WORKTREE_DIR")
GIT_FILE="$WORKTREE_DIR/.git"

if [ -f "$GIT_FILE" ]; then
  CURRENT_PATH=$(sed 's/gitdir: //' "$GIT_FILE")
  PARENT_DIR=$(dirname "$WORKTREE_DIR")
  MAIN_NAME=""

  if [ -d "$PARENT_DIR/main" ]; then
    MAIN_NAME="main"
  elif [[ "$CURRENT_PATH" =~ /([^/]+)/.git/worktrees/ ]]; then
    MAIN_NAME="${BASH_REMATCH[1]}"
  fi

  MAIN_NAME="${MAIN_NAME:-main}"
  CORRECT_PATH="../$MAIN_NAME/.git/worktrees/$WORKTREE_NAME"
  echo "gitdir: $CORRECT_PATH" > "$GIT_FILE"
  echo "✓ Fixed git worktree path (using relative path)"
else
  echo "⚠ No .git file found"
fi
"#;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

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
        assert!(summary.warnings.is_empty());

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

        workflow
            .teardown(TeardownRequest {
                work_feature: summary.work_feature.clone(),
                branch_prefix: None,
                delete_branch: true,
                force_remove: true,
                complete_spec: false,
            })
            .unwrap();
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
            })
            .unwrap();

        assert!(summary.worktree_removed);
        assert!(summary.branch_deleted);
        assert!(!worktree_path.exists());

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
            })
            .unwrap();
    }
}
