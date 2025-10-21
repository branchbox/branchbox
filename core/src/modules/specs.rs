//! Feature Specs Module
//!
//! Manages feature specification lifecycle tracking:
//! - Feature spec discovery and search
//! - Feature spec creation
//! - Feature spec lifecycle management (backlog → in-progress → completed)
//! - Feature spec frontmatter updating

use super::Module;
use crate::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Feature specification status
#[derive(Debug, Clone, PartialEq)]
pub enum SpecStatus {
    Backlog,
    InProgress,
    Completed,
}

impl SpecStatus {
    fn as_str(&self) -> &str {
        match self {
            SpecStatus::Backlog => "backlog",
            SpecStatus::InProgress => "in-progress",
            SpecStatus::Completed => "completed",
        }
    }
}

/// Feature Specs module
pub struct SpecsModule {
    enabled: bool,
    specs_dir: PathBuf,
    spec_file: Option<PathBuf>,
    feature_title: String,
    work_feature: String,
}

impl SpecsModule {
    /// Create a new Specs module
    pub fn new() -> Self {
        Self {
            enabled: false,
            specs_dir: PathBuf::new(),
            spec_file: None,
            feature_title: String::new(),
            work_feature: String::new(),
        }
    }

    /// Create directory structure for specs
    fn create_directory_structure(&self) -> Result<()> {
        for status in &[
            SpecStatus::Backlog,
            SpecStatus::InProgress,
            SpecStatus::Completed,
        ] {
            let dir = self.specs_dir.join(status.as_str());
            if !dir.exists() {
                fs::create_dir_all(&dir)?;
                tracing::info!("Created {} directory", status.as_str());
            }
        }
        Ok(())
    }

    /// Move spec file to a different status directory
    fn move_spec(&mut self, from_status: SpecStatus, to_status: SpecStatus) -> Result<()> {
        if let Some(spec_file) = &self.spec_file {
            let spec_basename = spec_file
                .file_name()
                .ok_or_else(|| Error::validation("Invalid spec filename".to_string()))?;

            let from_path = self.specs_dir.join(from_status.as_str()).join(spec_basename);
            let to_path = self.specs_dir.join(to_status.as_str()).join(spec_basename);

            if from_path.exists() {
                fs::rename(&from_path, &to_path)?;
                self.spec_file = Some(to_path.clone());
                tracing::info!(
                    "Moved spec from {} to {}",
                    from_status.as_str(),
                    to_status.as_str()
                );
            }
        }
        Ok(())
    }

    /// Update spec frontmatter with worktree information
    fn update_spec_frontmatter(&self, spec_file: &Path, feature_dir: &Path) -> Result<()> {
        if !spec_file.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(spec_file)?;

        // Simple frontmatter update - look for status field
        // In a real implementation, this would use a proper YAML parser
        let updated = if content.starts_with("---") {
            // Has frontmatter, update it
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            if parts.len() >= 3 {
                let frontmatter = parts[1];
                let body = parts[2];

                let mut updated_frontmatter = String::new();
                let mut status_updated = false;

                for line in frontmatter.lines() {
                    if line.trim().starts_with("status:") {
                        updated_frontmatter.push_str("status: in-progress\n");
                        status_updated = true;
                    } else if line.trim().starts_with("worktree:") {
                        updated_frontmatter.push_str(&format!(
                            "worktree: {}\n",
                            feature_dir.display()
                        ));
                    } else {
                        updated_frontmatter.push_str(line);
                        updated_frontmatter.push('\n');
                    }
                }

                if !status_updated {
                    updated_frontmatter.push_str("status: in-progress\n");
                }

                format!("---\n{}---{}", updated_frontmatter, body)
            } else {
                content
            }
        } else {
            // No frontmatter, add it
            format!(
                "---\nstatus: in-progress\nworktree: {}\n---\n\n{}",
                feature_dir.display(),
                content
            )
        };

        fs::write(spec_file, updated)?;
        tracing::info!("Updated spec frontmatter");
        Ok(())
    }
}

impl Default for SpecsModule {
    fn default() -> Self {
        Self::new()
    }
}

