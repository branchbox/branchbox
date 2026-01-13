//! Devcontainer commands
//!
//! Commands for managing devcontainer configuration and lifecycle.
//!
//! This module provides two sets of commands:
//! 1. **Configuration commands** (sync, configure, detect, add-tunnel, inject-agents)
//!    - Manage devcontainer.json and compose files
//! 2. **Runtime commands** (up, exec, down, build, read-configuration)
//!    - Manage container lifecycle (equivalent to @devcontainers/cli)

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use worktree_core::devcontainer_runtime::{
    DevcontainerRuntime, DownOptions, ExecOptions, UpOptions,
};
use worktree_core::modules::{
    add_cloudflared_service, configure_workspace_settings, detect_container_user,
    detect_main_service, inject_coding_agent_mounts, DevcontainerModule, Module,
};
use worktree_core::workflows::feature::{FeatureStatus, FeatureWorkflow};

#[derive(Subcommand)]
pub enum DevcontainerCommands {
    // === Runtime commands (like @devcontainers/cli) ===
    /// Create and run dev container
    Up {
        /// Workspace folder path (defaults to current directory)
        #[arg(default_value = ".")]
        workspace_folder: PathBuf,

        /// Docker CLI path
        #[arg(long)]
        docker_path: Option<String>,

        /// Docker Compose CLI path
        #[arg(long)]
        docker_compose_path: Option<String>,

        /// Remove existing container before starting
        #[arg(long)]
        remove_existing_container: bool,

        /// Build with --no-cache
        #[arg(long)]
        build_no_cache: bool,

        /// Skip post-create commands
        #[arg(long)]
        skip_post_create: bool,

        /// Remote environment variables (name=value)
        #[arg(long)]
        remote_env: Vec<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Execute a command on a running dev container
    Exec {
        /// Command to execute
        #[arg(required = true, last = true)]
        cmd: Vec<String>,

        /// Workspace folder path (defaults to current directory)
        #[arg(short = 'w', long, default_value = ".")]
        workspace_folder: PathBuf,

        /// Docker CLI path
        #[arg(long)]
        docker_path: Option<String>,

        /// Docker Compose CLI path
        #[arg(long)]
        docker_compose_path: Option<String>,

        /// User to run command as
        #[arg(long, short)]
        user: Option<String>,

        /// Working directory in container
        #[arg(long)]
        workdir: Option<String>,

        /// Remote environment variables (name=value)
        #[arg(long)]
        remote_env: Vec<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Stop and remove dev container
    Down {
        /// Workspace folder path (defaults to current directory)
        #[arg(default_value = ".")]
        workspace_folder: PathBuf,

        /// Docker CLI path
        #[arg(long)]
        docker_path: Option<String>,

        /// Docker Compose CLI path
        #[arg(long)]
        docker_compose_path: Option<String>,

        /// Remove volumes
        #[arg(long, short)]
        volumes: bool,

        /// Remove orphan containers
        #[arg(long)]
        remove_orphans: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Build a dev container image
    Build {
        /// Workspace folder path (defaults to current directory)
        #[arg(default_value = ".")]
        workspace_folder: PathBuf,

        /// Docker CLI path
        #[arg(long)]
        docker_path: Option<String>,

        /// Docker Compose CLI path
        #[arg(long)]
        docker_compose_path: Option<String>,

        /// Build with --no-cache
        #[arg(long)]
        no_cache: bool,

        /// Image name (for Dockerfile builds)
        #[arg(long)]
        image_name: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Read and output devcontainer configuration
    ReadConfiguration {
        /// Workspace folder path (defaults to current directory)
        #[arg(default_value = ".")]
        workspace_folder: PathBuf,

        /// Output as JSON (always JSON for this command)
        #[arg(long)]
        json: bool,
    },

    // === Configuration commands ===
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

    /// Configure devcontainer workspace settings for worktree compatibility
    Configure {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Detect main service, container user, and ports from devcontainer
    Detect {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Stack hint for port detection (e.g., flask, rails, node)
        #[arg(short, long)]
        stack: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Add cloudflared tunnel service to compose file
    AddTunnel {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Main service name to add depends_on (auto-detected if not specified)
        #[arg(short, long)]
        service: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Inject AI coding agent volume mounts into compose file
    InjectAgents {
        /// Project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub fn execute(cmd: DevcontainerCommands) -> Result<()> {
    match cmd {
        // Runtime commands
        DevcontainerCommands::Up {
            workspace_folder,
            docker_path,
            docker_compose_path,
            remove_existing_container,
            build_no_cache,
            skip_post_create,
            remote_env,
            json,
        } => cmd_up(
            workspace_folder,
            docker_path,
            docker_compose_path,
            remove_existing_container,
            build_no_cache,
            skip_post_create,
            remote_env,
            json,
        ),
        DevcontainerCommands::Exec {
            cmd: command,
            workspace_folder,
            docker_path,
            docker_compose_path,
            user,
            workdir,
            remote_env,
            json,
        } => cmd_exec(
            command,
            workspace_folder,
            docker_path,
            docker_compose_path,
            user,
            workdir,
            remote_env,
            json,
        ),
        DevcontainerCommands::Down {
            workspace_folder,
            docker_path,
            docker_compose_path,
            volumes,
            remove_orphans,
            json,
        } => cmd_down(
            workspace_folder,
            docker_path,
            docker_compose_path,
            volumes,
            remove_orphans,
            json,
        ),
        DevcontainerCommands::Build {
            workspace_folder,
            docker_path,
            docker_compose_path,
            no_cache,
            image_name,
            json,
        } => cmd_build(
            workspace_folder,
            docker_path,
            docker_compose_path,
            no_cache,
            image_name,
            json,
        ),
        DevcontainerCommands::ReadConfiguration {
            workspace_folder,
            json,
        } => cmd_read_configuration(workspace_folder, json),

        // Configuration commands
        DevcontainerCommands::Sync {
            path,
            strategy,
            dry_run,
        } => sync(path, strategy, dry_run),
        DevcontainerCommands::Configure { path, json } => configure(path, json),
        DevcontainerCommands::Detect { path, stack, json } => detect(path, stack, json),
        DevcontainerCommands::AddTunnel {
            path,
            service,
            json,
        } => add_tunnel(path, service, json),
        DevcontainerCommands::InjectAgents { path, json } => inject_agents(path, json),
    }
}

// === Runtime command implementations ===

fn parse_env_vars(env_args: Vec<String>) -> HashMap<String, String> {
    env_args
        .into_iter()
        .filter_map(|s| {
            let parts: Vec<&str> = s.splitn(2, '=').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn cmd_up(
    workspace_folder: PathBuf,
    docker_path: Option<String>,
    docker_compose_path: Option<String>,
    remove_existing_container: bool,
    build_no_cache: bool,
    skip_post_create: bool,
    remote_env: Vec<String>,
    json_output: bool,
) -> Result<()> {
    let runtime =
        DevcontainerRuntime::with_docker(&workspace_folder, docker_path, docker_compose_path)?;

    if !runtime.is_docker_available() {
        if json_output {
            println!(
                "{}",
                serde_json::json!({"outcome": "error", "message": "Docker is not available"})
            );
        } else {
            eprintln!("Error: Docker is not available. Please ensure Docker is installed and running.");
        }
        std::process::exit(1);
    }

    let options = UpOptions {
        remove_existing: remove_existing_container,
        build_no_cache,
        skip_post_create,
        remote_env: parse_env_vars(remote_env),
    };

    let result = runtime.up(options).context("Failed to start devcontainer")?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Devcontainer Up");
        println!();
        println!("Outcome:          {}", result.outcome);
        println!("Container ID:     {}", result.container_id);
        if let Some(user) = &result.remote_user {
            println!("Remote User:      {}", user);
        }
        println!("Workspace Folder: {}", result.remote_workspace_folder);
        if let Some(project) = &result.compose_project_name {
            println!("Compose Project:  {}", project);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_exec(
    command: Vec<String>,
    workspace_folder: PathBuf,
    docker_path: Option<String>,
    docker_compose_path: Option<String>,
    user: Option<String>,
    workdir: Option<String>,
    remote_env: Vec<String>,
    json_output: bool,
) -> Result<()> {
    if command.is_empty() {
        anyhow::bail!("No command specified");
    }

    let runtime =
        DevcontainerRuntime::with_docker(&workspace_folder, docker_path, docker_compose_path)?;

    let options = ExecOptions {
        user,
        workdir,
        remote_env: parse_env_vars(remote_env),
    };

    let result = runtime.exec(&command, options)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        // For non-JSON output, just print stdout/stderr directly
        if !result.stdout.is_empty() {
            print!("{}", result.stdout);
        }
        if !result.stderr.is_empty() {
            eprint!("{}", result.stderr);
        }
    }

    // Exit with the same code as the executed command
    if result.exit_code != 0 {
        std::process::exit(result.exit_code);
    }

    Ok(())
}

fn cmd_down(
    workspace_folder: PathBuf,
    docker_path: Option<String>,
    docker_compose_path: Option<String>,
    volumes: bool,
    remove_orphans: bool,
    json_output: bool,
) -> Result<()> {
    let runtime =
        DevcontainerRuntime::with_docker(&workspace_folder, docker_path, docker_compose_path)?;

    let options = DownOptions {
        volumes,
        remove_orphans,
    };

    let result = runtime.down(options).context("Failed to stop devcontainer")?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Devcontainer Down");
        println!();
        println!("Outcome: {}", result.outcome);
        if let Some(containers) = &result.removed_containers {
            println!("Removed containers:");
            for id in containers {
                println!("  - {}", id);
            }
        }
    }

    Ok(())
}

fn cmd_build(
    workspace_folder: PathBuf,
    docker_path: Option<String>,
    docker_compose_path: Option<String>,
    no_cache: bool,
    image_name: Option<String>,
    json_output: bool,
) -> Result<()> {
    let runtime =
        DevcontainerRuntime::with_docker(&workspace_folder, docker_path, docker_compose_path)?;

    let result = runtime
        .build(no_cache, image_name)
        .context("Failed to build devcontainer")?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Devcontainer Build");
        println!();
        println!("Outcome: {}", result.outcome);
        if let Some(image) = &result.image_name {
            println!("Image:   {}", image);
        }
    }

    Ok(())
}

fn cmd_read_configuration(workspace_folder: PathBuf, json_output: bool) -> Result<()> {
    let runtime = DevcontainerRuntime::new(&workspace_folder)?;
    let config = runtime.read_configuration();

    if json_output {
        println!("{}", serde_json::to_string_pretty(&config)?);
    } else {
        println!("Devcontainer Configuration");
        println!();
        println!("Workspace:      {}", config.workspace_folder);
        println!("Config Path:    {}", config.config_path);
        println!("Container Type: {}", config.container_type);
        if let Some(name) = &config.configuration.name {
            println!("Name:           {}", name);
        }
        if let Some(image) = &config.configuration.image {
            println!("Image:          {}", image);
        }
        if let Some(service) = &config.configuration.service {
            println!("Service:        {}", service);
        }
    }

    Ok(())
}

// === Configuration command implementations ===

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

// JSON output structures
#[derive(Serialize)]
struct ConfigureOutput {
    devcontainer_modified: bool,
    compose_modified: bool,
    changes: Vec<String>,
}

#[derive(Serialize)]
struct DetectOutput {
    service_name: Option<String>,
    port: u16,
    service_url: String,
    container_user: String,
    home_path: String,
}

#[derive(Serialize)]
struct AddTunnelOutput {
    compose_modified: bool,
    env_created: bool,
    changes: Vec<String>,
}

#[derive(Serialize)]
struct InjectAgentsOutput {
    compose_modified: bool,
    container_user: Option<String>,
    home_path: Option<String>,
    changes: Vec<String>,
}

fn configure(path: Option<PathBuf>, json_output: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    let devcontainer_dir = project_path.join(".devcontainer");
    if !devcontainer_dir.exists() {
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "error": "No .devcontainer directory found"
                })
            );
        } else {
            eprintln!("No .devcontainer directory found at {}", project_path.display());
        }
        std::process::exit(1);
    }

    let outcome = configure_workspace_settings(&devcontainer_dir)
        .context("Failed to configure workspace settings")?;

    if json_output {
        let output = ConfigureOutput {
            devcontainer_modified: outcome.devcontainer_modified,
            compose_modified: outcome.compose_modified,
            changes: outcome.changes,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Devcontainer Workspace Configuration");
        println!();

        if outcome.changes.is_empty() {
            println!("No changes needed - workspace settings are already configured.");
        } else {
            println!("Changes made:");
            for change in &outcome.changes {
                println!("  - {}", change);
            }
            println!();
            if outcome.devcontainer_modified {
                println!("Updated devcontainer.json");
            }
            if outcome.compose_modified {
                println!("Updated compose file");
            }
        }
    }

    Ok(())
}

fn detect(path: Option<PathBuf>, stack: Option<String>, json_output: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    let devcontainer_dir = project_path.join(".devcontainer");
    if !devcontainer_dir.exists() {
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "error": "No .devcontainer directory found"
                })
            );
        } else {
            eprintln!("No .devcontainer directory found at {}", project_path.display());
        }
        std::process::exit(1);
    }

    let stack_hint = stack.as_deref();
    let service_info =
        detect_main_service(&devcontainer_dir, stack_hint).context("Failed to detect service")?;

    // Read devcontainer.json for container user detection
    let config_path = devcontainer_dir.join("devcontainer.json");
    let container_user = if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)?;
        let config: serde_json::Value = serde_json::from_str(&contents).unwrap_or_default();
        detect_container_user(&devcontainer_dir, &config)
    } else {
        worktree_core::modules::ContainerUser::default()
    };

    if json_output {
        let output = DetectOutput {
            service_name: service_info.name,
            port: service_info.port,
            service_url: service_info.service_url,
            container_user: container_user.username,
            home_path: container_user.home_path,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Devcontainer Detection");
        println!();
        println!(
            "Service:        {}",
            service_info.name.as_deref().unwrap_or("(not detected)")
        );
        println!("Port:           {}", service_info.port);
        println!("Service URL:    {}", service_info.service_url);
        println!("Container User: {}", container_user.username);
        println!("Home Path:      {}", container_user.home_path);
    }

    Ok(())
}

fn add_tunnel(path: Option<PathBuf>, service: Option<String>, json_output: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    let devcontainer_dir = project_path.join(".devcontainer");
    if !devcontainer_dir.exists() {
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "error": "No .devcontainer directory found"
                })
            );
        } else {
            eprintln!("No .devcontainer directory found at {}", project_path.display());
        }
        std::process::exit(1);
    }

    let outcome = add_cloudflared_service(&devcontainer_dir, service.as_deref())
        .context("Failed to add cloudflared service")?;

    if json_output {
        let output = AddTunnelOutput {
            compose_modified: outcome.compose_modified,
            env_created: outcome.env_created,
            changes: outcome.changes,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Add Cloudflared Tunnel Service");
        println!();

        if outcome.changes.is_empty() {
            println!("No changes made - cloudflared service may already exist or no compose file found.");
        } else {
            println!("Changes made:");
            for change in &outcome.changes {
                println!("  - {}", change);
            }
            println!();
            if outcome.compose_modified {
                println!("Updated compose file with cloudflared service");
            }
            if outcome.env_created {
                println!("Created .cloudflared.env template");
                println!();
                println!("Next steps:");
                println!("  1. Get your tunnel token from Cloudflare Zero Trust dashboard");
                println!("  2. Add TUNNEL_TOKEN to .devcontainer/.cloudflared.env");
            }
        }
    }

    Ok(())
}

