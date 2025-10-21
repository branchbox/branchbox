//! Bootstrap system for generating devcontainer configurations
//!
//! This module implements the "meta" feature - a tool that can set up development
//! environments for any project, including itself.
//!
//! # Concept
//!
//! The worktree-manager can bootstrap complete devcontainer setups for:
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

use crate::{Error, Result};
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
    pub fn generate(&self, stack: Stack) -> Result<()> {
        // TODO: Implement generation
        let _ = stack;
        Err(Error::other("Bootstrap generation not yet implemented"))
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
}
