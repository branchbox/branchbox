//! Core configuration types for devcontainer generation
//!
//! These types map directly to the devcontainer.json schema and compose.yaml format,
//! providing type-safe construction and serialization.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Main devcontainer.json configuration
///
/// This struct represents the complete devcontainer.json file and can be
/// serialized to JSON or loaded from an existing file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DevcontainerConfig {
    /// Display name for the container
    pub name: String,

    /// Docker Compose file to use (relative path)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_compose_file: Option<String>,

    /// Primary service name in compose file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Workspace folder inside container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_folder: Option<String>,

    /// Workspace mount configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_mount: Option<String>,

    /// Container environment variables
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_env: Option<ContainerEnv>,

    /// Devcontainer features to install
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, Feature>>,

    /// Ports to forward from container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_ports: Option<Vec<PortConfig>>,

    /// Command to run after container creation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_create_command: Option<String>,

    /// Docker run arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_args: Option<Vec<String>>,

    /// VS Code / Editor customizations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customizations: Option<Customizations>,

    /// Additional properties not explicitly modeled
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl DevcontainerConfig {
    /// Create a new empty configuration
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set workspace folder to use dynamic worktree-compatible path
    pub fn with_dynamic_workspace(mut self) -> Self {
        self.workspace_folder =
            Some("/workspaces/${localWorkspaceFolderBasename}".to_string());
        self.workspace_mount = Some(
            "source=${localWorkspaceFolder},target=/workspaces/${localWorkspaceFolderBasename},type=bind,consistency=cached".to_string()
        );
        self
    }

    /// Add a feature
    pub fn add_feature(&mut self, uri: impl Into<String>, options: Feature) {
        let features = self.features.get_or_insert_with(BTreeMap::new);
        features.insert(uri.into(), options);
    }

    /// Add a forwarded port
    pub fn add_port(&mut self, port: u16) {
        let ports = self.forward_ports.get_or_insert_with(Vec::new);
        ports.push(PortConfig::Simple(port));
    }

    /// Set container environment variable
    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let env = self.container_env.get_or_insert_with(ContainerEnv::new);
        env.0.insert(key.into(), value.into());
    }

    /// Add a run argument
    pub fn add_run_arg(&mut self, arg: impl Into<String>) {
        let args = self.run_args.get_or_insert_with(Vec::new);
        args.push(arg.into());
    }

    /// Serialize to pretty-printed JSON
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Load from file
    pub fn from_file(path: &std::path::Path) -> Result<Self, crate::Error> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json(&content).map_err(|e| {
            crate::Error::validation(format!("Failed to parse devcontainer.json: {}", e))
        })
    }
}

/// Container environment variables
#[derive(Debug, Clone, Default)]
pub struct ContainerEnv(pub BTreeMap<String, String>);

impl ContainerEnv {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }
}

impl Serialize for ContainerEnv {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContainerEnv {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let map = BTreeMap::deserialize(deserializer)?;
        Ok(Self(map))
    }
}

/// Devcontainer feature configuration
///
/// Can be either an empty object `{}` or an object with options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum Feature {
    /// Feature with no options
    #[default]
    Empty,
    /// Feature with options
    Options(BTreeMap<String, serde_json::Value>),
}

impl Feature {
    /// Create empty feature (no options)
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Create feature with options
    pub fn with_options(options: BTreeMap<String, serde_json::Value>) -> Self {
        Self::Options(options)
    }

    /// Create feature with single option
    pub fn with_option(key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        let mut options = BTreeMap::new();
        options.insert(key.into(), value.into());
        Self::Options(options)
    }
}

impl From<()> for Feature {
    fn from(_: ()) -> Self {
        Self::Empty
    }
}

/// Port configuration - can be simple number or detailed config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortConfig {
    Simple(u16),
    Detailed {
        #[serde(rename = "port")]
        port: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol: Option<String>,
    },
}

/// Editor customizations container
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Customizations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vscode: Option<VscodeCustomizations>,
}

/// VS Code specific customizations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VscodeCustomizations {
    /// Extensions to install
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    /// VS Code settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<VscodeSettings>,
}

impl VscodeCustomizations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_extension(&mut self, ext: impl Into<String>) {
        let extensions = self.extensions.get_or_insert_with(Vec::new);
        extensions.push(ext.into());
    }
}

