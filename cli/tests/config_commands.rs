//! Project configuration commands (DESIGN §5.10-§5.11): `config get|set|unset|apply`, which
//! read and edit `.branchbox/config.json` in place, and `tunnel credentials set`, which stores
//! the Cloudflare token owner-only and points the config at it. Golden documents live in
//! `fixtures/contract/commands/`; rerun with `UPDATE_CONTRACT_FIXTURES=1` to rewrite them after
//! an intended change.
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use serde_json::{json, Value};
use std::fs;
use std::process::Output;
use support::{assert_fixture, assert_single_json, init_test_repo, normalize_json, TestRepo};

const AREA: &str = "commands";

/// A token-shaped secret that must never be printed.
const TOKEN: &str = "cf-token-0123456789abcdefghijklmnopqrstu";

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Run `tunnel credentials set <args>` with `stdin` piped in.
fn credentials_set(repo: &TestRepo, args: &[&str], stdin: &str) -> Output {
    branchbox_cmd!(repo.path())
        .args(["tunnel", "credentials", "set"])
        .args(args)
        .write_stdin(stdin)
        .output()
        .expect("run tunnel credentials set")
}

fn assert_token_never_printed(output: &Output) {
    for (stream, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        assert!(
            !String::from_utf8_lossy(bytes).contains(TOKEN),
            "the token leaked on {stream}"
        );
    }
}

fn credentials_file(repo: &TestRepo) -> std::path::PathBuf {
    repo.path().join(".branchbox/secure/cloudflared.env")
}

fn config(repo: &TestRepo) -> Value {
    serde_json::from_slice(&fs::read(repo.path().join(".branchbox/config.json")).unwrap()).unwrap()
}

#[test]
fn credentials_set_stores_the_token_from_stdin_owner_only() {
    let repo = init_test_repo();
    let output = credentials_set(
        &repo,
        &["--account-id", "acct-123", "--api-token-stdin", "--json"],
        &format!("{TOKEN}\n"),
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_token_never_printed(&output);

    let summary = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(
        summary,
        json!({
            "schema_version": 1,
            "credentials_path": "<repo>/main/.branchbox/secure/cloudflared.env",
            "account_id": "acct-123",
            "token_present": true
        })
    );
    assert_fixture(AREA, "credentials_set", &summary);

    assert_eq!(
        fs::read_to_string(credentials_file(&repo)).unwrap(),
        format!("CLOUDFLARE_API_TOKEN={TOKEN}\nCLOUDFLARE_ACCOUNT_ID=acct-123\n")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&credentials_file(&repo)), 0o600);
        assert_eq!(mode(credentials_file(&repo).parent().unwrap()), 0o700);
    }

    let cloudflared = &config(&repo)["tunnel"]["providers"]["cloudflared"];
    assert_eq!(
        cloudflared,
        &json!({
            "account_id": "acct-123",
            "api_token_path": ".branchbox/secure/cloudflared.env",
            "manual_instructions": false
        })
    );
}

#[test]
fn credentials_set_in_text_mode_never_prints_the_token() {
    let repo = init_test_repo();
    let output = credentials_set(
        &repo,
        &["--account-id", "acct-123", "--api-token-stdin"],
        TOKEN,
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_token_never_printed(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("🔐 Cloudflare credentials saved"),
        "{stdout}"
    );
    assert!(stdout.contains("API token: stored"), "{stdout}");
}

#[test]
fn credentials_set_keeps_other_config_keys_and_lines() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox/secure")).unwrap();
    fs::write(credentials_file(&repo), "# notes\nOTHER=1\n").unwrap();
    fs::write(
        repo.path().join(".branchbox/config.json"),
        "{\n  \"version\": \"1\",\n  \"x_team\": true,\n  \
         \"feature\": {\"branch_prefix\": \"spike\"}\n}\n",
    )
    .unwrap();

    let output = credentials_set(
        &repo,
        &["--account-id", "acct", "--api-token-stdin", "--json"],
        TOKEN,
    );
    assert!(output.status.success(), "{}", stderr_of(&output));

    assert_eq!(
        fs::read_to_string(credentials_file(&repo)).unwrap(),
        format!("CLOUDFLARE_API_TOKEN={TOKEN}\nCLOUDFLARE_ACCOUNT_ID=acct\n# notes\nOTHER=1\n")
    );
    let edited = fs::read_to_string(repo.path().join(".branchbox/config.json")).unwrap();
    assert!(
        edited.starts_with(
            "{\n  \"version\": \"1\",\n  \"x_team\": true,\n  \
             \"feature\": {\"branch_prefix\": \"spike\"},"
        ),
        "formatting kept:\n{edited}"
    );
    assert_eq!(config(&repo)["x_team"], true);
}

