//! Reading and editing `.branchbox/config.json` (`branchbox config`, DESIGN §5.10).
//!
//! Every key the config commands read or write is listed in [`KEYS`] with its type, default,
//! accepted values and description; `docs/docs/reference/configuration.md` is generated from the
//! same table ([`reference_markdown`]).
//!
//! Edits are format-preserving: the file is changed through the jsonc-parser CST, so indentation,
//! key order and keys BranchBox does not know survive. A change is checked before anything is
//! written: each value must fit its key, and the edited file must still strict-parse and load as a
//! [`BranchBoxConfig`]. Otherwise nothing is written, and the refusal names the key and the
//! expected values and, for a file that does not parse or load, the line and column. Comments are
//! refused, because every reader parses the file as strict JSON. Writes take the `.branchbox` lock
//! and replace the file atomically, keeping its permissions (new files are 0644).

use crate::atomic_fs::{self, LOCK_TIMEOUT};
use crate::config::{BranchBoxConfig, CONFIG_VERSION};
use crate::env_placeholders::parse_env_reference;
use crate::{Error, Result};
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde::Serialize;
use serde_json::{Map, Value};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `schema_version` of the `config get --json` and `config apply --json` payloads.
pub const SCHEMA_VERSION: u32 = 1;

/// The JSON type of a configuration key, as `config get --json` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyType {
    Bool,
    String,
    Enum,
    StringList,
}

impl KeyType {
    /// The name `config get --json` and the reference page use.
    pub fn as_str(self) -> &'static str {
        match self {
            KeyType::Bool => "bool",
            KeyType::String => "string",
            KeyType::Enum => "enum",
            KeyType::StringList => "string_list",
        }
    }
}

/// What a value must satisfy beyond its JSON type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// No further check (booleans, and enums, which are checked against `allowed`).
    None,
    /// A non-empty string on one line.
    Text,
    /// `<prefix>/<feature>` must be a valid branch name (`git check-ref-format --branch`).
    BranchPrefix,
    /// A literal Cloudflare account ID, or the `$CLOUDFLARE_ACCOUNT_ID` placeholder the tunnel
    /// provider resolves.
    AccountId,
    /// One DNS label: letters, digits and '-'.
    DnsLabel,
    /// A DNS zone such as `example.com`.
    DnsZone,
    /// Compose service names.
    ServiceNames,
}

/// A key's value when the file does not set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultValue {
    Null,
    Bool(bool),
    Text(&'static str),
    EmptyList,
}

/// One entry of the key registry.
#[derive(Debug)]
pub struct ConfigKey {
    /// Dotted path in `config.json`, e.g. `feature.branch_prefix`.
    pub key: &'static str,
    pub key_type: KeyType,
    /// The accepted values of an enum key (empty otherwise).
    pub allowed: &'static [&'static str],
    /// Whether the file may hold `null` here (the setting is optional).
    nullable: bool,
    default: DefaultValue,
    rule: Rule,
    pub description: &'static str,
}

/// Every key `branchbox config` reads and writes, in the order DESIGN §5.10 lists them.
pub static KEYS: &[ConfigKey] = &[
    ConfigKey {
        key: "runtime.provider",
        key_type: KeyType::Enum,
        allowed: &["container", "sbx", "local-vm", "in-guest"],
        nullable: false,
        default: DefaultValue::Text("container"),
        rule: Rule::None,
        description: "Runtime used when `feature start` gets no `--runtime`: `container` (Docker \
                      on the host), `sbx` (Docker Sandboxes microVMs), `local-vm` (local microVM \
                      driver) or `in-guest` (a devcontainer inside an isolation boundary the \
                      caller owns).",
    },
    ConfigKey {
        key: "runtime.sbx.run_services",
        key_type: KeyType::StringList,
        allowed: &[],
        nullable: false,
        default: DefaultValue::EmptyList,
        rule: Rule::ServiceNames,
        description: "Compose services a devcontainer starts inside Docker Sandboxes. Compose \
                      still starts their declared dependencies; other host-integrated sidecars \
                      stay stopped.",
    },
    ConfigKey {
        key: "feature.branch_prefix",
        key_type: KeyType::String,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Text("feature"),
        rule: Rule::BranchPrefix,
        description: "Prefix of the branch a new feature gets: feature `eta` starts on \
                      `<prefix>/eta`.",
    },
    ConfigKey {
        key: "feature.teardown.delete_branch_by_default",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(true),
        rule: Rule::None,
        description: "Delete the feature branch when the feature is torn down (`--keep-branch` \
                      overrides it for one teardown).",
    },
    ConfigKey {
        key: "feature.teardown.force_delete_unmerged_by_default",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(false),
        rule: Rule::None,
        description: "Delete a feature branch that has unmerged commits (`git branch -D`) \
                      without asking. When false, an unmerged branch is kept or the teardown is \
                      refused, depending on whether a prompt is possible.",
    },
    ConfigKey {
        key: "feature.teardown.prompt_force_delete_unmerged",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(true),
        rule: Rule::None,
        description: "In an interactive terminal, ask before force-deleting a branch with \
                      unmerged commits.",
    },
    ConfigKey {
        key: "tunnel.enabled",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(true),
        rule: Rule::None,
        description: "Provision a public tunnel for each feature.",
    },
    ConfigKey {
        key: "tunnel.default_provider",
        key_type: KeyType::Enum,
        allowed: &["cloudflared"],
        nullable: true,
        default: DefaultValue::Text("cloudflared"),
        rule: Rule::None,
        description: "Tunnel provider for new tunnels.",
    },
    ConfigKey {
        key: "tunnel.providers.cloudflared.account_id",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Null,
        rule: Rule::AccountId,
        description: "Cloudflare account that owns the tunnels, or `$CLOUDFLARE_ACCOUNT_ID` to \
                      read it from the environment. `branchbox tunnel credentials set` sets it \
                      together with the API token.",
    },
    ConfigKey {
        key: "tunnel.providers.cloudflared.tunnel_name_prefix",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Text("branchbox"),
        rule: Rule::DnsLabel,
        description: "Prefix of tunnel names and, with `dns_zone`, of feature hostnames \
                      (`<prefix>-<feature>.<dns_zone>`).",
    },
    ConfigKey {
        key: "tunnel.providers.cloudflared.dns_zone",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Null,
        rule: Rule::DnsZone,
        description: "Cloudflare DNS zone (for example `example.com`) in which tunnel hostnames \
                      are created. When unset, the zone is derived from the feature hostname.",
    },
    ConfigKey {
        key: "tunnel.providers.cloudflared.service_url",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Null,
        rule: Rule::Text,
        description: "Service the tunnel forwards to (for example `http://app:5001`). When \
                      unset, the project's adapter decides.",
    },
    ConfigKey {
        key: "tunnel.providers.cloudflared.manual_instructions",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(false),
        rule: Rule::None,
        description: "Print manual tunnel setup steps instead of provisioning through the \
                      Cloudflare API.",
    },
    ConfigKey {
        key: "tunnel.providers.cloudflared.api_token_path",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Null,
        rule: Rule::Text,
        description: "File holding `CLOUDFLARE_API_TOKEN`, relative to the repository root. \
                      `branchbox tunnel credentials set` writes \
                      `.branchbox/secure/cloudflared.env` (owner-only) and sets this key.",
    },
    ConfigKey {
        key: "editor.default_agent",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Null,
        rule: Rule::Text,
        description: "Coding agent the editor integration prefers (for example `codex` or \
                      `claude`).",
    },
    ConfigKey {
        key: "editor.auto_launch_agent_terminal",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(false),
        rule: Rule::None,
        description: "Open a terminal running the default agent when the editor attaches to a \
                      feature.",
    },
    ConfigKey {
        key: "editor.preferred_sidebar_view",
        key_type: KeyType::String,
        allowed: &[],
        nullable: true,
        default: DefaultValue::Null,
        rule: Rule::Text,
        description: "Editor view to focus on attach (for example `workbench.view.scm`).",
    },
    ConfigKey {
        key: "editor.hide_secondary_sidebar",
        key_type: KeyType::Bool,
        allowed: &[],
        nullable: false,
        default: DefaultValue::Bool(false),
        rule: Rule::None,
        description: "Hide the editor's secondary (right) sidebar on attach.",
    },
];

