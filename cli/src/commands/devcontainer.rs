//! Devcontainer commands
//!
//! Commands for managing devcontainer configuration and lifecycle.
//!
//! This module provides two sets of commands:
//! 1. **Configuration commands** (sync, configure, detect, add-tunnel, inject-agents)
//!    - Manage devcontainer.json and compose files
//! 2. **Runtime commands** (up, exec, down, build, read-configuration)
//!    - Manage container lifecycle (equivalent to @devcontainers/cli)

use crate::json_error::CliError;
use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use worktree_core::devcontainer_runtime::{
    DevcontainerConfig, DevcontainerRuntime, DevcontainerType, DownOptions, ExecOptions, UpOptions,
};
use worktree_core::modules::{
    add_cloudflared_service, configure_workspace_settings, detect_container_user,
    detect_main_service_from_config, inject_coding_agent_mounts, DevcontainerModule, Module,
    SyncStrategy,
};
use worktree_core::workflows::feature::{FeatureMetadata, FeatureStatus, FeatureWorkflow};
use worktree_core::{human, humanln, output};

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &["devcontainer-sync-json"];

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

        /// Remote environment variables for lifecycle commands (name=value)
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

        /// Remote environment variables for this command (name=value; overrides remoteEnv)
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

        /// Sync only this feature (repeatable). Any registered feature that has not been
        /// removed can be named; without it, every active feature is synced
        #[arg(long = "feature", value_name = "NAME")]
        features: Vec<String>,

        /// Emit the per-worktree results as JSON
        #[arg(long)]
        json: bool,
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

