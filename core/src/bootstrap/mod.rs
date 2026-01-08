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
//! - Python projects
//! - Generic projects
//!
//! # Usage
//!
//! ```no_run
//! use worktree_core::bootstrap::{Bootstrap, Stack, BootstrapOptions};
//! use std::path::Path;
//!
//! let project_path = Path::new("/path/to/project");
//! let bootstrap = Bootstrap::new(project_path);
//!
//! // Auto-detect stack and generate with auto-detected settings
//! let stack = bootstrap.detect_stack().unwrap();
//! bootstrap.generate(stack).unwrap();
//!
//! // Or generate with custom options
//! let options = BootstrapOptions {
//!     project_name: Some("my-app".to_string()),
//!     ruby_version: Some("3.2".to_string()),
//!     ..Default::default()
//! };
//! bootstrap.generate_with_options(stack, options).unwrap();
//! ```
//!
//! # What It Generates
//!
//! - `.devcontainer/devcontainer.json` - VS Code/Cursor configuration
//! - `.devcontainer/compose.yaml` - Docker Compose services
//! - `.devcontainer/Dockerfile` - Custom development image
//! - `.env.sample` - Environment variable template
//! - Stack-specific scripts and configurations

use crate::devcontainer::{DevcontainerBuilder, ProjectDetector, StackPreset};
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
    /// Python / Django / Flask
    Python,
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
            Stack::Python => "python",
            Stack::Generic => "generic",
        }
    }

    /// Convert to devcontainer StackPreset
    pub fn to_preset(&self) -> StackPreset {
        match self {
            Stack::Rails => StackPreset::Rails,
            Stack::NodeJs => StackPreset::NodeJs,
            Stack::Rust => StackPreset::Rust,
            Stack::Python => StackPreset::Python,
            Stack::Generic => StackPreset::Generic,
        }
    }
}

/// Options for customizing bootstrap generation
#[derive(Debug, Clone, Default)]
pub struct BootstrapOptions {
    /// Custom project name (detected from repo if not set)
    pub project_name: Option<String>,

    /// Ruby version (for Rails projects)
    pub ruby_version: Option<String>,

    /// Node.js version
    pub node_version: Option<String>,

    /// Python version (for Python projects)
    pub python_version: Option<String>,

    /// Application port
    pub port: Option<u16>,

    /// Include database configuration
    pub with_database: bool,

    /// Enable AI coding agent mounts
    pub coding_agents: bool,
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
    /// Uses project marker files to detect the project type.
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

        // Check for Python project
        if self.project_path.join("pyproject.toml").exists()
            || self.project_path.join("requirements.txt").exists()
            || self.project_path.join("setup.py").exists()
        {
            return Ok(Stack::Python);
        }

