//! Devcontainer commands
//!
//! Commands for managing devcontainer configuration across feature worktrees

use anyhow::{Context, Result};
use clap::Subcommand;
use std::io::Write;
use std::path::PathBuf;
use worktree_core::bootstrap::{Bootstrap, Stack};
use worktree_core::devcontainer::ProjectDetector;
use worktree_core::modules::{DevcontainerModule, Module};
use worktree_core::workflows::feature::{FeatureStatus, FeatureWorkflow};

#[derive(Subcommand)]
pub enum DevcontainerCommands {
    /// Sync devcontainer configuration to all feature worktrees
    Sync {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Sync strategy (copy or symlink)
        #[arg(short, long)]
        strategy: Option<String>,

        /// Dry run - show what would be synced without making changes
        #[arg(short = 'n', long)]
        dry_run: bool,
    },

    /// Validate devcontainer configuration
    Validate {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Output format (text or json)
        #[arg(short, long, default_value = "text")]
        format: String,
    },
}

pub fn execute(cmd: DevcontainerCommands) -> Result<()> {
    match cmd {
        DevcontainerCommands::Sync {
            path,
            strategy,
            dry_run,
        } => sync(path, strategy, dry_run),
        DevcontainerCommands::Validate { path, format } => validate(path, format),
    }
}

fn sync(path: Option<PathBuf>, strategy: Option<String>, dry_run: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    // Set strategy env var if provided
    if let Some(strat) = strategy {
        std::env::set_var("BRANCHBOX_DEVCONTAINER_STRATEGY", strat);
    }

    // Load workflow to find all active worktrees
    let workflow = FeatureWorkflow::new(&project_path)?;
    let features = workflow
        .list_features()
        .context("Failed to list features")?;

    let active_features: Vec<_> = features
        .into_iter()
        .filter(|f| f.status == FeatureStatus::Active)
        .collect();

    if active_features.is_empty() {
        println!("No active feature worktrees found");
        return Ok(());
    }

    println!(
        "🔄 Syncing devcontainer configuration to {} feature worktree(s)",
        active_features.len()
    );
    println!();

    if dry_run {
        println!("DRY RUN - no changes will be made");
        println!();
    }

    let mut synced_count = 0;
    let mut errors = Vec::new();
    let module = if dry_run {
        None
    } else {
        let mut module = DevcontainerModule::new();
        module
            .init(&project_path, project_path.as_path())
            .context("Failed to initialize devcontainer module")?;
        Some(module)
    };

    for feature in active_features {
        print!("  {} ... ", feature.work_feature);
        std::io::stdout().flush()?;

        // Check if worktree still exists
        if !feature.worktree_path.exists() {
            println!(
                "⚠️  worktree not found at {}",
                feature.worktree_path.display()
            );
            continue;
        }

        if dry_run {
            println!("would sync");
            synced_count += 1;
            continue;
        }

        let module_ref = module
            .as_ref()
            .expect("Devcontainer module should be initialised when not running in dry-run mode");
        let result = module_ref.sync_to(&feature.worktree_path);

        match result {
            Ok(outcome) => {
                let file_count = outcome.synced_files.len();
                let strategy = outcome.strategy;
                println!("✓ synced {} files ({})", file_count, strategy.as_str());
                synced_count += 1;
                if let Err(err) = workflow.record_devcontainer_sync(
                    &feature.work_feature,
                    Some(strategy.as_str()),
                    true,
                ) {
                    println!("    ⚠️ failed to update registry: {}", err);
                }
            }
            Err(e) => {
                println!("✗ failed: {}", e);
                errors.push((feature.work_feature.clone(), e.to_string()));
                if let Err(err) = workflow.record_devcontainer_sync(
                    &feature.work_feature,
                    Some(module_ref.strategy().as_str()),
                    false,
                ) {
                    println!("    ⚠️ failed to update registry: {}", err);
                }
            }
        }
    }

    println!();
    println!("✓ Successfully synced {} feature worktree(s)", synced_count);

    if !errors.is_empty() {
        println!();
        println!("⚠️  {} error(s) occurred:", errors.len());
        for (feature, error) in errors {
            println!("  - {}: {}", feature, error);
        }
    }

    Ok(())
}

