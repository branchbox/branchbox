//! Stack adapter system
//!
//! Auto-detects and configures for different stacks (Rails, Node.js, etc.)

use crate::{Error, Result};
use std::path::Path;

pub mod rails;
// pub mod nodejs;
// pub mod generic;

/// Stack adapter trait
pub trait Adapter {
    /// Adapter name (e.g., "Rails", "Node.js", "Generic")
    fn name(&self) -> &str;

    /// Detect if this adapter should be used for the project
    ///
    /// Returns confidence level (0-100)
    fn detect(&self, project_dir: &Path) -> u8;

    /// Get service URL for Cloudflare tunnel ingress
    fn service_url(&self) -> String {
        "http://localhost:3000".to_string()
    }

    /// Copy stack-specific secret files to worktree
    fn copy_secrets(&self, src: &Path, dest: &Path) -> Result<()>;

    /// Setup database (runs inside devcontainer)
    fn setup_database(&self, _worktree_dir: &Path) -> Result<()> {
        // Default: no database setup
        Ok(())
    }

    /// Health check
    fn healthcheck(&self, _worktree_dir: &Path) -> Result<()> {
        // Default: always healthy
        Ok(())
    }

    /// Cleanup temporary files
    fn cleanup(&self, _worktree_dir: &Path) -> Result<()> {
        // Default: no cleanup
        Ok(())
    }
}

/// Detect and load the best adapter for a project
pub fn detect_adapter(project_dir: &Path) -> Result<Box<dyn Adapter>> {
    let adapters: Vec<Box<dyn Adapter>> = vec![
        Box::new(rails::RailsAdapter),
        // Box::new(nodejs::NodejsAdapter),
        // Box::new(generic::GenericAdapter),
    ];

    let mut best_adapter: Option<Box<dyn Adapter>> = None;
    let mut best_confidence = 0;

    for adapter in adapters {
        let confidence = adapter.detect(project_dir);

        if confidence > best_confidence {
            best_confidence = confidence;
            best_adapter = Some(adapter);
        }
    }

    best_adapter.ok_or(Error::AdapterNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_detect_adapter() {
        // TODO: Add tests with temp directories
    }
}