/// The registry entry for `name`, or [`Error::ConfigUnknownKey`].
pub fn lookup_key(name: &str) -> Result<&'static ConfigKey> {
    KEYS.iter()
        .find(|key| key.key == name)
        .ok_or_else(|| Error::ConfigUnknownKey {
            key: name.to_string(),
        })
}

/// `path` as a section of the registry (a proper prefix of some key, such as `tunnel.providers`).
fn section(path: &str) -> Option<&'static str> {
    KEYS.iter().find_map(|key| {
        key.key
            .strip_prefix(path)
            .filter(|rest| rest.starts_with('.'))
            .map(|_| &key.key[..path.len()])
    })
}

impl ConfigKey {
    /// The value that applies when the file does not set this key.
    pub fn default_value(&self) -> Value {
        match self.default {
            DefaultValue::Null => Value::Null,
            DefaultValue::Bool(value) => Value::Bool(value),
            DefaultValue::Text(value) => Value::from(value),
            DefaultValue::EmptyList => Value::Array(Vec::new()),
        }
    }

    /// What a valid value looks like, as refusals and the reference page say it.
    pub fn expected(&self) -> String {
        match (self.key_type, self.rule) {
            (KeyType::Bool, _) => "true or false".to_string(),
            (KeyType::Enum, _) => format!("one of: {}", self.allowed.join(", ")),
            (KeyType::StringList, _) => {
                "a list of distinct Compose service names (letters, digits, '.', '_' and '-')"
                    .to_string()
            }
            (KeyType::String, Rule::BranchPrefix) => {
                "a branch prefix such that '<prefix>/<feature>' is a valid git branch name"
                    .to_string()
            }
            (KeyType::String, Rule::AccountId) => {
                "a Cloudflare account ID (letters, digits, '-' and '_') or $CLOUDFLARE_ACCOUNT_ID"
                    .to_string()
            }
            (KeyType::String, Rule::DnsLabel) => "letters, digits and '-'".to_string(),
            (KeyType::String, Rule::DnsZone) => "a DNS zone such as example.com".to_string(),
            (KeyType::String, _) => "a non-empty string on one line".to_string(),
        }
    }

    /// Check a value about to be written (from a merge patch, `config set` or another command).
    /// The refusal names the key, the expected values and the value given.
    pub fn validate(&self, value: &Value) -> Result<()> {
        if !self.has_valid_shape(value) || value.is_null() || !self.passes_rule(value)? {
            return Err(self.invalid(value));
        }
        Ok(())
    }

    /// Parse a `config set` argument into this key's value and validate it. Booleans are `true`
    /// or `false`; a list is comma-separated or a JSON array.
    pub fn parse_value(&self, text: &str) -> Result<Value> {
        let value = match self.key_type {
            KeyType::Bool => match text.trim().to_ascii_lowercase().as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::from(text),
            },
            KeyType::StringList if text.trim_start().starts_with('[') => serde_json::from_str(text)
                .map_err(|err| Error::ConfigInvalid {
                    message: format!(
                        "{} must be {}; '{text}' is not a JSON array ({err})",
                        self.key,
                        self.expected()
                    ),
                    key: Some(self.key.to_string()),
                    line: None,
                    column: None,
                    expected: Some(self.expected()),
                })?,
            KeyType::StringList => Value::Array(
                text.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(Value::from)
                    .collect(),
            ),
            KeyType::String | KeyType::Enum => Value::from(text),
        };
        self.validate(&value)?;
        Ok(value)
    }

    /// Whether `value` has the JSON shape serde needs to load this key (the type, and for enums
    /// the allowed values). `null` fits only optional keys.
    fn has_valid_shape(&self, value: &Value) -> bool {
        match (self.key_type, value) {
            (_, Value::Null) => self.nullable,
            (KeyType::Bool, Value::Bool(_)) | (KeyType::String, Value::String(_)) => true,
            (KeyType::Enum, Value::String(text)) => self.allowed.contains(&text.as_str()),
            (KeyType::StringList, Value::Array(items)) => items.iter().all(Value::is_string),
            _ => false,
        }
    }

    /// The semantic check of a well-shaped, non-null value.
    fn passes_rule(&self, value: &Value) -> Result<bool> {
        let single_line =
            |text: &str| !text.trim().is_empty() && !text.chars().any(char::is_control);
        Ok(match (self.rule, value) {
            (Rule::None, _) => true,
            (Rule::Text, Value::String(text)) => single_line(text),
            (Rule::BranchPrefix, Value::String(text)) => {
                single_line(text) && branch_prefix_is_valid(text)?
            }
            (Rule::AccountId, Value::String(text)) => {
                parse_env_reference(text) == Some("CLOUDFLARE_ACCOUNT_ID")
                    || (!text.is_empty()
                        && text
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
            }
            (Rule::DnsLabel, Value::String(text)) => is_dns_label(text),
            (Rule::DnsZone, Value::String(text)) => {
                text.contains('.') && text.split('.').all(is_dns_label)
            }
            (Rule::ServiceNames, Value::Array(items)) => {
                let names: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
                let unique = names
                    .iter()
                    .enumerate()
                    .all(|(index, name)| !names[..index].contains(name));
                unique && names.iter().all(|name| is_service_name(name))
            }
            _ => false,
        })
    }

    fn invalid(&self, value: &Value) -> Error {
        let expected = self.expected();
        Error::ConfigInvalid {
            message: format!(
                "{} must be {expected} (got {}). Nothing was changed.",
                self.key,
                display_value(value)
            ),
            key: Some(self.key.to_string()),
            line: None,
            column: None,
            expected: Some(expected),
        }
    }
}

/// `git check-ref-format --branch <prefix>/x`: whether feature branches under `prefix` are valid.
fn branch_prefix_is_valid(prefix: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", &format!("{prefix}/x")])
        .output()
        .map_err(|err| {
            Error::Io(io::Error::new(
                err.kind(),
                format!("Failed to run `git check-ref-format` to check the branch prefix: {err}"),
            ))
        })?;
    Ok(output.status.success())
}

