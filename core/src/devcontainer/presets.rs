//! Stack-specific presets for devcontainer generation
//!
//! Each preset provides sensible defaults for a specific technology stack,
//! including appropriate base images, features, extensions, and configurations.

use super::config::*;
use std::collections::BTreeMap;

/// Available stack presets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackPreset {
    /// Ruby on Rails preset
    Rails,
    /// Node.js / JavaScript preset
    NodeJs,
    /// Rust preset
    Rust,
    /// Python / Django / Flask preset
    Python,
    /// Generic development preset
    Generic,
}

impl StackPreset {
    /// Get the preset from a stack name string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "rails" | "ruby" => Some(Self::Rails),
            "nodejs" | "node" | "javascript" | "js" => Some(Self::NodeJs),
            "rust" => Some(Self::Rust),
            "python" | "django" | "flask" => Some(Self::Python),
            "generic" | "other" => Some(Self::Generic),
            _ => None,
        }
    }

    /// Get the service name for this preset
    pub fn service_name(&self) -> &'static str {
        match self {
            Self::Rails => "rails-app",
            Self::NodeJs => "nodejs-app",
            Self::Rust => "rust-dev",
            Self::Python => "python-app",
            Self::Generic => "dev",
        }
    }

    /// Get the default container user for this preset
    pub fn container_user(&self) -> &'static str {
        match self {
            Self::Rails => "vscode",
            Self::NodeJs => "node",
            Self::Rust => "vscode",
            Self::Python => "vscode",
            Self::Generic => "vscode",
        }
    }

    /// Get the default port for this preset
    pub fn default_port(&self) -> u16 {
        match self {
            Self::Rails => 3000,
            Self::NodeJs => 3000,
            Self::Rust => 8080,
            Self::Python => 5000,
            Self::Generic => 3000,
        }
    }

    /// Get the post-create command for this preset
    pub fn post_create_command(&self) -> &'static str {
        match self {
            Self::Rails => "bin/setup",
            Self::NodeJs => "npm install",
            Self::Rust => "cargo --version && rustc --version",
            Self::Python => "pip install -e .",
            Self::Generic => "echo 'Ready for development'",
        }
    }
}

/// Create a devcontainer.json configuration from a preset
pub fn devcontainer_config(
    preset: StackPreset,
    project_name: &str,
    options: &PresetOptions,
) -> DevcontainerConfig {
    let mut config = DevcontainerConfig::new(format!("{} Project", capitalize_first(project_name)));
    config.docker_compose_file = Some("compose.yaml".to_string());
    config.service = Some(preset.service_name().to_string());
    config = config.with_dynamic_workspace();

    // Standard run args
    config.run_args = Some(vec!["--ipc=host".to_string(), "--init".to_string()]);

    // Add preset-specific configuration
    match preset {
        StackPreset::Rails => configure_rails(&mut config, options),
        StackPreset::NodeJs => configure_nodejs(&mut config, options),
        StackPreset::Rust => configure_rust(&mut config, options),
        StackPreset::Python => configure_python(&mut config, options),
        StackPreset::Generic => configure_generic(&mut config, options),
    }

    // Add common features
    add_common_features(&mut config, options);

    config
}

/// Create a compose.yaml configuration from a preset
pub fn compose_config(
    preset: StackPreset,
    project_name: &str,
    options: &PresetOptions,
) -> ComposeConfig {
    let mut config = ComposeConfig::new();
    let service_name = preset.service_name();
    let container_user = preset.container_user();

    // Set project name with fallback
    let project_slug = slugify(project_name);
    config.name = Some(format!("${{DEVCONTAINER_NAME:-{}}}", project_slug));

    // Create main development service
    let mut main_service = ComposeService::dev_service(container_user, &project_slug);

    // Add preset-specific configuration
    match preset {
        StackPreset::Rails => {
            configure_rails_compose(&mut config, &mut main_service, options);
        }
        StackPreset::NodeJs => {
            if let Some(env) = main_service.environment.as_mut() {
                env.insert("NODE_ENV".to_string(), "development".to_string());
            } else {
                let mut env = BTreeMap::new();
                env.insert("NODE_ENV".to_string(), "development".to_string());
                main_service.environment = Some(env);
            }
        }
        StackPreset::Python => {
            configure_python_compose(&mut config, &mut main_service, options);
        }
        _ => {}
    }

    config.add_service(service_name, main_service);

    // Add standard volumes
    config.add_volume("dind-data");

    config
}