/// VS Code settings (key-value pairs)
#[derive(Debug, Clone, Default)]
pub struct VscodeSettings(pub BTreeMap<String, serde_json::Value>);

impl Serialize for VscodeSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VscodeSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let map = BTreeMap::deserialize(deserializer)?;
        Ok(Self(map))
    }
}

/// Run argument types
#[derive(Debug, Clone)]
pub enum RunArg {
    /// --env-file <path>
    EnvFile(String),
    /// --ipc=host
    IpcHost,
    /// --init
    Init,
    /// --shm-size=<size>
    ShmSize(String),
    /// Custom argument
    Custom(String),
}

impl RunArg {
    pub fn to_args(&self) -> Vec<String> {
        match self {
            RunArg::EnvFile(path) => vec!["--env-file".to_string(), path.clone()],
            RunArg::IpcHost => vec!["--ipc=host".to_string()],
            RunArg::Init => vec!["--init".to_string()],
            RunArg::ShmSize(size) => vec![format!("--shm-size={}", size)],
            RunArg::Custom(arg) => vec![arg.clone()],
        }
    }
}

// =============================================================================
// Docker Compose Configuration
// =============================================================================

/// Docker Compose configuration (compose.yaml)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComposeConfig {
    /// Compose project name (can use env vars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Services defined in the compose file
    pub services: BTreeMap<String, ComposeService>,

    /// Named volumes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<BTreeMap<String, serde_json::Value>>,
}

impl ComposeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set project name with environment variable fallback
    pub fn with_project_name(mut self, name: &str, fallback: &str) -> Self {
        self.name = Some(format!("${{DEVCONTAINER_NAME:-{}}}", fallback));
        let _ = name; // Reserved for future use
        self
    }

    /// Add a service
    pub fn add_service(&mut self, name: impl Into<String>, service: ComposeService) {
        self.services.insert(name.into(), service);
    }

    /// Add a named volume
    pub fn add_volume(&mut self, name: impl Into<String>) {
        let volumes = self.volumes.get_or_insert_with(BTreeMap::new);
        volumes.insert(name.into(), serde_json::Value::Null);
    }

    /// Serialize to YAML
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    /// Parse from YAML string
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }
}

/// Docker Compose service configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComposeService {
    /// Build configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildConfig>,

    /// Image to use (if not building)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Container name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,

    /// Run in privileged mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privileged: Option<bool>,

    /// Volume mounts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<String>>,

    /// Environment variables
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<BTreeMap<String, String>>,

    /// Environment file(s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_file: Option<Vec<EnvFileEntry>>,

    /// Command to run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Service dependencies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,

    /// Restart policy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,

    /// Additional properties
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl ComposeService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a development service with standard BranchBox configuration
    pub fn dev_service(service_name: &str, project_fallback: &str) -> Self {
        Self {
            build: Some(BuildConfig {
                context: "..".to_string(),
                dockerfile: ".devcontainer/Dockerfile".to_string(),
                args: None,
            }),
            container_name: Some(format!("${{DEVCONTAINER_NAME:-{}}}", project_fallback)),
            privileged: Some(true),
            volumes: Some(Self::standard_volumes(service_name)),
            command: Some("sleep infinity".to_string()),
            env_file: Some(vec![
                EnvFileEntry::optional("../.env"),
                EnvFileEntry::optional(".branchbox.env"),
            ]),
            ..Default::default()
        }
    }

    /// Standard volume mounts for BranchBox worktree compatibility
    pub fn standard_volumes(container_user: &str) -> Vec<String> {
        let home_path = match container_user {
            "node" => "/home/node",
            "root" => "/root",
            _ => "/home/vscode",
        };

        vec![
            "../..:/workspaces:cached".to_string(),
            "dind-data:/var/lib/docker".to_string(),
            format!("${{SHARED_CONFIG_DIR:-../..}}/.codex:{}/.codex", home_path),
            format!(
                "${{SHARED_CONFIG_DIR:-../..}}/.claude:{}/.claude",
                home_path
            ),
            format!(
                "${{SHARED_CONFIG_DIR:-../..}}/.claude.json:{}/.claude.json",
                home_path
            ),
            format!(
                "${{SHARED_CONFIG_DIR:-../..}}/.gh:{}/.config/gh",
                home_path
            ),
        ]
    }

    /// Add a volume mount
    pub fn add_volume(&mut self, mount: impl Into<String>) {
        let volumes = self.volumes.get_or_insert_with(Vec::new);
        volumes.push(mount.into());
    }

    /// Set environment variable
    pub fn set_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let env = self.environment.get_or_insert_with(BTreeMap::new);
        env.insert(key.into(), value.into());
    }
}

