//! Cloudflare API credentials for tunnel provisioning (`tunnel credentials set`, DESIGN §5.11).
//!
//! The API token and account ID live in `<repo>/.branchbox/secure/cloudflared.env`, an env-style
//! file the Cloudflared tunnel provider reads. It is written atomically and is owner-only from
//! the moment it exists (0600, in a 0700 directory); lines BranchBox does not manage are kept.
//! [`set_cloudflare_credentials`] then points `tunnel.providers.cloudflared` in
//! `.branchbox/config.json` at the file, editing the config in place so its formatting and any
//! keys BranchBox does not know survive.
//!
//! The token is a secret: no error, log line or summary ever contains it. Refusals name what was
//! wrong with it, never its value.
//!
//! The config change goes through the config engine ([`crate::config_edit`]), so it gets the same
//! key validation, comment refusal and format preservation as `branchbox config`.

use crate::atomic_fs::{self, LOCK_TIMEOUT};
use crate::config_edit::{self, Edit};
use crate::env_placeholders::looks_like_env_placeholder;
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `schema_version` of the `tunnel credentials set --json` payload.
pub const SCHEMA_VERSION: u32 = 1;

/// Where the credentials live, relative to the repository root. This is also the value recorded
/// as `tunnel.providers.cloudflared.api_token_path`.
pub const CREDENTIALS_RELATIVE_PATH: &str = ".branchbox/secure/cloudflared.env";

const TOKEN_KEY: &str = "CLOUDFLARE_API_TOKEN";
const ACCOUNT_KEY: &str = "CLOUDFLARE_ACCOUNT_ID";

/// Longest token accepted. Cloudflare API tokens are 40 characters; anything near this limit is
/// a paste mistake, not a token.
const MAX_TOKEN_LEN: usize = 4096;

/// What to do with the stored API token.
#[derive(Clone, Copy)]
pub enum TokenChange<'a> {
    /// Store this token (surrounding whitespace is ignored).
    Set(&'a str),
    /// Remove the stored token; tunnels fall back to manual instructions.
    Clear,
}

// The token must never reach a log or an error report, even through `{:?}`.
impl std::fmt::Debug for TokenChange<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenChange::Set(_) => f.write_str("Set(<redacted>)"),
            TokenChange::Clear => f.write_str("Clear"),
        }
    }
}

/// The `tunnel credentials set --json` payload (DESIGN §5.11). Never carries the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialsSummary {
    pub schema_version: u32,
    /// Absolute path of the credentials file.
    pub credentials_path: PathBuf,
    /// The account ID the file now holds, if any.
    pub account_id: Option<String>,
    /// Whether the file now holds a non-empty API token.
    pub token_present: bool,
}

/// Store (or clear) the Cloudflare API token and account ID for the repository at `repo_root`
/// and point the project's tunnel configuration at them.
///
/// Storing sets `tunnel.providers.cloudflared.{api_token_path, manual_instructions: false}`
/// (plus `account_id` when given). Clearing removes `api_token_path` and sets
/// `manual_instructions: true`, so tunnels fall back to manual setup instructions.
///
/// Everything is validated before anything is written: an empty or malformed token, an invalid
/// account ID, or a `config.json` that is not strict JSON (comments) or does not load leaves
/// both files untouched. Both writes happen under the `.branchbox` lock.
pub fn set_cloudflare_credentials(
    repo_root: &Path,
    account_id: Option<&str>,
    token: TokenChange<'_>,
) -> Result<CredentialsSummary> {
    let account_id = account_id.map(validate_account_id).transpose()?;
    let token = match token {
        TokenChange::Set(token) => TokenChange::Set(validate_api_token(token)?),
        TokenChange::Clear => TokenChange::Clear,
    };

    let _lock = atomic_fs::lock_state_dir(&repo_root.join(".branchbox"), LOCK_TIMEOUT)?;

    // Plan the config change first, so a config.json that cannot be edited changes nothing.
    let planned = config_edit::plan(repo_root, |raw| cloudflared_edits(raw, account_id, token))?;

    let credentials_path = repo_root.join(CREDENTIALS_RELATIVE_PATH);
    let stored = write_cloudflare_env(&credentials_path, account_id, token)?;
    planned.commit()?;

    Ok(CredentialsSummary {
        schema_version: SCHEMA_VERSION,
        credentials_path,
        account_id: stored.account_id,
        token_present: stored.token_present,
    })
}

