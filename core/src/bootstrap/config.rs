//! Modular devcontainer configuration system
//!
//! This module provides composable configuration structures that can be:
//! - Loaded from existing devcontainer.json files
//! - Customized during interactive setup
//! - Serialized to devcontainer.json/compose.yaml
//!
//! # Architecture
//!
//! The configuration is split into composable components:
//! - `DevcontainerConfig` - Main devcontainer.json structure
//! - `ComposeConfig` - Docker Compose configuration
//! - `DockerfileConfig` - Dockerfile generation settings
//! - `StackPreset` - Stack-specific defaults (Rails, Node.js, etc.)
//!
//! # Example
//!
//! ```no_run
//! use worktree_core::bootstrap::config::{DevcontainerConfig, StackPreset};
//!
//! // Create config from preset
//! let config = DevcontainerConfig::from_preset(StackPreset::Rails, "my-app");
//!
//! // Customize it
//! let config = config
//!     .with_ruby_version("3.3.0")
//!     .with_forward_port(3000);
//!
//! // Serialize to JSON
//! let json = config.to_json_pretty().unwrap();
//! ```

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Main devcontainer.json configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevcontainerConfig {
    /// Display name for the container
    pub name: String,

    /// Docker Compose file path (relative to .devcontainer/)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_compose_file: Option<String>,

    /// Service name in the compose file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Workspace folder inside container
    pub workspace_folder: String,

    /// Workspace mount specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_mount: Option<String>,

    /// Container environment variables
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub container_env: BTreeMap<String, String>,

    /// Devcontainer features to install
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, FeatureOptions>,

    /// Ports to forward from the container
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward_ports: Vec<u16>,

    /// Command to run after container creation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_create_command: Option<String>,

    /// Additional docker run arguments
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_args: Vec<String>,

    /// VS Code / editor customizations
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customizations: Option<Customizations>,
}

/// Feature options (can be empty object or have configuration)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(untagged)]
pub enum FeatureOptions {
    #[default]
    Empty,
    Options(BTreeMap<String, serde_json::Value>),
}

impl From<()> for FeatureOptions {
    fn from(_: ()) -> Self {
        FeatureOptions::Empty
    }
}

impl From<BTreeMap<String, serde_json::Value>> for FeatureOptions {
    fn from(opts: BTreeMap<String, serde_json::Value>) -> Self {
        if opts.is_empty() {
            FeatureOptions::Empty
        } else {
            FeatureOptions::Options(opts)
        }
    }
}

/// VS Code customizations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Customizations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vscode: Option<VsCodeCustomizations>,
}

/// VS Code specific settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VsCodeCustomizations {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, serde_json::Value>,
}

/// Docker Compose service configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComposeConfig {
    /// Compose project name (used as container prefix)
    pub name: String,

    /// Main development service
    pub service: ServiceConfig,

    /// Additional services (postgres, redis, etc.)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_services: Vec<(String, ServiceConfig)>,

    /// Named volumes
    #[serde(default)]
    pub volumes: Vec<String>,
}

/// Individual service configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceConfig {
    /// Service name
    pub name: String,

    /// Container image
    pub image: String,

    /// Build context (if building locally)
    pub build_context: Option<String>,

    /// Dockerfile path
    pub dockerfile: Option<String>,

    /// Pull policy for the image
    pub pull_policy: String,

    /// Run in privileged mode
    pub privileged: bool,

    /// Use init process
    pub init: bool,

    /// IPC mode
    pub ipc: String,

    /// Container name
    pub container_name: String,

    /// Volume mounts
    pub volumes: Vec<String>,

    /// Environment files
    pub env_files: Vec<EnvFile>,

    /// Environment variables
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,

    /// Command to run
    pub command: Option<String>,

    /// Service dependencies
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
}

/// Environment file configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvFile {
    pub path: String,
    pub required: bool,
}

/// Dockerfile generation configuration
#[derive(Debug, Clone, PartialEq)]
pub struct DockerfileConfig {
    /// Base image
    pub base_image: String,

    /// System packages to install
    pub packages: Vec<String>,

    /// Whether to install mise version manager
    pub install_mise: bool,

    /// Runtime versions to pre-install (e.g., ruby@3.3)
    pub runtime_versions: Vec<String>,

    /// Default runtime version (e.g., ruby@3.3)
    pub default_runtime: Option<String>,

    /// Additional setup commands
    pub setup_commands: Vec<String>,

    /// Container user (typically vscode)
    pub user: String,
}

/// Stack-specific presets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackPreset {
    Rails,
    NodeJs,
    Rust,
    Generic,
}

impl std::fmt::Display for StackPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StackPreset::Rails => write!(f, "rails"),
            StackPreset::NodeJs => write!(f, "nodejs"),
            StackPreset::Rust => write!(f, "rust"),
            StackPreset::Generic => write!(f, "generic"),
        }
    }
}

/// Project information detected from the repository
#[derive(Debug, Clone, Default)]
pub struct ProjectInfo {
    /// Project name (from repo or directory name)
    pub name: String,

