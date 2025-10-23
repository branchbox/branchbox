//! Environment validation utilities for git worktree workflow
//!
//! Validates prerequisites and environment setup.

use crate::{Error, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Validate that a directory is a valid git worktree
///
/// Checks if .git exists (either as directory for main worktree or file for feature worktree)
///
/// # Examples
///
/// ```no_run
/// use worktree_core::validation::validate_git_worktree;
/// use std::path::Path;
///
/// let path = Path::new("/path/to/worktree");
/// if validate_git_worktree(path).is_ok() {
///     println!("Valid git worktree");
/// }
/// ```
pub fn validate_git_worktree(worktree_dir: &Path) -> Result<()> {
    let git_path = worktree_dir.join(".git");

    if !git_path.exists() {
        return Err(Error::validation(format!(
            "Not a git worktree: {} (no .git found)",
            worktree_dir.display()
        )));
    }

    // Main worktree: .git is a directory with config file
    if git_path.is_dir() {
        let config_path = git_path.join("config");
        if config_path.exists() {
            return Ok(());
        }
    }

    // Feature worktree: .git is a file containing gitdir reference
    if git_path.is_file() {
        // Read the gitdir reference
        let content = fs::read_to_string(&git_path)
            .map_err(|e| Error::validation(format!("Failed to read .git file: {}", e)))?;

        // Verify it has the gitdir reference
        if content.starts_with("gitdir:") {
            // Try to verify the worktree is valid by checking if the referenced path exists
            let gitdir = content.trim_start_matches("gitdir:").trim();
            let gitdir_path = if gitdir.starts_with('/') {
                PathBuf::from(gitdir)
            } else {
                worktree_dir.join(gitdir)
            };

            if gitdir_path.exists() {
                return Ok(());
            } else {
                return Err(Error::validation(format!(
                    "Git worktree path mismatch: referenced path does not exist: {}",
                    gitdir_path.display()
                )));
            }
        }
    }

    Err(Error::validation(format!(
        "Invalid git worktree: {}",
        worktree_dir.display()
    )))
}

/// Check if running on host (not in container)
///
/// Returns `Ok(())` if running on host, `Err` if in container.
pub fn validate_host_environment() -> Result<()> {
    // Check for Docker environment
    if Path::new("/.dockerenv").exists() {
        return Err(Error::validation(
            "Running in Docker container. This command must be run on the host machine."
                .to_string(),
        ));
    }

    // Check for Docker environment variable
    if std::env::var("DOCKER_CONTAINER").is_ok() {
        return Err(Error::validation(
            "Running in Docker container. This command must be run on the host machine."
                .to_string(),
        ));
    }

    Ok(())
}

/// Validate .env file exists and has required variables
///
/// # Examples
///
/// ```no_run
/// use worktree_core::validation::validate_env_file;
/// use std::path::Path;
///
/// let env_file = Path::new(".env");
/// let required_vars = vec!["APP_URL", "DATABASE_URL"];
///
/// if validate_env_file(env_file, &required_vars).is_ok() {
///     println!("Environment file is valid");
/// }
/// ```
pub fn validate_env_file(env_file: &Path, required_vars: &[&str]) -> Result<()> {
    if !env_file.exists() {
        return Err(Error::validation(format!(
            "Environment file not found: {}",
            env_file.display()
        )));
    }

    let content = fs::read_to_string(env_file)
        .map_err(|e| Error::validation(format!("Failed to read .env file: {}", e)))?;

    // Check for required variables
    for var in required_vars {
        let pattern = format!("{}=", var);
        if !content.lines().any(|line| line.starts_with(&pattern)) {
            return Err(Error::validation(format!(
                "Required environment variable not found in {}: {}",
                env_file.display(),
                var
            )));
        }
    }

    Ok(())
}

/// Parse .env file and extract all variables
///
/// Returns a HashMap of variable names to values.
///
/// # Examples
///
/// ```no_run
/// use worktree_core::validation::parse_env_file;
/// use std::path::Path;
///
/// let env_file = Path::new(".env");
/// let vars = parse_env_file(env_file).unwrap();
///
/// if let Some(app_url) = vars.get("APP_URL") {
///     println!("APP_URL: {}", app_url);
/// }
/// ```
pub fn parse_env_file(env_file: &Path) -> Result<HashMap<String, String>> {
    let content = fs::read_to_string(env_file)
        .map_err(|e| Error::validation(format!("Failed to read .env file: {}", e)))?;

    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse KEY=VALUE
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();

            vars.insert(key, value);
        }
    }

    Ok(vars)
}

/// App URL components
#[derive(Debug, Clone, PartialEq)]
pub struct AppUrl {
    /// Full URL (without protocol)
    pub url: String,
    /// Base prefix (subdomain part before first dot)
    pub base_prefix: String,
    /// Base domain (everything after first dot)
    pub base_domain: String,
}

