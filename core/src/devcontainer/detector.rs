//! Project detection for devcontainer configuration
//!
//! This module provides automatic detection of project settings like:
//! - Ruby version (from .ruby-version, Gemfile, or .tool-versions)
//! - Node version (from .nvmrc, .node-version, or package.json)
//! - Python version (from .python-version, pyproject.toml, or .tool-versions)
//! - Project name (from directory name, package.json, Cargo.toml, or Gemfile)

use std::path::PathBuf;

/// Project detector for extracting configuration from existing project files
#[derive(Debug)]
pub struct ProjectDetector {
    project_path: PathBuf,
}

impl ProjectDetector {
    /// Create a new project detector for the given path
    pub fn new(project_path: impl Into<PathBuf>) -> Self {
        Self {
            project_path: project_path.into(),
        }
    }

    /// Detect the project name from various sources
    ///
    /// Priority order:
    /// 1. package.json "name" field
    /// 2. Cargo.toml [package] name
    /// 3. Gemfile (extract from Rails.application or common patterns)
    /// 4. pyproject.toml [project] or [tool.poetry] name
    /// 5. Directory name (fallback)
    pub fn project_name(&self) -> Option<String> {
        self.project_name_from_package_json()
            .or_else(|| self.project_name_from_cargo_toml())
            .or_else(|| self.project_name_from_gemspec())
            .or_else(|| self.project_name_from_pyproject())
            .or_else(|| self.project_name_from_directory())
    }

    /// Get project name from directory (always succeeds for valid paths)
    pub fn project_name_from_directory(&self) -> Option<String> {
        self.project_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }

    /// Detect Ruby version from project files
    ///
    /// Priority order:
    /// 1. .ruby-version file
    /// 2. .tool-versions (ruby line)
    /// 3. Gemfile ruby directive
    /// 4. Default to latest stable (3.3)
    pub fn ruby_version(&self) -> Option<String> {
        self.ruby_version_from_file()
            .or_else(|| self.ruby_version_from_tool_versions())
            .or_else(|| self.ruby_version_from_gemfile())
    }

    /// Get Ruby version with fallback to default
    pub fn ruby_version_or_default(&self) -> String {
        self.ruby_version().unwrap_or_else(|| "3.3".to_string())
    }

    /// Detect Node.js version from project files
    ///
    /// Priority order:
    /// 1. .nvmrc file
    /// 2. .node-version file
    /// 3. .tool-versions (nodejs line)
    /// 4. package.json engines.node field
    /// 5. Default to LTS (20)
    pub fn node_version(&self) -> Option<String> {
        self.node_version_from_nvmrc()
            .or_else(|| self.node_version_from_node_version())
            .or_else(|| self.node_version_from_tool_versions())
            .or_else(|| self.node_version_from_package_json())
    }

    /// Get Node version with fallback to default
    pub fn node_version_or_default(&self) -> String {
        self.node_version().unwrap_or_else(|| "20".to_string())
    }

    /// Detect Python version from project files
    ///
    /// Priority order:
    /// 1. .python-version file
    /// 2. .tool-versions (python line)
    /// 3. pyproject.toml requires-python
    /// 4. Default to 3.12
    pub fn python_version(&self) -> Option<String> {
        self.python_version_from_file()
            .or_else(|| self.python_version_from_tool_versions())
            .or_else(|| self.python_version_from_pyproject())
    }

    /// Get Python version with fallback to default
    pub fn python_version_or_default(&self) -> String {
        self.python_version().unwrap_or_else(|| "3.12".to_string())
    }

    /// Detect Rust toolchain version from project files
    ///
    /// Priority order:
    /// 1. rust-toolchain.toml file
    /// 2. rust-toolchain file (legacy)
    /// 3. None (use rustup default)
    pub fn rust_version(&self) -> Option<String> {
        self.rust_version_from_toolchain_toml()
            .or_else(|| self.rust_version_from_toolchain_file())
    }

    /// Get Rust version with fallback to default (stable)
    pub fn rust_version_or_default(&self) -> String {
        self.rust_version().unwrap_or_else(|| "stable".to_string())
    }

