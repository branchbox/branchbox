//! Teardown plan types
//!
//! A [`TeardownPlan`] is the read-only answer to "what would `feature teardown` do to this
//! feature?". It lists the worktree's user changes, the BranchBox-generated files that are safe
//! to discard, what happens to the branch, and every blocker that makes teardown refuse before
//! it changes anything. It is printed by `feature teardown --dry-run --json`, carried in the
//! `teardown_refused` error envelope, and evaluated by the macOS app before it offers a teardown.
//!
//! These types are the Rust side of the JSON contract in DESIGN §5.5: field names, enum values
//! and the `kind`-tagged blocker shape must not change. Additions must stay optional so older
//! clients keep decoding.
//!
//! The module also holds the logic behind the plan:
//! - [`parse_porcelain_z`] reads `git status --porcelain=v1 -z`;
//! - [`classify_changes`] sorts each changed path into user work, BranchBox-generated files
//!   (rules R1-R7 of DESIGN §6.5) and the feature spec teardown keeps;
//! - [`build_teardown_plan`] turns those facts and the request into a plan with blockers.

use super::feature::{FeatureMetadata, FeatureStatus, FeatureTunnelStatus, TeardownRequest};
use crate::git::BranchMergeState;
use crate::modules::devcontainer::{
    baseline_digest, baseline_symlink_digest, DevcontainerBaseline,
};
use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Capabilities implemented by the plan-first teardown, reported by `branchbox version --json`:
/// - `teardown-plan`: `feature teardown --dry-run --json` prints the plan, and refusals carry it;
/// - `teardown-discard-changes`: `--discard-changes` discards user changes without implying
///   `git branch -D`, and teardown never discards them otherwise;
/// - `teardown-unmerged-preflight`: a non-interactive teardown that would fail to delete an
///   unmerged branch refuses before it changes anything.
pub const CAPABILITIES: &[&str] = &[
    "teardown-plan",
    "teardown-discard-changes",
    "teardown-unmerged-preflight",
];

/// `schema_version` of a [`TeardownPlan`] document.
pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// The largest file the classifier reads to compare contents. A bigger file is never treated
/// as BranchBox-generated.
pub const MAX_CLASSIFIED_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// How many status entries are classified one by one. Any further entries count as user
/// changes without being listed, and the change set is marked `truncated`.
pub const MAX_CLASSIFIED_ENTRIES: usize = 2_000;

/// How many changed files a refusal message names; the plan in the JSON envelope lists all.
pub const REFUSAL_LISTED_FILES: usize = 10;

/// The VS Code settings `feature start` writes into a worktree's `.vscode/settings.json`.
pub(crate) const VSCODE_PEACOCK_COLOR: &str = "peacock.color";
pub(crate) const VSCODE_PEACOCK_REMOTE_COLOR: &str = "peacock.remoteColor";
pub(crate) const VSCODE_WINDOW_TITLE: &str = "window.title";
pub(crate) const VSCODE_COLOR_CUSTOMIZATIONS: &str = "workbench.colorCustomizations";

/// Every setting `feature start` manages in `.vscode/settings.json`. A settings file that
/// differs from the committed one (or from `{}`) only in these keys is BranchBox-generated.
pub(crate) const VSCODE_MANAGED_SETTINGS: [&str; 4] = [
    VSCODE_PEACOCK_COLOR,
    VSCODE_PEACOCK_REMOTE_COLOR,
    VSCODE_WINDOW_TITLE,
    VSCODE_COLOR_CUSTOMIZATIONS,
];

/// The label of the one task `feature start` writes into `.vscode/tasks.json`.
pub(crate) const VSCODE_FEATURE_URL_TASK: &str = "Open Feature URL";

/// Files BranchBox writes into a worktree's `.devcontainer/` and nobody else should (rule R1).
const RESERVED_NAMES: [&str; 4] = [
    ".devcontainer/.branchbox.env",
    ".devcontainer/.cloudflared.env",
    ".devcontainer/.devcontainer.json",
    ".devcontainer/.branchbox-sbx-compose.yaml",
];

/// Staged sbx Compose inputs (`.branchbox-sbx-compose-input-<n>.yaml`) are generated wherever
/// they are written, like the reserved names above.
fn is_sbx_compose_input(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(".branchbox-sbx-compose-input-"))
}

/// Where `feature start` begins the block it appends to a worktree's `.env` (rule R6).
pub(crate) const ENV_FEATURE_SECTION_MARKER: &str = "# Feature-specific configuration";

/// Comment lines BranchBox writes into the `.env` feature block.
const ENV_MANAGED_COMMENTS: [&str; 2] = [
    "# Feature-specific configuration (managed by branchbox)",
    "# Database configuration (managed by database module)",
];

/// Variables BranchBox writes into the `.env` feature block.
const ENV_MANAGED_KEYS: [&str; 6] = [
    "WORK_FEATURE",
    "APP_URL",
    "COMPOSE_PROJECT_NAME",
    "DEVCONTAINER_NAME",
    "GIT_BRANCH",
    "DATABASE_NAME",
];

/// Everything teardown would do to a feature, computed before any change is made.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeardownPlan {
    pub schema_version: u32,
    pub work_feature: String,
    /// Whether the feature has an entry in `.branchbox/registry.json`.
    pub registered: bool,
    pub status: Option<FeatureStatus>,
    pub worktree: WorktreeState,
    pub changes: ChangeSet,
    pub branch: Option<BranchPlan>,
    pub defaults: TeardownDefaults,
    pub runtime: Option<RuntimeRef>,
    pub tunnel: Option<TunnelRef>,
    /// Reasons teardown refuses; empty means it may proceed.
    pub blockers: Vec<Blocker>,
    pub warnings: Vec<String>,
}

/// The feature worktree on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeState {
    pub path: PathBuf,
    pub exists: bool,
    pub locked: bool,
    pub lock_reason: Option<String>,
}

/// The worktree's uncommitted changes, split by who made them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeSet {
    /// `false` when `git status` could not be read; a `status_unavailable` blocker says why.
    pub status_available: bool,
    /// `true` when the change list was capped and more entries exist.
    pub truncated: bool,
    /// Changes the user made; discarding them needs explicit consent.
    pub user: Vec<UserChange>,
    /// Files BranchBox generated, which teardown may discard.
    pub generated: Vec<GeneratedChange>,
    /// Files teardown moves back into the main worktree instead of deleting.
    pub preserved: Vec<PreservedFile>,
}

/// One user-owned change in the worktree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserChange {
    /// Path relative to the worktree root.
    pub path: String,
    pub kind: ChangeKind,
    pub area: ChangeArea,
}

/// What kind of change `git status` reported.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Untracked,
    Modified,
    Added,
    Deleted,
    Typechange,
    Conflicted,
    Staged,
}

/// Which part of the project a change touches.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ChangeArea {
    Devcontainer,
    Compose,
    Vscode,
    Spec,
    Env,
    Other,
}

/// A BranchBox-generated file, with the rule that identified it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedChange {
    /// Path relative to the worktree root.
    pub path: String,
    pub rule: GeneratedRule,
}

/// Why a changed file counts as BranchBox-generated rather than user work.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedRule {
    ReservedName,
    DevcontainerBaseline,
    DerivedFromMain,
    DevcontainerEnvLink,
    EnvFeatureBlock,
    VscodeManagedKeys,
    VscodeManagedTasks,
}

/// A file teardown keeps by moving it to `destination` in the main worktree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreservedFile {
    pub path: String,
    pub destination: String,
}

/// What teardown will do with the feature branch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchPlan {
    pub name: String,
    pub source: BranchSource,
    pub exists: bool,
    pub upstream: Option<String>,
    /// The ref merge state is measured against (the upstream, else `HEAD`).
    pub reference: String,
    /// Human-readable name of `reference`, e.g. `main`.
    pub reference_name: String,
    /// Merged into `reference`, as `git branch -d` decides.
    pub merged: bool,
    pub merged_into_head: bool,
    /// Commits on the branch that `reference` does not have.
    pub ahead: u32,
    pub action: BranchAction,
}

/// Where the branch name came from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BranchSource {
    ExplicitPrefix,
    Registry,
    ConfigPrefix,
}

/// The branch step teardown will take.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BranchAction {
    Keep,
    Delete,
    ForceDelete,
}

/// The project's teardown defaults from `.branchbox/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeardownDefaults {
    pub delete_branch_by_default: bool,
    pub force_delete_unmerged_by_default: bool,
}

/// The runtime that teardown will stop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRef {
    pub provider: Option<String>,
    pub runtime_id: Option<String>,
}

/// The tunnel that teardown will remove.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelRef {
    pub status: Option<FeatureTunnelStatus>,
}

/// A reason teardown refuses before changing anything. Each one names its cause, and all but
/// `worktree_removal_failed` and `not_a_worktree` name the flag that overrides it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Blocker {
    UncommittedChanges {
        count: usize,
        message: String,
        #[serde(rename = "override")]
        override_hint: String,
    },
    UnmergedBranch {
        branch: String,
        ahead: u32,
        message: String,
        #[serde(rename = "override")]
        override_hint: String,
    },
    WorktreeLocked {
        reason: Option<String>,
        message: String,
        #[serde(rename = "override")]
        override_hint: String,
    },
    StatusUnavailable {
        cause: String,
        message: String,
        #[serde(rename = "override")]
        override_hint: String,
    },
    WorktreeRemovalFailed {
        cause: String,
        message: String,
    },
    /// Runtime cleanup could not be verified; keep the worktree's ownership/configuration for a retry.
    RuntimeCleanupFailed {
        cause: String,
        message: String,
        #[serde(rename = "override")]
        override_hint: String,
    },
    /// The directory at the feature's worktree path is not a linked worktree of this
    /// repository (the main worktree, an unrelated repository or a plain folder). Nothing
    /// overrides it: teardown never deletes such a directory.
    NotAWorktree {
        cause: String,
        message: String,
    },
    /// The feature spec could not be moved to the main worktree, so removing the worktree
    /// would lose it.
    SpecNotPreserved {
        path: String,
        cause: String,
        message: String,
        #[serde(rename = "override")]
        override_hint: String,
    },
}

/// Policy switches for plan-first teardown that [`TeardownRequest`] does not carry, so the
/// agent's request type keeps its shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TeardownOptions {
    /// Discard the worktree's uncommitted user changes instead of refusing.
    pub discard_changes: bool,
    /// Refuse up front when a requested branch delete would fail because it is unmerged.
    pub require_mergeable_branch: bool,
}

impl TeardownOptions {
    /// The behaviour of callers that predate these options (the agent's IPC teardown):
    /// `--force` still discards changes and no unmerged-branch preflight runs.
    pub fn legacy(r: &TeardownRequest) -> Self {
        Self {
            discard_changes: r.force_remove,
            require_mergeable_branch: false,
        }
    }
}

impl TeardownPlan {
    /// Whether teardown refuses this plan.
    pub fn is_blocked(&self) -> bool {
        !self.blockers.is_empty()
    }

    /// Whether uncommitted user changes block this plan.
    pub fn blocks_on_uncommitted_changes(&self) -> bool {
        self.blockers
            .iter()
            .any(|blocker| matches!(blocker, Blocker::UncommittedChanges { .. }))
    }

    /// The branch and commit count of an `unmerged_branch` blocker, if there is one.
    pub fn unmerged_branch(&self) -> Option<(&str, u32)> {
        self.blockers.iter().find_map(|blocker| match blocker {
            Blocker::UnmergedBranch { branch, ahead, .. } => Some((branch.as_str(), *ahead)),
            _ => None,
        })
    }

    /// Whether any user change touches devcontainer or compose files, the areas the 0.13
    /// teardown guarded ("Detected devcontainer/compose changes").
    pub fn has_module_area_changes(&self) -> bool {
        self.changes
            .user
            .iter()
            .any(|change| change.area.is_module_area())
    }

    /// The cause-naming refusal text: what blocks teardown and the flags that override it.
    pub fn refusal_message(&self) -> String {
        let mut message = format!(
            "Refusing to tear down '{}'; nothing was removed.",
            self.work_feature
        );
        for blocker in &self.blockers {
            message.push(' ');
            message.push_str(blocker.message());
        }
        message
    }

    /// The refusal for a blocked plan, before teardown changed anything.
    pub fn into_refusal(self) -> Error {
        Error::TeardownRefused {
            work_feature: self.work_feature.clone(),
            message: self.refusal_message(),
            plan: Box::new(self),
            changed_anything: false,
            completed_steps: Vec::new(),
        }
    }