fn inject_agents(path: Option<PathBuf>, json_output: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    let devcontainer_dir = project_path.join(".devcontainer");
    if !devcontainer_dir.exists() {
        if json_output {
            println!(
                "{}",
                serde_json::json!({
                    "error": "No .devcontainer directory found"
                })
            );
        } else {
            eprintln!("No .devcontainer directory found at {}", project_path.display());
        }
        std::process::exit(1);
    }

    let outcome =
        inject_coding_agent_mounts(&devcontainer_dir).context("Failed to inject agent mounts")?;

    if json_output {
        let output = InjectAgentsOutput {
            compose_modified: outcome.compose_modified,
            container_user: outcome.container_user.as_ref().map(|u| u.username.clone()),
            home_path: outcome.container_user.as_ref().map(|u| u.home_path.clone()),
            changes: outcome.changes,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Inject AI Coding Agent Mounts");
        println!();

        if let Some(user) = &outcome.container_user {
            println!("Container User: {} ({})", user.username, user.home_path);
            println!();
        }

        if outcome.changes.is_empty() {
            println!("No changes made - mounts may already exist or no compose file found.");
        } else {
            println!("Changes made:");
            for change in &outcome.changes {
                println!("  - {}", change);
            }
            println!();
            if outcome.compose_modified {
                println!("Updated compose file with coding agent mounts:");
                println!("  - .codex   (OpenAI Codex CLI)");
                println!("  - .claude  (Claude Code)");
                println!("  - .gh      (GitHub CLI)");
            }
        }
    }

    Ok(())
}