fn is_dns_label(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 63
        && !text.starts_with('-')
        && !text.ends_with('-')
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

/// Compose service names: a letter or digit, then letters, digits, '.', '_' or '-'.
fn is_service_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// A value as refusals and text output show it: JSON, so strings are quoted.
pub fn display_value(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

/// Where a key's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSource {
    /// `config.json` sets the key.
    File,
    /// The file does not set the key; its default applies.
    Default,
}

/// One row of the `config get --json` key table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyReport {
    pub key: &'static str,
    #[serde(rename = "type")]
    pub key_type: KeyType,
    pub allowed: &'static [&'static str],
    pub default: Value,
    /// The effective value: the file's, or the default.
    pub value: Value,
    pub source: ValueSource,
    pub description: &'static str,
}

/// The `config get --json` payload (DESIGN §5.10).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConfigDocument {
    pub schema_version: u32,
    /// Absolute path of `config.json` (whether or not it exists).
    pub path: PathBuf,
    pub exists: bool,
    /// The configuration BranchBox uses: the file with every default applied.
    pub effective: Value,
    /// The file as written (`{}` when it does not exist).
    pub file: Value,
    /// Every registry key, or only the one asked for.
    pub keys: Vec<KeyReport>,
}

/// One key whose value in `config.json` changed; `null` means not set.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConfigChange {
    pub key: &'static str,
    pub old: Value,
    pub new: Value,
}

/// The `config apply --json` payload (DESIGN §5.10); `config set`/`unset` return it too.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApplyResult {
    pub schema_version: u32,
    /// The keys whose value in the file changed (or would change, in a dry run).
    pub changed: Vec<ConfigChange>,
    /// The effective configuration after the change.
    pub effective: Value,
    /// Path of `config.json`, for text output.
    #[serde(skip)]
    pub path: PathBuf,
}

/// Read the configuration of the repository at `repo_root`: the effective values, the raw file
/// and the key table, or only `key`'s row. A file that is not strict JSON or does not load is
/// refused with `config_invalid` naming where.
pub fn get(repo_root: &Path, key: Option<&str>) -> Result<ConfigDocument> {
    let selected = key.map(lookup_key).transpose()?;
    let path = BranchBoxConfig::path(repo_root);
    let original = read_optional(&path)?;
    let text = original.as_deref().unwrap_or_default();
    let raw = parse_strict(&path, text)?;
    let effective = effective_value(&load_strict(&path, text, &raw, false)?)?;
    let keys = KEYS
        .iter()
        .filter(|candidate| selected.is_none_or(|selected| std::ptr::eq(selected, *candidate)))
        .map(|key| key_report(key, &raw, &effective))
        .collect();
    Ok(ConfigDocument {
        schema_version: SCHEMA_VERSION,
        path,
        exists: original.is_some(),
        effective,
        file: Value::Object(raw),
        keys,
    })
}

fn key_report(key: &'static ConfigKey, raw: &Map<String, Value>, effective: &Value) -> KeyReport {
    let value = lookup(effective, key.key)
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| key.default_value());
    KeyReport {
        key: key.key,
        key_type: key.key_type,
        allowed: key.allowed,
        default: key.default_value(),
        value,
        source: if lookup_in(raw, key.key).is_some() {
            ValueSource::File
        } else {
            ValueSource::Default
        },
        description: key.description,
    }
}

/// Apply an RFC 7386 JSON merge patch: an object whose leaves set registry keys, where `null`
/// unsets a key (or removes a whole section such as `tunnel.providers.cloudflared`). An unknown
/// key is refused with `config_unknown_key`, an invalid value with `config_invalid`, before
/// anything is written. With `dry_run`, nothing is written.
pub fn apply_patch(repo_root: &Path, patch: &Value, dry_run: bool) -> Result<ApplyResult> {
    let edits = patch_edits(patch)?;
    update(repo_root, dry_run, |_| Ok(edits.clone()))
}

/// `config set KEY VALUE`: parse `value` as the key's type and write it.
pub fn set(repo_root: &Path, key: &str, value: &str) -> Result<ApplyResult> {
    let key = lookup_key(key)?;
    let value = key.parse_value(value)?;
    update(repo_root, false, |_| {
        Ok(vec![Edit::Set(key, value.clone())])
    })
}

/// `config unset KEY`: remove the key from the file, so its default applies again.
pub fn unset(repo_root: &Path, key: &str) -> Result<ApplyResult> {
    let key = lookup_key(key)?;
    update(repo_root, false, |_| Ok(vec![Edit::Unset(key)]))
}

/// Plan the edits against the current file and, unless `dry_run`, write them under the lock. A
/// repository without `.branchbox` gets one only when something is written.
fn update(
    repo_root: &Path,
    dry_run: bool,
    edits: impl Fn(&Map<String, Value>) -> Result<Vec<Edit>>,
) -> Result<ApplyResult> {
    let state_dir = repo_root.join(".branchbox");
    if dry_run || !state_dir.is_dir() {
        let planned = plan(repo_root, &edits)?;
        if dry_run || !planned.writes() {
            return Ok(planned.result);
        }
    }
    let _lock = atomic_fs::lock_state_dir(&state_dir, LOCK_TIMEOUT)?;
    let planned = plan(repo_root, &edits)?;
    planned.commit()?;
    Ok(planned.result)
}