    /// The refusal for a teardown that had to stop before removing the worktree, after
    /// `completed_steps` already ran. The worktree and its registry entry are kept.
    pub(crate) fn into_stopped(self, completed_steps: Vec<String>) -> Error {
        let mut message = format!(
            "Stopped tearing down '{}' before removing its worktree; the worktree and its \
             registry entry were kept.",
            self.work_feature
        );
        if !completed_steps.is_empty() {
            message.push_str(&format!(" Already done: {}.", completed_steps.join("; ")));
        }
        for blocker in &self.blockers {
            message.push(' ');
            message.push_str(blocker.message());
        }
        Error::TeardownRefused {
            work_feature: self.work_feature.clone(),
            message,
            plan: Box::new(self),
            changed_anything: true,
            completed_steps,
        }
    }
}

impl ChangeArea {
    /// Devcontainer and compose files: the areas modules manage.
    pub fn is_module_area(self) -> bool {
        matches!(self, ChangeArea::Devcontainer | ChangeArea::Compose)
    }
}

impl Blocker {
    /// User changes that `--discard-changes` would discard; `listed` are named in the message.
    pub fn uncommitted_changes(worktree: &Path, listed: &[&UserChange], unlisted: usize) -> Self {
        let count = listed.len() + unlisted;
        let mut names: Vec<String> = listed
            .iter()
            .take(REFUSAL_LISTED_FILES)
            .map(|change| format!("{} ({})", change.path, change.kind.label()))
            .collect();
        let more = count - names.len();
        if more > 0 {
            names.push(format!("and {more} more"));
        }
        let them = if count == 1 { "it" } else { "them" };
        Blocker::UncommittedChanges {
            count,
            message: format!(
                "{count} uncommitted {} in {} would be lost: {}. Commit or stash {them}, or rerun \
                 with --discard-changes to discard {them}.",
                plural(count, "change", "changes"),
                worktree.display(),
                names.join(", ")
            ),
            override_hint: "--discard-changes".to_string(),
        }
    }

    /// A delete of `branch`, which has `ahead` commits that `reference_name` lacks.
    pub fn unmerged_branch(branch: &str, ahead: u32, reference_name: &str) -> Self {
        Blocker::UnmergedBranch {
            branch: branch.to_string(),
            ahead,
            message: format!(
                "Branch '{branch}' has {ahead} {} not merged into {reference_name}; rerun with \
                 --keep-branch to keep it, or --force-delete-branch to delete it anyway.",
                plural(ahead as usize, "commit", "commits")
            ),
            override_hint: "--keep-branch | --force-delete-branch".to_string(),
        }
    }

    /// The worktree is locked with `git worktree lock`.
    pub fn worktree_locked(worktree: &Path, reason: Option<String>) -> Self {
        let because = reason
            .as_deref()
            .map(|reason| format!(" (reason: {reason})"))
            .unwrap_or_default();
        Blocker::WorktreeLocked {
            message: format!(
                "Worktree {path} is locked{because}; unlock it with `git worktree unlock {path}`, \
                 or rerun with --force to remove it anyway.",
                path = worktree.display()
            ),
            reason,
            override_hint: "--force".to_string(),
        }
    }

    /// `git status` failed in the worktree, so its changes are unknown.
    pub fn status_unavailable(worktree: &Path, cause: String) -> Self {
        Blocker::StatusUnavailable {
            message: format!(
                "Cannot list the uncommitted changes in {}: {cause}. Repair the worktree, or rerun \
                 with --force to remove it without checking.",
                worktree.display()
            ),
            cause,
            override_hint: "--force".to_string(),
        }
    }

    /// `git worktree remove` failed after the runtime and modules were already stopped.
    pub fn worktree_removal_failed(worktree: &Path, cause: String) -> Self {
        Blocker::WorktreeRemovalFailed {
            message: format!(
                "git could not remove the worktree {}: {cause}. Rerun with --force to remove the \
                 directory anyway.",
                worktree.display()
            ),
            cause,
        }
    }

    pub fn runtime_cleanup_failed(worktree: &Path, cause: String) -> Self {
        let filter = format!("label=devcontainer.local_folder={}", worktree.display());
        let filter = format!("'{}'", filter.replace('\'', "'\\''"));
        Blocker::RuntimeCleanupFailed {
            message: format!(
                "Could not verify runtime cleanup for {}: {cause}. Start Docker or restore runtime access and retry. \
                 Inspect owned containers with `docker ps -a --filter {filter}`. \
                 --force removes the worktree anyway and can leave runtime resources; add --keep-branch to retain the branch, otherwise --force force-deletes it.",
                worktree.display()
            ),
            cause,
            override_hint: "--force".to_string(),
        }
    }

    /// The directory `worktree` is not a linked worktree of the repository; `cause` says what
    /// it is instead.
    pub fn not_a_worktree(worktree: &Path, cause: String) -> Self {
        Blocker::NotAWorktree {
            message: format!(
                "{} is not a BranchBox feature worktree: {cause}. Teardown never removes it, with \
                 or without --force; if it is a leftover folder, delete it yourself and rerun.",
                worktree.display()
            ),
            cause,
        }
    }

    /// Moving the feature spec at `path` (relative to `worktree`) to the main worktree failed.
    pub fn spec_not_preserved(worktree: &Path, path: &str, cause: String) -> Self {
        Blocker::SpecNotPreserved {
            message: format!(
                "Could not move the feature spec {path} out of {} ({cause}); removing the \
                 worktree would lose it. Fix the cause and rerun, or rerun with --force to remove \
                 it anyway.",
                worktree.display()
            ),
            path: path.to_string(),
            cause,
            override_hint: "--force".to_string(),
        }
    }

    /// The cause-naming sentence for this blocker.
    pub fn message(&self) -> &str {
        match self {
            Blocker::UncommittedChanges { message, .. }
            | Blocker::UnmergedBranch { message, .. }
            | Blocker::WorktreeLocked { message, .. }
            | Blocker::StatusUnavailable { message, .. }
            | Blocker::WorktreeRemovalFailed { message, .. }
            | Blocker::RuntimeCleanupFailed { message, .. }
            | Blocker::NotAWorktree { message, .. }
            | Blocker::SpecNotPreserved { message, .. } => message,
        }
    }
}

impl ChangeKind {
    /// The contract word for this kind, as serialized.
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Untracked => "untracked",
            ChangeKind::Modified => "modified",
            ChangeKind::Added => "added",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Typechange => "typechange",
            ChangeKind::Conflicted => "conflicted",
            ChangeKind::Staged => "staged",
        }
    }
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 {
        one
    } else {
        many
    }
}

/// One entry of `git status --porcelain=v1 -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// The staged status letter (`X`): ` ` unchanged, `M`, `A`, `D`, `R`, `C`, `T`, `U`, or
    /// `?` for untracked.
    pub index: char,
    /// The working-tree status letter (`Y`), same alphabet.
    pub worktree: char,
    /// Path relative to the worktree root, `/`-separated, exactly as git wrote it.
    pub path: String,
    /// For a rename or copy, the path it came from.
    pub original_path: Option<String>,
}

impl StatusEntry {
    /// What kind of change this is, in the contract vocabulary.
    pub fn kind(&self) -> ChangeKind {
        match (self.index, self.worktree) {
            ('?', '?') => ChangeKind::Untracked,
            _ if self.is_conflicted() => ChangeKind::Conflicted,
            (x, y) if x == 'T' || y == 'T' => ChangeKind::Typechange,
            ('A', _) => ChangeKind::Added,
            (x, y) if x == 'D' || y == 'D' => ChangeKind::Deleted,
            (_, 'M') => ChangeKind::Modified,
            ('M' | 'R' | 'C', _) => ChangeKind::Staged,
            _ => ChangeKind::Modified,
        }
    }

    /// An unmerged path (`DD`, `AU`, `UD`, `UA`, `DU`, `AA`, `UU`).
    pub fn is_conflicted(&self) -> bool {
        self.index == 'U'
            || self.worktree == 'U'
            || (self.index == 'D' && self.worktree == 'D')
            || (self.index == 'A' && self.worktree == 'A')
    }

    /// Untracked (`??`).
    pub fn is_untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }

    /// Nothing is staged: the change exists only in the working tree (or the file is
    /// untracked), so the index still matches `HEAD`.
    fn is_unstaged_only(&self) -> bool {
        self.index == ' ' || self.is_untracked()
    }

    /// A tracked file removed from the working tree, with nothing else staged: its content is
    /// still in `HEAD`, so discarding the deletion loses nothing.
    pub fn is_pure_deletion(&self) -> bool {
        matches!((self.index, self.worktree), (' ', 'D') | ('D', ' '))
    }
}

/// Parse the NUL-framed output of `git status --porcelain=v1 -z`.
///
/// Each record is `XY PATH`, followed by a second `ORIG_PATH` record for renames and copies.
/// Paths are raw (no quoting, no ` -> `), so spaces, quotes, arrows and non-ASCII names come
/// through unchanged; bytes that are not UTF-8 are replaced, which can only make a path fail
/// to match a generated-file rule. Ignored entries (`!!`) are skipped.
///
/// # Errors
///
/// A record that is not `XY PATH`, or a rename without its original path, is an error: an
/// unreadable status must never look like a clean worktree.
pub fn parse_porcelain_z(raw: &[u8]) -> std::result::Result<Vec<StatusEntry>, String> {
    let mut entries = Vec::new();
    let mut fields = raw.split(|byte| *byte == 0);
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 || record[2] != b' ' || !record[..2].is_ascii() {
            return Err(format!(
                "malformed status record '{}'",
                String::from_utf8_lossy(record)
            ));
        }
        let index = char::from(record[0]);
        let worktree = char::from(record[1]);
        if index == '!' && worktree == '!' {
            continue;
        }
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        let original_path = if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') {
            match fields.next() {
                Some(original) if !original.is_empty() => {
                    Some(String::from_utf8_lossy(original).into_owned())
                }
                _ => return Err(format!("rename of '{path}' without its original path")),
            }
        } else {
            None
        };
        entries.push(StatusEntry {
            index,
            worktree,
            path,
            original_path,
        });
    }
    Ok(entries)
}

/// Whether `path` is relative and stays inside the root it is joined to: no leading `/`, no
/// empty, `.` or `..` components. Anything else is never read and never generated.
fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\0')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
}

/// Paths modules own: `.devcontainer/` and `compose/` trees and compose files anywhere. The
/// generated sbx configuration is not module-managed (teardown always discards it).
pub(crate) fn is_module_managed_path(path: &str) -> bool {
    use std::ffi::OsStr;

    if matches!(
        path,
        ".devcontainer/.devcontainer.json" | ".devcontainer/.branchbox-sbx-compose.yaml"
    ) || is_sbx_compose_input(path)
    {
        return false;
    }

    const MODULE_PREFIXES: [&str; 2] = [".devcontainer", "compose"];
    if MODULE_PREFIXES
        .iter()
        .any(|prefix| path_matches_prefix(path, prefix))
    {
        return true;
    }

    const MODULE_FILES: [&str; 4] = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yml",
        "docker-compose.yaml",
    ];

    if let Some(name) = Path::new(path).file_name().and_then(OsStr::to_str) {
        return MODULE_FILES.contains(&name);
    }

    false
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    if path == prefix {
        return true;
    }
    path.strip_prefix(prefix)
        .map(|remainder| remainder.starts_with('/'))
        .unwrap_or(false)
}

/// Which part of the project `path` belongs to.
pub fn area_for_path(path: &str) -> ChangeArea {
    if path_matches_prefix(path, ".devcontainer") {
        return ChangeArea::Devcontainer;
    }
    if is_module_managed_path(path) {
        return ChangeArea::Compose;
    }
    if path_matches_prefix(path, ".vscode") {
        return ChangeArea::Vscode;
    }
    if path_matches_prefix(path, "docs/features") {
        return ChangeArea::Spec;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == ".env" || name.starts_with(".env.") {
        return ChangeArea::Env;
    }
    ChangeArea::Other
}

/// A file as the classifier sees it. Symbolic links are reported, never followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileProbe {
    /// Nothing exists at the path.
    Missing,
    /// A regular file of at most [`MAX_CLASSIFIED_FILE_BYTES`], with its contents.
    File(Vec<u8>),
    /// A symbolic link, with its target as written.
    Symlink(PathBuf),
    /// Anything the classifier does not compare: a directory, a special or oversized file, a
    /// path through a symlinked directory, or a read error.
    Unreadable,
}

