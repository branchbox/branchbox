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
}