/// `init` with tunnels declined saved `"cloudflared": null`; entering credentials afterwards
/// (the app's Sharing card) replaces it with the provider settings.
#[test]
fn credentials_set_replaces_a_null_cloudflared_section() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    fs::write(
        repo.path().join(".branchbox/config.json"),
        "{\n  \"version\": \"1\",\n  \"x_team\": {\"keep\": true},\n  \"tunnel\": {\n    \
         \"enabled\": false,\n    \"default_provider\": null,\n    \"providers\": {\n      \
         \"cloudflared\": null\n    }\n  }\n}\n",
    )
    .unwrap();

    let output = credentials_set(
        &repo,
        &["--account-id", "acct", "--api-token-stdin", "--json"],
        TOKEN,
    );
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_token_never_printed(&output);
    let config = config(&repo);
    assert_eq!(
        config["tunnel"]["providers"]["cloudflared"]["account_id"],
        "acct"
    );
    assert_eq!(config["x_team"]["keep"], true);
}

#[test]
fn empty_stdin_is_refused_and_the_existing_file_is_untouched() {
    let repo = init_test_repo();
    let stored = credentials_set(
        &repo,
        &["--account-id", "acct", "--api-token-stdin", "--json"],
        TOKEN,
    );
    assert!(stored.status.success(), "{}", stderr_of(&stored));
    let before = fs::read(credentials_file(&repo)).unwrap();
    let config_before = fs::read(repo.path().join(".branchbox/config.json")).unwrap();

    for input in ["", "  \n\t\n"] {
        let output = credentials_set(
            &repo,
            &["--account-id", "other", "--api-token-stdin", "--json"],
            input,
        );
        assert_eq!(output.status.code(), Some(1), "{}", stderr_of(&output));
        let envelope = assert_single_json(&output);
        assert_eq!(
            envelope["error"]["code"], "validation_failed",
            "{envelope:#}"
        );
        assert!(envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("empty Cloudflare API token"));
    }
    assert_eq!(fs::read(credentials_file(&repo)).unwrap(), before);
    assert_eq!(
        fs::read(repo.path().join(".branchbox/config.json")).unwrap(),
        config_before
    );
}

#[test]
fn a_token_with_spaces_is_refused_without_echoing_it() {
    let repo = init_test_repo();
    let output = credentials_set(
        &repo,
        &["--account-id", "acct", "--api-token-stdin", "--json"],
        &format!("Bearer {TOKEN}"),
    );
    assert_eq!(output.status.code(), Some(1));
    assert_token_never_printed(&output);
    let envelope = assert_single_json(&output);
    assert_eq!(envelope["error"]["code"], "validation_failed");
    assert!(!credentials_file(&repo).exists());
}

#[test]
fn clear_removes_the_token_and_restores_manual_instructions() {
    let repo = init_test_repo();
    let stored = credentials_set(
        &repo,
        &["--account-id", "acct", "--api-token-stdin", "--json"],
        TOKEN,
    );
    assert!(stored.status.success(), "{}", stderr_of(&stored));

    let output = credentials_set(&repo, &["--clear", "--json"], "");
    assert!(output.status.success(), "{}", stderr_of(&output));
    let summary = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(summary["token_present"], false);
    assert_eq!(summary["account_id"], "acct");
    assert_fixture(AREA, "credentials_cleared", &summary);

    assert_eq!(
        fs::read_to_string(credentials_file(&repo)).unwrap(),
        "CLOUDFLARE_ACCOUNT_ID=acct\n"
    );
    let cloudflared = &config(&repo)["tunnel"]["providers"]["cloudflared"];
    assert_eq!(
        cloudflared,
        &json!({"account_id": "acct", "manual_instructions": true})
    );
}