        // Fallback to generic
        Ok(Stack::Generic)
    }

    /// Get a project detector for this project path
    pub fn detector(&self) -> ProjectDetector {
        ProjectDetector::new(&self.project_path)
    }

    /// Generate devcontainer configuration with auto-detected settings
    ///
    /// Creates all necessary files for a complete devcontainer setup,
    /// automatically detecting project name, versions, and other settings.
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
        // Use default options with auto-detection
        self.generate_with_options(stack, BootstrapOptions::default())
    }

    /// Generate devcontainer configuration with custom options
    ///
    /// Creates all necessary files for a complete devcontainer setup,
    /// using the provided options to customize generation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use worktree_core::bootstrap::{Bootstrap, Stack, BootstrapOptions};
    /// use std::path::Path;
    ///
    /// let project = Path::new("/path/to/rails-project");
    /// let bootstrap = Bootstrap::new(project);
    /// let options = BootstrapOptions {
    ///     project_name: Some("my-awesome-app".to_string()),
    ///     ruby_version: Some("3.2".to_string()),
    ///     ..Default::default()
    /// };
    /// bootstrap.generate_with_options(Stack::Rails, options).unwrap();
    /// ```
    pub fn generate_with_options(&self, stack: Stack, options: BootstrapOptions) -> Result<()> {
        use std::fs;

        tracing::info!("Generating devcontainer for {} stack", stack.as_str());

        // Build configuration using the new modular system
        let mut builder = DevcontainerBuilder::new(&self.project_path)
            .with_preset(stack.to_preset())
            .auto_detect();

        // Apply custom options
        if let Some(name) = options.project_name {
            builder = builder.with_project_name(name);
        }
        if let Some(version) = options.ruby_version {
            builder = builder.with_ruby_version(version);
        }
        if let Some(version) = options.node_version {
            builder = builder.with_node_version(version);
        }
        if let Some(version) = options.python_version {
            builder = builder.with_python_version(version);
        }
        if let Some(port) = options.port {
            builder = builder.with_port(port);
        }
        if options.with_database {
            builder = builder.with_database(true);
        }
        builder = builder.with_coding_agents(options.coding_agents);

        // Build and write the configuration
        let output = builder.build()?;
        let write_result = output.write_to(&self.project_path)?;

        for file in &write_result.files {
            tracing::info!("Created: {}", file.display());
        }
        if write_result.env_sample_skipped {
            tracing::info!(
                "Skipped (already exists): {}",
                self.project_path.join(".env.sample").display()
            );
        }

        // Generate the BranchBox env overrides placeholder (lives under .devcontainer)
        let devcontainer_dir = self.project_path.join(".devcontainer");
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

    /// Generate devcontainer.json using the legacy template system
    ///
    /// This method is kept for backward compatibility. New code should use
    /// `generate()` or `generate_with_options()` instead.
    #[deprecated(since = "0.6.0", note = "Use generate() or generate_with_options() instead")]
    pub fn generate_devcontainer_json(&self, stack: Stack) -> Result<String> {
        templates::devcontainer_json(stack)
    }

    /// Generate compose.yaml using the legacy template system
    #[deprecated(since = "0.6.0", note = "Use generate() or generate_with_options() instead")]
    pub fn generate_compose_yaml(&self, stack: Stack) -> Result<String> {
        templates::compose_yaml(stack)
    }

    /// Generate Dockerfile using the legacy template system
    #[deprecated(since = "0.6.0", note = "Use generate() or generate_with_options() instead")]
    pub fn generate_dockerfile(&self, stack: Stack) -> Result<String> {
        templates::dockerfile(stack)
    }

    /// Generate .env.sample using the legacy template system
    #[deprecated(since = "0.6.0", note = "Use generate() or generate_with_options() instead")]
    pub fn generate_env_sample(&self, stack: Stack) -> Result<String> {
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
        // Create a Cargo.toml so the project name is detected
        fs::write(
            temp_dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-rust-project\"",
        )
        .unwrap();

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

        // Check that files have content - the new system generates dynamic project names
        let devcontainer_json =
            fs::read_to_string(temp_dir.path().join(".devcontainer/devcontainer.json")).unwrap();
        // The name should contain the project name (from Cargo.toml or directory)
        assert!(
            devcontainer_json.contains("Project")
                || devcontainer_json.contains("my-rust-project"),
            "devcontainer.json should contain project name: {}",
            devcontainer_json
        );

        let compose_yaml =
            fs::read_to_string(temp_dir.path().join(".devcontainer/compose.yaml")).unwrap();
        assert!(
            compose_yaml.contains("rust-dev"),
            "compose.yaml should contain rust-dev service"
        );

        let dockerfile =
            fs::read_to_string(temp_dir.path().join(".devcontainer/Dockerfile")).unwrap();
        assert!(
            dockerfile.to_lowercase().contains("rust"),
            "Dockerfile should reference rust"
        );

        let branchbox_env =
            fs::read_to_string(temp_dir.path().join(".devcontainer/.branchbox.env")).unwrap();
        assert!(branchbox_env.contains("WORK_FEATURE=main"));
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
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Python, Stack::Generic] {
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

    #[test]
    fn test_detect_python_stack() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        assert_eq!(bootstrap.detect_stack().unwrap(), Stack::Python);
    }

    #[test]
    fn test_detect_python_from_requirements() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("requirements.txt"), "flask>=2.0").unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        assert_eq!(bootstrap.detect_stack().unwrap(), Stack::Python);
    }

    #[test]
    fn test_generate_with_options() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join(".ruby-version"), "3.2.2").unwrap();
        fs::write(temp_dir.path().join("Gemfile"), "gem 'rails'").unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        let options = BootstrapOptions {
            project_name: Some("my-custom-app".to_string()),
            ruby_version: Some("3.2".to_string()),
            ..Default::default()
        };

        bootstrap
            .generate_with_options(Stack::Rails, options)
            .unwrap();

        // Check that files were created
        assert!(temp_dir
            .path()
            .join(".devcontainer/devcontainer.json")
            .exists());

        // Check that custom project name is used
        let devcontainer_json =
            fs::read_to_string(temp_dir.path().join(".devcontainer/devcontainer.json")).unwrap();
        assert!(
            devcontainer_json.contains("My-custom-app"),
            "Should contain custom project name"
        );

        // Check that Ruby version is used in Dockerfile
        let dockerfile =
            fs::read_to_string(temp_dir.path().join(".devcontainer/Dockerfile")).unwrap();
        assert!(
            dockerfile.contains("3.2"),
            "Dockerfile should contain Ruby version 3.2"
        );
    }

    #[test]
    fn test_auto_detect_ruby_version() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join(".ruby-version"), "3.1.4").unwrap();
        fs::write(temp_dir.path().join("Gemfile"), "gem 'rails'").unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::Rails).unwrap();

        // Check that auto-detected Ruby version is used
        let dockerfile =
            fs::read_to_string(temp_dir.path().join(".devcontainer/Dockerfile")).unwrap();
        assert!(
            dockerfile.contains("3.1"),
            "Dockerfile should contain auto-detected Ruby version 3.1"
        );
    }

    #[test]
    fn test_auto_detect_project_name() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("package.json"),
            r#"{"name": "my-node-app"}"#,
        )
        .unwrap();

        let bootstrap = Bootstrap::new(temp_dir.path());
        bootstrap.generate(Stack::NodeJs).unwrap();

        // Check that auto-detected project name is used
        let devcontainer_json =
            fs::read_to_string(temp_dir.path().join(".devcontainer/devcontainer.json")).unwrap();
        assert!(
            devcontainer_json.contains("My-node-app")
                || devcontainer_json.contains("my-node-app"),
            "Should contain auto-detected project name"
        );
    }

    #[test]
    fn test_stack_to_preset_conversion() {
        assert_eq!(Stack::Rails.to_preset(), StackPreset::Rails);
        assert_eq!(Stack::NodeJs.to_preset(), StackPreset::NodeJs);
        assert_eq!(Stack::Rust.to_preset(), StackPreset::Rust);
        assert_eq!(Stack::Python.to_preset(), StackPreset::Python);
        assert_eq!(Stack::Generic.to_preset(), StackPreset::Generic);
    }
}