/// One change to `config.json`.
#[derive(Debug, Clone)]
pub(crate) enum Edit {
    /// Set a key to an already validated value.
    Set(&'static ConfigKey, Value),
    /// Remove a key.
    Unset(&'static ConfigKey),
    /// Remove a whole section object (a merge patch's `null` on, say, `tunnel.providers`).
    RemoveSection(&'static str),
}

impl Edit {
    /// A validated [`Edit::Set`] of the registry key `name`.
    pub(crate) fn set(name: &str, value: Value) -> Result<Self> {
        let key = lookup_key(name)?;
        key.validate(&value)?;
        Ok(Edit::Set(key, value))
    }

    /// An [`Edit::Unset`] of the registry key `name`.
    pub(crate) fn unset(name: &str) -> Result<Self> {
        Ok(Edit::Unset(lookup_key(name)?))
    }

    /// Whether applying this edit to `raw` changes anything.
    fn changes(&self, raw: &Map<String, Value>) -> bool {
        match self {
            Edit::Set(key, value) => lookup_in(raw, key.key) != Some(value),
            Edit::Unset(key) => lookup_in(raw, key.key).is_some(),
            Edit::RemoveSection(section) => lookup_in(raw, section).is_some(),
        }
    }
}

/// The edits a merge patch asks for, each value validated.
fn patch_edits(patch: &Value) -> Result<Vec<Edit>> {
    let Value::Object(patch) = patch else {
        return Err(Error::validation(format!(
            "A config patch must be a JSON object of keys to set (null unsets a key), for example \
             {{\"feature\": {{\"branch_prefix\": \"spike\"}}}}; got {}",
            display_value(patch)
        )));
    };
    let mut edits = Vec::new();
    collect_patch(patch, "", &mut edits)?;
    Ok(edits)
}

fn collect_patch(patch: &Map<String, Value>, prefix: &str, edits: &mut Vec<Edit>) -> Result<()> {
    for (name, value) in patch {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        if let Ok(key) = lookup_key(&path) {
            if value.is_null() {
                edits.push(Edit::Unset(key));
            } else {
                key.validate(value)?;
                edits.push(Edit::Set(key, value.clone()));
            }
        } else if let Some(section) = section(&path) {
            match value {
                Value::Object(inner) => collect_patch(inner, &path, edits)?,
                Value::Null => edits.push(Edit::RemoveSection(section)),
                other => {
                    return Err(Error::ConfigInvalid {
                        message: format!(
                            "{path} is a section and must be an object of its keys, or null to \
                             remove it (got {}). Nothing was changed.",
                            display_value(other)
                        ),
                        key: Some(path),
                        line: None,
                        column: None,
                        expected: Some("an object".to_string()),
                    });
                }
            }
        } else if path == "version" {
            // BranchBox owns the format version; a patch may only repeat it.
            if value != &Value::from(CONFIG_VERSION) {
                return Err(Error::ConfigInvalid {
                    message: format!(
                        "version is managed by BranchBox and must stay \"{CONFIG_VERSION}\" (got \
                         {}). Nothing was changed.",
                        display_value(value)
                    ),
                    key: Some(path),
                    line: None,
                    column: None,
                    expected: Some(format!("\"{CONFIG_VERSION}\"")),
                });
            }
        } else {
            return Err(Error::ConfigUnknownKey { key: path });
        }
    }
    Ok(())
}

/// Plan writing `config` over the existing `config.json` (what [`BranchBoxConfig::save`] does
/// for a file that exists): only the registry keys whose loaded value changed are edited, through
/// the CST, so keys this version does not know and the file's formatting survive. A section that
/// `config` drops (`tunnel.providers.cloudflared: None`) is removed. Values are written as given,
/// without the `config set` validators: they come from a loaded configuration.
pub(crate) fn plan_save(repo_root: &Path, config: &BranchBoxConfig) -> Result<PlannedChange> {
    let after = serde_json::to_value(config)?;
    plan(repo_root, |raw| {
        let mut loaded: BranchBoxConfig = serde_json::from_value(Value::Object(raw.clone()))?;
        if loaded.version.is_empty() {
            loaded.version = CONFIG_VERSION.to_string();
        }
        let before = serde_json::to_value(&loaded)?;
        Ok(save_edits(&before, &after))
    })
}

/// The edits that turn the loaded configuration `before` into `after`.
fn save_edits(before: &Value, after: &Value) -> Vec<Edit> {
    let mut edits = Vec::new();
    let mut removed: Vec<&'static str> = Vec::new();
    for key in KEYS {
        let (sections, _) = split_key(key.key);
        let mut walked = 0;
        let mut dropped_section = None;
        for section in sections {
            walked += section.len();
            let path = &key.key[..walked];
            walked += 1;
            let was = lookup(before, path).filter(|value| !value.is_null());
            let now = lookup(after, path).filter(|value| !value.is_null());
            if was.is_some() && now.is_none() {
                dropped_section = Some(path);
                break;
            }
        }
        if let Some(path) = dropped_section {
            if !removed.contains(&path) {
                removed.push(path);
                edits.push(Edit::RemoveSection(path));
            }
            continue;
        }
        let was = lookup(before, key.key).filter(|value| !value.is_null());
        let now = lookup(after, key.key).filter(|value| !value.is_null());
        match (was, now) {
            (Some(was), Some(now)) if was == now => {}
            (None, None) => {}
            (_, Some(now)) => edits.push(Edit::Set(key, now.clone())),
            (Some(_), None) => edits.push(Edit::Unset(key)),
        }
    }
    edits
}

/// A checked change to `config.json`, ready to be written.
pub(crate) struct PlannedChange {
    path: PathBuf,
    /// The new file text; `None` when nothing changes.
    rendered: Option<String>,
    pub(crate) result: ApplyResult,
}

impl PlannedChange {
    /// Whether committing writes the file.
    pub(crate) fn writes(&self) -> bool {
        self.rendered.is_some()
    }

    /// Write the change atomically. The caller holds the `.branchbox` lock.
    pub(crate) fn commit(&self) -> Result<()> {
        if let Some(rendered) = &self.rendered {
            atomic_fs::write_atomic(&self.path, rendered.as_bytes(), 0o644)?;
        }
        Ok(())
    }
}

/// Read `config.json`, work out the edits from its parsed content and check the result: it
/// must strict-parse and load as a [`BranchBoxConfig`]. Nothing is written. Callers that commit
/// the plan hold the `.branchbox` lock across `plan` and [`PlannedChange::commit`].
pub(crate) fn plan(
    repo_root: &Path,
    edits: impl FnOnce(&Map<String, Value>) -> Result<Vec<Edit>>,
) -> Result<PlannedChange> {
    let path = BranchBoxConfig::path(repo_root);
    let original = read_optional(&path)?;
    let text = original.as_deref().unwrap_or_default();
    let before = parse_strict(&path, text)?;
    let edits: Vec<Edit> = edits(&before)?
        .into_iter()
        .filter(|edit| edit.changes(&before))
        .collect();

    if edits.is_empty() {
        let effective = effective_value(&load_strict(&path, text, &before, false)?)?;
        return Ok(PlannedChange {
            result: ApplyResult {
                schema_version: SCHEMA_VERSION,
                changed: Vec::new(),
                effective,
                path: path.clone(),
            },
            path,
            rendered: None,
        });
    }

    let rendered = render(&path, original.as_deref(), &edits)?;
    let after = parse_strict(&path, &rendered)?;
    let effective = effective_value(&load_strict(&path, &rendered, &after, true)?)?;
    let changed = KEYS
        .iter()
        .filter_map(|key| {
            let old = lookup_in(&before, key.key).cloned().unwrap_or(Value::Null);
            let new = lookup_in(&after, key.key).cloned().unwrap_or(Value::Null);
            (old != new).then_some(ConfigChange {
                key: key.key,
                old,
                new,
            })
        })
        .collect();
    Ok(PlannedChange {
        result: ApplyResult {
            schema_version: SCHEMA_VERSION,
            changed,
            effective,
            path: path.clone(),
        },
        path,
        rendered: Some(rendered),
    })
}

/// Apply `edits` to the file text through the CST, keeping everything else byte for byte.
fn render(path: &Path, original: Option<&str>, edits: &[Edit]) -> Result<String> {
    let source = original.filter(|text| !text.trim().is_empty());
    let root =
        CstRootNode::parse(source.unwrap_or("{}\n"), &ParseOptions::default()).map_err(|err| {
            Error::ConfigInvalid {
                message: format!("{} could not be parsed: {err}", path.display()),
                key: None,
                line: None,
                column: None,
                expected: None,
            }
        })?;
    // `parse_strict` already checked that the root is an object.
    let object = root.object_value_or_set();
    for edit in edits {
        match edit {
            Edit::Set(key, value) => {
                let (sections, name) = split_key(key.key);
                let mut parent = object.clone();
                let mut walked = String::new();
                for section in sections {
                    if !walked.is_empty() {
                        walked.push('.');
                    }
                    walked.push_str(section);
                    // `"cloudflared": null` (what `BranchBoxConfig::save` used to write for an
                    // absent provider) is an unset section: replace it with an object, as an
                    // RFC 7386 merge would. Any other non-object value is refused.
                    let is_null = parent
                        .get(section)
                        .and_then(|property| property.value())
                        .is_some_and(|value| value.as_null_keyword().is_some());
                    if is_null {
                        parent = parent.object_value_or_set(section);
                        continue;
                    }
                    parent = parent.object_value_or_create(section).ok_or_else(|| {
                        Error::ConfigInvalid {
                            message: format!(
                                "{walked} in {} is not an object, so {} cannot be set inside it. \
                                 Fix the file (or remove {walked} with a null patch) and retry; \
                                 nothing was changed.",
                                path.display(),
                                key.key
                            ),
                            key: Some(walked.clone()),
                            line: None,
                            column: None,
                            expected: Some("an object".to_string()),
                        }
                    })?;
                }
                match parent.get(name) {
                    Some(property) => property.set_value(to_cst(value)),
                    None => {
                        parent.append(name, to_cst(value));
                    }
                }
            }
            Edit::Unset(key) => remove_property(&object, key.key),
            Edit::RemoveSection(section) => remove_property(&object, section),
        }
    }

    let mut rendered = root.to_string();
    // Keep the file's final newline (and give a new file one).
    if source.is_none_or(|text| text.ends_with('\n')) && !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

/// Remove the property at the dotted `path` under `object`, if it is there.
fn remove_property(object: &CstObject, path: &str) {
    let (sections, name) = split_key(path);
    let parent = sections.iter().try_fold(object.clone(), |parent, section| {
        parent.object_value(section)
    });
    if let Some(property) = parent.and_then(|parent| parent.get(name)) {
        property.remove();
    }
}

/// `a.b.c` → (`["a", "b"]`, `"c"`).
fn split_key(key: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let name = parts.pop().unwrap_or_default();
    (parts, name)
}

fn to_cst(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(escape_for_cst(value)),
        Value::Array(items) => CstInputValue::Array(items.iter().map(to_cst).collect()),
        Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(name, value)| (name.clone(), to_cst(value)))
                .collect(),
        ),
    }
}

/// jsonc-parser 0.27 escapes only `"` when it writes a string, so escape backslashes and control
/// characters here; the result is a valid JSON string once the parser adds its quote escapes.
fn escape_for_cst(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if u32::from(ch) < 0x20 => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(ch));
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

/// The value at the dotted `key` inside `root`, if every section on the way is an object.
fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(root, |value, part| value.as_object()?.get(part))
}