#[test]
fn a_config_with_comments_is_refused_naming_the_position() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    let original = "{\n  \"version\": \"1\" // keep\n}\n";
    fs::write(repo.path().join(".branchbox/config.json"), original).unwrap();

    let output = credentials_set(
        &repo,
        &["--account-id", "acct", "--api-token-stdin", "--json"],
        TOKEN,
    );
    assert_eq!(output.status.code(), Some(1));
    let envelope = assert_single_json(&output);
    assert_eq!(envelope["error"]["code"], "config_invalid", "{envelope:#}");
    assert_eq!(
        envelope["error"]["details"],
        json!({"line": 2, "column": 18})
    );
    assert!(!credentials_file(&repo).exists());
    assert_eq!(
        fs::read_to_string(repo.path().join(".branchbox/config.json")).unwrap(),
        original
    );
}

#[test]
fn credentials_set_outside_a_repository_is_refused() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .args([
            "tunnel",
            "credentials",
            "set",
            "--account-id",
            "acct",
            "--api-token-stdin",
            "--json",
        ])
        .write_stdin(TOKEN)
        .output()
        .expect("run tunnel credentials set");
    assert_eq!(output.status.code(), Some(1));
    let envelope = assert_single_json(&output);
    assert_eq!(
        envelope["error"]["code"], "not_a_git_repository",
        "{envelope:#}"
    );
    assert!(!temp.path().join(".branchbox").exists());
}

#[test]
fn the_token_flag_requires_stdin_and_an_account() {
    let repo = init_test_repo();
    let output = credentials_set(&repo, &["--account-id", "acct"], TOKEN);
    assert_eq!(output.status.code(), Some(2), "clap usage error");
    assert!(!credentials_file(&repo).exists());
}

// --- config ---------------------------------------------------------------------------------

fn config_path(repo: &TestRepo) -> std::path::PathBuf {
    repo.path().join(".branchbox/config.json")
}

fn config_cmd(repo: &TestRepo, args: &[&str]) -> Output {
    branchbox_cmd!(repo.path())
        .arg("config")
        .args(args)
        .output()
        .expect("run branchbox config")
}

/// `config apply --file - --json [extra]` with `patch` on standard input.
fn apply(repo: &TestRepo, patch: &str, extra: &[&str]) -> Output {
    branchbox_cmd!(repo.path())
        .args(["config", "apply", "--file", "-", "--json"])
        .args(extra)
        .write_stdin(patch)
        .output()
        .expect("run config apply")
}

/// Assert a failing `--json` command printed one envelope with `code`; returns it.
fn assert_envelope(output: &Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(1), "{}", stderr_of(output));
    let envelope = assert_single_json(output);
    assert_eq!(envelope["schema_version"], 1, "{envelope:#}");
    assert_eq!(envelope["error"]["code"], code, "{envelope:#}");
    envelope
}

