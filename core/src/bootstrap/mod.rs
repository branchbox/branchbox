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
//! - `.devcontainer/scripts/init-host.sh` - Host-side 1Password secret refresh
//! - `.devcontainer/scripts/setup-git.sh` - Container-side git/gh/signing setup
//! - `.env.sample` - Environment variable template
//! - Stack-specific scripts and configurations

use crate::{Error, Result};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
        fs::write(&devcontainer_json_path, devcontainer_json)?;
        tracing::info!("Created: {}", devcontainer_json_path.display());

        // Generate and write compose.yaml
        let compose_yaml = self.generate_compose_yaml(stack)?;
        let compose_yaml_path = devcontainer_dir.join("compose.yaml");
        fs::write(&compose_yaml_path, compose_yaml)?;
        tracing::info!("Created: {}", compose_yaml_path.display());

        // Generate and write Dockerfile
        let dockerfile = self.generate_dockerfile(stack)?;
        let dockerfile_path = devcontainer_dir.join("Dockerfile");
        fs::write(&dockerfile_path, dockerfile)?;
        tracing::info!("Created: {}", dockerfile_path.display());

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

        self.ensure_onepassword_assets(&devcontainer_dir)?;

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

    fn ensure_onepassword_assets(&self, devcontainer_dir: &Path) -> Result<()> {
        use std::fs;

        let scripts_dir = devcontainer_dir.join("scripts");
        fs::create_dir_all(&scripts_dir)?;

        write_if_missing(
            &scripts_dir.join("init-host.sh"),
            &self.generate_onepassword_init_host_script()?,
            0o755,
        )?;
        write_if_missing(
            &scripts_dir.join("setup-git.sh"),
            &self.generate_onepassword_setup_git_script()?,
            0o755,
        )?;
        write_if_missing(
            &devcontainer_dir.join(".github-token.env"),
            &self.generate_onepassword_github_token_env()?,
            0o600,
        )?;
        write_if_missing(
            &devcontainer_dir.join(".git-signing-key"),
            &self.generate_onepassword_git_signing_key()?,
            0o600,
        )?;
        write_if_missing(
            &devcontainer_dir.join(".gitconfig.env"),
            &self.generate_onepassword_gitconfig_env()?,
            0o600,
        )?;

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

    /// Generate host-side 1Password secret bootstrap script.
    fn generate_onepassword_init_host_script(&self) -> Result<String> {
        templates::onepassword_init_host_script()
    }

    /// Generate container-side git/gh/signing setup script.
    fn generate_onepassword_setup_git_script(&self) -> Result<String> {
        templates::onepassword_setup_git_script()
    }

    /// Generate mounted GitHub token placeholder.
    fn generate_onepassword_github_token_env(&self) -> Result<String> {
        templates::onepassword_github_token_env()
    }

    /// Generate mounted SSH signing key placeholder.
    fn generate_onepassword_git_signing_key(&self) -> Result<String> {
        templates::onepassword_git_signing_key()
    }

    /// Generate mounted git identity placeholder.
    fn generate_onepassword_gitconfig_env(&self) -> Result<String> {
        templates::onepassword_gitconfig_env()
    }
}

fn write_if_missing(path: &Path, contents: &str, mode: u32) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(Error::validation(format!(
                "Refusing to write through symlink: {}",
                path.display()
            )));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                tracing::info!("Skipped (already exists): {}", path.display());
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };
        file.write_all(contents.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        let mut file = match OpenOptions::new().create_new(true).write(true).open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                tracing::info!("Skipped (already exists): {}", path.display());
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };
        file.write_all(contents.as_bytes())?;
    }

    tracing::info!("Created: {}", path.display());

    Ok(())
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
        assert!(temp_dir
            .path()
            .join(".devcontainer/scripts/init-host.sh")
            .exists());
        assert!(temp_dir
            .path()
            .join(".devcontainer/scripts/setup-git.sh")
            .exists());
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
        assert!(temp_dir.path().join(".env.sample").exists());

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

        let init_host_script =
            fs::read_to_string(temp_dir.path().join(".devcontainer/scripts/init-host.sh")).unwrap();
        assert!(init_host_script.contains("op read"));

        let setup_git_script =
            fs::read_to_string(temp_dir.path().join(".devcontainer/scripts/setup-git.sh")).unwrap();
        assert!(setup_git_script.contains("credential.https://github.com.helper"));
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

    #[cfg(unix)]
    #[test]
    fn test_write_if_missing_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let protected_file = temp_dir.path().join("protected.txt");
        fs::write(&protected_file, "original").unwrap();

        let linked = temp_dir.path().join("linked.txt");
        symlink(&protected_file, &linked).unwrap();

        let err = write_if_missing(&linked, "mutated", 0o644).unwrap_err();
        assert!(err.to_string().contains("symlink"));
        assert_eq!(fs::read_to_string(&protected_file).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn test_onepassword_asset_permissions_are_hardened() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::Rust).unwrap();

        let init_host_mode =
            fs::metadata(temp_dir.path().join(".devcontainer/scripts/init-host.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
        assert_eq!(init_host_mode, 0o755);

        let setup_git_mode =
            fs::metadata(temp_dir.path().join(".devcontainer/scripts/setup-git.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
        assert_eq!(setup_git_mode, 0o755);

        let github_token_mode =
            fs::metadata(temp_dir.path().join(".devcontainer/.github-token.env"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
        assert_eq!(github_token_mode, 0o600);

        let signing_key_mode = fs::metadata(temp_dir.path().join(".devcontainer/.git-signing-key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(signing_key_mode, 0o600);

        let gitconfig_mode = fs::metadata(temp_dir.path().join(".devcontainer/.gitconfig.env"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(gitconfig_mode, 0o600);
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
            assert!(
                temp_dir
                    .path()
                    .join(".devcontainer/scripts/init-host.sh")
                    .exists(),
                "{} stack should create init-host.sh",
                stack.as_str()
            );
            assert!(
                temp_dir
                    .path()
                    .join(".devcontainer/scripts/setup-git.sh")
                    .exists(),
                "{} stack should create setup-git.sh",
                stack.as_str()
            );
            assert!(
                temp_dir
                    .path()
                    .join(".devcontainer/.github-token.env")
                    .exists(),
                "{} stack should create .github-token.env",
                stack.as_str()
            );
        }
    }
}
