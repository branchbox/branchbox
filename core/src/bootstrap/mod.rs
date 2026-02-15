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

        // Generate 1Password integration scripts
        let init_host = self.generate_init_host_sh()?;
        let init_host_path = devcontainer_dir.join("init-host.sh");
        fs::write(&init_host_path, init_host)?;
        Self::set_permissions_unix(&init_host_path, 0o755)?;
        tracing::info!("Created: {}", init_host_path.display());

        let setup_git = self.generate_setup_git_sh()?;
        let setup_git_path = devcontainer_dir.join("setup-git.sh");
        fs::write(&setup_git_path, setup_git)?;
        Self::set_permissions_unix(&setup_git_path, 0o755)?;
        tracing::info!("Created: {}", setup_git_path.display());

        // Create empty secret files so compose volume mounts don't fail
        let secret_files = [".github-token.env", ".git-signing-key", ".gitconfig.env"];
        for secret_file in &secret_files {
            let secret_path = devcontainer_dir.join(secret_file);
            if !secret_path.exists() {
                fs::write(&secret_path, "")?;
                Self::set_permissions_unix(&secret_path, 0o600)?;
                tracing::debug!("Created placeholder: {}", secret_path.display());
            }
        }

        // Ensure secret files are excluded from version control in the target project
        let gitignore_path = self.project_path.join(".gitignore");
        let mut gitignore_content = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path)?
        } else {
            String::new()
        };

        let mut gitignore_updated = false;
        let existing_lines: std::collections::HashSet<&str> = gitignore_content
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .collect();
        for secret_file in &secret_files {
            let entry = format!(".devcontainer/{}", secret_file);
            if !existing_lines.contains(entry.as_str()) {
                if !gitignore_content.is_empty() && !gitignore_content.ends_with('\n') {
                    gitignore_content.push('\n');
                }
                if !gitignore_updated {
                    gitignore_content
                        .push_str("\n# 1Password secrets (populated by init-host.sh, never committed)\n");
                }
                gitignore_content.push_str(&entry);
                gitignore_content.push('\n');
                gitignore_updated = true;
            }
        }

        if gitignore_updated {
            fs::write(&gitignore_path, &gitignore_content)?;
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

    /// Set file permissions on Unix systems (no-op on other platforms)
    #[cfg(unix)]
    fn set_permissions_unix(path: &std::path::Path, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn set_permissions_unix(_path: &std::path::Path, _mode: u32) -> Result<()> {
        Ok(())
    }

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