/// Build configuration for compose service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub context: String,
    pub dockerfile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<BTreeMap<String, String>>,
}

/// Environment file entry (can be required or optional)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvFileEntry {
    /// Simple path (required)
    Simple(String),
    /// Path with required flag
    Optional {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
    },
}

// Serialize Optional variant with `required: false`
impl EnvFileEntry {
    pub fn optional(path: impl Into<String>) -> Self {
        Self::Optional {
            path: path.into(),
            required: Some(false),
        }
    }
}


// =============================================================================
// Dockerfile Configuration
// =============================================================================

/// Dockerfile generation configuration
#[derive(Debug, Clone, Default)]
pub struct DockerfileConfig {
    /// Base image
    pub base_image: String,

    /// Build arguments
    pub args: Vec<(String, String)>,

    /// Packages to install via apt-get
    pub apt_packages: Vec<String>,

    /// User to switch to at the end
    pub user: Option<String>,

    /// Additional RUN commands
    pub run_commands: Vec<String>,
}

impl DockerfileConfig {
    pub fn new(base_image: impl Into<String>) -> Self {
        Self {
            base_image: base_image.into(),
            ..Default::default()
        }
    }

    /// Add a build argument
    pub fn add_arg(&mut self, name: impl Into<String>, default: impl Into<String>) {
        self.args.push((name.into(), default.into()));
    }

    /// Add apt packages to install
    pub fn add_apt_packages(&mut self, packages: &[&str]) {
        self.apt_packages
            .extend(packages.iter().map(|s| s.to_string()));
    }

    /// Set the final user
    pub fn set_user(&mut self, user: impl Into<String>) {
        self.user = Some(user.into());
    }

    /// Generate Dockerfile content
    pub fn to_dockerfile(&self) -> String {
        let mut lines = Vec::new();

        // Build arguments
        for (name, default) in &self.args {
            lines.push(format!("ARG {}=\"{}\"", name, default));
        }

        // Base image (may reference args)
        lines.push(format!("FROM {}", self.base_image));
        lines.push(String::new());

        // Switch to root for package installation
        lines.push("USER root".to_string());
        lines.push(String::new());

        // Install apt packages
        if !self.apt_packages.is_empty() {
            lines.push("RUN apt-get update && apt-get install -y \\".to_string());
            for (i, pkg) in self.apt_packages.iter().enumerate() {
                if i < self.apt_packages.len() - 1 {
                    lines.push(format!("    {} \\", pkg));
                } else {
                    lines.push(format!("    {} \\", pkg));
                }
            }
            lines.push("    && rm -rf /var/lib/apt/lists/*".to_string());
            lines.push(String::new());
        }

        // Additional run commands
        for cmd in &self.run_commands {
            lines.push(format!("RUN {}", cmd));
        }

        // Switch to final user
        if let Some(user) = &self.user {
            lines.push(format!("USER {}", user));
        }

        lines.push(String::new());
        lines.join("\n")
    }
}

// =============================================================================
// Database Configuration
// =============================================================================

/// Database configuration for compose services
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Database type
    pub db_type: DatabaseType,

    /// Service name in compose
    pub service_name: String,

    /// Database name
    pub database_name: String,

    /// Username
    pub username: String,

    /// Password
    pub password: String,

    /// Port
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseType {
    Postgres,
    Mysql,
    Sqlite,
}

impl DatabaseConfig {
    /// Create default Postgres configuration
    pub fn postgres(database_name: &str) -> Self {
        Self {
            db_type: DatabaseType::Postgres,
            service_name: "postgres".to_string(),
            database_name: database_name.to_string(),
            username: "postgres".to_string(),
            password: "postgres".to_string(),
            port: 5432,
        }
    }