/// What the classifier reads besides `git status`. [`FsChangeContext`] reads the disk and git;
/// tests substitute a fake.
pub trait ChangeContext {
    /// The feature worktree's file at `path` (relative to the worktree root).
    fn worktree_file(&self, path: &str) -> FileProbe;
    /// The main worktree's file at `path` (relative to the main worktree root).
    fn main_file(&self, path: &str) -> FileProbe;
    /// The size of the main worktree's regular file at `path`, when it is cheap to tell; the
    /// classifier then reads that file only when its size matches. `None` means unknown.
    fn main_file_len(&self, _path: &str) -> Option<u64> {
        None
    }
    /// The committed contents of `path` at the feature worktree's `HEAD`, if it has that file.
    fn head_file(&self, path: &str) -> Option<Vec<u8>>;
    /// The devcontainer sync baseline digest of `relative` (a path inside `.devcontainer/`).
    fn baseline_digest(&self, relative: &str) -> Option<String>;
    /// Whether a link points exactly to the main-worktree source that symlink sync would use.
    /// A matching recorded digest is required separately; arbitrary links remain user work.
    fn is_devcontainer_sync_link(&self, _path: &str, _target: &Path) -> bool {
        false
    }
}

/// The [`ChangeContext`] of a real feature worktree and its main worktree.
#[derive(Debug, Clone)]
pub struct FsChangeContext {
    worktree: PathBuf,
    main: PathBuf,
    baseline: Option<DevcontainerBaseline>,
}

impl FsChangeContext {
    /// `baseline` is the feature's devcontainer sync baseline, if one was recorded.
    pub fn new(
        worktree: impl Into<PathBuf>,
        main: impl Into<PathBuf>,
        baseline: Option<DevcontainerBaseline>,
    ) -> Self {
        Self {
            worktree: worktree.into(),
            main: main.into(),
            baseline,
        }
    }
}

impl ChangeContext for FsChangeContext {
    fn worktree_file(&self, path: &str) -> FileProbe {
        probe_file(&self.worktree, path)
    }

    fn main_file(&self, path: &str) -> FileProbe {
        probe_file(&self.main, path)
    }

    fn main_file_len(&self, path: &str) -> Option<u64> {
        if !is_safe_relative_path(path) {
            return None;
        }
        fs::symlink_metadata(self.main.join(path))
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
    }

    fn head_file(&self, path: &str) -> Option<Vec<u8>> {
        if !is_safe_relative_path(path) {
            return None;
        }
        let output = Command::new("git")
            .current_dir(&self.worktree)
            .args(["cat-file", "blob", &format!("HEAD:{path}")])
            .output()
            .ok()?;
        (output.status.success() && output.stdout.len() as u64 <= MAX_CLASSIFIED_FILE_BYTES)
            .then_some(output.stdout)
    }

    fn baseline_digest(&self, relative: &str) -> Option<String> {
        self.baseline.as_ref()?.get(relative).cloned()
    }

    fn is_devcontainer_sync_link(&self, path: &str, target: &Path) -> bool {
        if !path.starts_with(".devcontainer/") || !is_safe_relative_path(path) {
            return false;
        }
        let destination = self.worktree.join(path);
        let Some(parent) = destination.parent() else {
            return false;
        };
        pathdiff::diff_paths(self.main.join(path), parent).as_deref() == Some(target)
            && matches!(
                self.main_file(path),
                FileProbe::File(_) | FileProbe::Symlink(_)
            )
    }
}

/// Look at `root/relative` without following any symbolic link: every directory on the way
/// must be a real directory, a final symlink is reported with its target, and a regular file
/// is read only up to [`MAX_CLASSIFIED_FILE_BYTES`].
pub fn probe_file(root: &Path, relative: &str) -> FileProbe {
    if !is_safe_relative_path(relative) {
        return FileProbe::Unreadable;
    }
    let mut current = root.to_path_buf();
    let mut components = relative.split('/').peekable();
    while let Some(component) = components.next() {
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return FileProbe::Missing,
            Err(_) => return FileProbe::Unreadable,
        };
        if components.peek().is_some() {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return FileProbe::Unreadable;
            }
            continue;
        }
        if metadata.file_type().is_symlink() {
            return fs::read_link(&current)
                .map(FileProbe::Symlink)
                .unwrap_or(FileProbe::Unreadable);
        }
        if !metadata.is_file() || metadata.len() > MAX_CLASSIFIED_FILE_BYTES {
            return FileProbe::Unreadable;
        }
        return read_regular_file(&current).unwrap_or(FileProbe::Unreadable);
    }
    FileProbe::Unreadable
}

/// Read a regular file that must not have turned into a symlink since it was inspected.
fn read_regular_file(path: &Path) -> io::Result<FileProbe> {
    let file = open_no_follow(path)?;
    if !file.metadata()?.is_file() {
        return Ok(FileProbe::Unreadable);
    }
    let mut bytes = Vec::new();
    file.take(MAX_CLASSIFIED_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CLASSIFIED_FILE_BYTES {
        return Ok(FileProbe::Unreadable);
    }
    Ok(FileProbe::File(bytes))
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// A worktree's changes, sorted by [`classify_changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub changes: ChangeSet,
    /// User changes past [`MAX_CLASSIFIED_ENTRIES`], counted but not listed.
    pub unlisted_user_changes: usize,
    /// Listed user changes that are pure deletions ([`StatusEntry::is_pure_deletion`]).
    pub pure_deletions: BTreeSet<String>,
}

impl Classification {
    /// The listed user changes that hold content teardown would destroy: all but the pure
    /// deletions. (`unlisted_user_changes` counts the rest, which are never read.)
    pub fn content_changes(&self) -> impl Iterator<Item = &UserChange> {
        self.changes
            .user
            .iter()
            .filter(|change| !self.pure_deletions.contains(&change.path))
    }
}

/// The spec destination teardown moves `work_feature`'s spec to, relative to the main worktree.
pub fn spec_destination(work_feature: &str, complete_spec: bool) -> String {
    let status = if complete_spec {
        "completed"
    } else {
        "backlog"
    };
    format!("docs/features/{status}/{work_feature}.md")
}

/// Sort `entries` (from `git status`) into user changes, BranchBox-generated files and the
/// feature spec teardown keeps, applying DESIGN §6.5's rules in order:
///
/// - R1 reserved names in `.devcontainer/` are generated;
/// - R2 the one spec of this feature that teardown moves (or copies) to the main worktree first,
///   `kept_spec`, is preserved, and so is the deletion of a promoted spec (its content is in
///   `HEAD`). Any other copy under `docs/features/{in-progress,backlog,completed}/<name>.md` is
///   lost with the worktree, so it goes through the remaining rules like any other file;
/// - R3 a `.devcontainer/` file whose digest equals the feature's sync baseline is generated;
/// - R4 a file identical to the main worktree's (or a symlink with the same target) is
///   generated;
/// - R5 `.devcontainer/.env` linking (or identical) to the worktree `.env` is generated;
/// - R6 a `.env` that is main's `.env` plus only BranchBox's feature block is generated;
/// - R7 `.vscode/settings.json` that differs from the committed file (or `{}`) only in the
///   settings BranchBox manages, and `.vscode/tasks.json` holding only the feature-URL task,
///   are generated;
/// - anything else is a user change.
///
/// R3-R7 apply only to changes with nothing staged; renames, conflicts and paths that are not
/// plain relative paths are always user changes. After [`MAX_CLASSIFIED_ENTRIES`] entries the
/// rest are counted as user changes without being read.
///
/// `kept_spec` is the worktree-relative path of the spec teardown moves out: the first of
/// `in-progress/`, `backlog/` and `completed/` that exists, or `None` when teardown moves no spec
/// (an in-guest teardown, or after the move already ran).
pub fn classify_changes(
    entries: &[StatusEntry],
    context: &dyn ChangeContext,
    work_feature: &str,
    complete_spec: bool,
    kept_spec: Option<&str>,
) -> Classification {
    let spec_paths = ["in-progress", "backlog", "completed"]
        .map(|status| format!("docs/features/{status}/{work_feature}.md"));
    let destination = spec_destination(work_feature, complete_spec);
    let mut changes = ChangeSet {
        status_available: true,
        truncated: false,
        user: Vec::new(),
        generated: Vec::new(),
        preserved: Vec::new(),
    };
    let mut pure_deletions = BTreeSet::new();
    let mut unlisted_user_changes = 0;

    for (position, entry) in entries.iter().enumerate() {
        if position >= MAX_CLASSIFIED_ENTRIES {
            unlisted_user_changes = entries.len() - position;
            changes.truncated = true;
            break;
        }
        match classify_entry(entry, context, &spec_paths, kept_spec) {
            Verdict::Generated(rule) => changes.generated.push(GeneratedChange {
                path: entry.path.clone(),
                rule,
            }),
            Verdict::Preserved => changes.preserved.push(PreservedFile {
                path: entry.path.clone(),
                destination: destination.clone(),
            }),
            Verdict::User => {
                if entry.is_pure_deletion() {
                    pure_deletions.insert(entry.path.clone());
                }
                changes.user.push(UserChange {
                    path: entry.path.clone(),
                    kind: entry.kind(),
                    area: area_for_path(&entry.path),
                });
            }
        }
    }

    Classification {
        changes,
        unlisted_user_changes,
        pure_deletions,
    }
}

enum Verdict {
    Generated(GeneratedRule),
    Preserved,
    User,
}

fn classify_entry(
    entry: &StatusEntry,
    context: &dyn ChangeContext,
    spec_paths: &[String; 3],
    kept_spec: Option<&str>,
) -> Verdict {
    let path = entry.path.as_str();
    if entry.original_path.is_some() || !is_safe_relative_path(path) {
        return Verdict::User;
    }
    if RESERVED_NAMES.contains(&path) || is_sbx_compose_input(path) {
        return Verdict::Generated(GeneratedRule::ReservedName);
    }
    if spec_paths.iter().any(|spec| spec == path)
        && (kept_spec == Some(path) || context.worktree_file(path) == FileProbe::Missing)
    {
        return Verdict::Preserved;
    }
    if entry.is_conflicted() || !entry.is_unstaged_only() {
        return Verdict::User;
    }
    let file = context.worktree_file(path);
    generated_rule(path, entry, &file, context).map_or(Verdict::User, Verdict::Generated)
}

/// Rules R3-R7 for an unstaged change whose worktree file is `file`.
fn generated_rule(
    path: &str,
    entry: &StatusEntry,
    file: &FileProbe,
    context: &dyn ChangeContext,
) -> Option<GeneratedRule> {
    if let Some(relative) = path.strip_prefix(".devcontainer/") {
        let digest = match file {
            FileProbe::File(bytes) => Some(baseline_digest(bytes)),
            FileProbe::Symlink(target) if context.is_devcontainer_sync_link(path, target) => {
                Some(baseline_symlink_digest(target))
            }
            _ => None,
        };
        if digest.is_some() && context.baseline_digest(relative) == digest {
            return Some(GeneratedRule::DevcontainerBaseline);
        }
    }
    if derived_from_main(path, file, context) {
        return Some(GeneratedRule::DerivedFromMain);
    }
    match path {
        ".devcontainer/.env" if links_worktree_env(file, context) => {
            Some(GeneratedRule::DevcontainerEnvLink)
        }
        ".env" if holds_only_feature_block(file, context) => Some(GeneratedRule::EnvFeatureBlock),
        ".vscode/settings.json" if has_only_managed_settings(entry, file, context) => {
            Some(GeneratedRule::VscodeManagedKeys)
        }
        ".vscode/tasks.json" if holds_only_feature_url_task(file) => {
            Some(GeneratedRule::VscodeManagedTasks)
        }
        _ => None,
    }
}

/// R4: byte-identical to the main worktree's file, or a symlink with the same target. Main's
/// file is read only when its size matches.
fn derived_from_main(path: &str, file: &FileProbe, context: &dyn ChangeContext) -> bool {
    if let FileProbe::File(bytes) = file {
        if context
            .main_file_len(path)
            .is_some_and(|len| len != bytes.len() as u64)
        {
            return false;
        }
    }
    match file {
        FileProbe::File(_) | FileProbe::Symlink(_) => context.main_file(path) == *file,
        FileProbe::Missing | FileProbe::Unreadable => false,
    }
}

/// R5: `.devcontainer/.env` is the `../.env` link start creates, or a copy of the worktree
/// `.env` (start's fallback where symlinks fail).
fn links_worktree_env(file: &FileProbe, context: &dyn ChangeContext) -> bool {
    match file {
        FileProbe::Symlink(target) => target == Path::new("../.env"),
        FileProbe::File(bytes) => context.worktree_file(".env") == FileProbe::File(bytes.clone()),
        FileProbe::Missing | FileProbe::Unreadable => false,
    }
}

/// R6: the worktree `.env` is main's `.env` followed by BranchBox's feature block, and the
/// block holds nothing but BranchBox's comments, blank lines and managed variables.
fn holds_only_feature_block(file: &FileProbe, context: &dyn ChangeContext) -> bool {
    let FileProbe::File(bytes) = file else {
        return false;
    };
    let FileProbe::File(main_bytes) = context.main_file(".env") else {
        return false;
    };
    let (Ok(text), Ok(main_text)) = (std::str::from_utf8(bytes), std::str::from_utf8(&main_bytes))
    else {
        return false;
    };
    let Some(marker) = text.find(ENV_FEATURE_SECTION_MARKER) else {
        return false;
    };
    let main_base = main_text
        .find(ENV_FEATURE_SECTION_MARKER)
        .map_or(main_text, |position| &main_text[..position]);
    if text[..marker].trim_end() != main_base.trim_end() {
        return false;
    }
    text[marker..].lines().all(|line| {
        let line = line.trim();
        line.is_empty()
            || ENV_MANAGED_COMMENTS.contains(&line)
            || line
                .split_once('=')
                .is_some_and(|(key, _)| ENV_MANAGED_KEYS.contains(&key.trim()))
    })
}

/// R7: `.vscode/settings.json` equals the committed file (`{}` when untracked) once the
/// settings BranchBox manages are removed from both.
fn has_only_managed_settings(
    entry: &StatusEntry,
    file: &FileProbe,
    context: &dyn ChangeContext,
) -> bool {
    let FileProbe::File(bytes) = file else {
        return false;
    };
    let Some(current) = parse_jsonc_object(bytes) else {
        return false;
    };
    let committed = if entry.is_untracked() {
        Map::new()
    } else {
        match context
            .head_file(".vscode/settings.json")
            .as_deref()
            .and_then(parse_jsonc_object)
        {
            Some(committed) => committed,
            None => return false,
        }
    };
    without_managed_settings(current) == without_managed_settings(committed)
}

fn without_managed_settings(mut settings: Map<String, Value>) -> Map<String, Value> {
    for key in VSCODE_MANAGED_SETTINGS {
        settings.remove(key);
    }
    settings
}

/// R7: `.vscode/tasks.json` holds exactly one task, the feature-URL task start writes.
fn holds_only_feature_url_task(file: &FileProbe) -> bool {
    let FileProbe::File(bytes) = file else {
        return false;
    };
    let Some(tasks) = parse_jsonc_object(bytes) else {
        return false;
    };
    let Some(Value::Array(tasks)) = tasks.get("tasks") else {
        return false;
    };
    let labels: Vec<Option<&str>> = tasks
        .iter()
        .map(|task| task.get("label").and_then(Value::as_str))
        .collect();
    labels == [Some(VSCODE_FEATURE_URL_TASK)]
}

/// A JSON (with comments) object, or `None` for anything else.
pub(crate) fn parse_jsonc_object(bytes: &[u8]) -> Option<Map<String, Value>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match jsonc_parser::parse_to_serde_value(text, &Default::default()) {
        Ok(Some(Value::Object(object))) => Some(object),
        _ => None,
    }
}