/// What a credentials file holds after a write (never the token itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredCredentials {
    pub account_id: Option<String>,
    pub token_present: bool,
}

/// Write the managed lines of the env file at `path`: `CLOUDFLARE_API_TOKEN` per `token` and,
/// when given, `CLOUDFLARE_ACCOUNT_ID`. Every other line is kept as it was. The file is replaced
/// atomically at 0600 (an existing wider file is narrowed first), in a 0700 directory created or
/// tightened as needed. Clearing a file that does not exist creates nothing.
///
/// Callers that may race other BranchBox writers hold the `.branchbox` lock.
pub(crate) fn write_cloudflare_env(
    path: &Path,
    account_id: Option<&str>,
    token: TokenChange<'_>,
) -> Result<StoredCredentials> {
    let account_id = account_id.map(validate_account_id).transpose()?;
    let token = match token {
        TokenChange::Set(token) => Some(validate_api_token(token)?),
        TokenChange::Clear => None,
    };

    let dir = path.parent().ok_or_else(|| {
        Error::validation(format!(
            "Credentials path {} has no parent directory",
            path.display()
        ))
    })?;
    let dir_exists = inspect_private_dir(dir)?;
    let existing = if dir_exists {
        read_optional(path)?
    } else {
        None
    };
    if existing.is_none() && token.is_none() && account_id.is_none() {
        return Ok(StoredCredentials {
            account_id: None,
            token_present: false,
        });
    }

    let mut updates = vec![(TOKEN_KEY, token)];
    if let Some(account_id) = account_id {
        updates.push((ACCOUNT_KEY, Some(account_id)));
    }
    let rendered = render_env(existing.as_deref().unwrap_or_default(), &updates);

    if !dir_exists {
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent).map_err(|err| io_error_at("create", parent, err))?;
        }
        create_private_dir(dir).map_err(|err| io_error_at("create", dir, err))?;
    }
    restrict_mode(dir, 0o700)?;
    narrow_existing_file(path)?;
    atomic_fs::write_atomic(path, rendered.as_bytes(), 0o600)?;

    Ok(StoredCredentials {
        account_id: env_value(&rendered, ACCOUNT_KEY),
        token_present: env_value(&rendered, TOKEN_KEY).is_some(),
    })
}

/// The token, trimmed, or a refusal naming what is wrong with it (never the token).
fn validate_api_token(token: &str) -> Result<&str> {
    let token = token.trim();
    if token.is_empty() {
        return Err(Error::validation(
            "Refusing to store an empty Cloudflare API token: the input was empty or only \
             whitespace. Nothing was changed.",
        ));
    }
    if token
        .chars()
        .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        // cause-withheld: the token is a credential, so only the rule it broke is named.
        return Err(Error::validation(
            "Refusing to store the Cloudflare API token: it contains spaces, line breaks or \
             other control characters, and a token is a single word. Nothing was changed.",
        ));
    }
    if token.len() > MAX_TOKEN_LEN {
        return Err(Error::validation(format!(
            "Refusing to store the Cloudflare API token: it is {} bytes long, more than the \
             {MAX_TOKEN_LEN} accepted (Cloudflare API tokens are 40 characters). Nothing was \
             changed.",
            token.len()
        )));
    }
    Ok(token)
}