impl Module for SpecsModule {
    fn name(&self) -> &str {
        "specs"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        // Check if specs directory exists or FEATURES_DIR is set
        let features_dir = std::env::var("FEATURES_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| project_dir.join("docs/features"));

        features_dir.exists() || std::env::var("FEATURES_DIR").is_ok()
    }

    fn init(&mut self, main_dir: &Path, feature_dir: &Path) -> Result<()> {
        // Set specs directory
        self.specs_dir = if let Ok(features_dir) = std::env::var("FEATURES_DIR") {
            PathBuf::from(features_dir)
        } else {
            main_dir.join("docs/features")
        };

        self.work_feature = feature_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid feature directory name".to_string()))?
            .to_string();

        // Create directory structure
        self.create_directory_structure()?;

        tracing::info!("Initialized specs directory: {:?}", self.specs_dir);
        self.enabled = true;
        Ok(())
    }

    fn setup(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Setting up feature specification...");

        if self.spec_file.is_none() {
            tracing::warn!("No spec file set, skipping setup");
            return Ok(());
        }

        // Move spec to in-progress if not already there
        if let Some(spec_file) = &self.spec_file {
            let spec_basename = spec_file
                .file_name()
                .ok_or_else(|| Error::validation("Invalid spec filename".to_string()))?;

            let in_progress_path = self
                .specs_dir
                .join(SpecStatus::InProgress.as_str())
                .join(spec_basename);

            // Check if spec is in backlog
            let backlog_path = self
                .specs_dir
                .join(SpecStatus::Backlog.as_str())
                .join(spec_basename);

            if backlog_path.exists() && !in_progress_path.exists() {
                fs::rename(&backlog_path, &in_progress_path)?;
                tracing::info!("Moved spec to in-progress");
            }

            // Update frontmatter
            if in_progress_path.exists() {
                self.update_spec_frontmatter(&in_progress_path, feature_dir)?;
                tracing::info!("Feature spec ready: {:?}", in_progress_path);
            }
        }

        Ok(())
    }

    fn teardown(&self, _main_dir: &Path, feature_dir: &Path) -> Result<()> {
        tracing::info!("Handling feature specification...");

        let work_feature = feature_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::validation("Invalid feature directory name".to_string()))?;

        let spec_file = self
            .specs_dir
            .join(SpecStatus::InProgress.as_str())
            .join(format!("{}.md", work_feature));

        if !spec_file.exists() {
            tracing::info!("No feature spec found");
            return Ok(());
        }

        // In the Rust version, we'll just log what would happen
        // In a real implementation, this would prompt the user
        tracing::info!("Feature spec found: {:?}", spec_file);
        tracing::info!("Spec remains in in-progress (use --complete flag to move to completed)");

        Ok(())
    }

    fn validate(&self, _main_dir: &Path, _feature_dir: &Path) -> Result<()> {
        // Validate that specs directory exists
        if !self.specs_dir.exists() {
            return Err(Error::validation(format!(
                "Specs directory not found: {:?}",
                self.specs_dir
            )));
        }

        // Validate directory structure
        for status in &[
            SpecStatus::Backlog,
            SpecStatus::InProgress,
            SpecStatus::Completed,
        ] {
            let dir = self.specs_dir.join(status.as_str());
            if !dir.exists() {
                tracing::warn!("Missing directory: {:?}", dir);
                fs::create_dir_all(&dir)?;
                tracing::info!("Created {} directory", status.as_str());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_detect() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("docs/features")).unwrap();

        let module = SpecsModule::new();
        assert!(module.detect(temp_dir.path()));
    }

    #[test]
    fn test_init() {
        let main_dir = TempDir::new().unwrap();
        let feature_dir = main_dir.path().join("feature-test");
        std::fs::create_dir(&feature_dir).unwrap();

        let mut module = SpecsModule::new();
        module.init(main_dir.path(), &feature_dir).unwrap();

        assert!(module.enabled);
        assert!(module.specs_dir.join("backlog").exists());
        assert!(module.specs_dir.join("in-progress").exists());
        assert!(module.specs_dir.join("completed").exists());
    }

    #[test]
    fn test_create_directory_structure() {
        let temp_dir = TempDir::new().unwrap();
        let specs_dir = temp_dir.path().join("specs");

        let module = SpecsModule {
            enabled: true,
            specs_dir: specs_dir.clone(),
            spec_file: None,
            feature_title: String::new(),
            work_feature: String::new(),
        };

        module.create_directory_structure().unwrap();

        assert!(specs_dir.join("backlog").exists());
        assert!(specs_dir.join("in-progress").exists());
        assert!(specs_dir.join("completed").exists());
    }

    #[test]
    fn test_update_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let spec_file = temp_dir.path().join("test.md");

        std::fs::write(&spec_file, "# Test Feature\n\nSome content").unwrap();

        let module = SpecsModule::new();
        module
            .update_spec_frontmatter(&spec_file, temp_dir.path())
            .unwrap();

        let content = std::fs::read_to_string(&spec_file).unwrap();
        assert!(content.contains("status: in-progress"));
        assert!(content.contains("worktree:"));
    }
}