/// Create a Dockerfile configuration from a preset
pub fn dockerfile_config(preset: StackPreset, options: &PresetOptions) -> DockerfileConfig {
    match preset {
        StackPreset::Rails => {
            let ruby_version = options.ruby_version.as_deref().unwrap_or("3.3");
            let mut config = DockerfileConfig::new(format!(
                "ghcr.io/rails/devcontainer/images/ruby:${{RUBY_VERSION}}"
            ));
            config.add_arg("RUBY_VERSION", ruby_version);
            config.add_apt_packages(&["build-essential", "git", "curl"]);
            config.set_user("vscode");
            config
        }
        StackPreset::NodeJs => {
            let node_version = options.node_version.as_deref().unwrap_or("20");
            let mut config = DockerfileConfig::new(format!(
                "mcr.microsoft.com/devcontainers/javascript-node:{}",
                node_version
            ));
            config.add_apt_packages(&["git", "curl"]);
            config.set_user("node");
            config
        }
        StackPreset::Rust => {
            let mut config =
                DockerfileConfig::new("mcr.microsoft.com/devcontainers/rust:latest".to_string());
            config.add_apt_packages(&[
                "git",
                "curl",
                "ca-certificates",
                "build-essential",
                "pkg-config",
                "libssl-dev",
            ]);
            config.set_user("vscode");
            config
        }
        StackPreset::Python => {
            let python_version = options.python_version.as_deref().unwrap_or("3.12");
            let mut config = DockerfileConfig::new(format!(
                "mcr.microsoft.com/devcontainers/python:{}",
                python_version
            ));
            config.add_apt_packages(&["git", "curl", "build-essential"]);
            config.set_user("vscode");
            config
        }
        StackPreset::Generic => {
            let mut config =
                DockerfileConfig::new("mcr.microsoft.com/devcontainers/base:ubuntu".to_string());
            config.add_apt_packages(&["build-essential", "git", "curl", "vim"]);
            config.set_user("vscode");
            config
        }
    }
}

/// Generate .env.sample content from a preset
pub fn env_sample(preset: StackPreset, project_name: &str, options: &PresetOptions) -> String {
    let port = options.port.unwrap_or_else(|| preset.default_port());
    let project_slug = slugify(project_name);

    let mut lines = vec![
        format!("# {} Development Environment", capitalize_first(project_name)),
        String::new(),
    ];

    match preset {
        StackPreset::Rails => {
            let db_name = format!("{}_development", project_slug.replace('-', "_"));
            lines.extend(vec![
                format!(
                    "DATABASE_URL=postgres://postgres:postgres@postgres:5432/{}",
                    db_name
                ),
                "RAILS_ENV=development".to_string(),
                format!("APP_URL=dev.localhost:{}", port),
            ]);
        }
        StackPreset::NodeJs => {
            lines.extend(vec![
                "NODE_ENV=development".to_string(),
                format!("PORT={}", port),
                format!("APP_URL=dev.localhost:{}", port),
            ]);
        }
        StackPreset::Rust => {
            lines.extend(vec![
                "RUST_LOG=debug".to_string(),
                "RUST_BACKTRACE=1".to_string(),
                "APP_URL=dev.localhost".to_string(),
            ]);
        }
        StackPreset::Python => {
            lines.extend(vec![
                "FLASK_ENV=development".to_string(),
                "PYTHONDONTWRITEBYTECODE=1".to_string(),
                "PYTHONUNBUFFERED=1".to_string(),
                format!("PORT={}", port),
                format!("APP_URL=dev.localhost:{}", port),
            ]);
        }
        StackPreset::Generic => {
            lines.extend(vec!["APP_URL=dev.localhost".to_string()]);
        }
    }

    lines.push(String::new());
    lines.join("\n")
}

/// Options for customizing preset generation
#[derive(Debug, Clone, Default)]
pub struct PresetOptions {
    /// Ruby version (for Rails)
    pub ruby_version: Option<String>,

    /// Node.js version
    pub node_version: Option<String>,

    /// Python version
    pub python_version: Option<String>,

    /// Application port
    pub port: Option<u16>,

    /// Include database configuration
    pub with_database: bool,

    /// Database type (postgres, mysql, sqlite)
    pub database_type: Option<String>,

    /// Include AI coding agent mounts
    pub coding_agents: bool,

    /// Additional VS Code extensions
    pub extra_extensions: Vec<String>,
}

