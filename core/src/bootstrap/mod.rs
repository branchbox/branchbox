//! Bootstrap system for generating devcontainer configurations
//!
//! This module implements the "meta" feature - a tool that can set up development
//! environments for any project, including itself.
//!
//! # Concept
//!
//! The branchbox can bootstrap complete devcontainer setups for:
//! - Rails projects
//! - Node.js projects
//! - Rust projects (including itself!)
//! - Generic projects
//!
//! # Usage
//!
//! ```no_run
//! use worktree_core::bootstrap::{Bootstrap, Stack};
//! use std::path::Path;
//!
//! let project_path = Path::new("/path/to/project");
//! let bootstrap = Bootstrap::new(project_path);
//!
//! // Auto-detect stack
//! let stack = bootstrap.detect_stack().unwrap();
//!
//! // Generate devcontainer configuration
//! bootstrap.generate(stack).unwrap();
//! ```
//!
//! # What It Generates
//!
//! - `.devcontainer/devcontainer.json` - VS Code/Cursor configuration
//! - `.devcontainer/compose.yaml` - Docker Compose services
//! - `.devcontainer/Dockerfile` - Custom development image
//! - `.env.sample` - Environment variable template
//! - Stack-specific scripts and configurations

use crate::Result;
use std::path::PathBuf;

pub mod templates;

/// Supported stack types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    /// Ruby on Rails
    Rails,
    /// Node.js / JavaScript
    NodeJs,
    /// Rust
    Rust,
    /// Generic/Unknown
    Generic,
}

impl Stack {
    /// Get stack name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Stack::Rails => "rails",
            Stack::NodeJs => "nodejs",
            Stack::Rust => "rust",
            Stack::Generic => "generic",
        }
    }
}

/// Bootstrap configuration builder
pub struct Bootstrap {
    project_path: PathBuf,
}

impl Bootstrap {
    /// Create a new Bootstrap instance
    pub fn new(project_path: impl Into<PathBuf>) -> Self {
        Self {
            project_path: project_path.into(),
        }
    }

    /// Auto-detect project stack
    ///
    /// Uses the adapter system to detect the project type.
    pub fn detect_stack(&self) -> Result<Stack> {
        // Check for Rust project
        if self.project_path.join("Cargo.toml").exists() {
            return Ok(Stack::Rust);
        }

        // Check for Rails project
        if self.project_path.join("Gemfile").exists() {
            if let Ok(content) = std::fs::read_to_string(self.project_path.join("Gemfile")) {
                if content.contains("gem \"rails\"") || content.contains("gem 'rails'") {
                    return Ok(Stack::Rails);
                }
            }
        }

        // Check for Node.js project
        if self.project_path.join("package.json").exists() {
            return Ok(Stack::NodeJs);
        }

        // Fallback to generic
        Ok(Stack::Generic)
    }