impl AppUrl {
    /// Parse APP_URL from a string
    ///
    /// Strips protocol (http:// or https://) and splits into prefix and domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use worktree_core::validation::AppUrl;
    ///
    /// let app_url = AppUrl::parse("https://rida-mbp-agentify.rida.me").unwrap();
    /// assert_eq!(app_url.base_prefix, "rida-mbp-agentify");
    /// assert_eq!(app_url.base_domain, "rida.me");
    /// assert_eq!(app_url.url, "rida-mbp-agentify.rida.me");
    /// ```
    pub fn parse(url: &str) -> Result<Self> {
        // Strip protocol
        let url = url
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://");

        if url.is_empty() {
            return Err(Error::validation("APP_URL cannot be empty".to_string()));
        }

        // Split into prefix and domain
        let parts: Vec<&str> = url.splitn(2, '.').collect();

        if parts.len() < 2 {
            return Err(Error::validation(format!(
                "Invalid APP_URL format (expected subdomain.domain): {}",
                url
            )));
        }

        Ok(Self {
            url: url.to_string(),
            base_prefix: parts[0].to_string(),
            base_domain: parts[1].to_string(),
        })
    }

    /// Parse APP_URL from .env file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use worktree_core::validation::AppUrl;
    /// use std::path::Path;
    ///
    /// let env_file = Path::new(".env");
    /// let app_url = AppUrl::from_env_file(env_file).unwrap();
    /// println!("Base prefix: {}", app_url.base_prefix);
    /// ```
    pub fn from_env_file(env_file: &Path) -> Result<Self> {
        let vars = parse_env_file(env_file)?;

        let url = vars
            .get("APP_URL")
            .ok_or_else(|| Error::validation("APP_URL not found in .env file".to_string()))?;

        Self::parse(url)
    }
}

/// Cloudflare credentials
#[derive(Debug, Clone)]
pub struct CloudflareCredentials {
    pub api_key: Option<String>,
    pub account_id: Option<String>,
}

impl CloudflareCredentials {
    /// Read Cloudflare credentials from .env file
    ///
    /// Returns `None` values if credentials are not found (not an error).
    pub fn from_env_file(env_file: &Path) -> Result<Self> {
        let vars = parse_env_file(env_file)?;

        Ok(Self {
            api_key: vars.get("CLOUDFLARE_API_KEY").cloned(),
            account_id: vars.get("CLOUDFLARE_ACCOUNT_ID").cloned(),
        })
    }

    /// Check if credentials are complete
    pub fn is_complete(&self) -> bool {
        self.api_key.is_some() && self.account_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_app_url() {
        let url = AppUrl::parse("https://rida-mbp-agentify.rida.me").unwrap();
        assert_eq!(url.base_prefix, "rida-mbp-agentify");
        assert_eq!(url.base_domain, "rida.me");
        assert_eq!(url.url, "rida-mbp-agentify.rida.me");

        let url = AppUrl::parse("dev.example.com").unwrap();
        assert_eq!(url.base_prefix, "dev");
        assert_eq!(url.base_domain, "example.com");
    }

    #[test]
    fn test_parse_app_url_invalid() {
        assert!(AppUrl::parse("").is_err());
        assert!(AppUrl::parse("nodomain").is_err());
    }

    #[test]
    fn test_parse_env_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "APP_URL=rida-mbp-agentify.rida.me").unwrap();
        writeln!(temp_file, "DATABASE_URL=\"postgres://localhost/db\"").unwrap();
        writeln!(temp_file, "# Comment line").unwrap();
        writeln!(temp_file, "").unwrap();
        writeln!(temp_file, "REDIS_URL='redis://localhost'").unwrap();
        temp_file.flush().unwrap();

        let vars = parse_env_file(temp_file.path()).unwrap();

        assert_eq!(vars.get("APP_URL").unwrap(), "rida-mbp-agentify.rida.me");
        assert_eq!(vars.get("DATABASE_URL").unwrap(), "postgres://localhost/db");
        assert_eq!(vars.get("REDIS_URL").unwrap(), "redis://localhost");
        assert_eq!(vars.len(), 3);
    }

    #[test]
    fn test_validate_env_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "APP_URL=test.com").unwrap();
        writeln!(temp_file, "DATABASE_URL=postgres://localhost").unwrap();
        temp_file.flush().unwrap();

        // Valid
        assert!(validate_env_file(temp_file.path(), &["APP_URL"]).is_ok());
        assert!(validate_env_file(temp_file.path(), &["APP_URL", "DATABASE_URL"]).is_ok());

        // Missing variable
        assert!(validate_env_file(temp_file.path(), &["MISSING_VAR"]).is_err());
    }

    #[test]
    fn test_cloudflare_credentials() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "CLOUDFLARE_API_KEY=secret").unwrap();
        writeln!(temp_file, "CLOUDFLARE_ACCOUNT_ID=account123").unwrap();
        temp_file.flush().unwrap();

        let creds = CloudflareCredentials::from_env_file(temp_file.path()).unwrap();
        assert_eq!(creds.api_key.as_deref(), Some("secret"));
        assert_eq!(creds.account_id.as_deref(), Some("account123"));
        assert!(creds.is_complete());
    }

    #[test]
    fn test_cloudflare_credentials_incomplete() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "CLOUDFLARE_API_KEY=secret").unwrap();
        temp_file.flush().unwrap();

        let creds = CloudflareCredentials::from_env_file(temp_file.path()).unwrap();
        assert!(creds.api_key.is_some());
        assert!(creds.account_id.is_none());
        assert!(!creds.is_complete());
    }

    #[test]
    fn test_validate_host_environment() {
        // This will pass on host, fail in container
        // Can't reliably test both scenarios without actual container
        let result = validate_host_environment();
        // Just ensure it doesn't panic
        let _ = result;
    }
}