    /// Ruby version (from .ruby-version)
    pub ruby_version: Option<String>,

    /// Node.js version (from .nvmrc, .node-version, package.json)
    pub node_version: Option<String>,

    /// Python version (from .python-version)
    pub python_version: Option<String>,

    /// Default port for the application
    pub port: Option<u16>,

    /// Database type detected (postgres, mysql, sqlite)
    pub database: Option<String>,
}

impl ProjectInfo {
    /// Detect project information from a directory
    pub fn detect(path: &Path) -> Self {
        // Detect project name from directory first
        let mut name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        // Try to get from git remote (more accurate)
        if let Ok(output) = std::process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(path)
            .output()
        {
            if output.status.success() {
                let url = String::from_utf8_lossy(&output.stdout);
                if let Some(repo_name) = extract_repo_name(&url) {
                    name = repo_name;
                }
            }
        }

        ProjectInfo {
            name,
            ruby_version: detect_ruby_version(path),
            node_version: detect_node_version(path),
            python_version: detect_python_version(path),
            database: detect_database(path),
            port: detect_port(path),
        }
    }

    /// Get display name (dasherized, lowercase)
    pub fn display_name(&self) -> String {
        dasherize(&self.name)
    }
}

/// Extract repository name from git URL
fn extract_repo_name(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.trim_end_matches('/').trim_end_matches(".git");

    // Handle SSH format: git@github.com:user/repo
    if url.contains(':') && url.starts_with("git@") {
        return url.rsplit(':').next()?.rsplit('/').next().map(String::from);
    }

    // Handle HTTPS format: https://github.com/user/repo
    url.rsplit('/').next().map(String::from)
}

