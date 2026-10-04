//! Project detection: what BranchBox would use for a folder (`branchbox detect`, DESIGN §5.7).
//!
//! [`detect_project`] reads the folder without changing anything: the stack the bootstrapper
//! would generate for, the adapter and modules a feature start would run, and whether the
//! folder belongs to a git repository that BranchBox has already initialized.

use crate::adapters;
use crate::bootstrap::{Bootstrap, Stack};
use crate::modules;
use crate::{Error, Result};
use serde::{Serialize, Serializer};
use std::path::{Path, PathBuf};
use std::process::Command;

/// `schema_version` of the `detect --json` payload.
pub const SCHEMA_VERSION: u32 = 1;

/// The `detect --json` payload (DESIGN §5.7).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectDetection {
    pub schema_version: u32,
    /// The inspected folder, made absolute (symlinks are not resolved).
    pub project: PathBuf,
    /// Whether the folder is inside a git work tree.
    pub git_repository: bool,
    /// Whether the repository's main worktree has a BranchBox registry
    /// (`.branchbox/registry.json`, the rule `init` uses to recognise a set-up project).
    pub initialized: bool,
    /// Serialized as [`Stack::as_str`] (`rails`, `nodejs`, `rust`, `generic`).
    #[serde(serialize_with = "serialize_stack")]
    pub stack: Stack,
    /// Stable lowercase adapter id ([`adapter_id`]), e.g. `generic` or `nodejs`.
    pub adapter: String,
    /// The adapter's display name (`Generic`, `Node.js`), shown by the text output.
    #[serde(skip)]
    pub adapter_name: String,
    /// Modules a feature start would run, in execution order.
    pub modules: Vec<String>,
    pub has_devcontainer: bool,
    pub has_env: bool,
    /// Module planning warnings (e.g. a module whose dependency is missing).
    pub warnings: Vec<String>,
}

fn serialize_stack<S: Serializer>(
    stack: &Stack,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(stack.as_str())
}

/// The machine id of an adapter display name: lowercase ASCII letters and digits only, so
/// `Generic` → `generic`, `Rails` → `rails` and `Node.js` → `nodejs` (the same vocabulary as
/// [`Stack::as_str`]).
pub fn adapter_id(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

/// Inspect `path` (a folder; relative paths resolve against the current directory). Fails only
/// when the folder does not exist or cannot be read.
pub fn detect_project(path: &Path) -> Result<ProjectDetection> {
    let project = std::path::absolute(path).map_err(|err| {
        Error::validation(format!(
            "Cannot resolve project path {}: {err}",
            path.display()
        ))
    })?;
    if !project.is_dir() {
        return Err(Error::validation(format!(
            "Project directory not found: {} is not a directory",
            project.display()
        )));
    }

    let stack = Bootstrap::new(&project).detect_stack()?;
    let adapter_name = adapters::detect_adapter(&project)?.name().to_string();
    let plan = modules::detect_modules(&project, &[]);
    let main_root = main_worktree_root(&project);

    Ok(ProjectDetection {
        schema_version: SCHEMA_VERSION,
        git_repository: main_root.is_some(),
        initialized: is_initialized(main_root.as_deref().unwrap_or(&project)),
        stack,
        adapter: adapter_id(&adapter_name),
        adapter_name,
        modules: plan
            .handles
            .iter()
            .map(|handle| handle.name.clone())
            .collect(),
        has_devcontainer: project.join(".devcontainer").is_dir(),
        has_env: project.join(".env").is_file(),
        warnings: plan.warnings,
        project,
    })
}

/// Whether BranchBox has been set up in the main worktree `root`: its registry exists. This is
/// the rule `init` uses to recognise a set-up project (`.branchbox/registry.json` is definitive).
pub fn is_initialized(root: &Path) -> bool {
    root.join(".branchbox/registry.json").is_file()
}

/// The main worktree of the git work tree containing `dir`, or `None` when `dir` is not inside
/// one (or git is unavailable). For a linked (feature) worktree this is the repository's main
/// worktree, whose `.branchbox` holds the registry.
fn main_worktree_root(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(MAIN_WORKTREE_QUERY)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_main_worktree_root(dir, &String::from_utf8_lossy(&output.stdout))
}

/// The `git rev-parse` arguments whose output [`parse_main_worktree_root`] reads.
pub(crate) const MAIN_WORKTREE_QUERY: &[&str] = &[
    "rev-parse",
    "--is-inside-work-tree",
    "--show-toplevel",
    "--git-common-dir",
];

/// The main worktree named by the output of `git rev-parse` [`MAIN_WORKTREE_QUERY`] run in
/// `dir`, or `None` when `dir` is not inside a work tree.
pub(crate) fn parse_main_worktree_root(dir: &Path, stdout: &str) -> Option<PathBuf> {
    let mut lines = stdout.lines().map(str::trim);
    if lines.next() != Some("true") {
        return None;
    }
    let toplevel = PathBuf::from(lines.next().filter(|line| !line.is_empty())?);
    let common_dir = PathBuf::from(lines.next().filter(|line| !line.is_empty())?);
    // In a subfolder git prints the common dir relative to it (`../../.git`); resolve the `..`
    // lexically so reported paths read `<main>/.branchbox/…`, not `<dir>/../../.branchbox/…`.
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        normalize_lexically(&dir.join(common_dir))
    };
    // The common dir is `<main>/.git` in the standard layout; anything else (a separate git
    // dir) has no main worktree to point at, so the work tree itself stands in.
    match (common_dir.file_name(), common_dir.parent()) {
        (Some(name), Some(parent)) if name == ".git" => Some(parent.to_path_buf()),
        _ => Some(toplevel),
    }
}

