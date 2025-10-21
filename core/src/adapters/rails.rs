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
}