fn lookup_in<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    let (first, rest) = key.split_once('.').unwrap_or((key, ""));
    let value = map.get(first)?;
    if rest.is_empty() {
        Some(value)
    } else {
        lookup(value, rest)
    }
}

/// The file's text, or `None` when it does not exist.
fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(Error::Io(io::Error::new(
            err.kind(),
            format!("Failed to read {}: {err}", path.display()),
        ))),
    }
}

/// Parse `text` as strict JSON holding an object. Empty text is an empty object. Comments,
/// trailing commas and anything else serde rejects are refused with the line and column.
fn parse_strict(path: &Path, text: &str) -> Result<Map<String, Value>> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(Error::ConfigInvalid {
            message: format!(
                "{} must hold a JSON object, not {}. Fix the file and retry.",
                path.display(),
                display_value(&other)
            ),
            key: None,
            line: None,
            column: None,
            expected: Some("a JSON object".to_string()),
        }),
        Err(err) => Err(Error::ConfigInvalid {
            message: format!(
                "{} is not strict JSON (line {}, column {}: {err}); comments and trailing \
                 commas are not supported. Fix the file and retry; nothing was changed.",
                path.display(),
                err.line(),
                err.column()
            ),
            key: None,
            line: Some(err.line()),
            column: Some(err.column()),
            expected: None,
        }),
    }
}

/// Load the parsed file as a [`BranchBoxConfig`]. On failure the refusal names the first
/// registry key (or section) whose value has the wrong type, plus serde's line and column.
/// `edited` says whether `text` is the result of an edit (so nothing was written).
fn load_strict(
    path: &Path,
    text: &str,
    raw: &Map<String, Value>,
    edited: bool,
) -> Result<BranchBoxConfig> {
    let source = if text.trim().is_empty() { "{}" } else { text };
    let err = match serde_json::from_str::<BranchBoxConfig>(source) {
        Ok(mut config) => {
            if config.version.is_empty() {
                config.version = CONFIG_VERSION.to_string();
            }
            return Ok(config);
        }
        Err(err) => err,
    };

    let culprit = misplaced_section(raw).or_else(|| {
        KEYS.iter().find_map(|key| {
            let value = lookup_in(raw, key.key)?;
            (!key.has_valid_shape(value)).then(|| Culprit {
                key: key.key.to_string(),
                cause: format!(
                    "{} must be {} (got {})",
                    key.key,
                    key.expected(),
                    display_value(value)
                ),
                expected: key.expected(),
            })
        })
    });
    let (key, cause, expected) = match culprit {
        Some(culprit) => (
            Some(culprit.key),
            format!("{}; ", culprit.cause),
            Some(culprit.expected),
        ),
        None => (None, String::new(), None),
    };
    let outcome = if edited {
        "would not load after this change"
    } else {
        "does not load"
    };
    Err(Error::ConfigInvalid {
        message: format!(
            "{} {outcome}: {cause}line {}, column {}: {err}. Fix the file and retry{}",
            path.display(),
            err.line(),
            err.column(),
            if edited {
                "; nothing was changed."
            } else {
                "."
            }
        ),
        key,
        line: Some(err.line()),
        column: Some(err.column()),
        expected,
    })
}

/// The key (or section) that keeps a file from loading, and why.
struct Culprit {
    key: String,
    cause: String,
    expected: String,
}

/// The first registry section that the file holds as something other than an object.
fn misplaced_section(raw: &Map<String, Value>) -> Option<Culprit> {
    KEYS.iter().find_map(|key| {
        let (sections, _) = split_key(key.key);
        let mut walked = String::new();
        let mut current = raw;
        for section in sections {
            if !walked.is_empty() {
                walked.push('.');
            }
            walked.push_str(section);
            match current.get(section)? {
                Value::Object(inner) => current = inner,
                other => {
                    return Some(Culprit {
                        cause: format!("{walked} must be an object (got {})", display_value(other)),
                        key: walked,
                        expected: "an object".to_string(),
                    });
                }
            }
        }
        None
    })
}

/// The effective configuration as JSON: every default applied, including the tunnel provider
/// default that tunnel commands apply before use.
fn effective_value(config: &BranchBoxConfig) -> Result<Value> {
    let mut config = config.clone();
    config.tunnel.ensure_defaults();
    Ok(serde_json::to_value(config)?)
}