/// `path` with `.` components dropped and each `..` removing the component before it, without
/// touching the disk (symlinks are not resolved, matching how `project` is reported).
fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component);
                }
            }
            other => normalized.push(other),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn a_relative_common_dir_from_a_subfolder_names_the_main_worktree_plainly() {
        let root = parse_main_worktree_root(
            Path::new("/r/main/sub/deeper"),
            "true\n/r/main\n../../.git\n",
        );
        assert_eq!(root, Some(PathBuf::from("/r/main")));
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("README.md"), "# demo\n").unwrap();
        git(dir, &["add", "README.md"]);
        git(dir, &["commit", "-q", "-m", "init"]);
    }

    #[test]
    fn adapter_ids_are_lowercase_alphanumerics() {
        assert_eq!(adapter_id("Generic"), "generic");
        assert_eq!(adapter_id("Rails"), "rails");
        assert_eq!(adapter_id("Node.js"), "nodejs");
    }

    #[test]
    fn plain_folder_is_not_a_repository() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\n").unwrap();

        let detection = detect_project(temp.path()).unwrap();
        assert_eq!(detection.project, temp.path());
        assert!(!detection.git_repository);
        assert!(!detection.initialized);
        assert_eq!(detection.stack, Stack::Rust);
        assert_eq!(detection.adapter, "generic");
        assert_eq!(detection.adapter_name, "Generic");
        assert!(!detection.has_devcontainer);
        assert!(!detection.has_env);
    }

    #[test]
    fn initialized_repository_with_devcontainer_and_env() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path();
        init_repo(repo);
        fs::create_dir_all(repo.join(".devcontainer")).unwrap();
        fs::write(repo.join(".devcontainer/devcontainer.json"), "{}").unwrap();
        fs::write(repo.join(".env"), "APP_URL=dev.example.com\n").unwrap();
        fs::write(repo.join("package.json"), "{}").unwrap();
        fs::create_dir_all(repo.join(".branchbox")).unwrap();
        fs::write(
            repo.join(".branchbox/registry.json"),
            r#"{"version":"1","features":[]}"#,
        )
        .unwrap();

        let detection = detect_project(repo).unwrap();
        assert!(detection.git_repository);
        assert!(detection.initialized);
        assert_eq!(detection.stack, Stack::NodeJs);
        assert_eq!(detection.adapter, "nodejs");
        assert!(detection.has_devcontainer);
        assert!(detection.has_env);
        assert!(detection.modules.contains(&"devcontainer".to_string()));

        let value = serde_json::to_value(&detection).unwrap();
        assert_eq!(value["stack"], "nodejs");
        assert_eq!(value["adapter"], "nodejs");
        assert!(value.get("adapter_name").is_none());
    }

    #[test]
    fn subfolders_and_linked_worktrees_report_the_main_worktree_registry() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("main");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        fs::create_dir_all(repo.join(".branchbox")).unwrap();
        fs::write(repo.join(".branchbox/registry.json"), "{}").unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        let linked = temp.path().join("eta");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature/eta",
                linked.to_str().unwrap(),
            ],
        );

        for dir in [repo.join("src"), linked] {
            let detection = detect_project(&dir).unwrap();
            assert!(detection.git_repository, "{}", dir.display());
            assert!(detection.initialized, "{}", dir.display());
        }
    }

    #[test]
    fn registry_directory_alone_is_not_initialized() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        assert!(!detect_project(temp.path()).unwrap().initialized);
    }

    #[test]
    fn relative_paths_become_absolute() {
        let detection = detect_project(Path::new(".")).unwrap();
        assert!(detection.project.is_absolute());
    }

    #[test]
    fn rev_parse_output_is_read_leniently() {
        let dir = Path::new("/r/main/src");
        assert_eq!(
            parse_main_worktree_root(dir, "true\n/r/main\n/r/main/.git\n"),
            Some(PathBuf::from("/r/main"))
        );
        assert_eq!(
            parse_main_worktree_root(dir, "true\n/r/main\n../.git\n"),
            Some(PathBuf::from("/r/main"))
        );
        assert_eq!(
            parse_main_worktree_root(dir, "true\n/r/eta\n/r/git-dir\n"),
            Some(PathBuf::from("/r/eta"))
        );
        assert_eq!(parse_main_worktree_root(dir, "false\n"), None);
        assert_eq!(parse_main_worktree_root(dir, "true\n"), None);
    }

    #[test]
    fn missing_folder_is_refused_with_its_path() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("nope");
        let err = detect_project(&missing).unwrap_err();
        assert_eq!(err.code(), "validation_failed");
        assert!(err.to_string().contains(&missing.display().to_string()));

        let file = temp.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(detect_project(&file).is_err());
    }
}