impl PresetOptions {
    pub fn new() -> Self {
        Self {
            coding_agents: true,
            ..Default::default()
        }
    }

    /// Create options with detected values from a ProjectDetector
    pub fn from_detector(detector: &super::detector::ProjectDetector) -> Self {
        let detected = detector.detect_all();
        Self {
            ruby_version: detected.ruby_version,
            node_version: detected.node_version,
            python_version: detected.python_version,
            port: detected.default_port,
            coding_agents: true,
            ..Default::default()
        }
    }
}

// =============================================================================
// Private helper functions
// =============================================================================

fn configure_rails(config: &mut DevcontainerConfig, options: &PresetOptions) {
    // Environment variables
    config.set_env("DB_HOST", "postgres");
    config.set_env("DOCKER_CONTAINER", "true");

    // Rails-specific features
    config.add_feature(
        "ghcr.io/rails/devcontainer/features/activestorage",
        Feature::empty(),
    );
    config.add_feature(
        "ghcr.io/rails/devcontainer/features/postgres-client",
        Feature::empty(),
    );

    // Ports
    let port = options.port.unwrap_or(3000);
    config.add_port(port);
    config.add_port(5432); // Postgres

    // Run args
    if let Some(args) = config.run_args.as_mut() {
        args.insert(0, "--env-file".to_string());
        args.insert(1, "../.env".to_string());
        args.push("--shm-size=1gb".to_string());
    }

    // Post-create command
    config.post_create_command = Some("bin/setup".to_string());
}

fn configure_nodejs(config: &mut DevcontainerConfig, options: &PresetOptions) {
    config.set_env("DOCKER_CONTAINER", "true");

    let port = options.port.unwrap_or(3000);
    config.add_port(port);

    config.post_create_command = Some("npm install".to_string());
}

fn configure_rust(config: &mut DevcontainerConfig, _options: &PresetOptions) {
    config.set_env("CARGO_HOME", "/workspaces/${localWorkspaceFolderBasename}/.cargo");
    config.set_env("DOCKER_CONTAINER", "true");
    config.set_env("RUST_BACKTRACE", "1");

    // Rust extensions
    let mut customizations = Customizations::default();
    let mut vscode = VscodeCustomizations::new();
    vscode.extensions = Some(vec![
        "rust-lang.rust-analyzer".to_string(),
        "tamasfe.even-better-toml".to_string(),
        "serayuzgur.crates".to_string(),
        "vadimcn.vscode-lldb".to_string(),
    ]);

    // Rust settings
    let mut settings = BTreeMap::new();
    settings.insert(
        "rust-analyzer.checkOnSave".to_string(),
        serde_json::json!(true),
    );
    settings.insert(
        "rust-analyzer.check".to_string(),
        serde_json::json!({"command": "clippy"}),
    );
    let mut rust_settings = BTreeMap::new();
    rust_settings.insert(
        "editor.defaultFormatter".to_string(),
        serde_json::json!("rust-lang.rust-analyzer"),
    );
    rust_settings.insert("editor.formatOnSave".to_string(), serde_json::json!(true));
    settings.insert("[rust]".to_string(), serde_json::json!(rust_settings));
    vscode.settings = Some(VscodeSettings(settings));

    customizations.vscode = Some(vscode);
    config.customizations = Some(customizations);

    // gRPC port
    config.add_port(50051);

    config.post_create_command = Some("cargo --version && rustc --version".to_string());
}

fn configure_python(config: &mut DevcontainerConfig, options: &PresetOptions) {
    config.set_env("DOCKER_CONTAINER", "true");
    config.set_env("PYTHONDONTWRITEBYTECODE", "1");
    config.set_env("PYTHONUNBUFFERED", "1");

    let port = options.port.unwrap_or(5000);
    config.add_port(port);

    // Python extensions
    let mut customizations = Customizations::default();
    let mut vscode = VscodeCustomizations::new();
    vscode.extensions = Some(vec![
        "ms-python.python".to_string(),
        "ms-python.vscode-pylance".to_string(),
        "ms-python.black-formatter".to_string(),
    ]);
    customizations.vscode = Some(vscode);
    config.customizations = Some(customizations);

    config.post_create_command = Some("pip install -e .".to_string());
}

fn configure_generic(config: &mut DevcontainerConfig, _options: &PresetOptions) {
    config.post_create_command = Some("echo 'Ready for development'".to_string());
}