/// The configuration reference page, `docs/docs/reference/configuration.md`, generated from
/// [`KEYS`]. A test keeps the committed page in sync.
pub fn reference_markdown() -> String {
    let mut page = String::from(
        "---\n\
         sidebar_position: 3\n\
         ---\n\
         \n\
         <!-- Generated from the key registry in core/src/config_edit.rs. Do not edit by hand: \
         run `UPDATE_CONFIG_REFERENCE=1 cargo test -p worktree-core config_edit` to regenerate. -->\n\
         \n\
         # Configuration Reference\n\
         \n\
         Each project keeps its BranchBox settings in `.branchbox/config.json` at the repository \
         root. Every setting is optional: a key the file does not set takes its default.\n\
         \n\
         Read and change the settings with `branchbox config`:\n\
         \n\
         - `branchbox config get [KEY] [--json]` shows the effective value of every key (or one \
         key), its default and whether the file sets it.\n\
         - `branchbox config set KEY VALUE` sets a key. Booleans are `true` or `false`; a list is \
         comma-separated (`web,worker`) or a JSON array.\n\
         - `branchbox config unset KEY` removes a key, so its default applies again.\n\
         - `branchbox config apply --file PATCH.json [--dry-run] [--json]` applies a JSON merge \
         patch (RFC 7386); `--file -` reads it from standard input. `null` unsets a key.\n\
         \n\
         Changes are checked before anything is written: an unknown key or an invalid value is \
         refused, naming the key and the accepted values, and the file is left as it was. Edits \
         keep the file's formatting, its permissions and any keys BranchBox does not know. The \
         file must be strict JSON: comments and trailing commas are refused with their line and \
         column.\n\
         \n\
         ## Keys\n\
         \n\
         | Key | Type | Default | Accepted values |\n\
         |---|---|---|---|\n",
    );
    for key in KEYS {
        let _ = writeln!(
            page,
            "| [`{}`](#{}) | {} | `{}` | {} |",
            key.key,
            key.key.replace('.', ""),
            key.key_type.as_str(),
            display_value(&key.default_value()),
            markdown_text(&key.expected()).replace('|', "\\|")
        );
    }
    for key in KEYS {
        let _ = write!(
            page,
            "\n### `{}`\n\n{}\n\n- Type: {}\n- Default: `{}`\n- Accepted values: {}\n",
            key.key,
            key.description,
            key.key_type.as_str(),
            display_value(&key.default_value()),
            markdown_text(&key.expected())
        );
    }
    page
}

