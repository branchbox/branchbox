//! Stack adapter system
//!
//! Auto-detects and configures for different stacks (Rails, Node.js, etc.)

use crate::{Error, Result};
use std::path::Path;

pub mod generic;
pub mod nodejs;
pub mod rails;

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
///
/// Tries all adapters and returns the one with highest confidence.
/// Generic adapter always matches with confidence 10, so this never fails.
///
/// # Example
///
/// ```no_run
/// use worktree_core::adapters::detect_adapter;
/// use std::path::Path;
///
/// let project = Path::new("/path/to/project");
/// let adapter = detect_adapter(project).unwrap();
/// println!("Detected stack: {}", adapter.name());
/// ```
pub fn detect_adapter(project_dir: &Path) -> Result<Box<dyn Adapter>> {
    let adapters: Vec<Box<dyn Adapter>> = vec![
        Box::new(rails::RailsAdapter),
        Box::new(nodejs::NodeJsAdapter),
        Box::new(generic::GenericAdapter),
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

    // Generic adapter always has confidence 10, so we should always have a match
    best_adapter.ok_or(Error::AdapterNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_detect_rails_adapter() {
        let temp_dir = TempDir::new().unwrap();
        let mut gemfile = fs::File::create(temp_dir.path().join("Gemfile")).unwrap();
        writeln!(gemfile, "gem \"rails\"").unwrap();

        let adapter = detect_adapter(temp_dir.path()).unwrap();
        assert_eq!(adapter.name(), "Rails");
    }

    #[test]
    fn test_detect_nodejs_adapter() {
        let temp_dir = TempDir::new().unwrap();
        fs::File::create(temp_dir.path().join("package.json")).unwrap();

        let adapter = detect_adapter(temp_dir.path()).unwrap();
        assert_eq!(adapter.name(), "Node.js");
    }

    #[test]
    fn test_detect_generic_adapter() {
        let temp_dir = TempDir::new().unwrap();
        // No stack-specific files

        let adapter = detect_adapter(temp_dir.path()).unwrap();
        assert_eq!(adapter.name(), "Generic");
    }

    #[test]
    fn test_rails_beats_generic() {
        let temp_dir = TempDir::new().unwrap();
        let mut gemfile = fs::File::create(temp_dir.path().join("Gemfile")).unwrap();
        writeln!(gemfile, "gem \"rails\"").unwrap();

        let adapter = detect_adapter(temp_dir.path()).unwrap();
        // Rails confidence (95) should beat Generic confidence (10)
        assert_eq!(adapter.name(), "Rails");
    }

    #[test]
    fn test_adapter_default_service_url() {
        struct TestAdapter;
        impl Adapter for TestAdapter {
            fn name(&self) -> &str {
                "Test"
            }
            fn detect(&self, _: &Path) -> u8 {
                0
            }
            fn copy_secrets(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
        }

        let adapter = TestAdapter;
        assert_eq!(adapter.service_url(), "http://localhost:3000");
    }

    #[test]
    fn test_adapter_default_setup_database() {
        struct TestAdapter;
        impl Adapter for TestAdapter {
            fn name(&self) -> &str {
                "Test"
            }
            fn detect(&self, _: &Path) -> u8 {
                0
            }
            fn copy_secrets(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
        }

        let adapter = TestAdapter;
        let temp_dir = TempDir::new().unwrap();
        assert!(adapter.setup_database(temp_dir.path()).is_ok());
    }

    #[test]
    fn test_adapter_default_healthcheck() {
        struct TestAdapter;
        impl Adapter for TestAdapter {
            fn name(&self) -> &str {
                "Test"
            }
            fn detect(&self, _: &Path) -> u8 {
                0
            }
            fn copy_secrets(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
        }

        let adapter = TestAdapter;
        let temp_dir = TempDir::new().unwrap();
        assert!(adapter.healthcheck(temp_dir.path()).is_ok());
    }

    #[test]
    fn test_adapter_default_cleanup() {
        struct TestAdapter;
        impl Adapter for TestAdapter {
            fn name(&self) -> &str {
                "Test"
            }
            fn detect(&self, _: &Path) -> u8 {
                0
            }
            fn copy_secrets(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
        }

        let adapter = TestAdapter;
        let temp_dir = TempDir::new().unwrap();
        assert!(adapter.cleanup(temp_dir.path()).is_ok());
    }
}