impl DevcontainerCommands {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        match self {
            DevcontainerCommands::Up { json, .. }
            | DevcontainerCommands::Exec { json, .. }
            | DevcontainerCommands::Down { json, .. }
            | DevcontainerCommands::Build { json, .. }
            | DevcontainerCommands::ReadConfiguration { json, .. }
            | DevcontainerCommands::Configure { json, .. }
            | DevcontainerCommands::Detect { json, .. }
            | DevcontainerCommands::AddTunnel { json, .. }
            | DevcontainerCommands::InjectAgents { json, .. }
            | DevcontainerCommands::Sync { json, .. } => *json,
        }
    }
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
            features,
            json,
        } => sync(SyncArgs {
            path,
            strategy,
            dry_run,
            features,
            json,
        }),
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
            output::emit_json(
                &serde_json::json!({"outcome": "error", "message": "Docker is not available"}),
            )?;
        } else {
            eprintln!(
                "Error: Docker is not available. Please ensure Docker is installed and running."
            );
        }
        std::process::exit(1);
    }

    let options = UpOptions {
        remove_existing: remove_existing_container,
        build_no_cache,
        skip_post_create,
        remote_env: parse_env_vars(remote_env),
    };

    let result = runtime
        .up(options)
        .context("Failed to start devcontainer")?;

    if json_output {
        output::emit_json(&result)?;
    } else {
        humanln!("Devcontainer Up");
        humanln!();
        humanln!("Outcome:          {}", result.outcome);
        humanln!("Container ID:     {}", result.container_id);
        if let Some(user) = &result.remote_user {
            humanln!("Remote User:      {}", user);
        }
        humanln!("Workspace Folder: {}", result.remote_workspace_folder);
        if let Some(project) = &result.compose_project_name {
            humanln!("Compose Project:  {}", project);
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
        output::emit_json(&result)?;
    } else {
        // For non-JSON output, just print stdout/stderr directly
        if !result.stdout.is_empty() {
            human!("{}", result.stdout);
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

    let result = runtime
        .down(options)
        .context("Failed to stop devcontainer")?;

    if json_output {
        output::emit_json(&result)?;
    } else {
        humanln!("Devcontainer Down");
        humanln!();
        humanln!("Outcome: {}", result.outcome);
        if let Some(containers) = &result.removed_containers {
            humanln!("Removed containers:");
            for id in containers {
                humanln!("  - {}", id);
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
        output::emit_json(&result)?;
    } else {
        humanln!("Devcontainer Build");
        humanln!();
        humanln!("Outcome: {}", result.outcome);
        if let Some(image) = &result.image_name {
            humanln!("Image:   {}", image);
        }
    }

    Ok(())
}

fn cmd_read_configuration(workspace_folder: PathBuf, json_output: bool) -> Result<()> {
    let runtime = DevcontainerRuntime::new(&workspace_folder)?;
    let config = runtime.read_configuration();

    if json_output {
        output::emit_json(&config)?;
    } else {
        humanln!("Devcontainer Configuration");
        humanln!();
        humanln!("Workspace:      {}", config.workspace_folder);
        humanln!("Config Path:    {}", config.config_path);
        humanln!("Container Type: {}", config.container_type);
        if let Some(name) = &config.configuration.name {
            humanln!("Name:           {}", name);
        }
        if let Some(image) = &config.configuration.image {
            humanln!("Image:          {}", image);
        }
        if let Some(service) = &config.configuration.service {
            humanln!("Service:        {}", service);
        }
    }

    Ok(())
}

// === Configuration command implementations ===

/// `schema_version` of the `devcontainer sync --json` payload.
const SYNC_SCHEMA_VERSION: u32 = 1;

/// The `devcontainer sync` flags.
struct SyncArgs {
    path: Option<PathBuf>,
    strategy: Option<String>,
    dry_run: bool,
    features: Vec<String>,
    json: bool,
}

/// What happened to one feature worktree (DESIGN §5.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SyncStatus {
    Synced,
    WouldSync,
    Skipped,
    Failed,
}

/// One row of the sync report.
#[derive(Debug, Serialize)]
struct SyncResult {
    work_feature: String,
    worktree_path: PathBuf,
    status: SyncStatus,
    /// Files written (paths relative to `.devcontainer`); empty unless `synced`.
    files: Vec<String>,
    skip_reason: Option<String>,
    error: Option<String>,
    /// Whether the feature's registry entry recorded this sync (`last_sync_at`,
    /// `devcontainer_outdated`, `sync_strategy`).
    registry_updated: bool,
    /// Why the registry entry could not be updated; the text report shows it.
    #[serde(skip)]
    registry_error: Option<String>,
}

impl SyncResult {
    fn new(feature: &FeatureMetadata, status: SyncStatus) -> Self {
        Self {
            work_feature: feature.work_feature.clone(),
            worktree_path: feature.worktree_path.clone(),
            status,
            files: Vec::new(),
            skip_reason: None,
            error: None,
            registry_updated: false,
            registry_error: None,
        }
    }
}

/// The `devcontainer sync --json` payload (DESIGN §5.8). The counts add up to the rows: in a
/// dry run `synced` counts the worktrees that would be synced.
#[derive(Debug, Serialize)]
struct SyncReport {
    schema_version: u32,
    dry_run: bool,
    strategy: &'static str,
    results: Vec<SyncResult>,
    synced: usize,
    failed: usize,
    skipped: usize,
}

impl SyncReport {
    fn new(dry_run: bool, strategy: SyncStrategy, results: Vec<SyncResult>) -> Self {
        let count = |wanted: &[SyncStatus]| {
            results
                .iter()
                .filter(|result| wanted.contains(&result.status))
                .count()
        };
        Self {
            schema_version: SYNC_SCHEMA_VERSION,
            dry_run,
            strategy: strategy.as_str(),
            synced: count(&[SyncStatus::Synced, SyncStatus::WouldSync]),
            failed: count(&[SyncStatus::Failed]),
            skipped: count(&[SyncStatus::Skipped]),
            results,
        }
    }
}

/// Sync the main worktree's `.devcontainer` into feature worktrees: every active feature, or
/// the ones named with `--feature`. Text mode prints each worktree's row as it finishes; JSON
/// mode collects every result, then prints one document. Any failed worktree makes the command
/// exit 1 (after the report, in text and JSON mode alike).
fn sync(args: SyncArgs) -> Result<()> {
    let report = collect_sync(&args, !args.json)?;
    if args.json {
        output::emit_json(&report)?;
    } else {
        print_sync_summary(&report);
    }

    if report.failed > 0 {
        let failed: Vec<&str> = report
            .results
            .iter()
            .filter(|result| result.status == SyncStatus::Failed)
            .map(|result| result.work_feature.as_str())
            .collect();
        anyhow::bail!(
            "Devcontainer sync failed for {} of {} feature worktree(s): {}",
            report.failed,
            report.results.len(),
            failed.join(", ")
        );
    }
    Ok(())
}

/// Sync each target and collect the rows; with `print_rows`, the text report's header and each
/// row are printed as the sync goes (refusals happen before anything is printed).
fn collect_sync(args: &SyncArgs, print_rows: bool) -> Result<SyncReport> {
    let project_path = args.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    // The devcontainer module reads the strategy from the environment.
    if let Some(strategy) = &args.strategy {
        std::env::set_var("BRANCHBOX_DEVCONTAINER_STRATEGY", strategy);
    }

    let workflow = FeatureWorkflow::new(&project_path)?;
    let features = workflow
        .list_features()
        .context("Failed to list features")?;
    let registry = workflow.repo_root().join(".branchbox/registry.json");
    let selected = select_sync_targets(features, &args.features, &registry)?;

    // A dry run, or a run with nothing to sync, reports without the source (as 0.13 did).
    let source = workflow.repo_root().join(".devcontainer");
    let module = if source.is_dir() {
        let mut module = DevcontainerModule::new();
        module
            .init(workflow.repo_root(), workflow.repo_root())
            .context("Failed to initialize devcontainer module")?;
        Some(module)
    } else if args.dry_run || selected.is_empty() {
        None
    } else {
        return Err(CliError::devcontainer_source_missing(&source).into());
    };
    let strategy = module
        .as_ref()
        .map_or_else(requested_strategy, DevcontainerModule::strategy);

    if print_rows {
        print_sync_header(selected.len(), args.dry_run);
    }
    let results = selected
        .iter()
        .map(|feature| {
            if print_rows {
                human!("  {} ... ", feature.work_feature);
            }
            let result = if !feature.worktree_path.exists() {
                let mut result = SyncResult::new(feature, SyncStatus::Skipped);
                result.skip_reason = Some(format!(
                    "worktree not found at {}",
                    feature.worktree_path.display()
                ));
                result
            } else {
                // Without a dry run there is always a module here: a missing source was refused.
                match module.as_ref().filter(|_| !args.dry_run) {
                    Some(module) => sync_feature(&workflow, module, feature),
                    None => SyncResult::new(feature, SyncStatus::WouldSync),
                }
            };
            if print_rows {
                print_sync_row(&result, strategy.as_str());
            }
            result
        })
        .collect();

    Ok(SyncReport::new(args.dry_run, strategy, results))
}

/// The features to sync: every active one by default; with `--feature`, exactly those named
/// (any status but `removed`), in the order given. A name the registry does not hold, or holds
/// only as removed, is refused before anything is synced.
fn select_sync_targets(
    features: Vec<FeatureMetadata>,
    names: &[String],
    registry: &Path,
) -> Result<Vec<FeatureMetadata>> {
    if names.is_empty() {
        return Ok(features
            .into_iter()
            .filter(|feature| feature.status == FeatureStatus::Active)
            .collect());
    }

    let mut selected: Vec<FeatureMetadata> = Vec::new();
    for name in names {
        if selected.iter().any(|feature| &feature.work_feature == name) {
            continue;
        }
        match features
            .iter()
            .find(|feature| &feature.work_feature == name)
        {
            Some(feature) if feature.status != FeatureStatus::Removed => {
                selected.push(feature.clone())
            }
            Some(_) => {
                return Err(CliError::new(
                    "feature_not_found",
                    format!(
                        "Feature '{name}' was removed (see {}); only features that still have a \
                         worktree can be synced",
                        registry.display()
                    ),
                )
                .with_details(json!({ "name": name, "registry": registry.display().to_string() }))
                .into())
            }
            None => return Err(CliError::feature_not_found(name, registry).into()),
        }
    }
    Ok(selected)
}

/// Sync one worktree, record its baseline (so teardown recognises the synced files as
/// BranchBox's own) and record the outcome in the registry.
fn sync_feature(
    workflow: &FeatureWorkflow,
    module: &DevcontainerModule,
    feature: &FeatureMetadata,
) -> SyncResult {
    let outcome = module.sync_to(&feature.worktree_path).and_then(|outcome| {
        module
            .record_baseline(&feature.worktree_path)
            .map(|()| outcome)
            .map_err(|err| {
                worktree_core::Error::other(format!(
                    "synced the devcontainer files but failed to record the sync baseline: {err}"
                ))
            })
    });

    let (mut result, strategy, success) = match outcome {
        Ok(outcome) => {
            let mut result = SyncResult::new(feature, SyncStatus::Synced);
            result.files = outcome.synced_files;
            // Directory walk order varies by filesystem; reports list files sorted.
            result.files.sort();
            (result, outcome.strategy, true)
        }
        Err(err) => {
            let mut result = SyncResult::new(feature, SyncStatus::Failed);
            result.error = Some(err.to_string());
            (result, module.strategy(), false)
        }
    };
    match workflow.record_devcontainer_sync(&feature.work_feature, Some(strategy.as_str()), success)
    {
        Ok(()) => result.registry_updated = true,
        Err(err) => result.registry_error = Some(err.to_string()),
    }
    result
}

/// The strategy the devcontainer module would pick, for reports made without one: `symlink`
/// when `BRANCHBOX_DEVCONTAINER_STRATEGY` asks for it, otherwise `copy`.
fn requested_strategy() -> SyncStrategy {
    match std::env::var("BRANCHBOX_DEVCONTAINER_STRATEGY") {
        Ok(value) if value.eq_ignore_ascii_case("symlink") => SyncStrategy::Symlink,
        _ => SyncStrategy::Copy,
    }
}

/// The 0.13 text report, line for line (clients of 0.13 parse `✓ synced`, `would sync`,
/// `✗ failed: …` and `N error(s) occurred`).
/// The text report's opening lines, before any worktree is synced.
fn print_sync_header(count: usize, dry_run: bool) {
    if count == 0 {
        humanln!("No active feature worktrees found");
        return;
    }
    humanln!(
        "🔄 Syncing devcontainer configuration to {} feature worktree(s)",
        count
    );
    humanln!();
    if dry_run {
        humanln!("DRY RUN - no changes will be made");
        humanln!();
    }
}

/// The rest of one worktree's text row (its `  name ... ` prefix is printed before the sync).
fn print_sync_row(result: &SyncResult, strategy: &str) {
    match result.status {
        SyncStatus::Synced => humanln!("✓ synced {} files ({})", result.files.len(), strategy),
        SyncStatus::WouldSync => humanln!("would sync"),
        SyncStatus::Skipped => {
            humanln!("⚠️  {}", result.skip_reason.as_deref().unwrap_or("skipped"))
        }
        SyncStatus::Failed => {
            humanln!("✗ failed: {}", result.error.as_deref().unwrap_or("unknown"))
        }
    }
    if let Some(err) = &result.registry_error {
        humanln!("    ⚠️ failed to update registry: {}", err);
    }
}

/// The text report's closing lines, after every row.
fn print_sync_summary(report: &SyncReport) {
    if report.results.is_empty() {
        return;
    }
    humanln!();
    humanln!(
        "✓ Successfully synced {} feature worktree(s)",
        report.synced
    );

    if report.failed > 0 {
        humanln!();
        humanln!("⚠️  {} error(s) occurred:", report.failed);
        for result in &report.results {
            if let (SyncStatus::Failed, Some(error)) = (result.status, &result.error) {
                humanln!("  - {}: {}", result.work_feature, error);
            }
        }
    }
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
    container_type: &'static str,
    configured_user: Option<String>,
    workspace_folder: String,
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
            output::emit_json(&serde_json::json!({
                "error": "No .devcontainer directory found"
            }))?;
        } else {
            eprintln!(
                "No .devcontainer directory found at {}",
                project_path.display()
            );
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
        output::emit_json(&output)?;
    } else {
        humanln!("Devcontainer Workspace Configuration");
        humanln!();

        if outcome.changes.is_empty() {
            humanln!("No changes needed - workspace settings are already configured.");
        } else {
            humanln!("Changes made:");
            for change in &outcome.changes {
                humanln!("  - {}", change);
            }
            humanln!();
            if outcome.devcontainer_modified {
                humanln!("Updated devcontainer.json");
            }
            if outcome.compose_modified {
                humanln!("Updated compose file");
            }
        }
    }

    Ok(())
}

fn detect(path: Option<PathBuf>, stack: Option<String>, json_output: bool) -> Result<()> {
    let project_path = path.unwrap_or_else(|| PathBuf::from("."));
    let project_path =
        std::fs::canonicalize(&project_path).context("Failed to resolve project path")?;

    let standard_config = project_path.join(".devcontainer/devcontainer.json");
    let config_path = if standard_config.exists() {
        standard_config
    } else {
        project_path.join(".devcontainer.json")
    };
    if !config_path.exists() {
        if json_output {
            output::emit_json(&serde_json::json!({
                "error": "No devcontainer configuration found"
            }))?;
        } else {
            eprintln!(
                "No devcontainer configuration found at {}",
                project_path.display()
            );
        }
        std::process::exit(1);
    }

    let devcontainer_dir = config_path.parent().unwrap_or(&project_path);
    let contents = std::fs::read_to_string(&config_path)?;
    // Detection reads only its own fields. Unrelated valid fields (e.g. nullable remoteEnv values)
    // must not fail because a runtime implementation models them more narrowly.
    let config: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&contents, &Default::default())
            .map_err(|err| {
                anyhow::anyhow!(
                    "Failed to parse {} as JSONC: {err:?}",
                    config_path.display()
                )
            })?
            .unwrap_or_default();
    let runtime_config = DevcontainerConfig {
        remote_user: config
            .get("remoteUser")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        container_user: config
            .get("containerUser")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        workspace_folder: config
            .get("workspaceFolder")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        ..DevcontainerConfig::default()
    };
    let container_type = if config
        .get("dockerComposeFile")
        .is_some_and(|value| !value.is_null())
    {
        DevcontainerType::DockerCompose
    } else if config.get("build").is_some_and(|value| !value.is_null())
        || config
            .get("dockerFile")
            .is_some_and(|value| !value.is_null())
    {
        DevcontainerType::Dockerfile
    } else {
        DevcontainerType::Image
    };
    // Only active Compose configurations have a Compose service. An image/Dockerfile workspace can
    // still contain unused scaffold files; those do not describe its running environment.
    let service_info = if container_type == DevcontainerType::DockerCompose {
        detect_main_service_from_config(devcontainer_dir, &config, stack.as_deref())
            .context("Failed to detect service")?
    } else {
        worktree_core::modules::ServiceInfo {
            name: None,
            port: 0,
            service_url: String::new(),
        }
    };

    let mut container_user = detect_container_user(devcontainer_dir, &config);
    let configured_user = runtime_config.effective_remote_user().map(str::to_owned);
    if let Some(user) = &configured_user {
        container_user.username = user.clone();
        container_user.home_path = if user == "root" {
            "/root".to_string()
        } else {
            format!("/home/{user}")
        };
    }
    let workspace_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let workspace_folder = runtime_config.effective_workspace_folder(workspace_name);
    let container_type = match container_type {
        DevcontainerType::DockerCompose => "compose",
        DevcontainerType::Dockerfile => "dockerfile",
        DevcontainerType::Image => "image",
    };

    if json_output {
        let output = DetectOutput {
            service_name: service_info.name,
            port: service_info.port,
            service_url: service_info.service_url,
            container_user: container_user.username,
            home_path: container_user.home_path,
            container_type,
            configured_user,
            workspace_folder,
        };
        output::emit_json(&output)?;
    } else {
        humanln!("Devcontainer Detection");
        humanln!();
        humanln!(
            "Service:        {}",
            service_info.name.as_deref().unwrap_or("(not detected)")
        );
        humanln!("Port:           {}", service_info.port);
        humanln!("Service URL:    {}", service_info.service_url);
        humanln!("Container User: {}", container_user.username);
        humanln!("Home Path:      {}", container_user.home_path);
        humanln!("Container Type: {}", container_type);
        humanln!("Workspace:      {}", workspace_folder);
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
            output::emit_json(&serde_json::json!({
                "error": "No .devcontainer directory found"
            }))?;
        } else {
            eprintln!(
                "No .devcontainer directory found at {}",
                project_path.display()
            );
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
        output::emit_json(&output)?;
    } else {
        humanln!("Add Cloudflared Tunnel Service");
        humanln!();

        if outcome.changes.is_empty() {
            humanln!(
                "No changes made - cloudflared service may already exist or no compose file found."
            );
        } else {
            humanln!("Changes made:");
            for change in &outcome.changes {
                humanln!("  - {}", change);
            }
            humanln!();
            if outcome.compose_modified {
                humanln!("Updated compose file with cloudflared service");
            }
            if outcome.env_created {
                humanln!("Created .cloudflared.env template");
                humanln!();
                humanln!("Next steps:");
                humanln!("  1. Get your tunnel token from Cloudflare Zero Trust dashboard");
                humanln!("  2. Add TUNNEL_TOKEN to .devcontainer/.cloudflared.env");
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
            output::emit_json(&serde_json::json!({
                "error": "No .devcontainer directory found"
            }))?;
        } else {
            eprintln!(
                "No .devcontainer directory found at {}",
                project_path.display()
            );
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
        output::emit_json(&output)?;
    } else {
        humanln!("Inject AI Coding Agent Mounts");
        humanln!();

        if let Some(user) = &outcome.container_user {
            humanln!("Container User: {} ({})", user.username, user.home_path);
            humanln!();
        }

        if outcome.changes.is_empty() {
            humanln!("No changes made - mounts may already exist or no compose file found.");
        } else {
            humanln!("Changes made:");
            for change in &outcome.changes {
                humanln!("  - {}", change);
            }
            humanln!();
            if outcome.compose_modified {
                humanln!("Updated compose file with coding agent mounts:");
                humanln!("  - .codex   (OpenAI Codex CLI)");
                humanln!("  - .claude  (Claude Code)");
                humanln!("  - .gh      (GitHub CLI)");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::str::FromStr;
    use worktree_core::workflows::feature::FeatureStatus;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: DevcontainerCommands,
    }

    fn parse(args: &[&str]) -> DevcontainerCommands {
        let mut argv = vec!["devcontainer"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
            .unwrap_or_else(|err| panic!("{args:?}: {err}"))
            .command
    }

    #[test]
    fn sync_json_selects_machine_mode_and_collects_repeated_features() {
        assert!(!parse(&["sync"]).wants_json());
        let command = parse(&["sync", "--json", "--feature", "eta", "--feature", "zeta"]);
        assert!(command.wants_json());
        let DevcontainerCommands::Sync { features, .. } = command else {
            panic!("not a sync");
        };
        assert_eq!(features, ["eta", "zeta"]);
        assert_eq!(CAPABILITIES, ["devcontainer-sync-json"]);
    }

    fn feature(name: &str, status: &str) -> FeatureMetadata {
        serde_json::from_value(json!({
            "work_feature": name,
            "branch_name": format!("feature/{name}"),
            "worktree_path": format!("/r/{name}"),
            "base_branch": null,
            "feature_url": null,
            "compose_project_name": null,
            "env_path": null,
            "status": FeatureStatus::from_str(status).unwrap(),
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "removed_at": null
        }))
        .unwrap()
    }

    #[test]
    fn sync_targets_default_to_active_features_and_follow_feature_order() {
        let registry = Path::new("/r/main/.branchbox/registry.json");
        let features = || {
            vec![
                feature("eta", "active"),
                feature("zeta", "degraded"),
                feature("old", "removed"),
            ]
        };
        let names = |selected: Vec<FeatureMetadata>| -> Vec<String> {
            selected.into_iter().map(|f| f.work_feature).collect()
        };

        assert_eq!(
            names(select_sync_targets(features(), &[], registry).unwrap()),
            ["eta"]
        );
        let named = ["zeta".to_string(), "eta".to_string(), "zeta".to_string()];
        assert_eq!(
            names(select_sync_targets(features(), &named, registry).unwrap()),
            ["zeta", "eta"]
        );

        let removed = select_sync_targets(features(), &["old".to_string()], registry)
            .err()
            .unwrap();
        let removed = removed.downcast_ref::<CliError>().unwrap();
        assert_eq!(removed.code, "feature_not_found");
        assert!(
            removed.message.contains("was removed"),
            "{}",
            removed.message
        );

        let unknown = select_sync_targets(features(), &["nope".to_string()], registry)
            .err()
            .unwrap();
        assert_eq!(
            unknown.downcast_ref::<CliError>().unwrap().details,
            Some(json!({"name": "nope", "registry": "/r/main/.branchbox/registry.json"}))
        );
    }

    #[test]
    fn report_counts_add_up_to_the_rows() {
        let row = |status| SyncResult::new(&feature("eta", "active"), status);
        let report = SyncReport::new(
            true,
            SyncStrategy::Symlink,
            vec![
                row(SyncStatus::WouldSync),
                row(SyncStatus::Skipped),
                row(SyncStatus::Failed),
            ],
        );
        assert_eq!((report.synced, report.skipped, report.failed), (1, 1, 1));
        assert_eq!(report.strategy, "symlink");
        let value = serde_json::to_value(&report).unwrap();
        assert!(value["results"][0].get("registry_error").is_none());
    }
}