    /// Generate compose service for this database
    pub fn to_compose_service(&self) -> ComposeService {
        match self.db_type {
            DatabaseType::Postgres => {
                let mut service = ComposeService::new();
                service.image = Some("pgvector/pgvector:pg16".to_string());
                service.restart = Some("unless-stopped".to_string());
                service.volumes = Some(vec!["postgres-data:/var/lib/postgresql/data".to_string()]);
                let mut env = BTreeMap::new();
                env.insert("POSTGRES_USER".to_string(), self.username.clone());
                env.insert("POSTGRES_PASSWORD".to_string(), self.password.clone());
                service.environment = Some(env);
                service
            }
            DatabaseType::Mysql => {
                let mut service = ComposeService::new();
                service.image = Some("mysql:8".to_string());
                service.restart = Some("unless-stopped".to_string());
                service.volumes = Some(vec!["mysql-data:/var/lib/mysql".to_string()]);
                let mut env = BTreeMap::new();
                env.insert("MYSQL_ROOT_PASSWORD".to_string(), self.password.clone());
                env.insert("MYSQL_DATABASE".to_string(), self.database_name.clone());
                service.environment = Some(env);
                service
            }
            DatabaseType::Sqlite => {
                // SQLite doesn't need a service
                ComposeService::new()
            }
        }
    }

    /// Generate DATABASE_URL for this database
    pub fn database_url(&self, host: Option<&str>) -> String {
        let host = host.unwrap_or(&self.service_name);
        match self.db_type {
            DatabaseType::Postgres => {
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    self.username, self.password, host, self.port, self.database_name
                )
            }
            DatabaseType::Mysql => {
                format!(
                    "mysql://{}:{}@{}:{}/{}",
                    self.username, self.password, host, self.port, self.database_name
                )
            }
            DatabaseType::Sqlite => {
                format!("sqlite://./db/{}.sqlite3", self.database_name)
            }
        }
    }
}

/// Volume mount configuration
#[derive(Debug, Clone)]
pub struct VolumeMount {
    pub source: String,
    pub target: String,
    pub options: Option<String>,
}

impl VolumeMount {
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            options: None,
        }
    }

    pub fn with_options(mut self, options: impl Into<String>) -> Self {
        self.options = Some(options.into());
        self
    }

    pub fn to_string(&self) -> String {
        match &self.options {
            Some(opts) => format!("{}:{}:{}", self.source, self.target, opts),
            None => format!("{}:{}", self.source, self.target),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devcontainer_config_serialization() {
        let mut config = DevcontainerConfig::new("Test Project");
        config.docker_compose_file = Some("compose.yaml".to_string());
        config.service = Some("app".to_string());
        config = config.with_dynamic_workspace();
        config.set_env("DOCKER_CONTAINER", "true");
        config.add_port(3000);

        let json = config.to_json_pretty().unwrap();
        assert!(json.contains("\"name\": \"Test Project\""));
        assert!(json.contains("${localWorkspaceFolderBasename}"));
        assert!(json.contains("DOCKER_CONTAINER"));
    }

    #[test]
    fn test_feature_serialization() {
        let empty = Feature::empty();
        let json = serde_json::to_string(&empty).unwrap();
        assert_eq!(json, "null"); // Empty serializes as null which becomes {}

        let with_version = Feature::with_option("version", "20");
        let json = serde_json::to_string(&with_version).unwrap();
        assert!(json.contains("\"version\":\"20\""));
    }

    #[test]
    fn test_compose_service_standard_volumes() {
        let volumes = ComposeService::standard_volumes("vscode");
        assert!(volumes.iter().any(|v| v.contains("../..:/workspaces")));
        assert!(volumes.iter().any(|v| v.contains(".codex")));
        assert!(volumes.iter().any(|v| v.contains(".claude")));
        assert!(volumes.iter().any(|v| v.contains(".gh")));
    }

    #[test]
    fn test_dockerfile_generation() {
        let mut config = DockerfileConfig::new("mcr.microsoft.com/devcontainers/base:ubuntu");
        config.add_apt_packages(&["git", "curl", "vim"]);
        config.set_user("vscode");

        let dockerfile = config.to_dockerfile();
        assert!(dockerfile.contains("FROM mcr.microsoft.com/devcontainers/base:ubuntu"));
        assert!(dockerfile.contains("apt-get install"));
        assert!(dockerfile.contains("git"));
        assert!(dockerfile.contains("USER vscode"));
    }

    #[test]
    fn test_database_config_url() {
        let pg = DatabaseConfig::postgres("myapp_development");
        assert_eq!(
            pg.database_url(None),
            "postgres://postgres:postgres@postgres:5432/myapp_development"
        );
    }
}