/// The branch teardown acts on and where its name came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBranch {
    pub name: String,
    pub source: BranchSource,
}

/// What `git status` said about the worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStatus {
    /// The worktree directory does not exist, so there are no changes to lose.
    Missing,
    /// The classified changes.
    Classified(Classification),
    /// `git status` failed; the cause names why.
    Unavailable(String),
}

/// Everything [`build_teardown_plan`] decides from. The feature workflow gathers it from the
/// registry, git and the project configuration; tests build it directly.
#[derive(Debug, Clone)]
pub struct PlanInputs<'a> {
    pub request: &'a TeardownRequest,
    pub options: TeardownOptions,
    pub recorded: Option<&'a FeatureMetadata>,
    pub worktree: WorktreeState,
    pub status: WorktreeStatus,
    pub branch: ResolvedBranch,
    /// `None` when the merge state could not be read (a warning says why).
    pub merge_state: Option<BranchMergeState>,
    pub defaults: TeardownDefaults,
    pub warnings: Vec<String>,
}

/// Whether teardown under `request` and `options` discards user changes. `--force` always
/// does, so a caller cannot ask for a forced removal that still refuses over dirt.
pub fn discards_changes(request: &TeardownRequest, options: &TeardownOptions) -> bool {
    options.discard_changes || request.force_remove
}

/// The listed user changes that block teardown under `request` and `options`. The legacy
/// `force_remove_modules` lets devcontainer and compose changes go, and nothing else.
pub fn blocking_changes<'c>(
    changes: impl IntoIterator<Item = &'c UserChange>,
    request: &TeardownRequest,
    options: &TeardownOptions,
) -> Vec<&'c UserChange> {
    if discards_changes(request, options) {
        return Vec::new();
    }
    changes
        .into_iter()
        .filter(|change| !(request.force_remove_modules && change.area.is_module_area()))
        .collect()
}

/// The branch step teardown takes for `request`: keep, delete (`git branch -d`), or force-delete
/// (`-D`, from `--force-delete-branch` or, for compatibility, `--force`).
pub fn branch_action(request: &TeardownRequest) -> BranchAction {
    if !request.delete_branch {
        BranchAction::Keep
    } else if request.force_delete_branch || request.force_remove {
        BranchAction::ForceDelete
    } else {
        BranchAction::Delete
    }
}

/// Decide the plan: the branch action and every blocker, from facts gathered beforehand. Pure,
/// so each decision is unit-tested without a repository.
///
/// Blockers:
/// - `uncommitted_changes` when user changes exist and the request does not discard them;
/// - `unmerged_branch` when `options.require_mergeable_branch` and a plain delete would fail
///   because the branch has commits its reference lacks;
/// - `worktree_locked` and `status_unavailable` unless `--force`.
pub fn build_teardown_plan(inputs: PlanInputs<'_>) -> TeardownPlan {
    let PlanInputs {
        request,
        options,
        recorded,
        worktree,
        status,
        branch,
        merge_state,
        defaults,
        mut warnings,
    } = inputs;
    let mut blockers = Vec::new();

    let changes = match status {
        WorktreeStatus::Missing => {
            if !request.force_remove {
                warnings.push(format!(
                    "Worktree directory '{}' does not exist; tearing down what is left needs \
                     --force",
                    worktree.path.display()
                ));
            }
            ChangeSet::empty(true)
        }
        WorktreeStatus::Classified(classification) => {
            let blocking = blocking_changes(&classification.changes.user, request, &options);
            let unlisted = if discards_changes(request, &options) {
                0
            } else {
                classification.unlisted_user_changes
            };
            if !blocking.is_empty() || unlisted > 0 {
                blockers.push(Blocker::uncommitted_changes(
                    &worktree.path,
                    &blocking,
                    unlisted,
                ));
            }
            if classification.changes.truncated {
                warnings.push(format!(
                    "Only the first {MAX_CLASSIFIED_ENTRIES} changes were classified; {} more \
                     count as uncommitted changes",
                    classification.unlisted_user_changes
                ));
            }
            classification.changes
        }
        WorktreeStatus::Unavailable(cause) => {
            if !request.force_remove {
                blockers.push(Blocker::status_unavailable(&worktree.path, cause));
            }
            ChangeSet::empty(false)
        }
    };

    if worktree.locked && !request.force_remove {
        blockers.push(Blocker::worktree_locked(
            &worktree.path,
            worktree.lock_reason.clone(),
        ));
    }

    let action = branch_action(request);
    let branch_plan = merge_state.map(|state| {
        if options.require_mergeable_branch
            && action == BranchAction::Delete
            && state.exists
            && !state.merged
        {
            blockers.push(Blocker::unmerged_branch(
                &branch.name,
                state.ahead,
                &state.reference_name,
            ));
        }
        if action != BranchAction::Keep && !state.exists {
            warnings.push(format!(
                "Branch '{}' does not exist; there is no branch to delete",
                branch.name
            ));
        }
        BranchPlan {
            name: branch.name.clone(),
            source: branch.source,
            exists: state.exists,
            upstream: state.upstream,
            reference: state.reference,
            reference_name: state.reference_name,
            merged: state.merged,
            merged_into_head: state.merged_into_head,
            ahead: state.ahead,
            action,
        }
    });

    TeardownPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        work_feature: request.work_feature.clone(),
        registered: recorded.is_some(),
        status: recorded.map(|metadata| metadata.status.clone()),
        worktree,
        changes,
        branch: branch_plan,
        defaults,
        runtime: recorded.map(|metadata| RuntimeRef {
            provider: Some(metadata.runtime.provider.to_string()),
            runtime_id: metadata.runtime.runtime_id.clone(),
        }),
        tunnel: recorded.and_then(|metadata| {
            metadata.tunnel.as_ref().map(|tunnel| TunnelRef {
                status: Some(tunnel.status.clone()),
            })
        }),
        blockers,
        warnings,
    }
}

