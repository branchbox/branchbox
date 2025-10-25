//! Generic fallback adapter

use super::Adapter;
use crate::Result;
use std::fs;
use std::path::Path;

/// Generic adapter for unknown project types
pub struct GenericAdapter;

impl Adapter for GenericAdapter {
    fn name(&self) -> &str {
        "Generic"
    }

    fn detect(&self, _project_dir: &Path) -> u8 {
        // Always match with lowest confidence
        // This is the fallback when no other adapter matches
        10
    }

    fn service_url(&self) -> String {
        "http://dev:3000".to_string()
    }

    fn copy_secrets(&self, src: &Path, dest: &Path) -> Result<()> {
        // Copy common secret files
        let secret_files = vec![".env", ".env.local", ".env.development", ".secrets"];

        let mut copied = false;

        for file in secret_files {
            let src_path = src.join(file);
            if src_path.exists() && src_path.is_file() {
                fs::copy(&src_path, dest.join(file))?;
                tracing::info!("Copied {}", file);
                copied = true;
            }
        }

        if !copied {
            tracing::info!("No secret files found to copy");
        }

        Ok(())
    }

    fn cleanup(&self, worktree_dir: &Path) -> Result<()> {
        // Clean up common temporary files and directories
        let paths_to_clean = vec![".cache", "tmp", "temp", ".tmp"];

        for path in paths_to_clean {
            let full_path = worktree_dir.join(path);
            if full_path.exists() && full_path.is_dir() {
                let _ = fs::remove_dir_all(&full_path);
                tracing::debug!("Cleaned up {}", path);
            }
        }

        tracing::info!("Cleaned up temporary files");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_always_detects() {
        let temp_dir = TempDir::new().unwrap();

        let adapter = GenericAdapter;
        assert_eq!(adapter.detect(temp_dir.path()), 10);
    }

    #[test]
    fn test_service_url() {
        let adapter = GenericAdapter;
        assert_eq!(adapter.service_url(), "http://dev:3000");
    }

    #[test]
    fn test_name() {
        let adapter = GenericAdapter;
        assert_eq!(adapter.name(), "Generic");
    }

    #[test]
    fn test_copy_secrets() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        // Create secret files
        fs::write(src_dir.path().join(".env"), "SECRET=value").unwrap();
        fs::write(src_dir.path().join(".secrets"), "API_KEY=123").unwrap();
        fs::write(src_dir.path().join("other.txt"), "not a secret").unwrap();

        let adapter = GenericAdapter;
        adapter.copy_secrets(src_dir.path(), dest_dir.path()).unwrap();

        // Verify secret files were copied
        assert!(dest_dir.path().join(".env").exists());
        assert!(dest_dir.path().join(".secrets").exists());
        assert!(!dest_dir.path().join("other.txt").exists());
    }

    #[test]
    fn test_copy_secrets_none() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let adapter = GenericAdapter;
        // Should not error if no secrets
        adapter.copy_secrets(src_dir.path(), dest_dir.path()).unwrap();
    }

    #[test]
    fn test_cleanup() {
        let worktree_dir = TempDir::new().unwrap();

        // Create temp directories
        fs::create_dir_all(worktree_dir.path().join(".cache")).unwrap();
        fs::create_dir_all(worktree_dir.path().join("tmp")).unwrap();
        fs::create_dir_all(worktree_dir.path().join(".tmp")).unwrap();
        fs::write(worktree_dir.path().join("tmp/data"), "temp data").unwrap();

        let adapter = GenericAdapter;
        adapter.cleanup(worktree_dir.path()).unwrap();

        // Verify cleanup
        assert!(!worktree_dir.path().join(".cache").exists());
        assert!(!worktree_dir.path().join("tmp").exists());
        assert!(!worktree_dir.path().join(".tmp").exists());
    }

    #[test]
    fn test_cleanup_none() {
        let worktree_dir = TempDir::new().unwrap();

        let adapter = GenericAdapter;
        // Should not error if nothing to clean
        adapter.cleanup(worktree_dir.path()).unwrap();
    }
}
