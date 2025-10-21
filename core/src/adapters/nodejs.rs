//! Node.js stack adapter

use super::Adapter;
use crate::Result;
use std::fs;
use std::path::Path;

/// Node.js stack adapter
pub struct NodeJsAdapter;

impl Adapter for NodeJsAdapter {
    fn name(&self) -> &str {
        "Node.js"
    }

    fn detect(&self, project_dir: &Path) -> u8 {
        let package_json = project_dir.join("package.json");

        if package_json.exists() {
            return 90;
        }

        // Check for yarn.lock or package-lock.json
        if project_dir.join("yarn.lock").exists() || project_dir.join("package-lock.json").exists() {
            return 80;
        }

        0
    }

    fn service_url(&self) -> String {
        "http://nodejs-app:3000".to_string()
    }

    fn copy_secrets(&self, src: &Path, dest: &Path) -> Result<()> {
        let mut copied = false;

        // Copy .env files
        for env_file in &[".env", ".env.local", ".env.development"] {
            let src_path = src.join(env_file);
            if src_path.exists() {
                let dest_path = dest.join(env_file);
                fs::copy(&src_path, &dest_path)?;
                tracing::info!("Copied {}", env_file);
                copied = true;
            }
        }

        // Copy .npmrc if it exists (private registry credentials)
        let npmrc_src = src.join(".npmrc");
        if npmrc_src.exists() {
            fs::copy(&npmrc_src, dest.join(".npmrc"))?;
            tracing::info!("Copied .npmrc");
            copied = true;
        }

        if !copied {
            tracing::info!("No Node.js secret files found to copy");
        }

        Ok(())
    }

    fn cleanup(&self, worktree_dir: &Path) -> Result<()> {
        // Remove node_modules and build artifacts
        let paths_to_clean = vec![
            "node_modules/.cache",
            ".next/cache",
            ".nuxt",
            "dist",
            "build",
        ];

        for path in paths_to_clean {
            let full_path = worktree_dir.join(path);
            if full_path.exists() {
                let _ = fs::remove_dir_all(&full_path);
                tracing::debug!("Cleaned up {}", path);
            }
        }

        tracing::info!("Cleaned up Node.js temporary files");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_detect_nodejs() {
        let temp_dir = TempDir::new().unwrap();
        let mut package_json = fs::File::create(temp_dir.path().join("package.json")).unwrap();
        writeln!(package_json, "{{}}").unwrap();

        let adapter = NodeJsAdapter;
        assert_eq!(adapter.detect(temp_dir.path()), 90);
    }

    #[test]
    fn test_detect_with_lock_file() {
        let temp_dir = TempDir::new().unwrap();
        fs::File::create(temp_dir.path().join("package-lock.json")).unwrap();

        let adapter = NodeJsAdapter;
        assert_eq!(adapter.detect(temp_dir.path()), 80);
    }

    #[test]
    fn test_detect_not_nodejs() {
        let temp_dir = TempDir::new().unwrap();

        let adapter = NodeJsAdapter;
        assert_eq!(adapter.detect(temp_dir.path()), 0);
    }
}
