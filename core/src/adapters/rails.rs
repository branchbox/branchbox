//! Rails stack adapter

use super::Adapter;
use crate::Result;
use std::fs;
use std::path::Path;

/// Rails stack adapter
pub struct RailsAdapter;

impl Adapter for RailsAdapter {
    fn name(&self) -> &str {
        "Rails"
    }

    fn detect(&self, project_dir: &Path) -> u8 {
        let gemfile = project_dir.join("Gemfile");

        if !gemfile.exists() {
            return 0;
        }

        // Read Gemfile and look for 'gem "rails"' or "gem 'rails'"
        if let Ok(content) = fs::read_to_string(&gemfile) {
            if content.contains("gem \"rails\"") || content.contains("gem 'rails'") {
                return 95;
            }
        }

        0
    }

    fn service_url(&self) -> String {
        "http://rails-app:3000".to_string()
    }

    fn copy_secrets(&self, src: &Path, dest: &Path) -> Result<()> {
        let mut copied = false;

        // Copy config/master.key
        let master_key_src = src.join("config/master.key");
        if master_key_src.exists() {
            let master_key_dest = dest.join("config/master.key");
            fs::create_dir_all(master_key_dest.parent().unwrap())?;
            fs::copy(&master_key_src, &master_key_dest)?;
            tracing::info!("Copied config/master.key");
            copied = true;
        }

        // Copy config/credentials/*.key
        let credentials_dir = src.join("config/credentials");
        if credentials_dir.exists() && credentials_dir.is_dir() {
            let dest_credentials_dir = dest.join("config/credentials");
            fs::create_dir_all(&dest_credentials_dir)?;

            for entry in fs::read_dir(&credentials_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().and_then(|s| s.to_str()) == Some("key") {
                    let filename = path.file_name().unwrap();
                    let dest_path = dest_credentials_dir.join(filename);
                    fs::copy(&path, &dest_path)?;
                    tracing::info!("Copied {:?}", filename);
                    copied = true;
                }
            }
        }

        if !copied {
            tracing::info!("No Rails secret files found to copy");
        }

        Ok(())
    }

    fn cleanup(&self, worktree_dir: &Path) -> Result<()> {
        // Remove Rails temporary files
        let tmp_dir = worktree_dir.join("tmp");

        if tmp_dir.exists() {
            for subdir in &["cache", "pids", "sockets"] {
                let path = tmp_dir.join(subdir);
                if path.exists() {
                    let _ = fs::remove_dir_all(&path);
                }
            }
            tracing::info!("Cleaned up Rails tmp/ directory");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_detect_rails() {
        let temp_dir = TempDir::new().unwrap();
        let mut gemfile = fs::File::create(temp_dir.path().join("Gemfile")).unwrap();
        writeln!(gemfile, "gem \"rails\"").unwrap();

        let adapter = RailsAdapter;
        assert_eq!(adapter.detect(temp_dir.path()), 95);
    }

    #[test]
    fn test_detect_not_rails() {
        let temp_dir = TempDir::new().unwrap();

        let adapter = RailsAdapter;
        assert_eq!(adapter.detect(temp_dir.path()), 0);
    }

    #[test]
    fn test_service_url() {
        let adapter = RailsAdapter;
        assert_eq!(adapter.service_url(), "http://rails-app:3000");
    }

    #[test]
    fn test_name() {
        let adapter = RailsAdapter;
        assert_eq!(adapter.name(), "Rails");
    }

    #[test]
    fn test_copy_secrets_master_key() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        // Create config/master.key in source
        let config_dir = src_dir.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("master.key"), "test_master_key_content").unwrap();

        let adapter = RailsAdapter;
        adapter
            .copy_secrets(src_dir.path(), dest_dir.path())
            .unwrap();

        // Verify master.key was copied
        let dest_master_key = dest_dir.path().join("config/master.key");
        assert!(dest_master_key.exists());
        assert_eq!(
            fs::read_to_string(dest_master_key).unwrap(),
            "test_master_key_content"
        );
    }

    #[test]
    fn test_copy_secrets_credentials_keys() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        // Create config/credentials/*.key files
        let credentials_dir = src_dir.path().join("config/credentials");
        fs::create_dir_all(&credentials_dir).unwrap();
        fs::write(credentials_dir.join("production.key"), "prod_key").unwrap();
        fs::write(credentials_dir.join("staging.key"), "staging_key").unwrap();
        fs::write(credentials_dir.join("development.yml.enc"), "not_a_key").unwrap();

        let adapter = RailsAdapter;
        adapter
            .copy_secrets(src_dir.path(), dest_dir.path())
            .unwrap();

        // Verify only .key files were copied
        let dest_credentials = dest_dir.path().join("config/credentials");
        assert!(dest_credentials.join("production.key").exists());
        assert!(dest_credentials.join("staging.key").exists());
        assert!(!dest_credentials.join("development.yml.enc").exists());
    }

    #[test]
    fn test_copy_secrets_no_secrets() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();

        let adapter = RailsAdapter;
        // Should not error even if no secrets exist
        adapter
            .copy_secrets(src_dir.path(), dest_dir.path())
            .unwrap();
    }

    #[test]
    fn test_cleanup() {
        let worktree_dir = TempDir::new().unwrap();

        // Create tmp/ subdirectories
        let tmp_dir = worktree_dir.path().join("tmp");
        fs::create_dir_all(tmp_dir.join("cache")).unwrap();
        fs::create_dir_all(tmp_dir.join("pids")).unwrap();
        fs::create_dir_all(tmp_dir.join("sockets")).unwrap();
        fs::write(tmp_dir.join("cache/test.cache"), "data").unwrap();
        fs::write(tmp_dir.join("pids/server.pid"), "1234").unwrap();

        let adapter = RailsAdapter;
        adapter.cleanup(worktree_dir.path()).unwrap();

        // Verify tmp subdirs were removed
        assert!(!tmp_dir.join("cache").exists());
        assert!(!tmp_dir.join("pids").exists());
        assert!(!tmp_dir.join("sockets").exists());
    }

    #[test]
    fn test_cleanup_no_tmp_dir() {
        let worktree_dir = TempDir::new().unwrap();

        let adapter = RailsAdapter;
        // Should not error if tmp/ doesn't exist
        adapter.cleanup(worktree_dir.path()).unwrap();
    }
}
