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
use std::collections::HashSet;
use std::path::Path;

pub mod compose;
pub mod database;
pub mod specs;
pub mod tunnel;

pub use compose::ComposeModule;
pub use database::{DatabaseEngine, DatabaseModule};
pub use specs::{SpecStatus, SpecsModule};
pub use tunnel::TunnelModule;

/// Detected module with dependency metadata.
pub struct ModuleHandle {
    pub name: String,
    pub dependencies: Vec<String>,
    pub module: Box<dyn Module>,
}

impl ModuleHandle {
    fn new(module: Box<dyn Module>) -> Self {
        let name = module.name().to_string();
        let dependencies = module
            .dependencies()
            .iter()
            .map(|value| value.to_string())
            .collect();

        Self {
            name,
            dependencies,
            module,
        }
    }
}

/// Ordered plan of modules plus dependency warnings.
pub struct ModulePlan {
    pub handles: Vec<ModuleHandle>,
    pub warnings: Vec<String>,
}

impl ModulePlan {
    pub fn new(handles: Vec<ModuleHandle>, warnings: Vec<String>) -> Self {
        Self { handles, warnings }
    }
}

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

    /// Declares module dependencies (other module names that must execute before this module).
    fn dependencies(&self) -> &[&str] {
        &[]
    }
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
/// Returns a plan containing ordered modules and dependency warnings.
///
/// # Arguments
///
/// * `project_dir` - Project directory to detect modules in
/// * `skip_modules` - Optional list of module names to skip
pub fn detect_modules(project_dir: &Path, skip_modules: &[String]) -> ModulePlan {
    let skip_set: HashSet<String> = skip_modules.iter().cloned().collect();

    let mut detected: Vec<ModuleHandle> = all_modules()
        .into_iter()
        .filter(|module| {
            let name = module.name();
            !skip_set.contains(name) && module.detect(project_dir)
        })
        .map(ModuleHandle::new)
        .collect();

    let mut ordered = Vec::with_capacity(detected.len());
    let mut satisfied = HashSet::new();
    let mut warnings = Vec::new();

    while !detected.is_empty() {
        let mut progress = false;
        let mut idx = 0;

        while idx < detected.len() {
            let unresolved: Vec<&String> = detected[idx]
                .dependencies
                .iter()
                .filter(|dep| !satisfied.contains(dep.as_str()))
                .collect();

            if unresolved.is_empty() {
                let handle = detected.remove(idx);
                satisfied.insert(handle.name.clone());
                ordered.push(handle);
                progress = true;
            } else {
                let missing: Vec<&String> = unresolved
                    .into_iter()
                    .filter(|dep| !detected.iter().any(|handle| &handle.name == *dep))
                    .collect();

                if !missing.is_empty() {
                    let missing_list = missing
                        .iter()
                        .map(|dep| dep.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    warnings.push(format!(
                        "Module '{}' depends on missing module(s): {}",
                        detected[idx].name, missing_list
                    ));
                    let handle = detected.remove(idx);
                    satisfied.insert(handle.name.clone());
                    ordered.push(handle);
                    progress = true;
                } else {
                    idx += 1;
                }
            }
        }

        if !progress {
            // Circular dependency detected. Log warning and append remaining modules in original order.
            let names: Vec<String> = detected.iter().map(|handle| handle.name.clone()).collect();
            if !names.is_empty() {
                warnings.push(format!(
                    "Detected circular module dependency involving: {}",
                    names.join(", ")
                ));
            }
            ordered.append(&mut detected);
            break;
        }
    }

    ModulePlan::new(ordered, warnings)
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
        let plan = detect_modules(temp_dir.path(), &[]);

        // Tunnel module is always enabled for manual setup
        assert!(!plan.handles.is_empty());
        assert!(plan.warnings.is_empty());
        assert!(plan.handles.iter().any(|h| h.name == "tunnel"));
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

        let plan = detect_modules(temp_dir.path(), &[]);
        let names: Vec<&str> = plan.handles.iter().map(|m| m.name.as_str()).collect();

        assert!(names.contains(&"compose"));
    }

    #[test]
    fn test_detect_modules_with_database() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("config")).unwrap();
        std::fs::write(temp_dir.path().join("config/database.yml"), "test").unwrap();

        let plan = detect_modules(temp_dir.path(), &[]);
        let names: Vec<&str> = plan.handles.iter().map(|m| m.name.as_str()).collect();

        assert!(names.contains(&"database"));
    }

    #[test]
    fn test_skip_modules() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".devcontainer")).unwrap();
        std::fs::write(
            temp_dir.path().join(".devcontainer/compose.yaml"),
            "version: '3'",
        )
        .unwrap();

        // Test skipping compose module
        let plan = detect_modules(temp_dir.path(), &[String::from("compose")]);
        let names: Vec<&str> = plan.handles.iter().map(|m| m.name.as_str()).collect();

        assert!(!names.contains(&"compose"));
    }

    #[test]
    fn test_module_plan_new() {
        let plan = ModulePlan::new(vec![], vec!["test warning".to_string()]);
        assert_eq!(plan.warnings.len(), 1);
        assert_eq!(plan.warnings[0], "test warning");
        assert_eq!(plan.handles.len(), 0);
    }

    #[test]
    fn test_module_default_dependencies() {
        struct TestModule;
        impl Module for TestModule {
            fn name(&self) -> &str {
                "test"
            }
            fn detect(&self, _: &Path) -> bool {
                true
            }
            fn init(&mut self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn setup(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn teardown(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn validate(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
        }

        let module = TestModule;
        let deps: &[&str] = module.dependencies();
        assert_eq!(deps.len(), 0);
    }

    #[test]
    fn test_module_handle_new() {
        struct TestModule;
        impl Module for TestModule {
            fn name(&self) -> &str {
                "test"
            }
            fn detect(&self, _: &Path) -> bool {
                true
            }
            fn init(&mut self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn setup(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn teardown(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn validate(&self, _: &Path, _: &Path) -> Result<()> {
                Ok(())
            }
            fn dependencies(&self) -> &[&str] {
                &["other"]
            }
        }

        let handle = ModuleHandle::new(Box::new(TestModule));
        assert_eq!(handle.name, "test");
        assert_eq!(handle.dependencies, vec!["other"]);
    }
}
