//! Modular devcontainer configuration system
//!
//! This module provides a structured, type-safe way to build and serialize
//! devcontainer configurations. Instead of static templates, it uses composable
//! components that can be dynamically configured based on project detection
//! and user input.
//!
//! # Architecture
//!
//! - `config` - Core configuration types (DevcontainerConfig, ComposeConfig, etc.)
//! - `detector` - Project detection for versions and settings (Ruby, Node, project name)
//! - `builder` - Builder pattern for constructing configurations
//! - `presets` - Stack-specific presets (Rails, Node.js, Rust, Generic)
//!
//! # Example
//!
//! ```no_run
//! use worktree_core::devcontainer::{DevcontainerConfig, ProjectDetector, StackPreset};
//! use std::path::Path;
//!
//! let project_path = Path::new("/path/to/rails-app");
//! let detector = ProjectDetector::new(project_path);
//!
//! // Create config from preset with detected values
//! let mut config = DevcontainerConfig::from_preset(StackPreset::Rails);
//! config.name = detector.project_name().unwrap_or_else(|| "My App".to_string());
//!
//! // Serialize to JSON
//! let json = config.to_json_pretty()?;
//! # Ok::<(), worktree_core::Error>(())
//! ```

pub mod builder;
pub mod config;
pub mod detector;
pub mod presets;

pub use builder::DevcontainerBuilder;
pub use config::{
    ComposeConfig, ComposeService, ContainerEnv, DatabaseConfig, DevcontainerConfig,
    DockerfileConfig, Feature, PortConfig, RunArg, VolumeMount, VscodeCustomizations,
    VscodeSettings,
};
pub use detector::{DetectedProject, ProjectDetector};
pub use presets::StackPreset;