/// The account ID, trimmed, or a refusal naming the value and the expected form.
fn validate_account_id(account_id: &str) -> Result<&str> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err(Error::validation(
            "Cloudflare account ID is empty; pass the account ID shown in the Cloudflare \
             dashboard. Nothing was changed.",
        ));
    }
    if looks_like_env_placeholder(account_id) {
        return Err(Error::validation(format!(
            "Cloudflare account ID '{account_id}' is an environment placeholder; pass the \
             literal account ID. Nothing was changed."
        )));
    }
    if !account_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(Error::validation(format!(
            "Cloudflare account ID '{account_id}' is invalid: expected letters, digits, '-' or \
             '_' only (the 32-character ID from the Cloudflare dashboard). Nothing was changed."
        )));
    }
    Ok(account_id)
}

/// Apply `updates` to env-file text: a `Some` value replaces the first `KEY=` line (dropping
/// any later duplicates, which readers would ignore) or, when the key is absent, is added at
/// the top so it takes precedence over older credentials such as `CLOUDFLARE_API_KEY`; `None`
/// removes every `KEY=` line. Other lines keep their text and order.
fn render_env(existing: &str, updates: &[(&str, Option<&str>)]) -> String {
    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    let mut prepend = Vec::new();
    for (key, value) in updates {
        let prefix = format!("{key}=");
        let mut seen = false;
        lines.retain_mut(|line| {
            if !line.starts_with(&prefix) {
                return true;
            }
            match value {
                Some(value) if !seen => {
                    seen = true;
                    *line = format!("{prefix}{value}");
                    true
                }
                _ => false,
            }
        });
        if let (Some(value), false) = (value, seen) {
            prepend.push(format!("{prefix}{value}"));
        }
    }
    prepend.extend(lines);

    let mut rendered = prepend.join("\n");
    if !rendered.is_empty() {
        rendered.push('\n');
    }
    rendered
}

/// The first non-empty value of `key` in env-file text, unquoted, as the tunnel provider reads it.
fn env_value(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    content
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .map(|value| value.trim().trim_matches(['"', '\'']).trim())
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// The config edits for a credentials change: point `tunnel.providers.cloudflared` at the stored
/// token (or back at manual instructions when clearing), plus `account_id` when given. Clearing a
/// project whose config has no Cloudflared section, without an account ID, changes nothing.
fn cloudflared_edits(
    raw: &Map<String, Value>,
    account_id: Option<&str>,
    token: TokenChange<'_>,
) -> Result<Vec<Edit>> {
    let has_cloudflared = raw
        .get("tunnel")
        .and_then(|tunnel| tunnel.get("providers"))
        .and_then(|providers| providers.get("cloudflared"))
        .is_some_and(Value::is_object);
    if matches!(token, TokenChange::Clear) && !has_cloudflared && account_id.is_none() {
        return Ok(Vec::new());
    }

    let mut edits = Vec::new();
    if let Some(account_id) = account_id {
        edits.push(Edit::set(
            "tunnel.providers.cloudflared.account_id",
            Value::from(account_id),
        )?);
    }
    match token {
        TokenChange::Set(_) => {
            edits.push(Edit::set(
                "tunnel.providers.cloudflared.api_token_path",
                Value::from(CREDENTIALS_RELATIVE_PATH),
            )?);
            edits.push(Edit::set(
                "tunnel.providers.cloudflared.manual_instructions",
                Value::Bool(false),
            )?);
        }
        TokenChange::Clear => {
            edits.push(Edit::unset("tunnel.providers.cloudflared.api_token_path")?);
            edits.push(Edit::set(
                "tunnel.providers.cloudflared.manual_instructions",
                Value::Bool(true),
            )?);
        }
    }
    Ok(edits)
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

/// Whether the credentials directory `dir` exists. Refuses a symlink (so the secret cannot be
/// redirected elsewhere) and anything that is not a directory.
fn inspect_private_dir(dir: &Path) -> Result<bool> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::validation(format!(
            "Refusing to write credentials through symlinked directory {}",
            dir.display()
        ))),
        Ok(metadata) if !metadata.is_dir() => Err(Error::validation(format!(
            "Cannot store credentials in {}: it exists and is not a directory",
            dir.display()
        ))),
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(io_error_at("inspect", dir, err)),
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => Ok(()),
        result => result,
    }
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// Narrow an existing credentials file to 0600 before it is rewritten (atomic writes keep the
/// existing mode), so a file an older BranchBox wrote 0644 becomes owner-only.
fn narrow_existing_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => restrict_mode(path, 0o600),
        // A symlink is refused by the atomic write itself; nothing else to narrow.
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io_error_at("inspect", path, err)),
    }
}