impl ChangeSet {
    /// No changes; `status_available` says whether that is known or merely unread.
    pub fn empty(status_available: bool) -> Self {
        Self {
            status_available,
            truncated: false,
            user: Vec::new(),
            generated: Vec::new(),
            preserved: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The DESIGN §5.5 example document.
    fn design_example_json() -> serde_json::Value {
        json!({
            "schema_version": 1, "work_feature": "eta", "registered": true, "status": "active",
            "worktree": {"path": "/r/eta", "exists": true, "locked": false, "lock_reason": null},
            "changes": {"status_available": true, "truncated": false,
                "user": [
                    {"path": "README.md", "kind": "modified", "area": "other"},
                    {"path": "notes.txt", "kind": "untracked", "area": "other"}
                ],
                "generated": [
                    {"path": ".devcontainer/.branchbox.env", "rule": "reserved_name"},
                    {"path": ".vscode/settings.json", "rule": "vscode_managed_keys"}
                ],
                "preserved": [
                    {"path": "docs/features/in-progress/eta.md",
                     "destination": "docs/features/backlog/eta.md"}
                ]},
            "branch": {"name": "feature/eta", "source": "registry", "exists": true,
                "upstream": null, "reference": "HEAD", "reference_name": "main",
                "merged": false, "merged_into_head": false, "ahead": 3, "action": "delete"},
            "defaults": {"delete_branch_by_default": true,
                "force_delete_unmerged_by_default": false},
            "runtime": {"provider": "container", "runtime_id": null},
            "tunnel": {"status": "disabled"},
            "blockers": [
                {"kind": "uncommitted_changes", "count": 2, "message": "…",
                 "override": "--discard-changes"},
                {"kind": "unmerged_branch", "branch": "feature/eta", "ahead": 3, "message": "…",
                 "override": "--keep-branch | --force-delete-branch"}
            ],
            "warnings": []
        })
    }

    fn design_example_plan() -> TeardownPlan {
        TeardownPlan {
            schema_version: 1,
            work_feature: "eta".to_string(),
            registered: true,
            status: Some(FeatureStatus::Active),
            worktree: WorktreeState {
                path: PathBuf::from("/r/eta"),
                exists: true,
                locked: false,
                lock_reason: None,
            },
            changes: ChangeSet {
                status_available: true,
                truncated: false,
                user: vec![
                    UserChange {
                        path: "README.md".to_string(),
                        kind: ChangeKind::Modified,
                        area: ChangeArea::Other,
                    },
                    UserChange {
                        path: "notes.txt".to_string(),
                        kind: ChangeKind::Untracked,
                        area: ChangeArea::Other,
                    },
                ],
                generated: vec![
                    GeneratedChange {
                        path: ".devcontainer/.branchbox.env".to_string(),
                        rule: GeneratedRule::ReservedName,
                    },
                    GeneratedChange {
                        path: ".vscode/settings.json".to_string(),
                        rule: GeneratedRule::VscodeManagedKeys,
                    },
                ],
                preserved: vec![PreservedFile {
                    path: "docs/features/in-progress/eta.md".to_string(),
                    destination: "docs/features/backlog/eta.md".to_string(),
                }],
            },
            branch: Some(BranchPlan {
                name: "feature/eta".to_string(),
                source: BranchSource::Registry,
                exists: true,
                upstream: None,
                reference: "HEAD".to_string(),
                reference_name: "main".to_string(),
                merged: false,
                merged_into_head: false,
                ahead: 3,
                action: BranchAction::Delete,
            }),
            defaults: TeardownDefaults {
                delete_branch_by_default: true,
                force_delete_unmerged_by_default: false,
            },
            runtime: Some(RuntimeRef {
                provider: Some("container".to_string()),
                runtime_id: None,
            }),
            tunnel: Some(TunnelRef {
                status: Some(FeatureTunnelStatus::Disabled),
            }),
            blockers: vec![
                Blocker::UncommittedChanges {
                    count: 2,
                    message: "…".to_string(),
                    override_hint: "--discard-changes".to_string(),
                },
                Blocker::UnmergedBranch {
                    branch: "feature/eta".to_string(),
                    ahead: 3,
                    message: "…".to_string(),
                    override_hint: "--keep-branch | --force-delete-branch".to_string(),
                },
            ],
            warnings: Vec::new(),
        }
    }

    #[test]
    fn plan_serializes_to_the_design_contract() {
        let value = serde_json::to_value(design_example_plan()).unwrap();
        assert_eq!(value, design_example_json());
    }

    #[test]
    fn plan_deserializes_from_the_design_contract() {
        let plan: TeardownPlan = serde_json::from_value(design_example_json()).unwrap();
        assert_eq!(plan, design_example_plan());
    }

    #[test]
    fn enum_values_match_the_contract_vocabulary() {
        let kinds = [
            (ChangeKind::Untracked, "untracked"),
            (ChangeKind::Modified, "modified"),
            (ChangeKind::Added, "added"),
            (ChangeKind::Deleted, "deleted"),
            (ChangeKind::Typechange, "typechange"),
            (ChangeKind::Conflicted, "conflicted"),
            (ChangeKind::Staged, "staged"),
        ];
        for (kind, raw) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(raw));
        }

        let areas = [
            (ChangeArea::Devcontainer, "devcontainer"),
            (ChangeArea::Compose, "compose"),
            (ChangeArea::Vscode, "vscode"),
            (ChangeArea::Spec, "spec"),
            (ChangeArea::Env, "env"),
            (ChangeArea::Other, "other"),
        ];
        for (area, raw) in areas {
            assert_eq!(serde_json::to_value(area).unwrap(), json!(raw));
        }

        let rules = [
            (GeneratedRule::ReservedName, "reserved_name"),
            (GeneratedRule::DevcontainerBaseline, "devcontainer_baseline"),
            (GeneratedRule::DerivedFromMain, "derived_from_main"),
            (GeneratedRule::DevcontainerEnvLink, "devcontainer_env_link"),
            (GeneratedRule::EnvFeatureBlock, "env_feature_block"),
            (GeneratedRule::VscodeManagedKeys, "vscode_managed_keys"),
            (GeneratedRule::VscodeManagedTasks, "vscode_managed_tasks"),
        ];
        for (rule, raw) in rules {
            assert_eq!(serde_json::to_value(rule).unwrap(), json!(raw));
        }

        let sources = [
            (BranchSource::ExplicitPrefix, "explicit_prefix"),
            (BranchSource::Registry, "registry"),
            (BranchSource::ConfigPrefix, "config_prefix"),
        ];
        for (source, raw) in sources {
            assert_eq!(serde_json::to_value(source).unwrap(), json!(raw));
        }

        let actions = [
            (BranchAction::Keep, "keep"),
            (BranchAction::Delete, "delete"),
            (BranchAction::ForceDelete, "force_delete"),
        ];
        for (action, raw) in actions {
            assert_eq!(serde_json::to_value(action).unwrap(), json!(raw));
        }
    }

