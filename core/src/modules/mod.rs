//! Module system
//!
//! Composable feature components (tunnel, database, compose, specs)

use crate::Result;
use std::path::Path;

// pub mod tunnel;
// pub mod database;
// pub mod compose;
// pub mod specs;

/// Module trait
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