    /// Generate devcontainer configuration
    ///
    /// Creates all necessary files for a complete devcontainer setup.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use worktree_core::bootstrap::{Bootstrap, Stack};
    /// use std::path::Path;
    ///
    /// let project = Path::new("/path/to/rust-project");
    /// let bootstrap = Bootstrap::new(project);
    /// bootstrap.generate(Stack::Rust).unwrap();
    /// ```
    pub fn generate(&self, stack: Stack) -> Result<()> {
        use std::fs;

        tracing::info!("Generating devcontainer for {} stack", stack.as_str());

        // Create .devcontainer directory
        let devcontainer_dir = self.project_path.join(".devcontainer");
        fs::create_dir_all(&devcontainer_dir)?;
        tracing::debug!("Created directory: {}", devcontainer_dir.display());

        // Generate and write devcontainer.json
        let devcontainer_json = self.generate_devcontainer_json(stack)?;
        let devcontainer_json_path = devcontainer_dir.join("devcontainer.json");
        Self::safe_write(&devcontainer_json_path, &devcontainer_json, None)?;
        tracing::info!("Created: {}", devcontainer_json_path.display());

        // Generate and write compose.yaml
        let compose_yaml = self.generate_compose_yaml(stack)?;
        let compose_yaml_path = devcontainer_dir.join("compose.yaml");
        Self::safe_write(&compose_yaml_path, &compose_yaml, None)?;
        tracing::info!("Created: {}", compose_yaml_path.display());

        // Generate and write Dockerfile
        let dockerfile = self.generate_dockerfile(stack)?;
        let dockerfile_path = devcontainer_dir.join("Dockerfile");
        Self::safe_write(&dockerfile_path, &dockerfile, None)?;
        tracing::info!("Created: {}", dockerfile_path.display());

        // Generate 1Password integration scripts
        // On Unix, mode 0o755 is set atomically at creation — no separate
        // chmod call.  On non-Unix, mode is not enforced (see safe_write docs).
        let init_host = self.generate_init_host_sh()?;
        let init_host_path = devcontainer_dir.join("init-host.sh");
        Self::safe_write(&init_host_path, &init_host, Some(0o755))?;
        tracing::info!("Created: {}", init_host_path.display());

        let setup_git = self.generate_setup_git_sh()?;
        let setup_git_path = devcontainer_dir.join("setup-git.sh");
        Self::safe_write(&setup_git_path, &setup_git, Some(0o755))?;
        tracing::info!("Created: {}", setup_git_path.display());

        // Create empty secret files so compose volume mounts don't fail.
        // Called unconditionally — create_restricted_file is idempotent
        // (creates if missing, leaves existing regular files untouched,
        // replaces symlinks). No check-then-act guard needed.
        let secret_files = [".github-token.env", ".git-signing-key", ".gitconfig.env"];
        for secret_file in &secret_files {
            let secret_path = devcontainer_dir.join(secret_file);
            Self::create_restricted_file(&secret_path)?;
            tracing::debug!("Ensured placeholder exists: {}", secret_path.display());
        }

        // Ensure secret files are excluded from version control in the target project.
        // safe_read uses O_NOFOLLOW — a symlink here is treated as non-existent.
        let gitignore_path = self.project_path.join(".gitignore");
        let mut gitignore_content = Self::safe_read(&gitignore_path)?.unwrap_or_default();

        let mut gitignore_updated = false;
        let existing_lines: std::collections::HashSet<&str> = gitignore_content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let missing_entries: Vec<String> = secret_files
            .iter()
            .map(|f| format!(".devcontainer/{}", f))
            .filter(|e| !existing_lines.contains(e.as_str()))
            .collect();

        if !missing_entries.is_empty() {
            if !gitignore_content.is_empty() && !gitignore_content.ends_with('\n') {
                gitignore_content.push('\n');
            }

            const GITIGNORE_HEADER: &str =
                "# 1Password secrets (populated by init-host.sh, never committed)";
            if !gitignore_content.contains(GITIGNORE_HEADER) {
                gitignore_content.push_str(&format!("\n{}\n", GITIGNORE_HEADER));
            }

            for entry in &missing_entries {
                gitignore_content.push_str(entry);
                gitignore_content.push('\n');
            }
            gitignore_updated = true;
        }

        if gitignore_updated {
            Self::safe_write(&gitignore_path, &gitignore_content, None)?;
            tracing::info!("Updated .gitignore to exclude secret files");
        }

        // Generate and write .env.sample (in project root, not .devcontainer)
        let env_sample = self.generate_env_sample(stack)?;
        let env_sample_path = self.project_path.join(".env.sample");

        // Only create if it doesn't exist (don't overwrite)
        if !env_sample_path.exists() {
            fs::write(&env_sample_path, env_sample)?;
            tracing::info!("Created: {}", env_sample_path.display());
        } else {
            tracing::info!("Skipped (already exists): {}", env_sample_path.display());
        }

        // Generate the BranchBox env overrides placeholder (lives under .devcontainer)
        let branchbox_env = self.generate_branchbox_env()?;
        let branchbox_env_path = devcontainer_dir.join(".branchbox.env");
        if !branchbox_env_path.exists() {
            fs::write(&branchbox_env_path, branchbox_env)?;
            tracing::info!("Created: {}", branchbox_env_path.display());
        } else {
            tracing::info!("Skipped (already exists): {}", branchbox_env_path.display());
        }

        // Generate BranchBox quickstart docs
        let docs_dir = self.project_path.join("docs");
        fs::create_dir_all(&docs_dir)?;
        let branchbox_docs = self.generate_branchbox_docs()?;
        let branchbox_docs_path = docs_dir.join("BRANCHBOX.md");
        if !branchbox_docs_path.exists() {
            fs::write(&branchbox_docs_path, branchbox_docs)?;
            tracing::info!("Created: {}", branchbox_docs_path.display());
        } else {
            tracing::info!(
                "Skipped (already exists): {}",
                branchbox_docs_path.display()
            );
        }

        tracing::info!("✓ Bootstrap complete for {} stack", stack.as_str());
        tracing::info!("  Next steps:");
        tracing::info!("  1. Open project in VS Code/Cursor");
        tracing::info!("  2. Reopen in Container");
        tracing::info!("  3. Start developing!");

        Ok(())
    }