#[test]
fn config_get_without_a_file_reports_every_default() {
    let repo = init_test_repo();
    let output = config_cmd(&repo, &["get", "--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let document = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(document["exists"], false);
    assert_eq!(document["path"], "<repo>/main/.branchbox/config.json");
    assert_eq!(document["file"], json!({}));
    assert_eq!(document["effective"]["runtime"]["provider"], "container");
    let keys = document["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 18);
    assert!(keys
        .iter()
        .all(|key| key["source"] == "default" && key["value"] == key["default"]));
    assert_fixture(AREA, "config_get_defaults", &document);
    assert!(
        !repo.path().join(".branchbox").exists(),
        "reading creates nothing"
    );
}

#[test]
fn config_get_one_key_prints_its_value() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    fs::write(
        config_path(&repo),
        "{\"runtime\": {\"provider\": \"sbx\"}}\n",
    )
    .unwrap();

    let output = config_cmd(&repo, &["get", "runtime.provider"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "sbx\n");

    let output = config_cmd(&repo, &["get", "runtime.provider", "--json"]);
    let document = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(document["exists"], true);
    assert_eq!(document["file"], json!({"runtime": {"provider": "sbx"}}));
    assert_eq!(
        document["keys"],
        json!([{
            "key": "runtime.provider",
            "type": "enum",
            "allowed": ["container", "sbx", "local-vm", "in-guest"],
            "default": "container",
            "value": "sbx",
            "source": "file",
            "description": document["keys"][0]["description"],
        }])
    );
    assert_fixture(AREA, "config_get_key", &document);

    let text = config_cmd(&repo, &["get"]);
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("runtime.provider"), "{stdout}");
    assert!(stdout.contains("\"sbx\""), "{stdout}");
    assert!(stdout.contains("(default)"), "{stdout}");
}

#[test]
fn config_set_round_trips_and_keeps_unknown_keys_and_formatting() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    let original = "{\n    \"version\": \"1\",\n    \"x_team\": {\"keep\": [1, 2]},\n    \
                    \"feature\": {\n        \"branch_prefix\": \"feature\"\n    }\n}\n";
    fs::write(config_path(&repo), original).unwrap();

    let output = config_cmd(&repo, &["set", "feature.branch_prefix", "spike"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("✓ Set feature.branch_prefix to \"spike\" in "),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(config_path(&repo)).unwrap(),
        original.replace("\"feature\"\n", "\"spike\"\n")
    );
    let output = config_cmd(&repo, &["get", "feature.branch_prefix"]);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "spike\n");

    // `feature start` picks the new prefix up.
    let output = branchbox_cmd!(repo.path())
        .args(["feature", "start", "eta", "--minimal", "--json"])
        .output()
        .expect("run feature start");
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(assert_single_json(&output)["branch_name"], "spike/eta");

    let output = config_cmd(&repo, &["unset", "feature.branch_prefix"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("the default \"feature\" applies"));
    assert_eq!(config(&repo)["x_team"], json!({"keep": [1, 2]}));
    assert!(config(&repo)["feature"].get("branch_prefix").is_none());
}

#[test]
fn config_set_an_invalid_enum_names_the_key_and_the_allowed_values() {
    let repo = init_test_repo();
    let output = config_cmd(&repo, &["set", "runtime.provider", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        stderr_of(&output).contains(
            "Error: Configuration error: runtime.provider must be one of: container, sbx, \
             local-vm, in-guest (got \"nope\"). Nothing was changed."
        ),
        "{}",
        stderr_of(&output)
    );
    assert!(!config_path(&repo).exists());

    let output = config_cmd(&repo, &["set", "feature.branch_prefix", "bad prefix"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr_of(&output).contains("feature.branch_prefix must be a branch prefix"));
}

#[test]
fn config_apply_reads_a_merge_patch_from_stdin() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    fs::write(
        config_path(&repo),
        "{\n  \"x_team\": true,\n  \"tunnel\": {\"enabled\": true}\n}\n",
    )
    .unwrap();
    let patch = r#"{"runtime": {"provider": "sbx"}, "tunnel": {"enabled": null},
                    "feature": {"teardown": {"delete_branch_by_default": false}}}"#;

    let dry = apply(&repo, patch, &["--dry-run"]);
    assert!(dry.status.success(), "{}", stderr_of(&dry));
    let before = fs::read_to_string(config_path(&repo)).unwrap();
    assert!(
        before.contains("\"enabled\": true"),
        "a dry run writes nothing"
    );

    let output = apply(&repo, patch, &[]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let result = normalize_json(assert_single_json(&output), &[repo.root()]);
    assert_eq!(
        result,
        normalize_json(assert_single_json(&dry), &[repo.root()])
    );
    assert_eq!(
        result["changed"],
        json!([
            {"key": "runtime.provider", "old": null, "new": "sbx"},
            {"key": "feature.teardown.delete_branch_by_default", "old": null, "new": false},
            {"key": "tunnel.enabled", "old": true, "new": null}
        ])
    );
    assert_eq!(result["effective"]["runtime"]["provider"], "sbx");
    assert_fixture(AREA, "config_apply", &result);

    let written = config(&repo);
    assert_eq!(written["x_team"], true);
    assert_eq!(written["tunnel"], json!({}));

    // Applying it again changes nothing.
    let again = apply(&repo, patch, &[]);
    assert_eq!(assert_single_json(&again)["changed"], json!([]));

    // Text mode lists the changes.
    let text = branchbox_cmd!(repo.path())
        .args(["config", "apply", "--file", "-"])
        .write_stdin(r#"{"editor": {"default_agent": "codex"}}"#)
        .output()
        .expect("run config apply");
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(
        stdout.contains("editor.default_agent: (unset) → \"codex\""),
        "{stdout}"
    );
}

#[test]
fn config_apply_reads_a_patch_file() {
    let repo = init_test_repo();
    let patch = repo.root().join("patch.json");
    fs::write(&patch, r#"{"tunnel": {"enabled": false}}"#).unwrap();
    let output = branchbox_cmd!(repo.path())
        .args(["config", "apply", "--json", "--file"])
        .arg(&patch)
        .output()
        .expect("run config apply");
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(config(&repo), json!({"tunnel": {"enabled": false}}));

    let missing = branchbox_cmd!(repo.path())
        .args(["config", "apply", "--json", "--file", "nope.json"])
        .output()
        .expect("run config apply");
    let envelope = assert_envelope(&missing, "io_error");
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("nope.json"));
}

#[test]
fn config_apply_refuses_unknown_keys_bad_values_and_bad_json() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    fs::write(config_path(&repo), "{}\n").unwrap();

    let envelope = assert_envelope(
        &apply(&repo, r#"{"runtime": {"provder": "sbx"}}"#, &[]),
        "config_unknown_key",
    );
    assert_eq!(
        envelope["error"]["details"],
        json!({"key": "runtime.provder"})
    );
    assert_fixture(
        AREA,
        "envelope_config_unknown_key",
        &normalize_json(envelope, &[repo.root()]),
    );

    let envelope = assert_envelope(
        &apply(&repo, r#"{"tunnel": {"enabled": "yes"}}"#, &[]),
        "config_invalid",
    );
    assert_eq!(
        envelope["error"]["details"],
        json!({"key": "tunnel.enabled", "expected": "true or false"})
    );
    assert_fixture(
        AREA,
        "envelope_config_invalid_value",
        &normalize_json(envelope, &[repo.root()]),
    );

    let envelope = assert_envelope(&apply(&repo, "{not json", &[]), "validation_failed");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("The config patch from standard input is not valid JSON (line 1,"),
        "{message}"
    );
    assert!(message.ends_with("); nothing was changed"), "{message}");
    assert_envelope(&apply(&repo, "[]", &[]), "validation_failed");

    assert_eq!(fs::read_to_string(config_path(&repo)).unwrap(), "{}\n");
}

#[test]
fn config_with_comments_is_refused_naming_the_position() {
    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    let original = "{\n  // runtime\n  \"runtime\": {\"provider\": \"sbx\"}\n}\n";
    fs::write(config_path(&repo), original).unwrap();

    for output in [
        apply(&repo, r#"{"tunnel": {"enabled": false}}"#, &[]),
        config_cmd(&repo, &["get", "--json"]),
    ] {
        let envelope = assert_envelope(&output, "config_invalid");
        assert_eq!(
            envelope["error"]["details"],
            json!({"line": 2, "column": 3})
        );
        assert!(envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("comments"));
    }
    let output = config_cmd(&repo, &["set", "tunnel.enabled", "false"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(config_path(&repo)).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn an_owner_only_config_stays_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let repo = init_test_repo();
    fs::create_dir_all(repo.path().join(".branchbox")).unwrap();
    fs::write(config_path(&repo), "{}\n").unwrap();
    fs::set_permissions(config_path(&repo), fs::Permissions::from_mode(0o600)).unwrap();

    let output = apply(&repo, r#"{"editor": {"default_agent": "claude"}}"#, &[]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert_eq!(
        fs::metadata(config_path(&repo))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn config_outside_a_repository_is_refused() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = branchbox_cmd!(temp.path())
        .args(["config", "get", "--json"])
        .output()
        .expect("run config get");
    assert_envelope(&output, "not_a_git_repository");
}

/// `config apply` and `feature start` both take the `.branchbox` lock for their writes and
/// replace files atomically: run together, every start is recorded, every applied key lands,
/// and neither file is ever torn.
#[cfg(unix)]
#[test]
fn concurrent_config_applies_and_starts_do_not_corrupt_each_other() {
    use std::sync::Barrier;
    use std::thread;

    let repo = init_test_repo();
    let patches = [
        r#"{"runtime": {"sbx": {"run_services": ["web"]}}}"#,
        r#"{"feature": {"teardown": {"delete_branch_by_default": false}}}"#,
        r#"{"feature": {"teardown": {"prompt_force_delete_unmerged": false}}}"#,
        r#"{"tunnel": {"enabled": false}}"#,
        r#"{"editor": {"default_agent": "codex"}}"#,
        r#"{"editor": {"hide_secondary_sidebar": true}}"#,
    ];
    let starts: Vec<String> = (0..6).map(|index| format!("con-{index}")).collect();
    let barrier = Barrier::new(patches.len() + starts.len());

    let outputs: Vec<Output> = thread::scope(|scope| {
        let mut handles = Vec::new();
        for patch in patches {
            let barrier = &barrier;
            let mut cmd = branchbox_cmd!(repo.path());
            cmd.args(["config", "apply", "--file", "-", "--json"])
                .write_stdin(patch);
            handles.push(scope.spawn(move || {
                barrier.wait();
                cmd.output().expect("run config apply")
            }));
        }
        for name in &starts {
            let barrier = &barrier;
            let mut cmd = branchbox_cmd!(repo.path());
            cmd.args(["feature", "start", name, "--minimal", "--json"]);
            handles.push(scope.spawn(move || {
                barrier.wait();
                cmd.output().expect("run feature start")
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread panicked"))
            .collect()
    });
    for output in &outputs {
        assert!(output.status.success(), "{}", stderr_of(output));
        assert_single_json(output);
    }

    let written = config(&repo);
    assert_eq!(written["runtime"]["sbx"]["run_services"], json!(["web"]));
    assert_eq!(
        written["feature"]["teardown"]["delete_branch_by_default"],
        false
    );
    assert_eq!(
        written["feature"]["teardown"]["prompt_force_delete_unmerged"],
        false
    );
    assert_eq!(written["tunnel"]["enabled"], false);
    assert_eq!(written["editor"]["default_agent"], "codex");
    assert_eq!(written["editor"]["hide_secondary_sidebar"], true);

    let registry: Value =
        serde_json::from_slice(&fs::read(repo.path().join(".branchbox/registry.json")).unwrap())
            .unwrap();
    let mut recorded: Vec<String> = registry["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["work_feature"].as_str().unwrap().to_string())
        .collect();
    recorded.sort();
    assert_eq!(recorded, starts);
}

#[test]
fn config_text_mode_reports_no_ops_and_dry_runs() {
    let repo = init_test_repo();
    let stdout = |output: &Output| String::from_utf8_lossy(&output.stdout).into_owned();

    let output = config_cmd(&repo, &["get"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        stdout(&output).contains("(not created yet; showing defaults)"),
        "{}",
        stdout(&output)
    );

    let output = config_cmd(&repo, &["unset", "tunnel.enabled"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout(&output);
    assert!(text.starts_with("tunnel.enabled is not set in "), "{text}");
    assert!(
        text.ends_with(".branchbox/config.json; nothing changed (the default true applies)\n"),
        "{text}"
    );
    assert!(
        !repo.path().join(".branchbox").exists(),
        "a no-op creates nothing"
    );

    config_cmd(&repo, &["set", "tunnel.enabled", "false"]);
    let output = config_cmd(&repo, &["set", "tunnel.enabled", "false"]);
    assert_eq!(
        stdout(&output),
        "tunnel.enabled is already false; nothing changed\n"
    );

    let apply_text = |patch: &str, extra: &[&str]| {
        branchbox_cmd!(repo.path())
            .args(["config", "apply", "--file", "-"])
            .args(extra)
            .write_stdin(patch.to_string())
            .output()
            .expect("run config apply")
    };
    let output = apply_text(r#"{"tunnel": {"enabled": false}}"#, &[]);
    assert!(
        stdout(&output).starts_with("No changes to "),
        "{}",
        stdout(&output)
    );
    let output = apply_text(r#"{"tunnel": {"enabled": true}}"#, &["--dry-run"]);
    assert!(
        stdout(&output).starts_with("Would change "),
        "{}",
        stdout(&output)
    );
    assert!(stdout(&output).contains("tunnel.enabled: false → true"));
    assert_eq!(config(&repo), json!({"tunnel": {"enabled": false}}));
}

#[test]
fn config_apply_refuses_oversized_or_binary_stdin() {
    let repo = init_test_repo();
    let huge = format!("{{\"x\": \"{}\"}}", "a".repeat(1024 * 1024));
    let envelope = assert_envelope(&apply(&repo, &huge, &[]), "validation_failed");
    assert_eq!(
        envelope["error"]["message"],
        "The config patch on standard input is larger than 1048576 bytes; nothing was changed"
    );

    let output = branchbox_cmd!(repo.path())
        .args(["config", "apply", "--file", "-", "--json"])
        .write_stdin(vec![0xff, 0xfe, b'{', b'}'])
        .output()
        .expect("run config apply");
    let envelope = assert_envelope(&output, "validation_failed");
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not UTF-8"));
    assert!(!config_path(&repo).exists());
}