/// Set `path` to exactly `mode` unless it already has no group or other permissions.
#[cfg(unix)]
fn restrict_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let current = fs::metadata(path)
        .map_err(|err| io_error_at("inspect", path, err))?
        .permissions()
        .mode();
    if current & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|err| io_error_at("restrict permissions of", path, err))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn io_error_at(action: &str, path: &Path, err: io::Error) -> Error {
    Error::Io(io::Error::new(
        err.kind(),
        format!("Failed to {action} {}: {err}", path.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BranchBoxConfig;
    use tempfile::TempDir;

    const TOKEN: &str = "tok_0123456789abcdefghijABCDEFGHIJ-_xyzw";

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn repo() -> TempDir {
        TempDir::new().unwrap()
    }

    fn config(root: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(BranchBoxConfig::path(root)).unwrap()).unwrap()
    }

    #[test]
    fn stores_the_token_owner_only_and_points_the_config_at_it() {
        let temp = repo();
        let summary =
            set_cloudflare_credentials(temp.path(), Some("acct-123"), TokenChange::Set(TOKEN))
                .unwrap();

        let path = temp.path().join(CREDENTIALS_RELATIVE_PATH);
        assert_eq!(
            summary,
            CredentialsSummary {
                schema_version: 1,
                credentials_path: path.clone(),
                account_id: Some("acct-123".to_string()),
                token_present: true,
            }
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("CLOUDFLARE_API_TOKEN={TOKEN}\nCLOUDFLARE_ACCOUNT_ID=acct-123\n")
        );
        #[cfg(unix)]
        {
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }

        let loaded = BranchBoxConfig::load(temp.path()).unwrap();
        let cloudflared = loaded.tunnel.providers.cloudflared.unwrap();
        assert_eq!(cloudflared.account_id.as_deref(), Some("acct-123"));
        assert_eq!(
            cloudflared.api_token_path,
            Some(PathBuf::from(CREDENTIALS_RELATIVE_PATH))
        );
        assert!(!cloudflared.manual_instructions);
        assert!(!format!("{summary:?}").contains(TOKEN));
    }

    #[test]
    fn keeps_unmanaged_lines_unknown_config_keys_and_formatting() {
        let temp = repo();
        let secure = temp.path().join(".branchbox/secure");
        fs::create_dir_all(&secure).unwrap();
        fs::write(
            secure.join("cloudflared.env"),
            "# team notes\nCLOUDFLARE_API_TOKEN=old\nEXTRA=1\nCLOUDFLARE_API_TOKEN=stale\n",
        )
        .unwrap();
        let original = "{\n    \"version\": \"1\",\n    \"x_team\": {\"keep\": [1, 2]},\n    \
                        \"tunnel\": {\n        \"enabled\": false\n    }\n}\n";
        fs::write(BranchBoxConfig::path(temp.path()), original).unwrap();

        set_cloudflare_credentials(temp.path(), None, TokenChange::Set(TOKEN)).unwrap();

        assert_eq!(
            fs::read_to_string(secure.join("cloudflared.env")).unwrap(),
            format!("# team notes\nCLOUDFLARE_API_TOKEN={TOKEN}\nEXTRA=1\n")
        );
        let edited = fs::read_to_string(BranchBoxConfig::path(temp.path())).unwrap();
        assert!(
            edited.starts_with("{\n    \"version\": \"1\",\n    \"x_team\": {\"keep\": [1, 2]},"),
            "{edited}"
        );
        let value = config(temp.path());
        assert_eq!(value["x_team"], serde_json::json!({"keep": [1, 2]}));
        assert_eq!(value["tunnel"]["enabled"], false);
        assert_eq!(
            value["tunnel"]["providers"]["cloudflared"]["api_token_path"],
            CREDENTIALS_RELATIVE_PATH
        );
    }

    #[cfg(unix)]
    #[test]
    fn narrows_a_world_readable_file_and_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = repo();
        let secure = temp.path().join(".branchbox/secure");
        fs::create_dir_all(&secure).unwrap();
        fs::set_permissions(&secure, fs::Permissions::from_mode(0o755)).unwrap();
        let path = secure.join("cloudflared.env");
        fs::write(&path, "CLOUDFLARE_API_TOKEN=old\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(TOKEN)).unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&secure), 0o700);
    }

    #[test]
    fn empty_or_malformed_tokens_change_nothing() {
        let temp = repo();
        let path = temp.path().join(CREDENTIALS_RELATIVE_PATH);
        set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(TOKEN)).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        let config_before = fs::read_to_string(BranchBoxConfig::path(temp.path())).unwrap();

        for bad in ["", "   \n\t", "two words", "line\nbreak"] {
            let err = set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(bad))
                .unwrap_err();
            assert_eq!(err.code(), "validation_failed");
            assert!(err.to_string().contains("Nothing was changed"), "{err}");
        }
        let long = "x".repeat(MAX_TOKEN_LEN + 1);
        let err =
            set_cloudflare_credentials(temp.path(), None, TokenChange::Set(&long)).unwrap_err();
        assert!(!err.to_string().contains(&long));

        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(
            fs::read_to_string(BranchBoxConfig::path(temp.path())).unwrap(),
            config_before
        );
    }

    #[test]
    fn refusals_never_echo_the_token() {
        let temp = repo();
        let secret = "s3cr3t value";
        let err =
            set_cloudflare_credentials(temp.path(), None, TokenChange::Set(secret)).unwrap_err();
        assert!(!err.to_string().contains("s3cr3t"));
        assert!(!format!("{err:?}").contains("s3cr3t"));
        assert_eq!(format!("{:?}", TokenChange::Set(secret)), "Set(<redacted>)");
    }

    #[test]
    fn invalid_account_ids_name_the_value() {
        let temp = repo();
        for (account, expected) in [
            ("", "empty"),
            ("${CLOUDFLARE_ACCOUNT_ID}", "placeholder"),
            ("acct 1", "'acct 1' is invalid"),
        ] {
            let err =
                set_cloudflare_credentials(temp.path(), Some(account), TokenChange::Set(TOKEN))
                    .unwrap_err();
            assert!(err.to_string().contains(expected), "{err}");
        }
        assert!(!temp.path().join(".branchbox").exists());
    }

    #[test]
    fn a_config_with_comments_is_refused_before_anything_is_written() {
        let temp = repo();
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        fs::write(
            BranchBoxConfig::path(temp.path()),
            "{\n  // tunnels\n  \"version\": \"1\"\n}\n",
        )
        .unwrap();

        let err = set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(TOKEN))
            .unwrap_err();
        assert_eq!(err.code(), "config_invalid");
        let details = err.details().unwrap();
        assert_eq!(details["line"], 2);
        assert_eq!(details["column"], 3);
        assert!(err.to_string().contains("comments"), "{err}");
        assert!(!temp.path().join(CREDENTIALS_RELATIVE_PATH).exists());
    }

    #[test]
    fn a_config_that_would_not_load_is_refused() {
        let temp = repo();
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        fs::write(
            BranchBoxConfig::path(temp.path()),
            r#"{"runtime": {"provider": "nope"}}"#,
        )
        .unwrap();

        let err = set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(TOKEN))
            .unwrap_err();
        assert_eq!(err.code(), "config_invalid");
        assert!(err.to_string().contains("would not load"), "{err}");
        assert!(!temp.path().join(CREDENTIALS_RELATIVE_PATH).exists());
    }

    #[test]
    fn clear_removes_the_token_and_restores_manual_instructions() {
        let temp = repo();
        set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(TOKEN)).unwrap();

        let summary = set_cloudflare_credentials(temp.path(), None, TokenChange::Clear).unwrap();
        assert!(!summary.token_present);
        assert_eq!(summary.account_id.as_deref(), Some("acct"));
        assert_eq!(
            fs::read_to_string(temp.path().join(CREDENTIALS_RELATIVE_PATH)).unwrap(),
            "CLOUDFLARE_ACCOUNT_ID=acct\n"
        );
        let cloudflared = &config(temp.path())["tunnel"]["providers"]["cloudflared"];
        assert!(cloudflared.get("api_token_path").is_none(), "{cloudflared}");
        assert_eq!(cloudflared["manual_instructions"], true);
    }

    #[test]
    fn clearing_an_unconfigured_project_creates_nothing() {
        let temp = repo();
        let summary = set_cloudflare_credentials(temp.path(), None, TokenChange::Clear).unwrap();
        assert_eq!(summary.account_id, None);
        assert!(!summary.token_present);
        assert!(!temp.path().join(CREDENTIALS_RELATIVE_PATH).exists());
        assert!(!BranchBoxConfig::path(temp.path()).exists());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_secure_directory() {
        let temp = repo();
        let elsewhere = temp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, temp.path().join(".branchbox/secure")).unwrap();

        let err = set_cloudflare_credentials(temp.path(), Some("acct"), TokenChange::Set(TOKEN))
            .unwrap_err();
        assert!(err.to_string().contains("symlinked directory"), "{err}");
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    #[test]
    fn a_file_where_the_directory_belongs_is_refused() {
        let temp = repo();
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        fs::write(temp.path().join(".branchbox/secure"), "").unwrap();
        let err =
            set_cloudflare_credentials(temp.path(), None, TokenChange::Set(TOKEN)).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err}");
    }

    /// `init`'s token write goes through [`write_cloudflare_env`]: a file an older `init` wrote
    /// world-readable becomes owner-only and keeps its extra lines.
    #[cfg(unix)]
    #[test]
    fn init_token_write_replaces_a_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = repo();
        let path = temp.path().join(CREDENTIALS_RELATIVE_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "CLOUDFLARE_API_TOKEN=old\nCLOUDFLARE_ACCOUNT_ID=old\nTUNNEL_NOTE=x\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let stored = write_cloudflare_env(
            &path,
            Some("acct"),
            TokenChange::Set(&format!(" {TOKEN}\n")),
        )
        .unwrap();
        assert_eq!(
            stored,
            StoredCredentials {
                account_id: Some("acct".to_string()),
                token_present: true,
            }
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("CLOUDFLARE_API_TOKEN={TOKEN}\nCLOUDFLARE_ACCOUNT_ID=acct\nTUNNEL_NOTE=x\n")
        );
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn render_env_replaces_in_place_and_prepends_missing_keys() {
        assert_eq!(
            render_env("", &[(TOKEN_KEY, Some("t")), (ACCOUNT_KEY, Some("a"))]),
            "CLOUDFLARE_API_TOKEN=t\nCLOUDFLARE_ACCOUNT_ID=a\n"
        );
        assert_eq!(
            render_env(
                "CLOUDFLARE_API_KEY=legacy\nCLOUDFLARE_ACCOUNT_ID=old",
                &[(TOKEN_KEY, Some("t")), (ACCOUNT_KEY, Some("a"))]
            ),
            "CLOUDFLARE_API_TOKEN=t\nCLOUDFLARE_API_KEY=legacy\nCLOUDFLARE_ACCOUNT_ID=a\n"
        );
        assert_eq!(
            render_env("CLOUDFLARE_API_TOKEN=t\n", &[(TOKEN_KEY, None)]),
            ""
        );
        assert_eq!(
            env_value(
                "CLOUDFLARE_ACCOUNT_ID=\nCLOUDFLARE_ACCOUNT_ID=\"q\"\n",
                ACCOUNT_KEY
            ),
            Some("q".to_string())
        );
    }
}