    /// Detect the default port for this project
    pub fn default_port(&self) -> Option<u16> {
        // Check package.json scripts for port hints
        if let Some(port) = self.port_from_package_json() {
            return Some(port);
        }

        // Check Procfile
        if let Some(port) = self.port_from_procfile() {
            return Some(port);
        }

        // Stack-based defaults
        if self.project_path.join("Gemfile").exists() {
            return Some(3000); // Rails default
        }
        if self.project_path.join("package.json").exists() {
            return Some(3000); // Node.js default
        }
        if self.project_path.join("Cargo.toml").exists() {
            return Some(8080); // Rust default
        }
        if self.project_path.join("pyproject.toml").exists()
            || self.project_path.join("requirements.txt").exists()
        {
            return Some(5000); // Flask/Python default
        }

        None
    }

    // =========================================================================
    // Private helper methods
    // =========================================================================

    fn project_name_from_package_json(&self) -> Option<String> {
        let path = self.project_path.join("package.json");
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        json.get("name")?.as_str().map(|s| s.to_string())
    }

    fn project_name_from_cargo_toml(&self) -> Option<String> {
        let path = self.project_path.join("Cargo.toml");
        let content = std::fs::read_to_string(path).ok()?;

        // Simple TOML parsing for [package] name
        let mut in_package = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[package]" {
                in_package = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_package = false;
                continue;
            }
            if in_package && trimmed.starts_with("name") {
                // Parse: name = "project-name"
                if let Some(value) = trimmed.split('=').nth(1) {
                    let name = value.trim().trim_matches('"').trim_matches('\'');
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    fn project_name_from_gemspec(&self) -> Option<String> {
        // Look for *.gemspec files
        let entries = std::fs::read_dir(&self.project_path).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "gemspec").unwrap_or(false) {
                let content = std::fs::read_to_string(&path).ok()?;
                // Look for: spec.name = "gem-name" or s.name = 'gem-name'
                for line in content.lines() {
                    if line.contains(".name") && line.contains('=') {
                        if let Some(value) = line.split('=').nth(1) {
                            let name = value
                                .trim()
                                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
                            if !name.is_empty() && !name.contains("#{") {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn project_name_from_pyproject(&self) -> Option<String> {
        let path = self.project_path.join("pyproject.toml");
        let content = std::fs::read_to_string(path).ok()?;

        // Look for [project] name or [tool.poetry] name
        let mut in_section = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[project]" || trimmed == "[tool.poetry]" {
                in_section = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_section = false;
                continue;
            }
            if in_section && trimmed.starts_with("name") {
                if let Some(value) = trimmed.split('=').nth(1) {
                    let name = value.trim().trim_matches('"').trim_matches('\'');
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    fn ruby_version_from_file(&self) -> Option<String> {
        let path = self.project_path.join(".ruby-version");
        let content = std::fs::read_to_string(path).ok()?;
        let version = content.trim();
        // Handle formats like "ruby-3.3.0" or "3.3.0"
        let version = version.strip_prefix("ruby-").unwrap_or(version);
        Some(normalize_version(version))
    }

    fn ruby_version_from_tool_versions(&self) -> Option<String> {
        self.version_from_tool_versions("ruby")
    }

    fn ruby_version_from_gemfile(&self) -> Option<String> {
        let path = self.project_path.join("Gemfile");
        let content = std::fs::read_to_string(path).ok()?;

        for line in content.lines() {
            let trimmed = line.trim();
            // Match: ruby "3.3.0" or ruby '3.3.0' or ruby "~> 3.3"
            if trimmed.starts_with("ruby ") || trimmed.starts_with("ruby(") {
                // Extract version string
                if let Some(start) = trimmed.find(|c| c == '"' || c == '\'') {
                    let rest = &trimmed[start + 1..];
                    if let Some(end) = rest.find(|c| c == '"' || c == '\'') {
                        let version = &rest[..end];
                        // Handle "~> 3.3" style constraints
                        let version = version
                            .trim_start_matches("~>")
                            .trim_start_matches(">=")
                            .trim_start_matches("=")
                            .trim();
                        return Some(normalize_version(version));
                    }
                }
            }
        }
        None
    }

    fn node_version_from_nvmrc(&self) -> Option<String> {
        let path = self.project_path.join(".nvmrc");
        let content = std::fs::read_to_string(path).ok()?;
        let version = content.trim();
        // Handle formats like "v20.10.0", "lts/*", "20", etc.
        let version = version.strip_prefix('v').unwrap_or(version);
        if version.starts_with("lts") {
            return Some("lts".to_string());
        }
        Some(normalize_version(version))
    }

    fn node_version_from_node_version(&self) -> Option<String> {
        let path = self.project_path.join(".node-version");
        let content = std::fs::read_to_string(path).ok()?;
        let version = content.trim().strip_prefix('v').unwrap_or(content.trim());
        Some(normalize_version(version))
    }

    fn node_version_from_tool_versions(&self) -> Option<String> {
        self.version_from_tool_versions("nodejs")
    }

    fn node_version_from_package_json(&self) -> Option<String> {
        let path = self.project_path.join("package.json");
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;

        // Check engines.node
        if let Some(engines) = json.get("engines") {
            if let Some(node) = engines.get("node") {
                if let Some(version) = node.as_str() {
                    // Handle version constraints like ">=18", "^20", "20.x"
                    let version = version
                        .trim_start_matches(">=")
                        .trim_start_matches("^")
                        .trim_start_matches("~")
                        .trim_end_matches(".x")
                        .trim_end_matches(".*");
                    // Extract major version
                    if let Some(major) = version.split('.').next() {
                        if let Ok(num) = major.parse::<u32>() {
                            return Some(num.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn python_version_from_file(&self) -> Option<String> {
        let path = self.project_path.join(".python-version");
        let content = std::fs::read_to_string(path).ok()?;
        Some(normalize_version(content.trim()))
    }

    fn python_version_from_tool_versions(&self) -> Option<String> {
        self.version_from_tool_versions("python")
    }

    fn python_version_from_pyproject(&self) -> Option<String> {
        let path = self.project_path.join("pyproject.toml");
        let content = std::fs::read_to_string(path).ok()?;

        for line in content.lines() {
            let trimmed = line.trim();
            // Match: requires-python = ">=3.12"
            if trimmed.starts_with("requires-python") {
                if let Some(value) = trimmed.split('=').nth(1) {
                    let version = value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim_start_matches(">=")
                        .trim_start_matches("^")
                        .trim_start_matches("~=")
                        .trim();
                    return Some(normalize_version(version));
                }
            }
        }
        None
    }

    fn rust_version_from_toolchain_toml(&self) -> Option<String> {
        let path = self.project_path.join("rust-toolchain.toml");
        let content = std::fs::read_to_string(path).ok()?;

        // Look for [toolchain] channel = "1.75" or channel = "stable"
        let mut in_toolchain = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[toolchain]" {
                in_toolchain = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_toolchain = false;
                continue;
            }
            if in_toolchain && trimmed.starts_with("channel") {
                if let Some(value) = trimmed.split('=').nth(1) {
                    let channel = value.trim().trim_matches('"').trim_matches('\'');
                    return Some(channel.to_string());
                }
            }
        }
        None
    }

    fn rust_version_from_toolchain_file(&self) -> Option<String> {
        let path = self.project_path.join("rust-toolchain");
        let content = std::fs::read_to_string(path).ok()?;
        let version = content.trim();
        if !version.is_empty() {
            return Some(version.to_string());
        }
        None
    }

    fn version_from_tool_versions(&self, tool: &str) -> Option<String> {
        let path = self.project_path.join(".tool-versions");
        let content = std::fs::read_to_string(path).ok()?;

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0] == tool {
                return Some(normalize_version(parts[1]));
            }
        }
        None
    }

    fn port_from_package_json(&self) -> Option<u16> {
        let path = self.project_path.join("package.json");
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;

        // Check scripts for port hints
        if let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) {
            for script in scripts.values() {
                if let Some(cmd) = script.as_str() {
                    // Look for -p or --port flags
                    if let Some(port) = extract_port_from_command(cmd) {
                        return Some(port);
                    }
                }
            }
        }
        None
    }

    fn port_from_procfile(&self) -> Option<u16> {
        let path = self.project_path.join("Procfile");
        let content = std::fs::read_to_string(path).ok()?;

        for line in content.lines() {
            if let Some(port) = extract_port_from_command(line) {
                return Some(port);
            }
        }
        None
    }
}

/// Normalize a version string to major.minor format
fn normalize_version(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    match parts.len() {
        0 => version.to_string(),
        1 => parts[0].to_string(),
        _ => format!("{}.{}", parts[0], parts[1]),
    }
}

/// Extract port number from a command string
fn extract_port_from_command(cmd: &str) -> Option<u16> {
    // Match patterns like: -p 3000, --port 3000, PORT=3000, :3000
    let patterns = [
        r"-p\s+(\d+)",
        r"--port[=\s]+(\d+)",
        r"PORT[=:]\s*(\d+)",
        r":(\d{4,5})\b",
    ];

    for pattern in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(caps) = re.captures(cmd) {
                if let Some(port_str) = caps.get(1) {
                    if let Ok(port) = port_str.as_str().parse::<u16>() {
                        if port >= 1024 {
                            return Some(port);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Detected project information
#[derive(Debug, Clone, Default)]
pub struct DetectedProject {
    /// Project name
    pub name: Option<String>,

    /// Ruby version (if detected)
    pub ruby_version: Option<String>,

    /// Node.js version (if detected)
    pub node_version: Option<String>,

    /// Python version (if detected)
    pub python_version: Option<String>,

    /// Rust toolchain version (if detected)
    pub rust_version: Option<String>,

    /// Default port (if detected)
    pub default_port: Option<u16>,
}

impl ProjectDetector {
    /// Run all detectors and return a summary
    pub fn detect_all(&self) -> DetectedProject {
        DetectedProject {
            name: self.project_name(),
            ruby_version: self.ruby_version(),
            node_version: self.node_version(),
            python_version: self.python_version(),
            rust_version: self.rust_version(),
            default_port: self.default_port(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_project_name_from_package_json() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"name": "my-awesome-app"}"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(
            detector.project_name(),
            Some("my-awesome-app".to_string())
        );
    }

    #[test]
    fn test_project_name_from_cargo_toml() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            r#"
[package]
name = "my-rust-project"
version = "0.1.0"
"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(
            detector.project_name(),
            Some("my-rust-project".to_string())
        );
    }

    #[test]
    fn test_project_name_fallback_to_directory() {
        let temp = TempDir::new().unwrap();
        // No project files, should use directory name
        let detector = ProjectDetector::new(temp.path());
        assert!(detector.project_name().is_some());
    }

    #[test]
    fn test_ruby_version_from_file() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(".ruby-version"), "3.2.2").unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.ruby_version(), Some("3.2".to_string()));
    }

    #[test]
    fn test_ruby_version_with_prefix() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(".ruby-version"), "ruby-3.3.0").unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.ruby_version(), Some("3.3".to_string()));
    }

    #[test]
    fn test_ruby_version_from_gemfile() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("Gemfile"),
            r#"
source "https://rubygems.org"
ruby "3.2.0"
gem "rails"
"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.ruby_version(), Some("3.2".to_string()));
    }

    #[test]
    fn test_ruby_version_from_gemfile_constraint() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("Gemfile"),
            r#"
source "https://rubygems.org"
ruby "~> 3.3"
"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.ruby_version(), Some("3.3".to_string()));
    }

    #[test]
    fn test_node_version_from_nvmrc() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join(".nvmrc"), "v20.10.0").unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.node_version(), Some("20.10".to_string()));
    }

    #[test]
    fn test_node_version_from_package_json_engines() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"name": "test", "engines": {"node": ">=18"}}"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.node_version(), Some("18".to_string()));
    }

    #[test]
    fn test_tool_versions_parsing() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join(".tool-versions"),
            "ruby 3.2.2\nnodejs 20.10.0\npython 3.12.0",
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.ruby_version(), Some("3.2".to_string()));
        assert_eq!(detector.node_version(), Some("20.10".to_string()));
        assert_eq!(detector.python_version(), Some("3.12".to_string()));
    }

    #[test]
    fn test_default_port_rails() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Gemfile"), "gem 'rails'").unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.default_port(), Some(3000));
    }

    #[test]
    fn test_ruby_version_or_default() {
        let temp = TempDir::new().unwrap();
        // No ruby version file
        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.ruby_version_or_default(), "3.3".to_string());
    }

    #[test]
    fn test_rust_version_from_toolchain_toml() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("rust-toolchain.toml"),
            r#"
[toolchain]
channel = "1.75"
components = ["rustfmt", "clippy"]
"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.rust_version(), Some("1.75".to_string()));
    }

    #[test]
    fn test_rust_version_from_toolchain_toml_stable() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("rust-toolchain.toml"),
            r#"
[toolchain]
channel = "stable"
"#,
        )
        .unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.rust_version(), Some("stable".to_string()));
    }

    #[test]
    fn test_rust_version_from_legacy_file() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("rust-toolchain"), "nightly").unwrap();

        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.rust_version(), Some("nightly".to_string()));
    }

    #[test]
    fn test_rust_version_or_default() {
        let temp = TempDir::new().unwrap();
        // No rust toolchain file
        let detector = ProjectDetector::new(temp.path());
        assert_eq!(detector.rust_version_or_default(), "stable".to_string());
    }
}