fn add_common_features(config: &mut DevcontainerConfig, options: &PresetOptions) {
    // Docker-in-Docker
    config.add_feature(
        "ghcr.io/devcontainers/features/docker-in-docker:2",
        Feature::with_option("moby", "false"),
    );

    // GitHub CLI
    config.add_feature(
        "ghcr.io/devcontainers/features/github-cli:1",
        Feature::empty(),
    );

    // Node.js (for AI tools)
    let node_version = options.node_version.as_deref().unwrap_or("20");
    config.add_feature(
        "ghcr.io/devcontainers/features/node:1",
        Feature::with_option("version", node_version),
    );

    // AI coding packages
    if options.coding_agents {
        config.add_feature(
            "ghcr.io/rbarazi/devcontainer-features/ai-npm-packages:1",
            Feature::empty(),
        );
    }
}

fn configure_rails_compose(
    config: &mut ComposeConfig,
    main_service: &mut ComposeService,
    options: &PresetOptions,
) {
    // Add postgres dependency
    main_service.depends_on = Some(vec!["postgres".to_string()]);

    // Add postgres service
    if options.with_database || options.database_type.is_none() {
        let db_config = DatabaseConfig::postgres("app_development");
        config.add_service("postgres", db_config.to_compose_service());
        config.add_volume("postgres-data");
    }
}

fn configure_python_compose(
    _config: &mut ComposeConfig,
    main_service: &mut ComposeService,
    _options: &PresetOptions,
) {
    if let Some(env) = main_service.environment.as_mut() {
        env.insert("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string());
        env.insert("PYTHONUNBUFFERED".to_string(), "1".to_string());
    } else {
        let mut env = BTreeMap::new();
        env.insert("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string());
        env.insert("PYTHONUNBUFFERED".to_string(), "1".to_string());
        main_service.environment = Some(env);
    }
}

/// Convert a string to a URL-safe slug
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Capitalize the first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devcontainer_config_rails() {
        let options = PresetOptions {
            ruby_version: Some("3.2".to_string()),
            ..Default::default()
        };
        let config = devcontainer_config(StackPreset::Rails, "my-app", &options);

        assert!(config.name.contains("My-app"));
        assert_eq!(config.service, Some("rails-app".to_string()));
        assert!(config
            .workspace_folder
            .as_ref()
            .unwrap()
            .contains("${localWorkspaceFolderBasename}"));
    }

    #[test]
    fn test_compose_config_rails() {
        let options = PresetOptions::default();
        let config = compose_config(StackPreset::Rails, "my-app", &options);

        assert!(config.services.contains_key("rails-app"));
        assert!(config.services.contains_key("postgres"));
        assert!(config.volumes.as_ref().unwrap().contains_key("dind-data"));
        assert!(config
            .volumes
            .as_ref()
            .unwrap()
            .contains_key("postgres-data"));
    }

    #[test]
    fn test_dockerfile_config_rails() {
        let options = PresetOptions {
            ruby_version: Some("3.2".to_string()),
            ..Default::default()
        };
        let config = dockerfile_config(StackPreset::Rails, &options);
        let dockerfile = config.to_dockerfile();

        assert!(dockerfile.contains("ARG RUBY_VERSION=\"3.2\""));
        assert!(dockerfile.contains("ghcr.io/rails/devcontainer/images/ruby"));
        assert!(dockerfile.contains("USER vscode"));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("My Awesome App"), "my-awesome-app");
        assert_eq!(slugify("test_project"), "test-project");
        assert_eq!(slugify("  spaces  "), "spaces");
    }

    #[test]
    fn test_env_sample_rails() {
        let options = PresetOptions::default();
        let env = env_sample(StackPreset::Rails, "my-app", &options);

        assert!(env.contains("DATABASE_URL=postgres://"));
        assert!(env.contains("RAILS_ENV=development"));
        assert!(env.contains("APP_URL=dev.localhost:3000"));
    }

    #[test]
    fn test_preset_from_str() {
        assert_eq!(StackPreset::from_str("rails"), Some(StackPreset::Rails));
        assert_eq!(StackPreset::from_str("ruby"), Some(StackPreset::Rails));
        assert_eq!(StackPreset::from_str("nodejs"), Some(StackPreset::NodeJs));
        assert_eq!(StackPreset::from_str("RUST"), Some(StackPreset::Rust));
        assert_eq!(StackPreset::from_str("unknown"), None);
    }
}
