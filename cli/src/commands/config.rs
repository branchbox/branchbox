//! `branchbox config`: read and change `.branchbox/config.json` (DESIGN §5.10, capability
//! `config`).
//!
//! All four subcommands go through the core config engine
//! ([`worktree_core::config_edit`]): edits keep the file's formatting and unknown keys, values
//! are checked against the key registry, and nothing is written when a check fails. `get` and
//! `apply` print the §5.10 payloads with `--json`; `set` and `unset` are for people.

use crate::json_error::CliError;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use worktree_core::config_edit::{self, display_value, ApplyResult, ConfigDocument, ValueSource};
use worktree_core::workflows::feature::FeatureWorkflow;
use worktree_core::{humanln, output};

/// Contract capabilities this module adds to `branchbox version --json` (DESIGN §5.3).
pub const CAPABILITIES: &[&str] = &["config"];

/// Largest merge patch accepted; the whole configuration is a few kilobytes.
const MAX_PATCH_BYTES: u64 = 1024 * 1024;

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show the effective configuration (or one key) with defaults and sources
    Get(ConfigGetArgs),

    /// Set a configuration key
    Set(ConfigSetArgs),

    /// Remove a configuration key so its default applies again
    Unset(ConfigUnsetArgs),

    /// Apply a JSON merge patch (RFC 7386) to the configuration; `null` unsets a key
    Apply(ConfigApplyArgs),
}

#[derive(Args)]
pub struct ConfigGetArgs {
    /// Dotted key to show (e.g. runtime.provider); omit to show every key
    pub key: Option<String>,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Emit JSON output instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ConfigSetArgs {
    /// Dotted key to set (e.g. feature.branch_prefix)
    pub key: String,

    /// New value (parsed as the key's type)
    pub value: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,
}

#[derive(Args)]
pub struct ConfigUnsetArgs {
    /// Dotted key to remove (e.g. feature.branch_prefix)
    pub key: String,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,
}

#[derive(Args)]
pub struct ConfigApplyArgs {
    /// Merge patch to apply: a JSON file, or `-` to read it from stdin
    #[arg(long, value_name = "PATH")]
    pub file: PathBuf,

    /// Report what would change without writing the file
    #[arg(long)]
    pub dry_run: bool,

    /// Repository path (defaults to current directory)
    #[arg(long)]
    pub repo: Option<PathBuf>,

    /// Emit JSON output instead of human-readable text
    #[arg(long)]
    pub json: bool,
}

impl ConfigCommands {
    /// Whether this invocation asked for machine (`--json`) output.
    pub fn wants_json(&self) -> bool {
        match self {
            ConfigCommands::Get(args) => args.json,
            ConfigCommands::Apply(args) => args.json,
            ConfigCommands::Set(_) | ConfigCommands::Unset(_) => false,
        }
    }
}

pub fn execute(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Get(args) => run_get(args),
        ConfigCommands::Set(args) => run_set(args),
        ConfigCommands::Unset(args) => run_unset(args),
        ConfigCommands::Apply(args) => run_apply(args),
    }
}

/// The root of the repository at `repo` (default: the current directory), whose
/// `.branchbox/config.json` the command reads and writes.
fn repo_root(repo: Option<PathBuf>) -> Result<PathBuf> {
    let workflow = FeatureWorkflow::new(repo.unwrap_or_else(|| PathBuf::from(".")))?;
    Ok(workflow.repo_root().to_path_buf())
}

fn run_get(args: ConfigGetArgs) -> Result<()> {
    let root = repo_root(args.repo)?;
    let document = config_edit::get(&root, args.key.as_deref())?;
    if args.json {
        output::emit_json(&document)?;
    } else if args.key.is_some() {
        // One key: just its value, so scripts can use it (`$(branchbox config get KEY)`).
        if let Some(key) = document.keys.first() {
            humanln!("{}", plain_value(&key.value));
        }
    } else {
        print_document(&document);
    }
    Ok(())
}

fn run_set(args: ConfigSetArgs) -> Result<()> {
    let root = repo_root(args.repo)?;
    let result = config_edit::set(&root, &args.key, &args.value)?;
    match result.changed.first() {
        Some(change) => humanln!(
            "✓ Set {} to {} in {}",
            change.key,
            display_value(&change.new),
            result.path.display()
        ),
        None => humanln!(
            "{} is already {}; nothing changed",
            args.key,
            config_value(&result, &args.key)
        ),
    }
    Ok(())
}

fn run_unset(args: ConfigUnsetArgs) -> Result<()> {
    let root = repo_root(args.repo)?;
    let result = config_edit::unset(&root, &args.key)?;
    match result.changed.first() {
        Some(change) => humanln!(
            "✓ Unset {} in {}; the default {} applies",
            change.key,
            result.path.display(),
            config_value(&result, &args.key)
        ),
        None => humanln!(
            "{} is not set in {}; nothing changed (the default {} applies)",
            args.key,
            result.path.display(),
            config_value(&result, &args.key)
        ),
    }
    Ok(())
}