    #[test]
    fn blockers_are_tagged_by_kind() {
        let blockers = vec![
            Blocker::WorktreeLocked {
                reason: None,
                message: "locked".to_string(),
                override_hint: "--force".to_string(),
            },
            Blocker::StatusUnavailable {
                cause: "fatal: not a git repository".to_string(),
                message: "status".to_string(),
                override_hint: "--force".to_string(),
            },
            Blocker::WorktreeRemovalFailed {
                cause: "busy".to_string(),
                message: "removal".to_string(),
            },
            Blocker::NotAWorktree {
                cause: "main".to_string(),
                message: "not".to_string(),
            },
            Blocker::SpecNotPreserved {
                path: "docs/features/in-progress/eta.md".to_string(),
                cause: "denied".to_string(),
                message: "spec".to_string(),
                override_hint: "--force".to_string(),
            },
        ];
        let value = serde_json::to_value(&blockers).unwrap();
        assert_eq!(
            value,
            json!([
                {"kind": "worktree_locked", "reason": null, "message": "locked",
                 "override": "--force"},
                {"kind": "status_unavailable", "cause": "fatal: not a git repository",
                 "message": "status", "override": "--force"},
                {"kind": "worktree_removal_failed", "cause": "busy", "message": "removal"},
                {"kind": "not_a_worktree", "cause": "main", "message": "not"},
                {"kind": "spec_not_preserved", "path": "docs/features/in-progress/eta.md",
                 "cause": "denied", "message": "spec", "override": "--force"}
            ])
        );
        let round_trip: Vec<Blocker> = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip, blockers);
    }

    #[test]
    fn legacy_options_map_force_to_discard_without_unmerged_preflight() {
        let mut request = TeardownRequest {
            work_feature: "eta".to_string(),
            branch_prefix: None,
            delete_branch: true,
            force_delete_branch: false,
            force_remove: false,
            force_remove_modules: false,
            complete_spec: false,
            telemetry: false,
        };
        assert_eq!(
            TeardownOptions::legacy(&request),
            TeardownOptions::default()
        );

        request.force_remove = true;
        assert_eq!(
            TeardownOptions::legacy(&request),
            TeardownOptions {
                discard_changes: true,
                require_mergeable_branch: false,
            }
        );
    }

    // ---- status parsing ----------------------------------------------------------------

    fn entry(xy: &str, path: &str) -> StatusEntry {
        let mut chars = xy.chars();
        StatusEntry {
            index: chars.next().unwrap(),
            worktree: chars.next().unwrap(),
            path: path.to_string(),
            original_path: None,
        }
    }

    #[test]
    fn capabilities_name_the_three_teardown_guarantees() {
        assert_eq!(
            CAPABILITIES,
            [
                "teardown-plan",
                "teardown-discard-changes",
                "teardown-unmerged-preflight"
            ]
        );
    }

    #[test]
    fn porcelain_parser_keeps_awkward_paths_verbatim() {
        let raw = " M with space.txt\0?? \"quoted\".md\0?? a -> b.txt\0?? caf\u{e9}/\u{1f600}.txt\0A  new\nline\0";
        let entries = parse_porcelain_z(raw.as_bytes()).unwrap();
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "with space.txt",
                "\"quoted\".md",
                "a -> b.txt",
                "caf\u{e9}/\u{1f600}.txt",
                "new\nline"
            ]
        );
        assert_eq!((entries[0].index, entries[0].worktree), (' ', 'M'));
        assert!(entries[1].is_untracked());
        assert!(entries.iter().all(|entry| entry.original_path.is_none()));
    }

    #[test]
    fn porcelain_parser_pairs_renames_with_their_origin_and_skips_ignored() {
        let raw = b"R  new name.txt\0old name.txt\0!! target/\0C  copy.txt\0orig.txt\0";
        let entries = parse_porcelain_z(raw).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "new name.txt");
        assert_eq!(entries[0].original_path.as_deref(), Some("old name.txt"));
        assert_eq!(entries[1].original_path.as_deref(), Some("orig.txt"));
        assert!(parse_porcelain_z(b"").unwrap().is_empty());
    }

    #[test]
    fn porcelain_parser_rejects_what_it_cannot_read() {
        let err = parse_porcelain_z(b"?? fine\0garbage\0").unwrap_err();
        assert!(err.contains("garbage"), "{err}");
        assert!(parse_porcelain_z(b"MM\0").is_err());
        assert!(parse_porcelain_z(b"\xff\xfe x\0").is_err());
        let err = parse_porcelain_z(b"R  new.txt\0").unwrap_err();
        assert!(err.contains("new.txt"), "{err}");
    }

    #[test]
    fn status_letters_map_to_contract_kinds() {
        let cases = [
            ("??", ChangeKind::Untracked),
            (" M", ChangeKind::Modified),
            ("MM", ChangeKind::Modified),
            ("M ", ChangeKind::Staged),
            ("R ", ChangeKind::Staged),
            ("A ", ChangeKind::Added),
            ("AM", ChangeKind::Added),
            (" D", ChangeKind::Deleted),
            ("D ", ChangeKind::Deleted),
            ("MD", ChangeKind::Deleted),
            (" T", ChangeKind::Typechange),
            ("T ", ChangeKind::Typechange),
            ("UU", ChangeKind::Conflicted),
            ("AA", ChangeKind::Conflicted),
            ("DD", ChangeKind::Conflicted),
            ("DU", ChangeKind::Conflicted),
            (" X", ChangeKind::Modified),
        ];
        for (xy, kind) in cases {
            assert_eq!(entry(xy, "f").kind(), kind, "{xy}");
            assert_eq!(kind.label(), serde_json::to_value(kind).unwrap());
        }
        assert!(entry(" D", "f").is_pure_deletion());
        assert!(entry("D ", "f").is_pure_deletion());
        assert!(!entry("MD", "f").is_pure_deletion());
        assert!(!entry("??", "f").is_pure_deletion());
    }

    #[test]
    fn areas_follow_the_module_layout() {
        let cases = [
            (".devcontainer/devcontainer.json", ChangeArea::Devcontainer),
            (".devcontainer", ChangeArea::Devcontainer),
            (".devcontainer/.devcontainer.json", ChangeArea::Devcontainer),
            ("compose/app.yml", ChangeArea::Compose),
            ("deploy/docker-compose.yml", ChangeArea::Compose),
            ("compose.yaml", ChangeArea::Compose),
            (".vscode/settings.json", ChangeArea::Vscode),
            ("docs/features/backlog/x.md", ChangeArea::Spec),
            (".env", ChangeArea::Env),
            ("config/.env.local", ChangeArea::Env),
            (".devcontainerish/file", ChangeArea::Other),
            ("composer.json", ChangeArea::Other),
            ("README.md", ChangeArea::Other),
        ];
        for (path, area) in cases {
            assert_eq!(area_for_path(path), area, "{path}");
        }
        assert!(is_module_managed_path(".devcontainer/compose.yaml"));
        assert!(!is_module_managed_path(
            ".devcontainer/.branchbox-sbx-compose.yaml"
        ));
        assert!(ChangeArea::Compose.is_module_area());
        assert!(!ChangeArea::Vscode.is_module_area());
    }

    #[test]
    fn only_plain_relative_paths_are_safe() {
        for safe in ["a", "a/b.txt", ".env", "dir/.hidden/x"] {
            assert!(is_safe_relative_path(safe), "{safe}");
        }
        for unsafe_path in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "./a",
            "a/",
            "a\\b",
        ] {
            assert!(!is_safe_relative_path(unsafe_path), "{unsafe_path}");
        }
    }

    // ---- classification ----------------------------------------------------------------

    /// A [`ChangeContext`] backed by maps.
    #[derive(Default)]
    struct FakeContext {
        worktree: std::collections::HashMap<String, FileProbe>,
        main: std::collections::HashMap<String, FileProbe>,
        head: std::collections::HashMap<String, Vec<u8>>,
        baseline: std::collections::HashMap<String, String>,
    }

    impl FakeContext {
        fn worktree(mut self, path: &str, probe: FileProbe) -> Self {
            self.worktree.insert(path.to_string(), probe);
            self
        }
        fn main(mut self, path: &str, probe: FileProbe) -> Self {
            self.main.insert(path.to_string(), probe);
            self
        }
        fn head(mut self, path: &str, bytes: &str) -> Self {
            self.head
                .insert(path.to_string(), bytes.as_bytes().to_vec());
            self
        }
        fn baseline(mut self, relative: &str, bytes: &[u8]) -> Self {
            self.baseline
                .insert(relative.to_string(), baseline_digest(bytes));
            self
        }
    }

    impl ChangeContext for FakeContext {
        fn worktree_file(&self, path: &str) -> FileProbe {
            self.worktree
                .get(path)
                .cloned()
                .unwrap_or(FileProbe::Missing)
        }
        fn main_file(&self, path: &str) -> FileProbe {
            self.main.get(path).cloned().unwrap_or(FileProbe::Missing)
        }
        fn head_file(&self, path: &str) -> Option<Vec<u8>> {
            self.head.get(path).cloned()
        }
        fn baseline_digest(&self, relative: &str) -> Option<String> {
            self.baseline.get(relative).cloned()
        }
    }

    fn file(text: &str) -> FileProbe {
        FileProbe::File(text.as_bytes().to_vec())
    }

    /// Classify one entry and return its generated rule, or `None` for a user change.
    fn rule_for(entry: StatusEntry, context: &FakeContext) -> Option<GeneratedRule> {
        let classification = classify_changes(&[entry], context, "eta", false, None);
        assert!(classification.changes.preserved.is_empty());
        classification
            .changes
            .generated
            .first()
            .map(|generated| generated.rule)
    }

    #[test]
    fn r1_reserved_names_are_generated_whatever_their_state() {
        let context = FakeContext::default();
        for path in RESERVED_NAMES {
            assert_eq!(
                rule_for(entry("A ", path), &context),
                Some(GeneratedRule::ReservedName),
                "{path}"
            );
        }
        assert_eq!(
            rule_for(entry("??", ".devcontainer/.env.local"), &context),
            None
        );
    }

    #[test]
    fn r2_the_feature_spec_is_preserved_with_its_destination() {
        let kept = "docs/features/in-progress/eta.md";
        let entries = [
            entry("??", kept),
            entry(" D", "docs/features/backlog/eta.md"),
            entry("??", "docs/features/in-progress/other.md"),
        ];
        let context = FakeContext::default().worktree(kept, file("# eta"));
        let backlog = classify_changes(&entries, &context, "eta", false, Some(kept));
        assert_eq!(
            backlog
                .changes
                .preserved
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [kept, "docs/features/backlog/eta.md"]
        );
        assert!(backlog
            .changes
            .preserved
            .iter()
            .all(|file| file.destination == "docs/features/backlog/eta.md"));
        assert_eq!(backlog.changes.user.len(), 1);
        assert_eq!(backlog.changes.user[0].area, ChangeArea::Spec);

        let completed = classify_changes(&entries[..1], &context, "eta", true, Some(kept));
        assert_eq!(
            completed.changes.preserved,
            [PreservedFile {
                path: kept.to_string(),
                destination: "docs/features/completed/eta.md".to_string(),
            }]
        );
    }

    #[test]
    fn r2_only_the_spec_teardown_moves_is_preserved() {
        let kept = "docs/features/in-progress/eta.md";
        let second = "docs/features/completed/eta.md";
        let entries = [entry("??", kept), entry("??", second)];
        let context = FakeContext::default()
            .worktree(kept, file("# eta"))
            .worktree(second, file("# my completed notes"));
        let classification = classify_changes(&entries, &context, "eta", false, Some(kept));
        assert_eq!(classification.changes.preserved.len(), 1);
        assert_eq!(classification.changes.preserved[0].path, kept);
        assert_eq!(classification.changes.user.len(), 1);
        assert_eq!(classification.changes.user[0].path, second);

        // An in-guest teardown moves no spec: an edited spec is a user change.
        let in_guest = classify_changes(&entries[..1], &context, "eta", false, None);
        assert!(in_guest.changes.preserved.is_empty());
        assert_eq!(in_guest.changes.user.len(), 1);
        assert_eq!(in_guest.changes.user[0].path, kept);
    }

    #[test]
    fn r3_devcontainer_files_matching_the_sync_baseline_are_generated() {
        let synced = "{\"image\": \"alpine\", \"workspaceFolder\": \"/w\"}";
        let context = FakeContext::default()
            .worktree(".devcontainer/devcontainer.json", file(synced))
            .baseline("devcontainer.json", synced.as_bytes());
        assert_eq!(
            rule_for(entry(" M", ".devcontainer/devcontainer.json"), &context),
            Some(GeneratedRule::DevcontainerBaseline)
        );

        let edited = FakeContext::default()
            .worktree(
                ".devcontainer/devcontainer.json",
                file("{\"edited\": true}"),
            )
            .baseline("devcontainer.json", synced.as_bytes());
        assert_eq!(
            rule_for(entry(" M", ".devcontainer/devcontainer.json"), &edited),
            None
        );
        // The baseline only vouches for .devcontainer files.
        let outside = FakeContext::default()
            .worktree("devcontainer.json", file(synced))
            .baseline("devcontainer.json", synced.as_bytes());
        assert_eq!(rule_for(entry("??", "devcontainer.json"), &outside), None);
    }

    #[test]
    fn r4_files_identical_to_main_are_generated() {
        let context = FakeContext::default()
            .worktree("config/local.yml", file("same"))
            .main("config/local.yml", file("same"))
            .worktree("link", FileProbe::Symlink(PathBuf::from("target")))
            .main("link", FileProbe::Symlink(PathBuf::from("target")))
            .worktree("other-link", FileProbe::Symlink(PathBuf::from("a")))
            .main("other-link", FileProbe::Symlink(PathBuf::from("b")))
            .worktree("differs", file("mine"))
            .main("differs", file("theirs"))
            .worktree("big", FileProbe::Unreadable)
            .main("big", FileProbe::Unreadable);
        assert_eq!(
            rule_for(entry("??", "config/local.yml"), &context),
            Some(GeneratedRule::DerivedFromMain)
        );
        assert_eq!(
            rule_for(entry(" T", "link"), &context),
            Some(GeneratedRule::DerivedFromMain)
        );
        assert_eq!(rule_for(entry("??", "other-link"), &context), None);
        assert_eq!(rule_for(entry(" M", "differs"), &context), None);
        assert_eq!(rule_for(entry("??", "big"), &context), None);
        assert_eq!(rule_for(entry(" D", "gone"), &context), None);
    }

    #[test]
    fn r5_the_devcontainer_env_link_is_generated() {
        let link = FakeContext::default().worktree(
            ".devcontainer/.env",
            FileProbe::Symlink(PathBuf::from("../.env")),
        );
        assert_eq!(
            rule_for(entry("??", ".devcontainer/.env"), &link),
            Some(GeneratedRule::DevcontainerEnvLink)
        );
        let copy = FakeContext::default()
            .worktree(".devcontainer/.env", file("A=1\n"))
            .worktree(".env", file("A=1\n"));
        assert_eq!(
            rule_for(entry("??", ".devcontainer/.env"), &copy),
            Some(GeneratedRule::DevcontainerEnvLink)
        );
        let elsewhere = FakeContext::default().worktree(
            ".devcontainer/.env",
            FileProbe::Symlink(PathBuf::from("/etc/secrets")),
        );
        assert_eq!(
            rule_for(entry("??", ".devcontainer/.env"), &elsewhere),
            None
        );
        let edited = FakeContext::default()
            .worktree(".devcontainer/.env", file("A=2\n"))
            .worktree(".env", file("A=1\n"));
        assert_eq!(rule_for(entry("??", ".devcontainer/.env"), &edited), None);
    }

    const MAIN_ENV: &str = "APP_URL=dev.example.com\nSECRET=x\n";
    const FEATURE_BLOCK: &str = "\n# Feature-specific configuration (managed by branchbox)\n\
        WORK_FEATURE=eta\nAPP_URL='dev-eta.example.com'\nCOMPOSE_PROJECT_NAME=demo-eta\n\
        DEVCONTAINER_NAME=demo-eta\nGIT_BRANCH=feature/eta\n";

    fn env_rule(worktree_env: &str, main_env: Option<&str>) -> Option<GeneratedRule> {
        let mut context = FakeContext::default().worktree(".env", file(worktree_env));
        if let Some(main_env) = main_env {
            context = context.main(".env", file(main_env));
        }
        rule_for(entry("??", ".env"), &context)
    }

    #[test]
    fn r6_an_env_with_only_the_feature_block_is_generated() {
        let generated = format!("{MAIN_ENV}{FEATURE_BLOCK}");
        assert_eq!(
            env_rule(&generated, Some(MAIN_ENV)),
            Some(GeneratedRule::EnvFeatureBlock)
        );
        let with_database = format!(
            "{generated}\n# Database configuration (managed by database module)\nDATABASE_NAME=eta\n"
        );
        assert_eq!(
            env_rule(&with_database, Some(MAIN_ENV)),
            Some(GeneratedRule::EnvFeatureBlock)
        );
        // Main's own (stale) block is ignored when comparing the bases.
        let main_with_block = format!("{MAIN_ENV}{}", FEATURE_BLOCK.replace("=eta", "=old"));
        assert_eq!(
            env_rule(&generated, Some(&main_with_block)),
            Some(GeneratedRule::EnvFeatureBlock)
        );
    }

    #[test]
    fn r6_user_edits_to_the_env_are_user_changes() {
        let user_var = format!("{MAIN_ENV}{FEATURE_BLOCK}MY_TOKEN=abc\n");
        assert_eq!(env_rule(&user_var, Some(MAIN_ENV)), None);
        let user_comment = format!("{MAIN_ENV}{FEATURE_BLOCK}# remember this\n");
        assert_eq!(env_rule(&user_comment, Some(MAIN_ENV)), None);
        let base_edited = format!("APP_URL=other\n{FEATURE_BLOCK}");
        assert_eq!(env_rule(&base_edited, Some(MAIN_ENV)), None);
        assert_eq!(env_rule(MAIN_ENV, Some("OTHER=1\n")), None);
        assert_eq!(env_rule(&format!("{MAIN_ENV}{FEATURE_BLOCK}"), None), None);
        let context = FakeContext::default()
            .worktree(".env", FileProbe::File(vec![0xff, 0xfe]))
            .main(".env", file(MAIN_ENV));
        assert_eq!(rule_for(entry("??", ".env"), &context), None);
    }

    const MANAGED_SETTINGS: &str = r##"{
        "peacock.color": "#e67e22",
        "peacock.remoteColor": "#e67e22",
        "window.title": "${rootName} [eta]",
        "workbench.colorCustomizations": {"statusBar.background": "#e67e22"}
    }"##;

    #[test]
    fn r7_untracked_settings_with_only_managed_keys_are_generated() {
        let context =
            FakeContext::default().worktree(".vscode/settings.json", file(MANAGED_SETTINGS));
        assert_eq!(
            rule_for(entry("??", ".vscode/settings.json"), &context),
            Some(GeneratedRule::VscodeManagedKeys)
        );
        let with_user_key = FakeContext::default().worktree(
            ".vscode/settings.json",
            file(r#"{"window.title": "x", "editor.formatOnSave": true}"#),
        );
        assert_eq!(
            rule_for(entry("??", ".vscode/settings.json"), &with_user_key),
            None
        );
        let not_json = FakeContext::default().worktree(".vscode/settings.json", file("not json"));
        assert_eq!(
            rule_for(entry("??", ".vscode/settings.json"), &not_json),
            None
        );
        let not_object = FakeContext::default().worktree(".vscode/settings.json", file("[]"));
        assert_eq!(
            rule_for(entry("??", ".vscode/settings.json"), &not_object),
            None
        );
    }

    #[test]
    fn r7_tracked_settings_compare_with_the_committed_file() {
        let committed =
            "{\n  // team settings\n  \"editor.tabSize\": 2,\n  \"peacock.color\": \"#000\"\n}";
        let started = r##"{"editor.tabSize": 2, "peacock.color": "#e67e22", "window.title": "t"}"##;
        let context = FakeContext::default()
            .worktree(".vscode/settings.json", file(started))
            .head(".vscode/settings.json", committed);
        assert_eq!(
            rule_for(entry(" M", ".vscode/settings.json"), &context),
            Some(GeneratedRule::VscodeManagedKeys)
        );

        let edited = r##"{"editor.tabSize": 2, "editor.formatOnSave": true, "window.title": "t"}"##;
        let context = FakeContext::default()
            .worktree(".vscode/settings.json", file(edited))
            .head(".vscode/settings.json", committed);
        assert_eq!(
            rule_for(entry(" M", ".vscode/settings.json"), &context),
            None
        );

        let no_head = FakeContext::default().worktree(".vscode/settings.json", file(started));
        assert_eq!(
            rule_for(entry(" M", ".vscode/settings.json"), &no_head),
            None
        );
    }

    #[test]
    fn r7_tasks_holding_only_the_feature_url_task_are_generated() {
        let only_ours = r#"{"version": "2.0.0", "tasks": [{"label": "Open Feature URL"}]}"#;
        let context = FakeContext::default().worktree(".vscode/tasks.json", file(only_ours));
        assert_eq!(
            rule_for(entry("??", ".vscode/tasks.json"), &context),
            Some(GeneratedRule::VscodeManagedTasks)
        );
        for user_tasks in [
            r#"{"tasks": [{"label": "Open Feature URL"}, {"label": "Build"}]}"#,
            r#"{"tasks": [{"label": "Build"}]}"#,
            r#"{"tasks": [{"command": "x"}]}"#,
            r#"{"tasks": "Open Feature URL"}"#,
            r#"[]"#,
        ] {
            let context = FakeContext::default().worktree(".vscode/tasks.json", file(user_tasks));
            assert_eq!(
                rule_for(entry("??", ".vscode/tasks.json"), &context),
                None,
                "{user_tasks}"
            );
        }
        let unreadable =
            FakeContext::default().worktree(".vscode/tasks.json", FileProbe::Unreadable);
        assert_eq!(
            rule_for(entry("??", ".vscode/tasks.json"), &unreadable),
            None
        );
    }

    #[test]
    fn staged_conflicted_renamed_and_unsafe_entries_are_always_user_changes() {
        let context = FakeContext::default()
            .worktree("same.txt", file("x"))
            .main("same.txt", file("x"));
        assert_eq!(
            rule_for(entry("??", "same.txt"), &context),
            Some(GeneratedRule::DerivedFromMain)
        );
        assert_eq!(rule_for(entry("M ", "same.txt"), &context), None);
        assert_eq!(rule_for(entry("AM", "same.txt"), &context), None);
        assert_eq!(rule_for(entry("UU", "same.txt"), &context), None);
        let mut renamed = entry(" M", "same.txt");
        renamed.original_path = Some("old.txt".to_string());
        assert_eq!(rule_for(renamed, &context), None);
        let traversal = FakeContext::default()
            .worktree("../main/x", file("x"))
            .main("../main/x", file("x"));
        assert_eq!(rule_for(entry("??", "../main/x"), &traversal), None);
    }

    #[test]
    fn user_changes_carry_kind_area_and_pure_deletions() {
        let entries = [
            entry(" M", "README.md"),
            entry("??", "notes.txt"),
            entry(" D", "tmp/.keep"),
            entry("??", ".devcontainer/local.json"),
        ];
        let classification =
            classify_changes(&entries, &FakeContext::default(), "eta", false, None);
        assert_eq!(
            classification.changes.user,
            [
                UserChange {
                    path: "README.md".to_string(),
                    kind: ChangeKind::Modified,
                    area: ChangeArea::Other
                },
                UserChange {
                    path: "notes.txt".to_string(),
                    kind: ChangeKind::Untracked,
                    area: ChangeArea::Other
                },
                UserChange {
                    path: "tmp/.keep".to_string(),
                    kind: ChangeKind::Deleted,
                    area: ChangeArea::Other
                },
                UserChange {
                    path: ".devcontainer/local.json".to_string(),
                    kind: ChangeKind::Untracked,
                    area: ChangeArea::Devcontainer
                },
            ]
        );
        assert!(!classification.changes.truncated);
        let content: Vec<&str> = classification
            .content_changes()
            .map(|change| change.path.as_str())
            .collect();
        assert_eq!(
            content,
            ["README.md", "notes.txt", ".devcontainer/local.json"]
        );
    }

    #[test]
    fn classification_stops_listing_after_the_entry_cap() {
        let entries: Vec<StatusEntry> = (0..MAX_CLASSIFIED_ENTRIES + 5)
            .map(|index| entry("??", &format!("file-{index}.txt")))
            .collect();
        let classification =
            classify_changes(&entries, &FakeContext::default(), "eta", false, None);
        assert!(classification.changes.truncated);
        assert_eq!(classification.changes.user.len(), MAX_CLASSIFIED_ENTRIES);
        assert_eq!(classification.unlisted_user_changes, 5);
    }

    // ---- reading files -----------------------------------------------------------------

    #[test]
    fn probe_reads_regular_files_and_never_follows_symlinks() {
        let root = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        fs::write(root.path().join("plain.txt"), "hello").unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        fs::write(outside.path().join("secret.txt"), "outside").unwrap();

        assert_eq!(probe_file(root.path(), "plain.txt"), file("hello"));
        assert_eq!(probe_file(root.path(), "missing.txt"), FileProbe::Missing);
        assert_eq!(
            probe_file(root.path(), "dir/missing.txt"),
            FileProbe::Missing
        );
        assert_eq!(probe_file(root.path(), "dir"), FileProbe::Unreadable);
        assert_eq!(
            probe_file(root.path(), "plain.txt/x"),
            FileProbe::Unreadable
        );
        assert_eq!(probe_file(root.path(), "../x"), FileProbe::Unreadable);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(outside.path().join("secret.txt"), root.path().join("link")).unwrap();
            symlink(outside.path(), root.path().join("linked-dir")).unwrap();
            assert_eq!(
                probe_file(root.path(), "link"),
                FileProbe::Symlink(outside.path().join("secret.txt"))
            );
            assert_eq!(
                probe_file(root.path(), "linked-dir/secret.txt"),
                FileProbe::Unreadable,
                "a path through a symlinked directory is not read"
            );
        }
    }

    #[test]
    fn probe_skips_files_over_the_size_cap() {
        let root = tempfile::TempDir::new().unwrap();
        let big = fs::File::create(root.path().join("big.bin")).unwrap();
        big.set_len(MAX_CLASSIFIED_FILE_BYTES + 1).unwrap();
        assert_eq!(probe_file(root.path(), "big.bin"), FileProbe::Unreadable);
        let exact = fs::File::create(root.path().join("exact.bin")).unwrap();
        exact.set_len(MAX_CLASSIFIED_FILE_BYTES).unwrap();
        assert!(matches!(
            probe_file(root.path(), "exact.bin"),
            FileProbe::File(bytes) if bytes.len() as u64 == MAX_CLASSIFIED_FILE_BYTES
        ));
    }

    #[test]
    fn fs_context_reads_head_and_the_baseline() {
        let repo = tempfile::TempDir::new().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args([
                    "-c",
                    "user.email=t@example.com",
                    "-c",
                    "user.name=T",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(repo.path())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        fs::create_dir(repo.path().join(".vscode")).unwrap();
        fs::write(repo.path().join(".vscode/settings.json"), "{\"a\": 1}").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        let mut baseline = DevcontainerBaseline::new();
        baseline.insert("devcontainer.json".to_string(), "00ff".to_string());
        let context = FsChangeContext::new(repo.path(), repo.path(), Some(baseline));
        assert_eq!(
            context.head_file(".vscode/settings.json"),
            Some(b"{\"a\": 1}".to_vec())
        );
        assert_eq!(context.head_file("missing.json"), None);
        assert_eq!(context.head_file("../escape"), None);
        assert_eq!(
            context.baseline_digest("devcontainer.json").as_deref(),
            Some("00ff")
        );
        assert_eq!(context.baseline_digest("other.json"), None);
        assert_eq!(
            context.worktree_file(".vscode/settings.json"),
            context.main_file(".vscode/settings.json")
        );
        let no_baseline = FsChangeContext::new(repo.path(), repo.path(), None);
        assert_eq!(no_baseline.baseline_digest("devcontainer.json"), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_baseline_requires_the_recorded_exact_main_target() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::TempDir::new().unwrap();
        let main = temp.path().join("main");
        let worktree = temp.path().join("eta");
        let path = ".devcontainer/devcontainer.json";
        fs::create_dir_all(main.join(".devcontainer")).unwrap();
        fs::create_dir_all(worktree.join(".devcontainer")).unwrap();
        fs::write(main.join(path), "main config").unwrap();
        let owned_target = PathBuf::from("../../main/.devcontainer/devcontainer.json");
        symlink(&owned_target, worktree.join(path)).unwrap();
        let mut baseline = DevcontainerBaseline::new();
        baseline.insert(
            "devcontainer.json".to_string(),
            baseline_symlink_digest(&owned_target),
        );
        let classify = |baseline| {
            classify_changes(
                &[entry(" T", path)],
                &FsChangeContext::new(&worktree, &main, baseline),
                "eta",
                false,
                None,
            )
            .changes
        };
        let clean = classify(Some(baseline.clone()));
        assert!(clean.user.is_empty());
        assert_eq!(clean.generated[0].rule, GeneratedRule::DevcontainerBaseline);
        assert_eq!(
            classify(None).user[0].path,
            path,
            "a link needs recorded ownership"
        );

        fs::remove_file(worktree.join(path)).unwrap();
        let outside = temp.path().join("user-config.json");
        fs::write(&outside, "user config").unwrap();
        symlink(&outside, worktree.join(path)).unwrap();
        assert_eq!(classify(Some(baseline.clone())).user[0].path, path);
        // Even a baseline made from an arbitrary link cannot vouch for a non-main target.
        baseline.insert(
            "devcontainer.json".to_string(),
            baseline_symlink_digest(&outside),
        );
        assert_eq!(classify(Some(baseline)).user[0].path, path);
        assert_eq!(fs::read_to_string(&outside).unwrap(), "user config");
    }

    // ---- plan decisions ----------------------------------------------------------------

    fn request() -> TeardownRequest {
        TeardownRequest {
            work_feature: "eta".to_string(),
            branch_prefix: None,
            delete_branch: true,
            force_delete_branch: false,
            force_remove: false,
            force_remove_modules: false,
            complete_spec: false,
            telemetry: false,
        }
    }

    fn strict() -> TeardownOptions {
        TeardownOptions {
            discard_changes: false,
            require_mergeable_branch: true,
        }
    }

    fn worktree_state() -> WorktreeState {
        WorktreeState {
            path: PathBuf::from("/r/eta"),
            exists: true,
            locked: false,
            lock_reason: None,
        }
    }

    fn merged() -> BranchMergeState {
        BranchMergeState {
            exists: true,
            upstream: None,
            reference: "HEAD".to_string(),
            reference_name: "main".to_string(),
            merged: true,
            merged_into_head: true,
            ahead: 0,
        }
    }

    fn unmerged(ahead: u32) -> BranchMergeState {
        BranchMergeState {
            merged: false,
            merged_into_head: false,
            ahead,
            ..merged()
        }
    }

    fn classified(entries: &[StatusEntry]) -> WorktreeStatus {
        WorktreeStatus::Classified(classify_changes(
            entries,
            &FakeContext::default(),
            "eta",
            false,
            None,
        ))
    }

    fn inputs<'a>(
        request: &'a TeardownRequest,
        options: TeardownOptions,
        status: WorktreeStatus,
        merge_state: Option<BranchMergeState>,
    ) -> PlanInputs<'a> {
        PlanInputs {
            request,
            options,
            recorded: None,
            worktree: worktree_state(),
            status,
            branch: ResolvedBranch {
                name: "feature/eta".to_string(),
                source: BranchSource::Registry,
            },
            merge_state,
            defaults: TeardownDefaults {
                delete_branch_by_default: true,
                force_delete_unmerged_by_default: false,
            },
            warnings: Vec::new(),
        }
    }

    fn blocker_kinds(plan: &TeardownPlan) -> Vec<&'static str> {
        plan.blockers
            .iter()
            .map(|blocker| match blocker {
                Blocker::UncommittedChanges { .. } => "uncommitted_changes",
                Blocker::UnmergedBranch { .. } => "unmerged_branch",
                Blocker::WorktreeLocked { .. } => "worktree_locked",
                Blocker::StatusUnavailable { .. } => "status_unavailable",
                Blocker::WorktreeRemovalFailed { .. } => "worktree_removal_failed",
                Blocker::RuntimeCleanupFailed { .. } => "runtime_cleanup_failed",
                Blocker::NotAWorktree { .. } => "not_a_worktree",
                Blocker::SpecNotPreserved { .. } => "spec_not_preserved",
            })
            .collect()
    }

    #[test]
    fn a_clean_merged_feature_has_no_blockers() {
        let request = request();
        let plan = build_teardown_plan(inputs(&request, strict(), classified(&[]), Some(merged())));
        assert!(!plan.is_blocked());
        assert_eq!(plan.schema_version, PLAN_SCHEMA_VERSION);
        assert!(!plan.registered);
        assert_eq!(plan.runtime, None);
        let branch = plan.branch.unwrap();
        assert_eq!(branch.action, BranchAction::Delete);
        assert_eq!(branch.source, BranchSource::Registry);
        assert!(branch.merged);
    }

    #[test]
    fn user_changes_block_unless_discarded_or_forced() {
        let entries = [entry(" M", "README.md"), entry("??", "notes.txt")];
        let request = request();
        let plan = build_teardown_plan(inputs(
            &request,
            strict(),
            classified(&entries),
            Some(merged()),
        ));
        assert_eq!(blocker_kinds(&plan), ["uncommitted_changes"]);
        assert!(plan.blocks_on_uncommitted_changes());
        let Blocker::UncommittedChanges {
            count,
            message,
            override_hint,
        } = &plan.blockers[0]
        else {
            unreachable!()
        };
        assert_eq!(*count, 2);
        assert_eq!(override_hint, "--discard-changes");
        assert!(
            message.contains("README.md (modified), notes.txt (untracked)"),
            "{message}"
        );
        assert!(message.contains("--discard-changes"), "{message}");

        let discard = TeardownOptions {
            discard_changes: true,
            ..strict()
        };
        let plan = build_teardown_plan(inputs(
            &request,
            discard,
            classified(&entries),
            Some(merged()),
        ));
        assert!(!plan.is_blocked());
        assert_eq!(
            plan.changes.user.len(),
            2,
            "the plan still lists what is discarded"
        );

        let forced = TeardownRequest {
            force_remove: true,
            ..request.clone()
        };
        let plan = build_teardown_plan(inputs(
            &forced,
            strict(),
            classified(&entries),
            Some(merged()),
        ));
        assert!(!plan.is_blocked());
    }

    #[test]
    fn legacy_module_override_discards_only_module_files() {
        let modules_only = TeardownRequest {
            force_remove_modules: true,
            ..request()
        };
        let module_change = [entry("??", ".devcontainer/local.json")];
        let plan = build_teardown_plan(inputs(
            &modules_only,
            strict(),
            classified(&module_change),
            Some(merged()),
        ));
        assert!(!plan.is_blocked());

        let mixed = [
            entry("??", ".devcontainer/local.json"),
            entry("??", "notes.txt"),
        ];
        let plan = build_teardown_plan(inputs(
            &modules_only,
            strict(),
            classified(&mixed),
            Some(merged()),
        ));
        let Blocker::UncommittedChanges { count, message, .. } = &plan.blockers[0] else {
            unreachable!()
        };
        assert_eq!(*count, 1);
        assert!(
            message.contains("notes.txt") && !message.contains("local.json"),
            "{message}"
        );
        assert!(plan.has_module_area_changes());
    }

    #[test]
    fn unlisted_changes_block_and_warn() {
        let entries: Vec<StatusEntry> = (0..MAX_CLASSIFIED_ENTRIES + 3)
            .map(|index| entry("??", &format!("f{index}")))
            .collect();
        let request = request();
        let plan = build_teardown_plan(inputs(
            &request,
            strict(),
            classified(&entries),
            Some(merged()),
        ));
        let Blocker::UncommittedChanges { count, message, .. } = &plan.blockers[0] else {
            unreachable!()
        };
        assert_eq!(*count, MAX_CLASSIFIED_ENTRIES + 3);
        assert!(
            message.contains(&format!(
                "and {} more",
                MAX_CLASSIFIED_ENTRIES + 3 - REFUSAL_LISTED_FILES
            )),
            "{message}"
        );
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("3 more")),
            "{:?}",
            plan.warnings
        );

        let discard = TeardownOptions {
            discard_changes: true,
            ..strict()
        };
        let plan = build_teardown_plan(inputs(
            &request,
            discard,
            classified(&entries),
            Some(merged()),
        ));
        assert!(!plan.is_blocked());
        assert!(plan.changes.truncated);
    }

    #[test]
    fn an_unmerged_delete_blocks_only_when_the_preflight_is_required() {
        let request = request();
        let plan = build_teardown_plan(inputs(
            &request,
            strict(),
            classified(&[]),
            Some(unmerged(3)),
        ));
        assert_eq!(blocker_kinds(&plan), ["unmerged_branch"]);
        assert_eq!(plan.unmerged_branch(), Some(("feature/eta", 3)));
        let Blocker::UnmergedBranch {
            message,
            override_hint,
            ..
        } = &plan.blockers[0]
        else {
            unreachable!()
        };
        assert_eq!(override_hint, "--keep-branch | --force-delete-branch");
        assert!(
            message.contains("3 commits not merged into main"),
            "{message}"
        );

        let legacy = TeardownOptions::legacy(&request);
        let plan =
            build_teardown_plan(inputs(&request, legacy, classified(&[]), Some(unmerged(1))));
        assert!(!plan.is_blocked());

        for (variant, action) in [
            (
                TeardownRequest {
                    delete_branch: false,
                    ..request.clone()
                },
                BranchAction::Keep,
            ),
            (
                TeardownRequest {
                    force_delete_branch: true,
                    ..request.clone()
                },
                BranchAction::ForceDelete,
            ),
            (
                TeardownRequest {
                    force_remove: true,
                    ..request.clone()
                },
                BranchAction::ForceDelete,
            ),
        ] {
            let plan = build_teardown_plan(inputs(
                &variant,
                strict(),
                classified(&[]),
                Some(unmerged(1)),
            ));
            assert!(!plan.is_blocked(), "{action:?}");
            assert_eq!(plan.branch.unwrap().action, action);
        }
    }

    #[test]
    fn a_missing_branch_is_planned_with_a_warning() {
        let request = request();
        let missing = BranchMergeState {
            exists: false,
            merged: false,
            ..merged()
        };
        let plan = build_teardown_plan(inputs(&request, strict(), classified(&[]), Some(missing)));
        assert!(!plan.is_blocked());
        assert!(!plan.branch.as_ref().unwrap().exists);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("does not exist")));

        let plan = build_teardown_plan(inputs(&request, strict(), classified(&[]), None));
        assert_eq!(
            plan.branch, None,
            "an unreadable merge state is reported as no branch plan"
        );
        assert!(!plan.is_blocked());
    }

    #[test]
    fn locked_and_unreadable_worktrees_block_unless_forced() {
        let request = request();
        let mut locked = inputs(&request, strict(), classified(&[]), Some(merged()));
        locked.worktree.locked = true;
        locked.worktree.lock_reason = Some("on usb".to_string());
        let plan = build_teardown_plan(locked.clone());
        assert_eq!(blocker_kinds(&plan), ["worktree_locked"]);
        assert!(plan.blockers[0].message().contains("(reason: on usb)"));
        assert!(plan.blockers[0]
            .message()
            .contains("git worktree unlock /r/eta"));

        let status = WorktreeStatus::Unavailable("fatal: not a git repository".to_string());
        let plan = build_teardown_plan(inputs(&request, strict(), status.clone(), Some(merged())));
        assert_eq!(blocker_kinds(&plan), ["status_unavailable"]);
        assert!(!plan.changes.status_available);
        assert!(plan
            .refusal_message()
            .contains("fatal: not a git repository"));

        let forced = TeardownRequest {
            force_remove: true,
            ..request.clone()
        };
        let mut locked_forced = inputs(&forced, strict(), status, Some(merged()));
        locked_forced.worktree.locked = true;
        let plan = build_teardown_plan(locked_forced);
        assert!(!plan.is_blocked());
    }

    #[test]
    fn a_missing_worktree_has_nothing_to_lose() {
        let request = request();
        let mut missing = inputs(&request, strict(), WorktreeStatus::Missing, Some(merged()));
        missing.worktree.exists = false;
        let plan = build_teardown_plan(missing);
        assert!(!plan.is_blocked());
        assert!(plan.changes.status_available);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("needs --force")));

        let forced = TeardownRequest {
            force_remove: true,
            ..request.clone()
        };
        let mut missing = inputs(&forced, strict(), WorktreeStatus::Missing, Some(merged()));
        missing.worktree.exists = false;
        assert!(build_teardown_plan(missing).warnings.is_empty());
    }

    #[test]
    fn registry_facts_flow_into_the_plan() {
        let metadata: FeatureMetadata = serde_json::from_value(json!({
            "work_feature": "eta", "branch_name": "spike/eta", "worktree_path": "/r/eta",
            "status": "active", "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "runtime": {"provider": "sbx", "runtime_id": "sbx-1"},
            "tunnel": {"provider": "cloudflared", "status": "active",
                       "last_updated": "2026-01-01T00:00:00Z"}
        }))
        .unwrap();
        let request = request();
        let mut with_entry = inputs(&request, strict(), classified(&[]), Some(merged()));
        with_entry.recorded = Some(&metadata);
        let plan = build_teardown_plan(with_entry);
        assert!(plan.registered);
        assert_eq!(plan.status, Some(FeatureStatus::Active));
        assert_eq!(
            plan.runtime,
            Some(RuntimeRef {
                provider: Some("sbx".to_string()),
                runtime_id: Some("sbx-1".to_string())
            })
        );
        assert_eq!(
            plan.tunnel,
            Some(TunnelRef {
                status: Some(FeatureTunnelStatus::Active)
            })
        );
    }

    // ---- refusal messages --------------------------------------------------------------

    #[test]
    fn refusal_names_at_most_ten_files_and_every_fix() {
        let changes: Vec<UserChange> = (0..12)
            .map(|index| UserChange {
                path: format!("file-{index:02}.txt"),
                kind: ChangeKind::Untracked,
                area: ChangeArea::Other,
            })
            .collect();
        let listed: Vec<&UserChange> = changes.iter().collect();
        let blocker = Blocker::uncommitted_changes(Path::new("/r/eta"), &listed, 0);
        let message = blocker.message();
        assert!(
            message.starts_with("12 uncommitted changes in /r/eta would be lost: "),
            "{message}"
        );
        assert!(
            message.contains("file-09.txt (untracked), and 2 more."),
            "{message}"
        );
        assert!(!message.contains("file-10.txt"), "{message}");

        let one = Blocker::uncommitted_changes(Path::new("/r/eta"), &listed[..1], 0);
        assert!(one.message().starts_with("1 uncommitted change in"));
        assert!(one.message().contains("discard it."), "{}", one.message());
        assert!(Blocker::unmerged_branch("b", 1, "main")
            .message()
            .contains("1 commit not merged"));
        let unlocked = Blocker::worktree_locked(Path::new("/r/eta"), None);
        assert!(
            unlocked.message().contains("is locked; unlock"),
            "{}",
            unlocked.message()
        );
        let removal = Blocker::worktree_removal_failed(Path::new("/r/eta"), "busy".to_string());
        assert!(removal.message().contains("busy") && removal.message().contains("--force"));
        let main = Blocker::not_a_worktree(
            Path::new("/r/main"),
            "it is the repository's main worktree".to_string(),
        );
        assert!(
            main.message().contains("main worktree") && main.message().contains("never removes"),
            "{}",
            main.message()
        );
        let spec = Blocker::spec_not_preserved(
            Path::new("/r/eta"),
            "docs/features/in-progress/eta.md",
            "Permission denied".to_string(),
        );
        assert!(spec.message().contains("docs/features/in-progress/eta.md"));
        assert!(spec.message().contains("Permission denied") && spec.message().contains("--force"));
    }

    #[test]
    fn refusals_carry_the_plan_and_say_whether_anything_changed() {
        let request = request();
        let entries = [entry("??", "notes.txt")];
        let plan = build_teardown_plan(inputs(
            &request,
            strict(),
            classified(&entries),
            Some(unmerged(2)),
        ));
        let message = plan.refusal_message();
        assert!(
            message.starts_with("Refusing to tear down 'eta'; nothing was removed. 1 uncommitted"),
            "{message}"
        );
        assert!(message.contains("--keep-branch") && message.contains("--force-delete-branch"));

        match plan.clone().into_refusal() {
            Error::TeardownRefused {
                work_feature,
                message: refusal,
                plan: carried,
                changed_anything,
                completed_steps,
            } => {
                assert_eq!(work_feature, "eta");
                assert_eq!(refusal, message);
                assert_eq!(*carried, plan);
                assert!(!changed_anything);
                assert!(completed_steps.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }

        let steps = vec!["Stopped the container runtime".to_string()];
        match plan.into_stopped(steps.clone()) {
            Error::TeardownRefused {
                message,
                changed_anything,
                completed_steps,
                ..
            } => {
                assert!(
                    message.starts_with("Stopped tearing down 'eta' before removing its worktree"),
                    "{message}"
                );
                assert!(
                    message.contains("Already done: Stopped the container runtime."),
                    "{message}"
                );
                assert!(changed_anything);
                assert_eq!(completed_steps, steps);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn the_spec_destination_follows_complete_spec() {
        assert_eq!(
            spec_destination("eta", false),
            "docs/features/backlog/eta.md"
        );
        assert_eq!(
            spec_destination("eta", true),
            "docs/features/completed/eta.md"
        );
    }
}