/// Dasherize a string (convert spaces/underscores to dashes, lowercase)
fn dasherize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' | '_' => '-',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// Detect Ruby version from various sources
fn detect_ruby_version(path: &Path) -> Option<String> {
    // Check .ruby-version
    if let Ok(version) = std::fs::read_to_string(path.join(".ruby-version")) {
        let version = version.trim();
        if !version.is_empty() && !version.starts_with('#') {
            return Some(version.to_string());
        }
    }

    // Check .tool-versions
    if let Ok(content) = std::fs::read_to_string(path.join(".tool-versions")) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("ruby ") {
                return line.split_whitespace().nth(1).map(String::from);
            }
        }
    }

    // Check .mise.toml
    if let Ok(content) = std::fs::read_to_string(path.join(".mise.toml")) {
        // Simple TOML parsing for ruby version
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("ruby") && line.contains('=') {
                // Handle ruby = "3.3.0" or ruby = "3.3"
                if let Some(version) = line.split('=').nth(1) {
                    let version = version.trim().trim_matches('"').trim_matches('\'');
                    if !version.is_empty() {
                        return Some(version.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Detect Node.js version from various sources
fn detect_node_version(path: &Path) -> Option<String> {
    // Check .nvmrc
    if let Ok(version) = std::fs::read_to_string(path.join(".nvmrc")) {
        let version = version.trim();
        if !version.is_empty() {
            // Strip leading 'v' if present
            let version = version.strip_prefix('v').unwrap_or(version);
            return Some(version.to_string());
        }
    }

    // Check .node-version
    if let Ok(version) = std::fs::read_to_string(path.join(".node-version")) {
        let version = version.trim();
        if !version.is_empty() {
            let version = version.strip_prefix('v').unwrap_or(version);
            return Some(version.to_string());
        }
    }

    // Check .tool-versions
    if let Ok(content) = std::fs::read_to_string(path.join(".tool-versions")) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("nodejs ") || line.starts_with("node ") {
                return line.split_whitespace().nth(1).map(String::from);
            }
        }
    }

    // Check package.json engines.node
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(node) = json.get("engines").and_then(|e| e.get("node")) {
                if let Some(version) = node.as_str() {
                    // Handle semver ranges like ">=18.0.0" or "^20.0.0"
                    let version = version
                        .trim_start_matches(|c: char| !c.is_ascii_digit())
                        .split(|c: char| !c.is_ascii_digit() && c != '.')
                        .next()
                        .unwrap_or("20");
                    if !version.is_empty() {
                        return Some(version.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Detect Python version from various sources
fn detect_python_version(path: &Path) -> Option<String> {
    // Check .python-version
    if let Ok(version) = std::fs::read_to_string(path.join(".python-version")) {
        let version = version.trim();
        if !version.is_empty() && !version.starts_with('#') {
            return Some(version.to_string());
        }
    }

    // Check .tool-versions
    if let Ok(content) = std::fs::read_to_string(path.join(".tool-versions")) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("python ") {
                return line.split_whitespace().nth(1).map(String::from);
            }
        }
    }

    None
}

/// Detect database type from project files
fn detect_database(path: &Path) -> Option<String> {
    // Check Gemfile for Rails database
    if let Ok(content) = std::fs::read_to_string(path.join("Gemfile")) {
        if content.contains("gem \"pg\"") || content.contains("gem 'pg'") {
            return Some("postgres".to_string());
        }
        if content.contains("gem \"mysql2\"") || content.contains("gem 'mysql2'") {
            return Some("mysql".to_string());
        }
        if content.contains("gem \"sqlite3\"") || content.contains("gem 'sqlite3'") {
            return Some("sqlite".to_string());
        }
    }

    // Check package.json for Node.js database
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        if content.contains("\"pg\"") || content.contains("\"postgres\"") {
            return Some("postgres".to_string());
        }
        if content.contains("\"mysql\"") || content.contains("\"mysql2\"") {
            return Some("mysql".to_string());
        }
    }

    None
}

/// Detect default port from project files
fn detect_port(path: &Path) -> Option<u16> {
    // Check for Rails default
    if path.join("Gemfile").exists() {
        if let Ok(content) = std::fs::read_to_string(path.join("Gemfile")) {
            if content.contains("rails") {
                return Some(3000);
            }
        }
    }

    // Check package.json scripts for port hints
    if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // Check for common port patterns in scripts
            if let Some(scripts) = json.get("scripts") {
                let scripts_str = scripts.to_string();
                // Look for PORT=XXXX or --port XXXX patterns
                for port_str in ["3000", "5000", "8000", "8080", "4000"] {
                    if scripts_str.contains(&format!("PORT={}", port_str))
                        || scripts_str.contains(&format!("--port {}", port_str))
                        || scripts_str.contains(&format!("-p {}", port_str))
                    {
                        return port_str.parse().ok();
                    }
                }
            }
        }
        // Default for Node.js
        return Some(3000);
    }

    // Check for Flask/Django
    if path.join("requirements.txt").exists() || path.join("pyproject.toml").exists() {
        if let Ok(content) = std::fs::read_to_string(path.join("requirements.txt")) {
            if content.contains("flask") {
                return Some(5000);
            }
            if content.contains("django") {
                return Some(8000);
            }
        }
    }

    // Check for Rust gRPC default
    if path.join("Cargo.toml").exists() {
        return Some(50051);
    }

    None
}

impl DevcontainerConfig {
    /// Create a new devcontainer config from a stack preset
    pub fn from_preset(preset: StackPreset, project_name: &str) -> Self {
        let display_name = match preset {
            StackPreset::Rails => format!("{} (Rails)", titlecase(project_name)),
            StackPreset::NodeJs => format!("{} (Node.js)", titlecase(project_name)),
            StackPreset::Rust => format!("{} (Rust)", titlecase(project_name)),
            StackPreset::Generic => titlecase(project_name),
        };

        let service_name = match preset {
            StackPreset::Rails => "rails-app",
            StackPreset::NodeJs => "nodejs-app",
            StackPreset::Rust => "rust-dev",
            StackPreset::Generic => "dev",
        };

        let mut container_env = BTreeMap::new();
        container_env.insert("DOCKER_CONTAINER".to_string(), "true".to_string());

        // Add PATH for mise-based stacks
        match preset {
            StackPreset::Rails | StackPreset::NodeJs => {
                container_env.insert(
                    "PATH".to_string(),
                    "/home/vscode/.local/share/mise/shims:/home/vscode/.local/bin:${containerEnv:PATH}".to_string(),
                );
            }
            StackPreset::Rust => {
                container_env.insert(
                    "CARGO_HOME".to_string(),
                    "/workspaces/${localWorkspaceFolderBasename}/.cargo".to_string(),
                );
                container_env.insert("RUST_BACKTRACE".to_string(), "1".to_string());
            }
            StackPreset::Generic => {}
        }

        // Base features for all stacks
        let mut features = BTreeMap::new();
        features.insert(
            "ghcr.io/devcontainers/features/docker-in-docker:2".to_string(),
            feature_options([("moby", "false")]),
        );
        features.insert(
            "ghcr.io/devcontainers/features/github-cli:1".to_string(),
            FeatureOptions::Empty,
        );
        features.insert(
            "ghcr.io/rbarazi/devcontainer-features/ai-npm-packages:1".to_string(),
            FeatureOptions::Empty,
        );

        // Stack-specific features
        match preset {
            StackPreset::Rails => {
                features.insert(
                    "ghcr.io/devcontainers/features/node:1".to_string(),
                    feature_options([("version", "20")]),
                );
                features.insert(
                    "ghcr.io/rails/devcontainer/features/activestorage".to_string(),
                    FeatureOptions::Empty,
                );
            }
            StackPreset::Rust | StackPreset::Generic => {
                features.insert(
                    "ghcr.io/devcontainers/features/node:1".to_string(),
                    feature_options([("version", "20")]),
                );
            }
            StackPreset::NodeJs => {}
        }

        let forward_ports = match preset {
            StackPreset::Rails | StackPreset::NodeJs => vec![3000],
            StackPreset::Rust => vec![50051],
            StackPreset::Generic => vec![],
        };

        let post_create_command = match preset {
            StackPreset::Rails => {
                Some("mise trust --all && mise install && mise exec -- bin/setup".to_string())
            }
            StackPreset::NodeJs => {
                Some("mise trust --all && mise install && mise exec -- npm install".to_string())
            }
            StackPreset::Rust => Some("cargo --version && rustc --version".to_string()),
            StackPreset::Generic => None,
        };

        let mut run_args = vec!["--init".to_string()];
        match preset {
            StackPreset::Rails | StackPreset::NodeJs => {
                run_args = vec![
                    "--env-file".to_string(),
                    "../.env".to_string(),
                    "--ipc=host".to_string(),
                    "--shm-size=1gb".to_string(),
                    "--init".to_string(),
                ];
            }
            StackPreset::Rust => {
                run_args.insert(0, "--ipc=host".to_string());
            }
            StackPreset::Generic => {}
        }

        let customizations = match preset {
            StackPreset::Rust => Some(rust_customizations()),
            _ => None,
        };

        DevcontainerConfig {
            name: display_name,
            docker_compose_file: Some("compose.yaml".to_string()),
            service: Some(service_name.to_string()),
            workspace_folder: "/workspaces/${localWorkspaceFolderBasename}".to_string(),
            workspace_mount: Some(
                "source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename},type=bind,consistency=cached".to_string()
            ),
            container_env,
            features,
            forward_ports,
            post_create_command,
            run_args,
            customizations,
        }
    }

    /// Create config from project info with auto-detection
    pub fn from_project_info(preset: StackPreset, info: &ProjectInfo) -> Self {
        let mut config = Self::from_preset(preset, &info.display_name());

        // Apply detected port
        if let Some(port) = info.port {
            if !config.forward_ports.contains(&port) {
                config.forward_ports = vec![port];
            }
        }

        config
    }

    /// Load from existing devcontainer.json file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: DevcontainerConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Serialize to JSON string
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Builder method to set project name
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// Builder method to add/update a forward port
    pub fn with_forward_port(mut self, port: u16) -> Self {
        if !self.forward_ports.contains(&port) {
            self.forward_ports.push(port);
        }
        self
    }

    /// Builder method to set forward ports
    pub fn with_forward_ports(mut self, ports: Vec<u16>) -> Self {
        self.forward_ports = ports;
        self
    }

    /// Builder method to set post create command
    pub fn with_post_create_command(mut self, cmd: &str) -> Self {
        self.post_create_command = Some(cmd.to_string());
        self
    }

    /// Builder method to add a feature
    pub fn with_feature(mut self, feature: &str, options: FeatureOptions) -> Self {
        self.features.insert(feature.to_string(), options);
        self
    }
}

impl ComposeConfig {
    /// Create a new compose config from a stack preset
    pub fn from_preset(preset: StackPreset, project_name: &str, info: &ProjectInfo) -> Self {
        let display_name = dasherize(project_name);
        let service_name = match preset {
            StackPreset::Rails => "rails-app",
            StackPreset::NodeJs => "nodejs-app",
            StackPreset::Rust => "rust-dev",
            StackPreset::Generic => "dev",
        };

        let image = format!(
            "${{DEVCONTAINER_IMAGE:-ghcr.io/branchbox/branchbox/devcontainer-{}:latest}}",
            preset
        );

        // Base volumes for all stacks
        let mut volumes = vec![
            "../..:/workspaces:cached".to_string(),
            "dind-data:/var/lib/docker".to_string(),
            // AI coding agent mounts
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/codex:/home/vscode/.codex".to_string(),
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/claude:/home/vscode/.claude".to_string(),
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/claude.json:/home/vscode/.claude.json"
                .to_string(),
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/gh:/home/vscode/.config/gh".to_string(),
        ];

        // Stack-specific volumes
        match preset {
            StackPreset::Rails | StackPreset::NodeJs => {
                volumes
                    .insert(2, "mise-cache:/home/vscode/.local/share/mise".to_string());
            }
            _ => {}
        }

        if preset == StackPreset::NodeJs {
            volumes.insert(3, "npm-cache:/home/vscode/.npm".to_string());
        }

        let mut environment = BTreeMap::new();
        if preset == StackPreset::Rails {
            environment.insert(
                "SOLID_QUEUE_IN_PUMA".to_string(),
                "${SOLID_QUEUE_IN_PUMA:-false}".to_string(),
            );
        }

        let service = ServiceConfig {
            name: service_name.to_string(),
            image,
            build_context: Some("..".to_string()),
            dockerfile: Some(".devcontainer/Dockerfile".to_string()),
            pull_policy: "${DEVCONTAINER_PULL_POLICY:-missing}".to_string(),
            privileged: true,
            init: true,
            ipc: "host".to_string(),
            container_name: format!("${{DEVCONTAINER_NAME:-{}}}", display_name),
            volumes,
            env_files: vec![
                EnvFile {
                    path: "../.env".to_string(),
                    required: false,
                },
                EnvFile {
                    path: ".branchbox.env".to_string(),
                    required: false,
                },
            ],
            environment,
            command: Some("sleep infinity".to_string()),
            depends_on: vec![],
        };

        // Named volumes
        let mut named_volumes = vec!["dind-data".to_string()];
        match preset {
            StackPreset::Rails => {
                named_volumes.push("mise-cache".to_string());
            }
            StackPreset::NodeJs => {
                named_volumes.push("mise-cache".to_string());
                named_volumes.push("npm-cache".to_string());
            }
            _ => {}
        }

        // Add postgres volume if detected
        let additional_services = if info.database.as_deref() == Some("postgres") {
            named_volumes.push("postgres-data".to_string());
            vec![] // We'll add commented service in YAML output
        } else {
            vec![]
        };

        ComposeConfig {
            name: format!("${{DEVCONTAINER_NAME:-{}}}", display_name),
            service,
            additional_services,
            volumes: named_volumes,
        }
    }

    /// Serialize to YAML string
    pub fn to_yaml(&self, preset: StackPreset, info: &ProjectInfo) -> Result<String> {
        let mut yaml = String::new();

        // Header comment
        yaml.push_str(&format!(
            "# BranchBox {} Devcontainer\n",
            titlecase(&preset.to_string())
        ));
        match preset {
            StackPreset::Rails => {
                yaml.push_str("# Works with SQLite, Postgres, or MySQL based projects\n");
            }
            StackPreset::NodeJs => {
                yaml.push_str("# Supports npm, yarn, and pnpm package managers\n");
            }
            StackPreset::Rust => {
                yaml.push_str("# Full Rust toolchain with cargo and rustc\n");
            }
            StackPreset::Generic => {
                yaml.push_str("# Generic development container\n");
            }
        }

        yaml.push_str(&format!("name: {}\n", self.name));
        yaml.push_str("services:\n");

        // Main service
        yaml.push_str(&format!("  {}:\n", self.service.name));
        yaml.push_str("    # Official BranchBox pre-built image (instant startup)\n");
        yaml.push_str(
            "    # Falls back to local build if unavailable or DEVCONTAINER_PULL_POLICY=build\n",
        );
        yaml.push_str(&format!("    image: {}\n", self.service.image));

        if self.service.build_context.is_some() {
            yaml.push_str("    build:\n");
            yaml.push_str(&format!(
                "      context: {}\n",
                self.service.build_context.as_ref().unwrap()
            ));
            yaml.push_str(&format!(
                "      dockerfile: {}\n",
                self.service.dockerfile.as_ref().unwrap()
            ));
        }

        yaml.push_str(&format!("    pull_policy: {}\n", self.service.pull_policy));
        yaml.push_str(&format!("    privileged: {}\n", self.service.privileged));
        yaml.push_str(&format!("    init: {}\n", self.service.init));
        yaml.push_str(&format!("    ipc: {}\n", self.service.ipc));
        yaml.push_str(&format!("    container_name: {}\n", self.service.container_name));

        yaml.push_str("    volumes:\n");
        for vol in &self.service.volumes {
            yaml.push_str(&format!("      - {}\n", vol));
        }

        if let Some(ref cmd) = self.service.command {
            yaml.push_str(&format!("    command: {}\n", cmd));
        }

        yaml.push_str("    env_file:\n");
        for ef in &self.service.env_files {
            yaml.push_str(&format!("      - path: {}\n", ef.path));
            yaml.push_str(&format!("        required: {}\n", ef.required));
        }

        if !self.service.environment.is_empty() {
            yaml.push_str("    environment:\n");
            for (key, value) in &self.service.environment {
                yaml.push_str(&format!(
                    "      # {}\n",
                    match key.as_str() {
                        "SOLID_QUEUE_IN_PUMA" =>
                            "Disable Solid Queue Puma plugin by default (enable if you have queue DB configured)",
                        _ => "",
                    }
                ));
                yaml.push_str(&format!("      {}: {}\n", key, value));
            }
        }

        // Commented postgres service for Rails
        if preset == StackPreset::Rails {
            yaml.push_str(
                "    # Uncomment to enable Postgres (for apps using pg gem instead of sqlite3)\n",
            );
            yaml.push_str("    # depends_on:\n");
            yaml.push_str("    #   - postgres\n");
            yaml.push('\n');
            yaml.push_str("  # Postgres service - uncomment if your app uses PostgreSQL\n");
            yaml.push_str("  # postgres:\n");
            yaml.push_str("  #   image: pgvector/pgvector:pg16\n");
            yaml.push_str("  #   restart: unless-stopped\n");
            yaml.push_str("  #   volumes:\n");
            yaml.push_str("  #     - postgres-data:/var/lib/postgresql/data\n");
            yaml.push_str("  #   environment:\n");
            yaml.push_str("  #     POSTGRES_USER: postgres\n");
            yaml.push_str("  #     POSTGRES_PASSWORD: postgres\n");
        }

        yaml.push_str("\nvolumes:\n");
        for vol in &self.volumes {
            yaml.push_str(&format!("  {}: null\n", vol));
        }

        // Add commented postgres volume for Rails
        if preset == StackPreset::Rails && info.database != Some("postgres".to_string()) {
            yaml.push_str("  # postgres-data: null\n");
        }

        Ok(yaml)
    }
}

impl DockerfileConfig {
    /// Create from preset with optional detected versions
    pub fn from_preset(preset: StackPreset, info: &ProjectInfo) -> Self {
        let base_image = "mcr.microsoft.com/devcontainers/base:debian".to_string();

        let (packages, install_mise, runtime_versions, default_runtime, setup_commands) =
            match preset {
                StackPreset::Rails => {
                    let ruby_version = info.ruby_version.clone().unwrap_or_else(|| "3.3".to_string());
                    (
                        vec![
                            "build-essential",
                            "git",
                            "curl",
                            "libssl-dev",
                            "libreadline-dev",
                            "zlib1g-dev",
                            "libyaml-dev",
                            "libffi-dev",
                            "libgmp-dev",
                            "libpq-dev",
                            "default-libmysqlclient-dev",
                            "sqlite3",
                            "libsqlite3-dev",
                            "imagemagick",
                            "libvips",
                            "libopenslide0",
                            "poppler-utils",
                        ]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                        true,
                        vec![
                            format!("ruby@{}", major_minor(&ruby_version)),
                            "ruby@3.4".to_string(),
                        ],
                        Some(format!("ruby@{}", major_minor(&ruby_version))),
                        vec!["# Install gum for modern Rails bin/setup scripts".to_string()],
                    )
                }
                StackPreset::NodeJs => {
                    let node_version = info.node_version.clone().unwrap_or_else(|| "20".to_string());
                    (
                        vec![
                            "build-essential",
                            "git",
                            "curl",
                            "python3",
                            "libvips",
                            "libvips-dev",
                        ]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                        true,
                        vec![format!("node@{}", major_version(&node_version))],
                        Some(format!("node@{}", major_version(&node_version))),
                        vec![],
                    )
                }
                StackPreset::Rust => (
                    vec!["build-essential", "git", "curl", "pkg-config", "libssl-dev"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    false,
                    vec![],
                    None,
                    vec![],
                ),
                StackPreset::Generic => (
                    vec!["build-essential", "git", "curl"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    false,
                    vec![],
                    None,
                    vec![],
                ),
            };

        DockerfileConfig {
            base_image,
            packages,
            install_mise,
            runtime_versions,
            default_runtime,
            setup_commands,
            user: "vscode".to_string(),
        }
    }

    /// Generate Dockerfile content
    pub fn to_dockerfile(&self, preset: StackPreset) -> String {
        let mut dockerfile = String::new();

        // Header
        dockerfile.push_str(&format!(
            "# BranchBox {} Devcontainer\n",
            titlecase(&preset.to_string())
        ));
        match preset {
            StackPreset::Rails => {
                dockerfile.push_str("# Supports any Ruby version via mise version manager\n\n");
            }
            StackPreset::NodeJs => {
                dockerfile.push_str("# Supports any Node.js version via mise version manager\n\n");
            }
            StackPreset::Rust => {
                dockerfile.push_str("# Full Rust development environment\n\n");
            }
            StackPreset::Generic => {
                dockerfile.push_str("# Generic development environment\n\n");
            }
        }

        dockerfile.push_str(&format!("FROM {}\n\n", self.base_image));
        dockerfile.push_str("USER root\n\n");

        // Install packages
        if !self.packages.is_empty() {
            dockerfile.push_str("# Install build dependencies\n");
            dockerfile.push_str("RUN apt-get update && apt-get install -y \\\n");
            for pkg in &self.packages {
                dockerfile.push_str(&format!("    {} \\\n", pkg));
            }
            dockerfile.push_str("    && rm -rf /var/lib/apt/lists/*\n\n");
        }

        // Install gum for Rails
        if preset == StackPreset::Rails {
            dockerfile.push_str("# Install gum (for prettier bin/setup output in modern Rails apps)\n");
            dockerfile.push_str("RUN mkdir -p /etc/apt/keyrings && \\\n");
            dockerfile.push_str("    curl -fsSL https://repo.charm.sh/apt/gpg.key | gpg --dearmor -o /etc/apt/keyrings/charm.gpg && \\\n");
            dockerfile.push_str("    echo \"deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *\" | tee /etc/apt/sources.list.d/charm.list && \\\n");
            dockerfile.push_str(
                "    apt-get update && apt-get install -y gum && \\\n",
            );
            dockerfile.push_str("    rm -rf /var/lib/apt/lists/*\n\n");
        }

        // Install mise
        if self.install_mise {
            dockerfile.push_str("# Install mise for version management\n");
            dockerfile.push_str("RUN curl https://mise.run | sh && \\\n");
            dockerfile.push_str("    /root/.local/bin/mise settings set experimental true && \\\n");
            dockerfile
                .push_str("    /root/.local/bin/mise settings set legacy_version_file true && \\\n");
            dockerfile.push_str(
                "    echo 'eval \"$(/root/.local/bin/mise activate bash)\"' >> /etc/bash.bashrc\n\n",
            );

            dockerfile.push_str(&format!("USER {}\n\n", self.user));

            dockerfile.push_str(&format!(
                "# Install mise for {} user\n",
                self.user
            ));
            dockerfile.push_str("RUN curl https://mise.run | sh && \\\n");
            dockerfile.push_str(&format!(
                "    /home/{}/.local/bin/mise settings set experimental true && \\\n",
                self.user
            ));
            dockerfile.push_str(&format!(
                "    /home/{}/.local/bin/mise settings set legacy_version_file true && \\\n",
                self.user
            ));
            dockerfile.push_str(&format!(
                "    echo 'eval \"$(/home/{}/.local/bin/mise activate bash)\"' >> ~/.bashrc && \\\n",
                self.user
            ));
            dockerfile.push_str(&format!(
                "    echo 'eval \"$(/home/{}/.local/bin/mise activate zsh)\"' >> ~/.zshrc\n\n",
                self.user
            ));

            // Pre-install runtime versions
            if !self.runtime_versions.is_empty() {
                dockerfile.push_str("# Pre-install runtime versions to speed up first run\n");
                dockerfile.push_str("# Projects can override via version files\n");
                let versions = self.runtime_versions.join(" ");
                dockerfile.push_str(&format!(
                    "RUN /home/{}/.local/bin/mise install {}",
                    self.user, versions
                ));

                if let Some(ref default) = self.default_runtime {
                    dockerfile.push_str(&format!(
                        " && \\\n    /home/{}/.local/bin/mise use --global {}",
                        self.user, default
                    ));
                }
                dockerfile.push_str("\n\n");
            }

            dockerfile.push_str(&format!(
                "ENV PATH=\"/home/{}/.local/bin:/home/{}/.local/share/mise/shims:$PATH\"\n",
                self.user, self.user
            ));
        } else {
            dockerfile.push_str(&format!("USER {}\n", self.user));
        }

        dockerfile
    }
}

/// Helper to create feature options from key-value pairs
fn feature_options<const N: usize>(pairs: [(&str, &str); N]) -> FeatureOptions {
    let mut map = BTreeMap::new();
    for (k, v) in pairs {
        map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    FeatureOptions::Options(map)
}

/// Convert string to title case
fn titlecase(s: &str) -> String {
    let s = s.replace(['-', '_'], " ");
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Get major.minor version from version string
fn major_minor(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    match parts.len() {
        0 => version.to_string(),
        1 => parts[0].to_string(),
        _ => format!("{}.{}", parts[0], parts[1]),
    }
}

/// Get major version from version string
fn major_version(version: &str) -> String {
    version
        .split('.')
        .next()
        .unwrap_or(version)
        .to_string()
}

/// Create Rust-specific VS Code customizations
fn rust_customizations() -> Customizations {
    let mut settings = BTreeMap::new();

    let mut rust_settings = serde_json::Map::new();
    rust_settings.insert(
        "editor.defaultFormatter".to_string(),
        serde_json::Value::String("rust-lang.rust-analyzer".to_string()),
    );
    rust_settings.insert(
        "editor.formatOnSave".to_string(),
        serde_json::Value::Bool(true),
    );
    settings.insert(
        "[rust]".to_string(),
        serde_json::Value::Object(rust_settings),
    );

    let mut check_settings = serde_json::Map::new();
    check_settings.insert(
        "command".to_string(),
        serde_json::Value::String("clippy".to_string()),
    );
    settings.insert(
        "rust-analyzer.check".to_string(),
        serde_json::Value::Object(check_settings),
    );

    settings.insert(
        "rust-analyzer.checkOnSave".to_string(),
        serde_json::Value::Bool(true),
    );

    Customizations {
        vscode: Some(VsCodeCustomizations {
            extensions: vec![
                "rust-lang.rust-analyzer".to_string(),
                "tamasfe.even-better-toml".to_string(),
                "serayuzgur.crates".to_string(),
                "vadimcn.vscode-lldb".to_string(),
            ],
            settings,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_project_info_detect_name_from_directory() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        let info = ProjectInfo::detect(path);
        // Should use directory name
        assert!(!info.name.is_empty());
    }

    #[test]
    fn test_detect_ruby_version_from_file() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join(".ruby-version"), "3.3.0\n").unwrap();

        let version = detect_ruby_version(temp_dir.path());
        assert_eq!(version, Some("3.3.0".to_string()));
    }

    #[test]
    fn test_detect_ruby_version_from_tool_versions() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join(".tool-versions"), "ruby 3.2.2\nnode 20.0.0\n")
            .unwrap();

        let version = detect_ruby_version(temp_dir.path());
        assert_eq!(version, Some("3.2.2".to_string()));
    }

    #[test]
    fn test_detect_node_version_from_nvmrc() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join(".nvmrc"), "v20.10.0\n").unwrap();

        let version = detect_node_version(temp_dir.path());
        assert_eq!(version, Some("20.10.0".to_string()));
    }

    #[test]
    fn test_detect_database_postgres() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(
            temp_dir.path().join("Gemfile"),
            "source 'https://rubygems.org'\ngem 'rails'\ngem 'pg'\n",
        )
        .unwrap();

        let db = detect_database(temp_dir.path());
        assert_eq!(db, Some("postgres".to_string()));
    }

    #[test]
    fn test_devcontainer_config_from_preset_rails() {
        let config = DevcontainerConfig::from_preset(StackPreset::Rails, "my-app");

        assert!(config.name.contains("My App"));
        assert!(config.name.contains("Rails"));
        assert_eq!(config.service, Some("rails-app".to_string()));
        assert!(config.forward_ports.contains(&3000));
        assert!(config.features.contains_key(
            "ghcr.io/rails/devcontainer/features/activestorage"
        ));
    }

    #[test]
    fn test_devcontainer_config_from_preset_nodejs() {
        let config = DevcontainerConfig::from_preset(StackPreset::NodeJs, "my-api");

        assert!(config.name.contains("Node.js"));
        assert_eq!(config.service, Some("nodejs-app".to_string()));
        assert!(config.forward_ports.contains(&3000));
    }

    #[test]
    fn test_devcontainer_config_from_preset_rust() {
        let config = DevcontainerConfig::from_preset(StackPreset::Rust, "my-service");

        assert!(config.name.contains("Rust"));
        assert_eq!(config.service, Some("rust-dev".to_string()));
        assert!(config.forward_ports.contains(&50051));
        assert!(config.customizations.is_some());
    }

    #[test]
    fn test_devcontainer_config_serialization() {
        let config = DevcontainerConfig::from_preset(StackPreset::Generic, "test");
        let json = config.to_json_pretty().unwrap();

        assert!(json.contains("\"name\""));
        assert!(json.contains("\"workspaceFolder\""));
        assert!(json.contains("${localWorkspaceFolderBasename}"));
    }

    #[test]
    fn test_compose_config_from_preset() {
        let info = ProjectInfo::default();
        let config = ComposeConfig::from_preset(StackPreset::Rails, "my-rails-app", &info);

        assert!(config.name.contains("my-rails-app"));
        assert_eq!(config.service.name, "rails-app");
        assert!(config.service.privileged);
        assert!(config.volumes.contains(&"mise-cache".to_string()));
    }

    #[test]
    fn test_compose_config_yaml_output() {
        let info = ProjectInfo::default();
        let config = ComposeConfig::from_preset(StackPreset::Rails, "my-app", &info);
        let yaml = config.to_yaml(StackPreset::Rails, &info).unwrap();

        assert!(yaml.contains("name:"));
        assert!(yaml.contains("services:"));
        assert!(yaml.contains("rails-app:"));
        assert!(yaml.contains("../..:/workspaces:cached"));
        assert!(yaml.contains(".ai-agents/codex"));
    }

    #[test]
    fn test_dockerfile_config_rails() {
        let info = ProjectInfo {
            ruby_version: Some("3.3.0".to_string()),
            ..Default::default()
        };
        let config = DockerfileConfig::from_preset(StackPreset::Rails, &info);

        assert!(config.install_mise);
        assert!(config.packages.contains(&"imagemagick".to_string()));
        assert!(config
            .runtime_versions
            .iter()
            .any(|v| v.contains("ruby@3.3")));
    }

    #[test]
    fn test_dockerfile_output() {
        let info = ProjectInfo::default();
        let config = DockerfileConfig::from_preset(StackPreset::Rails, &info);
        let dockerfile = config.to_dockerfile(StackPreset::Rails);

        assert!(dockerfile.contains("FROM mcr.microsoft.com/devcontainers/base:debian"));
        assert!(dockerfile.contains("mise.run"));
        assert!(dockerfile.contains("mise install ruby@"));
    }

    #[test]
    fn test_extract_repo_name_https() {
        assert_eq!(
            extract_repo_name("https://github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_ssh() {
        assert_eq!(
            extract_repo_name("git@github.com:user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn test_dasherize() {
        assert_eq!(dasherize("My App"), "my-app");
        assert_eq!(dasherize("my_app"), "my-app");
        assert_eq!(dasherize("MyApp"), "myapp");
    }

    #[test]
    fn test_major_minor() {
        assert_eq!(major_minor("3.3.0"), "3.3");
        assert_eq!(major_minor("3.3"), "3.3");
        assert_eq!(major_minor("3"), "3");
    }

    #[test]
    fn test_builder_methods() {
        let config = DevcontainerConfig::from_preset(StackPreset::Generic, "test")
            .with_name("Custom Name")
            .with_forward_port(8080)
            .with_post_create_command("npm install");

        assert_eq!(config.name, "Custom Name");
        assert!(config.forward_ports.contains(&8080));
        assert_eq!(
            config.post_create_command,
            Some("npm install".to_string())
        );
    }
}