fn run_apply(args: ConfigApplyArgs) -> Result<()> {
    let root = repo_root(args.repo)?;
    let patch = read_patch(&args.file)?;
    let result = config_edit::apply_patch(&root, &patch, args.dry_run)?;
    if args.json {
        output::emit_json(&result)?;
        return Ok(());
    }

    if result.changed.is_empty() {
        humanln!("No changes to {}", result.path.display());
        return Ok(());
    }
    humanln!(
        "{} {}:",
        if args.dry_run {
            "Would change"
        } else {
            "✓ Changed"
        },
        result.path.display()
    );
    for change in &result.changed {
        humanln!(
            "  {}: {} → {}",
            change.key,
            unset_or_value(&change.old),
            unset_or_value(&change.new)
        );
    }
    Ok(())
}

/// Read and parse the merge patch from `file`, or from standard input for `-`.
fn read_patch(file: &Path) -> Result<Value> {
    let (source, text) = if file == Path::new("-") {
        let stdin = std::io::stdin();
        if stdin.is_terminal() && !output::is_interactive() {
            // Machine mode never waits for typed input.
            return Err(CliError::new(
                "validation_failed",
                "--file - reads the patch from standard input, which is a terminal; pipe the \
                 patch in or pass a file. Nothing was changed",
            )
            .into());
        }
        let mut bytes = Vec::new();
        stdin
            .lock()
            .take(MAX_PATCH_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("Failed to read the config patch from standard input")?;
        if bytes.len() as u64 > MAX_PATCH_BYTES {
            return Err(CliError::new(
                "validation_failed",
                format!(
                    "The config patch on standard input is larger than {MAX_PATCH_BYTES} bytes; \
                     nothing was changed"
                ),
            )
            .into());
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            CliError::new(
                "validation_failed",
                "The config patch on standard input is not UTF-8 text; nothing was changed",
            )
        })?;
        ("standard input".to_string(), text)
    } else {
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read the config patch {}", file.display()))?;
        (file.display().to_string(), text)
    };

    serde_json::from_str(&text).map_err(|err| {
        CliError::new(
            "validation_failed",
            format!(
                "The config patch from {source} is not valid JSON (line {}, column {}: {err}); \
                 nothing was changed",
                err.line(),
                err.column()
            ),
        )
        .into()
    })
}

fn print_document(document: &ConfigDocument) {
    humanln!(
        "Configuration: {}{}",
        document.path.display(),
        if document.exists {
            ""
        } else {
            " (not created yet; showing defaults)"
        }
    );
    let width = document
        .keys
        .iter()
        .map(|key| key.key.len())
        .max()
        .unwrap_or_default();
    for key in &document.keys {
        humanln!(
            "  {:width$}  {}{}",
            key.key,
            unset_or_value(&key.value),
            match key.source {
                ValueSource::File => "",
                ValueSource::Default => "  (default)",
            }
        );
    }
}

/// `key`'s effective value after a change, as JSON.
fn config_value(result: &ApplyResult, key: &str) -> String {
    let value = key
        .split('.')
        .try_fold(&result.effective, |value, part| value.get(part))
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| {
            config_edit::lookup_key(key)
                .ok()
                .map(|key| key.default_value())
        })
        .unwrap_or(Value::Null);
    display_value(&value)
}

/// A value for `config get KEY`: strings bare, everything else as JSON.
fn plain_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => display_value(other),
    }
}

fn unset_or_value(value: &Value) -> String {
    if value.is_null() {
        "(unset)".to_string()
    } else {
        display_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: ConfigCommands,
    }

    fn parse(args: &[&str]) -> ConfigCommands {
        let mut argv = vec!["config"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).unwrap().command
    }

    #[test]
    fn get_and_apply_report_their_json_flag() {
        assert!(parse(&["get", "--json"]).wants_json());
        assert!(parse(&["get", "runtime.provider", "--json"]).wants_json());
        assert!(parse(&["apply", "--file", "-", "--json"]).wants_json());
        assert!(!parse(&["apply", "--file", "-"]).wants_json());
        assert!(!parse(&["set", "tunnel.enabled", "false"]).wants_json());
        assert!(!parse(&["unset", "tunnel.enabled"]).wants_json());
        assert_eq!(CAPABILITIES, ["config"]);
    }

    #[test]
    fn values_print_for_people() {
        assert_eq!(plain_value(&Value::from("spike")), "spike");
        assert_eq!(plain_value(&serde_json::json!(["a"])), "[\"a\"]");
        assert_eq!(unset_or_value(&Value::Null), "(unset)");
        assert_eq!(unset_or_value(&Value::Bool(false)), "false");
    }

    #[test]
    fn a_missing_patch_file_names_the_path() {
        let err = read_patch(Path::new("/nonexistent/patch.json")).unwrap_err();
        assert!(
            format!("{err:#}").contains("/nonexistent/patch.json"),
            "{err:#}"
        );
    }
}