/// Escape plain text for the MDX docs site, where `<`, `>`, `{` and `}` start markup.
fn markdown_text(text: &str) -> String {
    text.chars().fold(String::new(), |mut escaped, ch| {
        if matches!(ch, '<' | '>' | '{' | '}') {
            escaped.push('\\');
        }
        escaped.push(ch);
        escaped
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn repo() -> TempDir {
        TempDir::new().unwrap()
    }

    fn write_config(root: &Path, text: &str) {
        fs::create_dir_all(root.join(".branchbox")).unwrap();
        fs::write(BranchBoxConfig::path(root), text).unwrap();
    }

    fn read_config(root: &Path) -> String {
        fs::read_to_string(BranchBoxConfig::path(root)).unwrap()
    }

    #[test]
    fn registry_defaults_match_what_the_config_loads() {
        // With every section present but empty, serde fills each key with its default; the only
        // code-level default is the tunnel name prefix, which the provider applies at use.
        let config: BranchBoxConfig = serde_json::from_str(
            r#"{"runtime":{"sbx":{}},"feature":{"teardown":{}},
                "tunnel":{"providers":{"cloudflared":{}}},"editor":{}}"#,
        )
        .unwrap();
        let effective = effective_value(&config).unwrap();
        for key in KEYS {
            let loaded = lookup(&effective, key.key).cloned().unwrap_or(Value::Null);
            if key.key == "tunnel.providers.cloudflared.tunnel_name_prefix" {
                assert_eq!(loaded, Value::Null);
                assert_eq!(key.default_value(), json!("branchbox"));
            } else {
                assert_eq!(loaded, key.default_value(), "{}", key.key);
            }
            assert!(key.has_valid_shape(&key.default_value()), "{}", key.key);
            assert!(!key.description.is_empty());
        }
        let defaults = effective_value(&BranchBoxConfig::default()).unwrap();
        assert_eq!(defaults["runtime"]["provider"], "container");
        assert_eq!(defaults["tunnel"]["providers"]["cloudflared"], Value::Null);
    }

    #[test]
    fn registry_keys_are_unique_and_sections_are_found() {
        for (index, key) in KEYS.iter().enumerate() {
            assert!(KEYS[..index].iter().all(|other| other.key != key.key));
            assert_eq!(key.allowed.is_empty(), key.key_type != KeyType::Enum);
        }
        assert_eq!(section("tunnel.providers"), Some("tunnel.providers"));
        assert_eq!(section("tunnel.provider"), None);
        assert_eq!(section("feature.branch_prefix"), None);
        assert!(matches!(
            lookup_key("nope"),
            Err(Error::ConfigUnknownKey { key }) if key == "nope"
        ));
    }

    #[test]
    fn get_without_a_file_reports_defaults() {
        let temp = repo();
        let document = get(temp.path(), None).unwrap();
        assert!(!document.exists);
        assert_eq!(document.path, temp.path().join(".branchbox/config.json"));
        assert_eq!(document.file, json!({}));
        assert_eq!(document.keys.len(), KEYS.len());
        assert!(document
            .keys
            .iter()
            .all(|key| key.source == ValueSource::Default && key.value == key.default));
        assert_eq!(document.effective["feature"]["branch_prefix"], "feature");
        assert_eq!(
            document.effective["tunnel"]["default_provider"],
            "cloudflared"
        );
        assert!(!temp.path().join(".branchbox").exists());
    }

    #[test]
    fn get_one_key_reports_its_file_value() {
        let temp = repo();
        write_config(
            temp.path(),
            r#"{"runtime": {"provider": "sbx"}, "tunnel": {"default_provider": null}}"#,
        );
        let document = get(temp.path(), Some("runtime.provider")).unwrap();
        assert!(document.exists);
        assert_eq!(
            serde_json::to_value(&document.keys).unwrap(),
            json!([{
                "key": "runtime.provider",
                "type": "enum",
                "allowed": ["container", "sbx", "local-vm", "in-guest"],
                "default": "container",
                "value": "sbx",
                "source": "file",
                "description": KEYS[0].description,
            }])
        );
        // A null optional key is "set" in the file, and its default is what applies.
        let provider = get(temp.path(), Some("tunnel.default_provider")).unwrap();
        assert_eq!(provider.keys[0].source, ValueSource::File);
        assert_eq!(provider.keys[0].value, "cloudflared");
        assert!(matches!(
            get(temp.path(), Some("runtime.nope")),
            Err(Error::ConfigUnknownKey { .. })
        ));
    }

    #[test]
    fn set_preserves_formatting_and_unknown_keys() {
        let temp = repo();
        let original = "{\n    \"version\": \"1\",\n    \"x_team\": {\"keep\": [1, 2]},\n    \
                        \"feature\": {\n        \"branch_prefix\": \"feature\"\n    }\n}\n";
        write_config(temp.path(), original);

        let result = set(temp.path(), "feature.branch_prefix", "spike").unwrap();
        assert_eq!(
            result.changed,
            vec![ConfigChange {
                key: "feature.branch_prefix",
                old: json!("feature"),
                new: json!("spike"),
            }]
        );
        assert_eq!(result.effective["feature"]["branch_prefix"], "spike");
        assert_eq!(
            read_config(temp.path()),
            original.replace("\"feature\"\n", "\"spike\"\n")
        );

        // Setting the same value again writes nothing.
        let again = set(temp.path(), "feature.branch_prefix", "spike").unwrap();
        assert!(again.changed.is_empty());
    }

    #[test]
    fn set_creates_missing_sections_and_files() {
        let temp = repo();
        let result = set(temp.path(), "runtime.sbx.run_services", "web, worker").unwrap();
        assert_eq!(result.changed[0].new, json!(["web", "worker"]));
        let value: Value = serde_json::from_str(&read_config(temp.path())).unwrap();
        assert_eq!(
            value,
            json!({"runtime": {"sbx": {"run_services": ["web", "worker"]}}})
        );
        assert!(read_config(temp.path()).ends_with("}\n"));

        set(temp.path(), "runtime.sbx.run_services", r#"["api"]"#).unwrap();
        set(temp.path(), "editor.auto_launch_agent_terminal", "TRUE").unwrap();
        let document = get(temp.path(), None).unwrap();
        assert_eq!(
            document.effective["runtime"]["sbx"]["run_services"],
            json!(["api"])
        );
        assert_eq!(
            document.effective["editor"]["auto_launch_agent_terminal"],
            true
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_keep_the_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = repo();
        write_config(temp.path(), "{}\n");
        let path = BranchBoxConfig::path(temp.path());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        set(temp.path(), "tunnel.enabled", "false").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let fresh = repo();
        set(fresh.path(), "tunnel.enabled", "false").unwrap();
        assert_eq!(
            fs::metadata(BranchBoxConfig::path(fresh.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }

    #[test]
    fn invalid_values_name_the_key_and_the_accepted_values() {
        let temp = repo();
        let err = set(temp.path(), "runtime.provider", "nope").unwrap_err();
        assert_eq!(err.code(), "config_invalid");
        assert_eq!(
            err.to_string(),
            "Configuration error: runtime.provider must be one of: container, sbx, local-vm, \
             in-guest (got \"nope\"). Nothing was changed."
        );
        assert_eq!(
            err.details().unwrap(),
            json!({"key": "runtime.provider", "expected": "one of: container, sbx, local-vm, in-guest"})
        );

        for (key, value) in [
            ("feature.branch_prefix", "feature/"),
            ("feature.branch_prefix", "has space"),
            ("feature.branch_prefix", ""),
            ("tunnel.enabled", "yes"),
            ("tunnel.default_provider", "ngrok"),
            ("tunnel.providers.cloudflared.account_id", "acct 1"),
            ("tunnel.providers.cloudflared.account_id", "$OTHER_VAR"),
            (
                "tunnel.providers.cloudflared.tunnel_name_prefix",
                "bad_prefix",
            ),
            ("tunnel.providers.cloudflared.dns_zone", "localhost"),
            ("tunnel.providers.cloudflared.service_url", " "),
            ("runtime.sbx.run_services", "web,web"),
            ("runtime.sbx.run_services", "-web"),
            ("runtime.sbx.run_services", "[1]"),
            ("runtime.sbx.run_services", "[oops"),
        ] {
            let err = set(temp.path(), key, value).unwrap_err();
            assert_eq!(err.code(), "config_invalid", "{key}={value}");
            assert_eq!(err.details().unwrap()["key"], key, "{key}={value}");
        }
        assert!(!temp.path().join(".branchbox").exists());

        for (key, value) in [
            ("feature.branch_prefix", "team/spike"),
            (
                "tunnel.providers.cloudflared.account_id",
                "${CLOUDFLARE_ACCOUNT_ID}",
            ),
            ("tunnel.providers.cloudflared.dns_zone", "dev.example.com"),
            ("tunnel.providers.cloudflared.tunnel_name_prefix", "bb-1"),
        ] {
            set(temp.path(), key, value).unwrap();
        }
    }

    /// `"cloudflared": null` is what `BranchBoxConfig::save` wrote for an absent provider (init
    /// with tunnels declined); edits inside it replace the null with an object.
    #[test]
    fn a_null_section_is_replaced_by_an_object_when_a_key_inside_it_is_set() {
        let temp = repo();
        let text = "{\n  \"x_team\": {\"keep\": true},\n  \"tunnel\": {\"enabled\": false, \
                    \"providers\": {\"cloudflared\": null}}\n}\n";
        write_config(temp.path(), text);
        set(
            temp.path(),
            "tunnel.providers.cloudflared.account_id",
            "acct-1",
        )
        .unwrap();
        let value: Value = serde_json::from_str(&read_config(temp.path())).unwrap();
        assert_eq!(
            value["tunnel"]["providers"]["cloudflared"]["account_id"],
            "acct-1"
        );
        assert_eq!(value["x_team"]["keep"], true);

        write_config(temp.path(), text);
        let patch =
            json!({"tunnel": {"providers": {"cloudflared": {"dns_zone": "dev.example.com"}}}});
        apply_patch(temp.path(), &patch, false).unwrap();
        let value: Value = serde_json::from_str(&read_config(temp.path())).unwrap();
        assert_eq!(
            value["tunnel"]["providers"]["cloudflared"]["dns_zone"],
            "dev.example.com"
        );
        assert_eq!(value["tunnel"]["enabled"], false);
        assert_eq!(value["x_team"]["keep"], true);

        // Other non-object values are still refused.
        write_config(
            temp.path(),
            "{\"tunnel\": {\"providers\": {\"cloudflared\": 3}}}\n",
        );
        let err = set(
            temp.path(),
            "tunnel.providers.cloudflared.account_id",
            "acct-1",
        )
        .unwrap_err();
        assert_eq!(err.code(), "config_invalid");
    }

    #[test]
    fn apply_patch_sets_unsets_and_removes_sections() {
        let temp = repo();
        write_config(
            temp.path(),
            "{\n  \"x_team\": 1,\n  \"feature\": {\"branch_prefix\": \"feat\"},\n  \
             \"tunnel\": {\"providers\": {\"cloudflared\": {\"account_id\": \"a\"}}}\n}\n",
        );
        let patch = json!({
            "version": "1",
            "runtime": {"provider": "sbx"},
            "feature": {"branch_prefix": null, "teardown": {"delete_branch_by_default": false}},
            "tunnel": {"providers": {"cloudflared": null}}
        });

        let before = read_config(temp.path());
        let dry = apply_patch(temp.path(), &patch, true).unwrap();
        assert_eq!(read_config(temp.path()), before, "a dry run writes nothing");

        let result = apply_patch(temp.path(), &patch, false).unwrap();
        assert_eq!(
            result,
            ApplyResult {
                path: dry.path.clone(),
                ..dry
            }
        );
        let changed: Vec<(&str, Value, Value)> = result
            .changed
            .iter()
            .map(|change| (change.key, change.old.clone(), change.new.clone()))
            .collect();
        assert_eq!(
            changed,
            vec![
                ("runtime.provider", Value::Null, json!("sbx")),
                ("feature.branch_prefix", json!("feat"), Value::Null),
                (
                    "feature.teardown.delete_branch_by_default",
                    Value::Null,
                    json!(false)
                ),
                (
                    "tunnel.providers.cloudflared.account_id",
                    json!("a"),
                    Value::Null
                ),
            ]
        );
        let value: Value = serde_json::from_str(&read_config(temp.path())).unwrap();
        assert_eq!(value["x_team"], 1);
        assert_eq!(
            value["feature"],
            json!({"teardown": {"delete_branch_by_default": false}})
        );
        assert_eq!(value["tunnel"], json!({"providers": {}}));
        assert_eq!(result.effective["feature"]["branch_prefix"], "feature");
        assert!(
            value.get("version").is_none(),
            "an unchanged version is not written"
        );
    }

    #[test]
    fn apply_patch_refuses_unknown_keys_and_bad_shapes_before_writing() {
        let temp = repo();
        write_config(temp.path(), "{\"x\": 1}\n");
        for (patch, code, key) in [
            (
                json!({"runtime": {"provder": "sbx"}}),
                "config_unknown_key",
                "runtime.provder",
            ),
            (json!({"x_team": true}), "config_unknown_key", "x_team"),
            (json!({"tunnel": true}), "config_invalid", "tunnel"),
            (json!({"version": "2"}), "config_invalid", "version"),
            (
                json!({"tunnel": {"enabled": "yes"}}),
                "config_invalid",
                "tunnel.enabled",
            ),
        ] {
            let err = apply_patch(temp.path(), &patch, false).unwrap_err();
            assert_eq!(err.code(), code, "{patch}");
            assert_eq!(err.details().unwrap()["key"], key, "{patch}");
        }
        let err = apply_patch(temp.path(), &json!(["x"]), false).unwrap_err();
        assert_eq!(err.code(), "validation_failed");
        assert_eq!(read_config(temp.path()), "{\"x\": 1}\n");
    }

    #[test]
    fn comments_and_broken_files_are_refused_with_their_position() {
        let temp = repo();
        write_config(temp.path(), "{\n  // tunnels\n  \"version\": \"1\"\n}\n");
        for err in [
            set(temp.path(), "tunnel.enabled", "false").unwrap_err(),
            get(temp.path(), None).unwrap_err(),
        ] {
            assert_eq!(err.code(), "config_invalid");
            assert_eq!(err.details().unwrap(), json!({"line": 2, "column": 3}));
            assert!(err.to_string().contains("comments"), "{err}");
        }

        write_config(temp.path(), "[1]");
        assert_eq!(
            get(temp.path(), None).unwrap_err().details().unwrap(),
            json!({"expected": "a JSON object"})
        );
    }

    #[test]
    fn a_file_that_does_not_load_names_the_key_line_and_column() {
        let temp = repo();
        write_config(
            temp.path(),
            "{\n  \"runtime\": {\n    \"provider\": \"nope\"\n  }\n}\n",
        );
        let err = get(temp.path(), None).unwrap_err();
        assert_eq!(err.code(), "config_invalid");
        let details = err.details().unwrap();
        assert_eq!(details["key"], "runtime.provider");
        assert_eq!(details["line"], 3);
        assert!(err.to_string().contains("does not load"), "{err}");

        // Editing another key does not hide the broken one, and nothing is written.
        let before = read_config(temp.path());
        let err = set(temp.path(), "tunnel.enabled", "false").unwrap_err();
        assert!(err.to_string().contains("would not load"), "{err}");
        assert_eq!(err.details().unwrap()["key"], "runtime.provider");
        assert_eq!(read_config(temp.path()), before);

        // Fixing it through the engine works.
        set(temp.path(), "runtime.provider", "sbx").unwrap();

        write_config(temp.path(), "{\"feature\": \"x\"}");
        let err = get(temp.path(), None).unwrap_err();
        assert_eq!(err.details().unwrap()["key"], "feature");
        let err = set(temp.path(), "feature.branch_prefix", "spike").unwrap_err();
        assert_eq!(err.details().unwrap()["key"], "feature");
        apply_patch(temp.path(), &json!({"feature": null}), false).unwrap();
        assert_eq!(read_config(temp.path()), "{}");
    }

    #[test]
    fn unset_removes_only_the_key_and_creates_nothing_when_absent() {
        let temp = repo();
        let result = unset(temp.path(), "feature.branch_prefix").unwrap();
        assert!(result.changed.is_empty());
        assert!(!temp.path().join(".branchbox").exists());

        write_config(
            temp.path(),
            "{\n  \"feature\": {\n    \"branch_prefix\": \"x\",\n    \"other\": 1\n  }\n}\n",
        );
        let result = unset(temp.path(), "feature.branch_prefix").unwrap();
        assert_eq!(result.changed[0].old, "x");
        assert_eq!(result.changed[0].new, Value::Null);
        let value: Value = serde_json::from_str(&read_config(temp.path())).unwrap();
        assert_eq!(value, json!({"feature": {"other": 1}}));
    }

    #[test]
    fn an_empty_file_is_an_empty_object_and_strings_are_escaped() {
        let temp = repo();
        write_config(temp.path(), "");
        assert!(get(temp.path(), None).unwrap().exists);
        set(temp.path(), "editor.default_agent", "say \"hi\" \\ there").unwrap();
        let value: Value = serde_json::from_str(&read_config(temp.path())).unwrap();
        assert_eq!(value["editor"]["default_agent"], "say \"hi\" \\ there");
    }

    #[test]
    fn strings_with_control_characters_round_trip_through_the_cst() {
        for text in [
            "tab\there",
            "line\nbreak",
            "bell\u{7}",
            "back\\slash \"quoted\"",
        ] {
            let root = CstRootNode::parse("{}", &ParseOptions::default()).unwrap();
            root.object_value_or_set()
                .append("value", to_cst(&Value::from(text)));
            let parsed: Value = serde_json::from_str(&root.to_string()).unwrap();
            assert_eq!(parsed["value"], text);
        }
    }

    #[test]
    fn a_file_without_a_final_newline_keeps_it_that_way() {
        let temp = repo();
        write_config(temp.path(), "{\"tunnel\": {\"enabled\": true}}");
        set(temp.path(), "tunnel.enabled", "false").unwrap();
        assert_eq!(
            read_config(temp.path()),
            "{\"tunnel\": {\"enabled\": false}}"
        );
    }

    #[test]
    fn the_configuration_reference_is_in_sync_with_the_registry() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/docs/reference/configuration.md");
        let generated = reference_markdown();
        if std::env::var_os("UPDATE_CONFIG_REFERENCE").is_some_and(|value| value == "1") {
            fs::write(&path, &generated).unwrap();
        }
        let committed = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == generated,
            "{} is out of date; regenerate it with UPDATE_CONFIG_REFERENCE=1 cargo test -p \
             worktree-core config_edit",
            path.display()
        );
        for key in KEYS {
            assert!(generated.contains(&format!("### `{}`", key.key)));
        }
    }
}