/// Validate devcontainer configuration
fn validate(path: Option<PathBuf>, format: String) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    let devcontainer_dir = project_path.join(".devcontainer");
    let devcontainer_json = devcontainer_dir.join("devcontainer.json");

    // Collect validation issues
    let mut issues: Vec<ValidationIssue> = Vec::new();
    let mut warnings: Vec<ValidationIssue> = Vec::new();

    // Check if .devcontainer exists
    if !devcontainer_dir.exists() {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            message: ".devcontainer directory not found".to_string(),
            suggestion: Some("Run `branchbox init` to create devcontainer configuration".to_string()),
        });
    } else {
        // Check devcontainer.json
        if !devcontainer_json.exists() {
            issues.push(ValidationIssue {
                severity: "error".to_string(),
                message: "devcontainer.json not found".to_string(),
                suggestion: Some("Run `branchbox init` to generate devcontainer.json".to_string()),
            });
        } else {
            // Parse and validate devcontainer.json
            let content = std::fs::read_to_string(&devcontainer_json)?;

            // Try parsing as JSON (allowing comments)
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => {
                    // Check for required BranchBox settings
                    validate_devcontainer_json(&json, &mut issues, &mut warnings);
                }
                Err(e) => {
                    // Try stripping comments first
                    let stripped = strip_jsonc_comments(&content);
                    match serde_json::from_str::<serde_json::Value>(&stripped) {
                        Ok(json) => {
                            validate_devcontainer_json(&json, &mut issues, &mut warnings);
                        }
                        Err(_) => {
                            issues.push(ValidationIssue {
                                severity: "error".to_string(),
                                message: format!("Invalid JSON in devcontainer.json: {}", e),
                                suggestion: Some("Fix the JSON syntax error".to_string()),
                            });
                        }
                    }
                }
            }
        }

        // Check compose.yaml if referenced
        let compose_files = ["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"];
        let compose_found = compose_files.iter().any(|f| devcontainer_dir.join(f).exists());

        if compose_found {
            for compose_file in compose_files {
                let compose_path = devcontainer_dir.join(compose_file);
                if compose_path.exists() {
                    validate_compose_file(&compose_path, &mut issues, &mut warnings)?;
                    break;
                }
            }
        }

        // Check Dockerfile if present
        let dockerfile = devcontainer_dir.join("Dockerfile");
        if dockerfile.exists() {
            validate_dockerfile(&dockerfile, &mut issues, &mut warnings)?;
        }
    }

    // Detect project settings for comparison
    let detector = ProjectDetector::new(&project_path);
    let detected = detector.detect_all();
    let bootstrap = Bootstrap::new(&project_path);
    let stack = bootstrap.detect_stack().unwrap_or(Stack::Generic);

    // Check validity before moving issues
    let is_valid = issues.is_empty();

    // Output results
    if format == "json" {
        let result = ValidationResult {
            valid: is_valid,
            stack: format!("{:?}", stack),
            detected: DetectedInfo {
                project_name: detected.name,
                ruby_version: detected.ruby_version,
                node_version: detected.node_version,
                python_version: detected.python_version,
                rust_version: detected.rust_version,
                port: detected.default_port,
            },
            issues,
            warnings,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
        if !is_valid {
            std::process::exit(1);
        }
        return Ok(());
    } else {
        // Text format
        println!();
        println!("Devcontainer Validation");
        println!("=======================");
        println!();
        println!("  Stack: {:?}", stack);
        println!("  Path:  {}", devcontainer_dir.display());
        println!();

        if issues.is_empty() && warnings.is_empty() {
            println!("✓ Configuration is valid");
            println!();
        } else {
            if !issues.is_empty() {
                println!("Errors:");
                for issue in &issues {
                    println!("  ✗ {}", issue.message);
                    if let Some(ref suggestion) = issue.suggestion {
                        println!("    → {}", suggestion);
                    }
                }
                println!();
            }

            if !warnings.is_empty() {
                println!("Warnings:");
                for warning in &warnings {
                    println!("  ⚠ {}", warning.message);
                    if let Some(ref suggestion) = warning.suggestion {
                        println!("    → {}", suggestion);
                    }
                }
                println!();
            }
        }

        // Show detected settings
        println!("Detected Settings:");
        if let Some(ref name) = detected.name {
            println!("  Project name:  {}", name);
        }
        match stack {
            Stack::Rails => {
                if let Some(ref v) = detected.ruby_version {
                    println!("  Ruby version:  {}", v);
                }
            }
            Stack::NodeJs => {
                if let Some(ref v) = detected.node_version {
                    println!("  Node version:  {}", v);
                }
            }
            Stack::Python => {
                if let Some(ref v) = detected.python_version {
                    println!("  Python version: {}", v);
                }
            }
            Stack::Rust => {
                if let Some(ref v) = detected.rust_version {
                    println!("  Rust version:  {}", v);
                }
            }
            Stack::Generic => {}
        }
        if let Some(port) = detected.default_port {
            println!("  Default port:  {}", port);
        }
        println!();

        if !is_valid {
            std::process::exit(1);
        }
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct ValidationResult {
    valid: bool,
    stack: String,
    detected: DetectedInfo,
    issues: Vec<ValidationIssue>,
    warnings: Vec<ValidationIssue>,
}

#[derive(serde::Serialize)]
struct DetectedInfo {
    project_name: Option<String>,
    ruby_version: Option<String>,
    node_version: Option<String>,
    python_version: Option<String>,
    rust_version: Option<String>,
    port: Option<u16>,
}

#[derive(serde::Serialize)]
struct ValidationIssue {
    severity: String,
    message: String,
    suggestion: Option<String>,
}

fn validate_devcontainer_json(
    json: &serde_json::Value,
    issues: &mut Vec<ValidationIssue>,
    warnings: &mut Vec<ValidationIssue>,
) {
    // Check for workspaceFolder setting
    if json.get("workspaceFolder").is_none() {
        warnings.push(ValidationIssue {
            severity: "warning".to_string(),
            message: "Missing workspaceFolder setting".to_string(),
            suggestion: Some("Add workspaceFolder for consistent workspace path".to_string()),
        });
    } else {
        // Check if workspaceFolder is compatible with BranchBox worktrees
        if let Some(folder) = json.get("workspaceFolder").and_then(|v| v.as_str()) {
            if !folder.starts_with("/workspaces") {
                warnings.push(ValidationIssue {
                    severity: "warning".to_string(),
                    message: format!("workspaceFolder '{}' may not work with BranchBox worktrees", folder),
                    suggestion: Some("Use /workspaces/<project> for multi-worktree compatibility".to_string()),
                });
            }
        }
    }

    // Check for dockerComposeFile or build configuration
    let has_compose = json.get("dockerComposeFile").is_some();
    let has_build = json.get("build").is_some();
    let has_image = json.get("image").is_some();

    if !has_compose && !has_build && !has_image {
        issues.push(ValidationIssue {
            severity: "error".to_string(),
            message: "No container configuration found".to_string(),
            suggestion: Some("Add dockerComposeFile, build, or image to devcontainer.json".to_string()),
        });
    }

    // Check for recommended settings
    if json.get("remoteUser").is_none() {
        warnings.push(ValidationIssue {
            severity: "warning".to_string(),
            message: "No remoteUser specified".to_string(),
            suggestion: Some("Set remoteUser to avoid running as root".to_string()),
        });
    }
}

fn validate_compose_file(
    path: &std::path::Path,
    _issues: &mut Vec<ValidationIssue>,
    warnings: &mut Vec<ValidationIssue>,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;

    // Check for workspace mount
    if !content.contains("/workspaces") && !content.contains("workspaces:") {
        warnings.push(ValidationIssue {
            severity: "warning".to_string(),
            message: "No /workspaces mount found in compose file".to_string(),
            suggestion: Some("Add workspace mount for BranchBox worktree support".to_string()),
        });
    }

    // Check for shared config mounts (coding agents)
    let coding_agent_configs = [".codex", ".claude", ".gh"];
    let missing_configs: Vec<&str> = coding_agent_configs
        .iter()
        .filter(|c| !content.contains(**c))
        .copied()
        .collect();

    if !missing_configs.is_empty() {
        warnings.push(ValidationIssue {
            severity: "warning".to_string(),
            message: format!("Missing shared config mounts: {}", missing_configs.join(", ")),
            suggestion: Some("Add SHARED_CONFIG_DIR mounts for coding agent credentials".to_string()),
        });
    }

    Ok(())
}

fn validate_dockerfile(
    path: &std::path::Path,
    _issues: &mut Vec<ValidationIssue>,
    warnings: &mut Vec<ValidationIssue>,
) -> Result<()> {
    let content = std::fs::read_to_string(path)?;

    // Check for USER directive
    let has_user = content
        .lines()
        .any(|line| line.trim().to_uppercase().starts_with("USER "));

    if !has_user {
        warnings.push(ValidationIssue {
            severity: "warning".to_string(),
            message: "No USER directive in Dockerfile".to_string(),
            suggestion: Some("Add USER directive to avoid running as root".to_string()),
        });
    }

    Ok(())
}

/// Strip JSONC-style comments from a string
fn strip_jsonc_comments(content: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }

        if !in_string && c == '/' {
            if chars.peek() == Some(&'/') {
                // Line comment - skip until newline
                while let Some(next) = chars.next() {
                    if next == '\n' {
                        result.push('\n');
                        break;
                    }
                }
                continue;
            } else if chars.peek() == Some(&'*') {
                // Block comment - skip until */
                chars.next(); // consume *
                while let Some(next) = chars.next() {
                    if next == '*' && chars.peek() == Some(&'/') {
                        chars.next(); // consume /
                        break;
                    }
                }
                continue;
            }
        }

        result.push(c);
    }

    result
}
