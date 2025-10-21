//! Module system
//!
//! Composable feature components (tunnel, database, compose, specs)
//!
//! Modules provide optional features that can be enabled per worktree:
//! - **Compose**: Docker Compose configuration management
//! - **Database**: Database isolation and setup
//! - **Tunnel**: Cloudflare tunnel provisioning
//! - **Specs**: Feature specification lifecycle tracking

use crate::Result;
use std::path::Path;

pub mod compose;
pub mod database;
pub mod specs;
pub mod tunnel;

pub use compose::ComposeModule;
pub use database::{DatabaseEngine, DatabaseModule};
pub use specs::{SpecStatus, SpecsModule};
pub use tunnel::TunnelModule;

/// Module trait
///
/// Modules implement the feature lifecycle:
/// 1. `detect()` - Check if module should be enabled
/// 2. `init()` - Initialize module configuration
/// 3. `setup()` - Setup resources during feature-start
/// 4. `validate()` - Validate configuration
/// 5. `teardown()` - Cleanup during feature-teardown
pub trait Module {
    /// Module name
    fn name(&self) -> &str;

    /// Detect if this module should be enabled
    fn detect(&self, project_dir: &Path) -> bool;

    /// Initialize module configuration
    fn init(&mut self, main_dir: &Path, feature_dir: &Path) -> Result<()>;

    /// Setup resources during feature-start
    fn setup(&self, main_dir: &Path, feature_dir: &Path) -> Result<()>;

    /// Cleanup resources during feature-teardown
    fn teardown(&self, main_dir: &Path, feature_dir: &Path) -> Result<()>;

    /// Validate module configuration
    fn validate(&self, main_dir: &Path, feature_dir: &Path) -> Result<()>;
}

/// Get all available modules
///
/// Returns a vector of boxed modules that can be used to
/// detect and configure features for a worktree.
pub fn all_modules() -> Vec<Box<dyn Module>> {
    vec![
        Box::new(ComposeModule::new()),
        Box::new(DatabaseModule::new()),
        Box::new(TunnelModule::new()),
        Box::new(SpecsModule::new()),
    ]
}

/// Detect and initialize enabled modules
///
/// Returns a vector of modules that are enabled for the project.
pub fn detect_modules(project_dir: &Path) -> Vec<Box<dyn Module>> {
    all_modules()
        .into_iter()
        .filter(|m| m.detect(project_dir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_all_modules() {
        let modules = all_modules();
        assert_eq!(modules.len(), 4);

        let names: Vec<&str> = modules.iter().map(|m| m.name()).collect();
        assert!(names.contains(&"compose"));
        assert!(names.contains(&"database"));
        assert!(names.contains(&"tunnel"));
        assert!(names.contains(&"specs"));
    }

    #[test]
    fn test_detect_modules_empty_project() {
        let temp_dir = TempDir::new().unwrap();
        let modules = detect_modules(temp_dir.path());

        // Tunnel module is always enabled for manual setup
        assert!(!modules.is_empty());
    }

    #[test]
    fn test_detect_modules_with_compose() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "version: '3'",
        )
        .unwrap();

        let modules = detect_modules(temp_dir.path());
        let names: Vec<&str> = modules.iter().map(|m| m.name()).collect();

        assert!(names.contains(&"compose"));
    }

    #[test]
    fn test_detect_modules_with_database() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("config")).unwrap();
        std::fs::write(temp_dir.path().join("config/database.yml"), "test").unwrap();

        let modules = detect_modules(temp_dir.path());
        let names: Vec<&str> = modules.iter().map(|m| m.name()).collect();

        assert!(names.contains(&"database"));
    }
}