    /// Generate devcontainer.json
    fn generate_devcontainer_json(&self, stack: Stack) -> Result<String> {
        templates::devcontainer_json(stack)
    }

    /// Generate compose.yaml
    fn generate_compose_yaml(&self, stack: Stack) -> Result<String> {
        templates::compose_yaml(stack)
    }

    /// Generate Dockerfile
    fn generate_dockerfile(&self, stack: Stack) -> Result<String> {
        templates::dockerfile(stack)
    }

    /// Generate .env.sample
    fn generate_env_sample(&self, stack: Stack) -> Result<String> {
        templates::env_sample(stack)
    }

    /// Generate .branchbox.env placeholder
    fn generate_branchbox_env(&self) -> Result<String> {
        templates::branchbox_env()
    }

    /// Generate docs/BRANCHBOX.md quickstart guide
    fn generate_branchbox_docs(&self) -> Result<String> {
        templates::branchbox_docs()
    }

    /// Write a file safely, refusing to follow symbolic links.
    ///
    /// ## Unix
    ///
    /// Uses `O_NOFOLLOW` so the kernel rejects symlinks at open time —
    /// no TOCTOU window.  When `mode` is `Some(m)`, permissions are set
    /// **atomically at creation** via `OpenOptionsExt::mode()`.
    ///
    /// ## Non-Unix
    ///
    /// Uses `tempfile::NamedTempFile` + `persist()` (atomic rename).
    /// **`mode` is ignored** — non-Unix platforms lack a portable way
    /// to set permissions atomically at file creation.  BranchBox
    /// primarily targets macOS/Linux where the Unix path is used.
    ///
    /// See CONTRIBUTING.md § "File-operation security patterns" for the
    /// full rationale and rules.
    #[cfg(unix)]
    fn safe_write(path: &std::path::Path, contents: &str, mode: Option<u32>) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        // No pre-check for symlinks — O_NOFOLLOW IS the check.
        // If the path is a symlink, open() returns ELOOP; we handle
        // that below by removing the symlink and retrying.
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW);
        if let Some(m) = mode {
            opts.mode(m);
        }

        let mut file = match opts.open(path) {
            Ok(f) => f,
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                // O_NOFOLLOW correctly refused a symlink — remove and retry.
                // The retry is still protected by O_NOFOLLOW.
                tracing::warn!(
                    "Refusing symlink at {}; removing and retrying with O_NOFOLLOW",
                    path.display()
                );
                fs::remove_file(path)?;
                opts.open(path)?
            }
            Err(e) => return Err(e.into()),
        };
        file.write_all(contents.as_bytes())?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn safe_write(path: &std::path::Path, contents: &str, _mode: Option<u32>) -> Result<()> {
        use std::io::Write;

        // Atomic write via NamedTempFile + persist.
        // - tempfile creates a file with an unpredictable name (safe
        //   against pre-placed symlinks at the temp path).
        // - persist() calls rename(), which replaces the directory entry
        //   at `path` — if it's a symlink, the symlink itself is
        //   replaced (not the target).
        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(contents.as_bytes())?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    /// Read a file safely, refusing to follow symbolic links.
    ///
    /// Returns `Ok(None)` when the file does not exist **or** is a
    /// symlink (symlinks are removed as a side-effect).
    ///
    /// ## Unix
    /// Atomic via `O_NOFOLLOW` — no TOCTOU window.
    ///
    /// ## Non-Unix
    /// Uses `symlink_metadata` (lstat) then `read_to_string`.  There is
    /// an inherent TOCTOU window; see the non-Unix implementation for
    /// details and rationale.
    #[cfg(unix)]
    fn safe_read(path: &std::path::Path) -> Result<Option<String>> {
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;

        // O_NOFOLLOW is the sole guard — no pre-check needed.
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(mut file) => {
                let mut content = String::new();
                file.read_to_string(&mut content)?;
                Ok(Some(content))
            }
            // ELOOP = path is a symlink and O_NOFOLLOW refused to follow it
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                tracing::warn!(
                    "Refusing symlink at {}; removing",
                    path.display()
                );
                fs::remove_file(path)?;
                Ok(None)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    #[cfg(not(unix))]
    fn safe_read(path: &std::path::Path) -> Result<Option<String>> {
        // Non-Unix platforms lack O_NOFOLLOW.  We use symlink_metadata()
        // (equivalent to lstat) which inspects the directory entry itself
        // without following symlinks.
        //
        // There is an inherent TOCTOU window between the metadata check
        // and the read — an attacker could swap the file for a symlink
        // between the two calls.  This is unavoidable without OS-specific
        // flags (like O_NOFOLLOW on Unix).  BranchBox primarily targets
        // macOS/Linux where the atomic O_NOFOLLOW path is used instead;
        // this fallback exists only for compilation on other platforms.
        //
        // NOTE: Do NOT use File::open() + file.metadata() here.
        // File::open() follows symlinks, so metadata on the handle
        // describes the target, not the symlink — is_symlink() would
        // always return false, making the check dead code.
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                tracing::warn!(
                    "Refusing symlink at {}; removing",
                    path.display()
                );
                fs::remove_file(path)?;
                Ok(None)
            }
            Ok(_) => Ok(Some(fs::read_to_string(path)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Ensure a placeholder file exists for Docker volume mounts.
    ///
    /// **Idempotent**: creates the file if missing, leaves existing
    /// regular files untouched (no truncate), and replaces symlinks.
    /// Safe to call unconditionally — no external check-then-act guard
    /// needed.
    ///
    /// ## Unix
    /// `O_CREAT` (without `O_TRUNC`) + `O_NOFOLLOW` + `.mode(0o600)`
    /// in a single `open()` syscall.  Permissions are set atomically.
    ///
    /// ## Non-Unix
    /// Uses `create_new` (O_EXCL) to create only if missing.  **File
    /// permissions are not explicitly restricted** — non-Unix platforms
    /// lack a portable atomic mode-at-creation API.
    #[cfg(unix)]
    fn create_restricted_file(path: &std::path::Path) -> Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        // No pre-check — O_NOFOLLOW is the guard.
        // No O_TRUNC — existing content preserved (idempotent).
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(_) => Ok(()),
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                tracing::warn!(
                    "Refusing symlink at {}; removing and retrying with O_NOFOLLOW",
                    path.display()
                );
                fs::remove_file(path)?;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(path)?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    #[cfg(not(unix))]
    fn create_restricted_file(path: &std::path::Path) -> Result<()> {
        // create_new (O_EXCL) atomically creates only if missing.
        // If already exists, leave regular files alone; replace symlinks.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Replace symlinks; leave regular files untouched.
                if fs::symlink_metadata(path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    let dir = path.parent().unwrap_or(std::path::Path::new("."));
                    let tmp = tempfile::NamedTempFile::new_in(dir)?;
                    tmp.persist(path).map_err(|e| e.error)?;
                }
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    // NOTE: set_permissions_unix was removed intentionally.
    // On Unix, permissions are now set atomically via safe_write(mode)
    // to prevent TOCTOU races between write and chmod.  On non-Unix,
    // mode is not enforced (see safe_write docs).  See CONTRIBUTING.md.

    /// Generate .devcontainer/init-host.sh (1Password host-side script)
    fn generate_init_host_sh(&self) -> Result<String> {
        templates::init_host_sh()
    }

    /// Generate .devcontainer/setup-git.sh (container-side git config)
    fn generate_setup_git_sh(&self) -> Result<String> {
        templates::setup_git_sh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_detect_rust_stack() {
        let temp_dir = TempDir::new().unwrap();
        let mut cargo_toml = fs::File::create(temp_dir.path().join("Cargo.toml")).unwrap();
        writeln!(cargo_toml, "[package]\nname = \"test\"").unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        assert_eq!(bootstrap.detect_stack().unwrap(), Stack::Rust);
    }

    #[test]
    fn test_detect_rails_stack() {
        let temp_dir = TempDir::new().unwrap();
        let mut gemfile = fs::File::create(temp_dir.path().join("Gemfile")).unwrap();
        writeln!(gemfile, "gem \"rails\"").unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        assert_eq!(bootstrap.detect_stack().unwrap(), Stack::Rails);
    }

    #[test]
    fn test_detect_nodejs_stack() {
        let temp_dir = TempDir::new().unwrap();
        let mut package_json = fs::File::create(temp_dir.path().join("package.json")).unwrap();
        writeln!(package_json, "{{}}").unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        assert_eq!(bootstrap.detect_stack().unwrap(), Stack::NodeJs);
    }

    #[test]
    fn test_detect_generic_stack() {
        let temp_dir = TempDir::new().unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        assert_eq!(bootstrap.detect_stack().unwrap(), Stack::Generic);
    }

    #[test]
    fn test_generate_creates_files() {
        let temp_dir = TempDir::new().unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::Rust).unwrap();

        // Check that all files were created
        assert!(temp_dir
            .path()
            .join(".devcontainer/devcontainer.json")
            .exists());
        assert!(temp_dir.path().join(".devcontainer/compose.yaml").exists());
        assert!(temp_dir.path().join(".devcontainer/Dockerfile").exists());
        assert!(temp_dir
            .path()
            .join(".devcontainer/.branchbox.env")
            .exists());
        assert!(temp_dir.path().join(".env.sample").exists());

        // Check 1Password integration scripts
        assert!(temp_dir
            .path()
            .join(".devcontainer/init-host.sh")
            .exists());
        assert!(temp_dir
            .path()
            .join(".devcontainer/setup-git.sh")
            .exists());

        // Check secret file placeholders
        assert!(temp_dir
            .path()
            .join(".devcontainer/.github-token.env")
            .exists());
        assert!(temp_dir
            .path()
            .join(".devcontainer/.git-signing-key")
            .exists());
        assert!(temp_dir
            .path()
            .join(".devcontainer/.gitconfig.env")
            .exists());

        // Check that files have content
        let devcontainer_json =
            fs::read_to_string(temp_dir.path().join(".devcontainer/devcontainer.json")).unwrap();
        assert!(devcontainer_json.contains("Rust"));

        let compose_yaml =
            fs::read_to_string(temp_dir.path().join(".devcontainer/compose.yaml")).unwrap();
        assert!(compose_yaml.contains("rust-dev"));

        let dockerfile =
            fs::read_to_string(temp_dir.path().join(".devcontainer/Dockerfile")).unwrap();
        assert!(dockerfile.contains("rust"));

        let branchbox_env =
            fs::read_to_string(temp_dir.path().join(".devcontainer/.branchbox.env")).unwrap();
        assert!(branchbox_env.contains("WORK_FEATURE=main"));
    }

    #[test]
    fn test_generate_updates_gitignore_with_secret_files() {
        let temp_dir = TempDir::new().unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::Rust).unwrap();

        let gitignore = fs::read_to_string(temp_dir.path().join(".gitignore")).unwrap();
        for secret_file in [".github-token.env", ".git-signing-key", ".gitconfig.env"] {
            let entry = format!(".devcontainer/{}", secret_file);
            assert!(
                gitignore.contains(&entry),
                ".gitignore should contain {entry}: {gitignore}"
            );
        }
    }

    #[test]
    fn test_generate_doesnt_duplicate_gitignore_entries() {
        let temp_dir = TempDir::new().unwrap();

        // Pre-populate .gitignore with one of the entries
        fs::write(
            temp_dir.path().join(".gitignore"),
            ".devcontainer/.github-token.env\n",
        )
        .unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::Rust).unwrap();

        let gitignore = fs::read_to_string(temp_dir.path().join(".gitignore")).unwrap();
        // Should only appear once
        assert_eq!(
            gitignore.matches(".devcontainer/.github-token.env").count(),
            1,
            "Entry should not be duplicated in .gitignore"
        );
        // Others should still be added
        assert!(gitignore.contains(".devcontainer/.git-signing-key"));
        assert!(gitignore.contains(".devcontainer/.gitconfig.env"));
    }

    #[test]
    fn test_generate_doesnt_overwrite_env_sample() {
        let temp_dir = TempDir::new().unwrap();

        // Create existing .env.sample
        let existing_content = "EXISTING=value\n";
        fs::write(temp_dir.path().join(".env.sample"), existing_content).unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::Rust).unwrap();

        // Check that .env.sample wasn't overwritten
        let content = fs::read_to_string(temp_dir.path().join(".env.sample")).unwrap();
        assert_eq!(content, existing_content);
    }

    #[test]
    fn test_generate_all_stacks() {
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Generic] {
            let temp_dir = TempDir::new().unwrap();
            let bootstrap = Bootstrap::new(temp_dir.path());

            bootstrap.generate(stack).unwrap();

            // Verify all files created
            assert!(
                temp_dir
                    .path()
                    .join(".devcontainer/devcontainer.json")
                    .exists(),
                "{} stack should create devcontainer.json",
                stack.as_str()
            );
            assert!(
                temp_dir.path().join(".devcontainer/compose.yaml").exists(),
                "{} stack should create compose.yaml",
                stack.as_str()
            );
            assert!(
                temp_dir.path().join(".devcontainer/Dockerfile").exists(),
                "{} stack should create Dockerfile",
                stack.as_str()
            );
            assert!(
                temp_dir
                    .path()
                    .join(".devcontainer/.branchbox.env")
                    .exists(),
                "{} stack should create .branchbox.env",
                stack.as_str()
            );
        }
    }
}
