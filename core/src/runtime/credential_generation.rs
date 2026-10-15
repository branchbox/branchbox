//! Optional trusted generation-directory contract. Ordinary manifests do not
//! enter this path. The outer runtime verifies signed issuance and commit;
//! BranchBox independently preserves the original source/consumer/path binding.
use super::{Error, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) const WORKLOAD_SLICE: &str = "agentifyworkloads.slice";

/// Opaque evidence that the closed original assignment and current generation
/// custody were independently validated. No caller can construct an opt-in.
#[derive(Debug, Clone)]
pub(super) struct Admission {
    _private: (),
}

pub(super) const TARGET: &str = "/run/branchbox/leases/credential-generations";
pub(super) const PROVIDER_WRAPPER: &str = "umask 0022; unset GH_TOKEN GITHUB_TOKEN AGENTIFY_SOURCE_TOKEN GIT_ASKPASS SSH_ASKPASS; for tool in /usr/bin/flock /usr/bin/date /usr/bin/timeout; do test -x \"$tool\" || exit 78; done; AGENTIFY_CREDENTIAL_BASE_PATH=$PATH; export AGENTIFY_CREDENTIAL_BASE_PATH; PATH=/run/branchbox/leases/credential-generations:$PATH; export PATH; exec \"$@\"";
const MAX_FILE: usize = 64 * 1024;
const SLOTS: [&str; 5] = [
    "model",
    "source_fetch",
    "source_delivery",
    "project_environment",
    "preview",
];
const GIT: &[u8] = include_bytes!("credential-generation/git");
const GH: &[u8] = include_bytes!("credential-generation/gh");
const ASKPASS: &[u8] = include_bytes!("credential-generation/git-askpass");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: String,
    manifest_sha256: String,
    identity_sha256: String,
    run_id: String,
    lease_id: String,
    writer_epoch: u64,
    absolute_deadline_at: DateTime<Utc>,
    source_path: PathBuf,
    original_references: BTreeMap<String, String>,
    project_environment_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: String,
    manifest_sha256: String,
    identity_sha256: String,
    writer_epoch: u64,
    generation: u64,
    offer_digest: String,
    expires_at: DateTime<Utc>,
    references: BTreeMap<String, String>,
    token_sha256: BTreeMap<String, String>,
}

/// Held through one actual tool operation. Native provider startup drops this
/// guard before exec; its Git/gh children acquire their own bounded read guard.
#[derive(Debug)]
pub(super) struct GenerationGuard {
    _lock: File,
}

impl Admission {
    pub(super) fn validate_requested_json(&self, value: &Value) -> Result<()> {
        // The receiver keeps this validation reachable only after opaque admission.
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                Value::Object(values) => {
                    for (key, value) in values {
                        if matches!(
                            key.to_ascii_lowercase().as_str(),
                            "cgroup_parent" | "cgroupparent" | "cgroup-parent"
                        ) || (key == "runArgs"
                            && value.as_array().is_some_and(|arguments| {
                                arguments.iter().any(|argument| {
                                    argument.as_str().is_some_and(|argument| {
                                        argument.trim().split('=').next() == Some("--cgroup-parent")
                                    })
                                })
                            }))
                        {
                            return Err(Error::validation(
                                "Renewable workload rejects repository cgroup-parent overrides",
                            ));
                        }
                        pending.push(value);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        pending.push(value);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl GenerationGuard {
    pub(super) fn admission(&self) -> Admission {
        Admission { _private: () }
    }
}

fn refused() -> Error {
    Error::validation("Managed credential generation refused")
}
fn require(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(refused())
    }
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn reference(value: &str) -> bool {
    value.starts_with("eg_")
        && (19..=163).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn bytes(path: &Path, owner: u32, maximum: usize, mode: u32) -> Result<Vec<u8>> {
    let before = fs::symlink_metadata(path).map_err(|_| refused())?;
    require(
        before.is_file()
            && before.nlink() == 1
            && before.uid() == owner
            && before.permissions().mode() & 0o777 == mode
            && before.len() <= maximum as u64,
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| refused())?;
    let opened = file.metadata().map_err(|_| refused())?;
    require(
        (
            before.dev(),
            before.ino(),
            before.len(),
            before.uid(),
            before.nlink(),
            before.mode(),
        ) == (
            opened.dev(),
            opened.ino(),
            opened.len(),
            opened.uid(),
            opened.nlink(),
            opened.mode(),
        ),
    )?;
    let mut result = Secret(Vec::new());
    file.by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut result.0)
        .map_err(|_| refused())?;
    let after = fs::symlink_metadata(path).map_err(|_| refused())?;
    require(
        (
            before.dev(),
            before.ino(),
            before.len(),
            before.uid(),
            before.nlink(),
            before.mode(),
        ) == (
            after.dev(),
            after.ino(),
            after.len(),
            after.uid(),
            after.nlink(),
            after.mode(),
        ) && result.0.len() <= maximum,
    )?;
    Ok(std::mem::take(&mut result.0))
}

struct Secret(Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: every byte is live and uniquely owned. A volatile wipe
            // cannot be removed as a dead store after hashing token material.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

fn directory(path: &Path, owner: u32, mode: u32) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| refused())?;
    require(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == owner
            && metadata.permissions().mode() & 0o777 == mode
            && fs::canonicalize(path).map_err(|_| refused())? == path,
    )
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_digest(value: &Value) -> Result<String> {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(key, value)| (key.clone(), sorted(value)))
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    Ok(hash(
        &serde_json::to_vec(&sorted(value)).map_err(|_| refused())?,
    ))
}

/// Validate the closed directory and apply ONLY the four original credential
/// leases. Tool/shared/capability ceilings are never extended by this overlay.
pub(super) fn apply(manifest: &mut Value, path: &Path) -> Result<Option<GenerationGuard>> {
    let Some(value) = manifest.get("credential_generation") else {
        return Ok(None);
    };
    let spec: Binding = serde_json::from_value(value.clone()).map_err(|_| refused())?;
    let owner = fs::symlink_metadata(path).map_err(|_| refused())?.uid();
    let run_root = path.parent().ok_or_else(refused)?;
    let source = run_root.join("materializations/credential-generations");
    let now = Utc::now();
    require(
        manifest["version"] == "3"
            && spec.schema_version == "branchbox.credential-generation-directory.v1"
            && spec.source_path == source
            && spec.run_id == manifest["run_id"]
            && spec.lease_id == manifest["lease_id"]
            && spec.writer_epoch > 0
            && hex(&spec.manifest_sha256)
            && hex(&spec.identity_sha256)
            && hex(&spec.project_environment_sha256)
            && now < spec.absolute_deadline_at
            && spec.absolute_deadline_at <= now + chrono::Duration::hours(4),
    )?;
    directory(run_root, owner, 0o700)?;
    directory(&run_root.join("materializations"), owner, 0o700)?;
    directory(&source, owner, 0o755)?;
    let spec_bytes = bytes(&source.join("binding.json"), owner, MAX_FILE, 0o444)?;
    let same: Value = serde_json::from_slice(&spec_bytes).map_err(|_| refused())?;
    require(&same == value)?;
    let mut original = manifest.clone();
    original
        .as_object_mut()
        .ok_or_else(refused)?
        .remove("credential_generation");
    require(canonical_digest(&original)? == spec.manifest_sha256)?;
    let lock_path = source.join(".operation-lock");
    require(bytes(&lock_path, owner, 0, 0o444)?.is_empty())?;
    let lock = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&lock_path)
        .map_err(|_| refused())?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: the File remains live and owned by the returned guard.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
            break;
        }
        require(
            std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock
                && Instant::now() < until,
        )?;
        std::thread::sleep(Duration::from_millis(10));
    }
    let opened = lock.metadata().map_err(|_| refused())?;
    let actual = fs::symlink_metadata(&lock_path).map_err(|_| refused())?;
    require(
        opened.is_file()
            && opened.nlink() == 1
            && opened.uid() == owner
            && opened.permissions().mode() & 0o777 == 0o444
            && (opened.dev(), opened.ino()) == (actual.dev(), actual.ino()),
    )?;
    require(
        fs::symlink_metadata(source.join("pending"))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
    )?;
    let now = Utc::now();
    for (name, expected) in [("git", GIT), ("gh", GH), ("git-askpass", ASKPASS)] {
        require(bytes(&source.join(name), owner, MAX_FILE, 0o555)? == expected)?;
    }
    let pointer = fs::read_link(source.join("current")).map_err(|_| refused())?;
    let Some(name) = pointer.to_str() else {
        return Err(refused());
    };
    require(
        name.len() == 17
            && name.starts_with('g')
            && name[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
    )?;
    let folder = source.join(&pointer);
    directory(&folder, owner, 0o755)?;
    let state: State =
        serde_json::from_slice(&bytes(&folder.join("state.json"), owner, MAX_FILE, 0o444)?)
            .map_err(|_| refused())?;
    require(
        state.schema_version == "branchbox.credential-generation-state.v1"
            && state.generation <= 8
            && pointer == PathBuf::from(format!("g{:016x}", state.generation))
            && hex(&state.offer_digest)
            && state.manifest_sha256 == spec.manifest_sha256
            && state.identity_sha256 == spec.identity_sha256
            && state.writer_epoch == spec.writer_epoch
            && now < state.expires_at
            && state.expires_at <= spec.absolute_deadline_at
            && (state.generation == 0 || state.expires_at <= now + chrono::Duration::seconds(3300))
            && spec.original_references.len() >= 4
            && spec.original_references.len() <= 5
            && spec.original_references.keys().eq(state.references.keys())
            && spec
                .original_references
                .keys()
                .all(|slot| SLOTS.contains(&slot.as_str()))
            && SLOTS[..4]
                .iter()
                .all(|slot| spec.original_references.contains_key(*slot))
            && spec
                .original_references
                .values()
                .all(|value| reference(value))
            && state.references.values().all(|value| reference(value)),
    )?;
    require(
        state.references.values().collect::<BTreeSet<_>>().len() == state.references.len()
            && ((state.generation == 0 && state.references == spec.original_references)
                || (state.generation > 0
                    && state
                        .references
                        .values()
                        .all(|value| !spec.original_references.values().any(|old| old == value)))),
    )?;
    require(
        bytes(&folder.join("expires-at"), owner, 32, 0o444)?
            == format!("{}\n", state.expires_at.timestamp()).as_bytes(),
    )?;
    require(
        state
            .token_sha256
            .keys()
            .map(String::as_str)
            .eq(["source_delivery", "source_fetch"]),
    )?;
    for (slot, file) in [
        ("source_fetch", "source-fetch"),
        ("source_delivery", "source-delivery"),
    ] {
        let token = Secret(bytes(&folder.join(file), owner, 16 * 1024 + 1, 0o444)?);
        let raw = token.0.strip_suffix(b"\n").ok_or_else(refused)?;
        require(
            (8..=16 * 1024).contains(&raw.len())
                && raw.iter().all(u8::is_ascii_graphic)
                && state.token_sha256[slot] == hash(raw),
        )?;
    }
    let leases = manifest["leases"].as_array_mut().ok_or_else(refused)?;
    let mut matched = BTreeSet::new();
    for lease in leases {
        for (slot, scope) in [
            ("model", "model-identity"),
            ("source_fetch", "source-control-identity"),
            ("source_delivery", "credential-generation"),
            ("project_environment", "project-environment"),
        ] {
            if lease["lease_id"] == spec.original_references[slot] {
                require(lease["scope"] == scope && matched.insert(slot))?;
                if slot == "project_environment" {
                    require(
                        lease["materializations"][0]["sha256"] == spec.project_environment_sha256,
                    )?;
                }
                lease["expires_at"] =
                    serde_json::to_value(state.expires_at).map_err(|_| refused())?;
            }
        }
    }
    require(matched.len() == 4)?;
    Ok(Some(GenerationGuard { _lock: lock }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        _temp: tempfile::TempDir,
        source: PathBuf,
        manifest: PathBuf,
        value: Value,
        state: Value,
    }
    fn write(path: &Path, bytes: &[u8], mode: u32) {
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let run = fs::canonicalize(temp.path())
                .unwrap()
                .join("00000001-1111-4111-8111-111111111111");
            fs::create_dir(&run).unwrap();
            fs::set_permissions(&run, fs::Permissions::from_mode(0o700)).unwrap();
            let materials = run.join("materializations");
            fs::create_dir(&materials).unwrap();
            fs::set_permissions(&materials, fs::Permissions::from_mode(0o700)).unwrap();
            let source = materials.join("credential-generations");
            fs::create_dir(&source).unwrap();
            fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
            let workspace = fs::canonicalize(temp.path()).unwrap().join("workspace");
            let repository = workspace.join("repository");
            fs::create_dir_all(&repository).unwrap();
            let project = materials.join("project-environment.env");
            write(&project, b"PUBLIC_SETTING=same-value\n", 0o600);
            let project_digest = hash(b"PUBLIC_SETTING=same-value\n");
            let until = Utc::now() + chrono::Duration::minutes(5);
            let refs = json!({"model":"eg_fixture_model_0000000000", "source_fetch":"eg_fixture_fetch_0000000000",
                "source_delivery":"eg_fixture_delivery_0000000000", "project_environment":"eg_fixture_environment_0000000000"});
            let mut value = json!({"version":"3", "run_id":"00000001-1111-4111-8111-111111111111",
            "lease_id":"00000002-1111-4111-8111-111111111111", "repository":{"path":repository,"revision":"a".repeat(40)},
            "outer_runtime_id":"vm-fixture","workspace":workspace,"task_branch":"feature/coding-demo",
            "tunnel_placement":"outer","published_ports":[],"workspace_consumer":{"uid":1000,"gid":1000},
            "leases":[
                {"lease_id":refs["model"],"scope":"model-identity","consumer":"coding-agent","expires_at":until,
                    "executable":"provider-cli","inherited_environment":["MODEL_ACCESS_TOKEN"],"materializations":[]},
                {"lease_id":refs["source_fetch"],"scope":"source-control-identity","consumer":"coding-agent","expires_at":until,"materializations":[]},
                {"lease_id":refs["source_delivery"],"scope":"credential-generation","consumer":"coding-agent","expires_at":until,
                    "materializations":[{"source_path":source,"target_path":TARGET}]},
                {"lease_id":refs["project_environment"],"scope":"project-environment","consumer":"app","expires_at":until,
                    "materializations":[{"source_path":project,"target_path":"/run/branchbox/leases/project-env","sha256":project_digest}]},
                {"lease_id":"tool-immutable","scope":"platform-tunnel","consumer":"outer-connector","expires_at":until,"materializations":[]}
            ]});
            let manifest_hash = canonical_digest(&value).unwrap();
            let binding = json!({"schema_version":"branchbox.credential-generation-directory.v1",
                "manifest_sha256":manifest_hash,"identity_sha256":"b".repeat(64),"writer_epoch":7,
                "run_id":value["run_id"],"lease_id":value["lease_id"],
                "absolute_deadline_at":Utc::now()+chrono::Duration::hours(2),"source_path":source,
                "original_references":refs,"project_environment_sha256":project_digest});
            value["credential_generation"] = binding.clone();
            let state = json!({"schema_version":"branchbox.credential-generation-state.v1",
                "manifest_sha256":manifest_hash,"identity_sha256":"b".repeat(64),"writer_epoch":7,
                "generation":0,"offer_digest":"d".repeat(64),"expires_at":until,"references":refs,
                "token_sha256":{"source_fetch":hash(b"synthetic-fetch-A"),"source_delivery":hash(b"synthetic-delivery-A")}});
            for (name, bytes, mode) in [
                ("git", GIT, 0o555),
                ("gh", GH, 0o555),
                ("git-askpass", ASKPASS, 0o555),
                (".operation-lock", b"".as_slice(), 0o444),
            ] {
                write(&source.join(name), bytes, mode);
            }
            write(
                &source.join("binding.json"),
                &serde_json::to_vec(&binding).unwrap(),
                0o444,
            );
            let manifest = run.join("assignment.json");
            write(&manifest, &serde_json::to_vec(&value).unwrap(), 0o600);
            let fixture = Self {
                _temp: temp,
                source,
                manifest,
                value,
                state,
            };
            fixture.generation(0, b"synthetic-fetch-A", b"synthetic-delivery-A");
            std::os::unix::fs::symlink("g0000000000000000", fixture.source.join("current"))
                .unwrap();
            fixture
        }
        fn generation(&self, number: u64, fetch: &[u8], delivery: &[u8]) {
            let folder = self.source.join(format!("g{number:016x}"));
            fs::create_dir(&folder).unwrap();
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
            let mut state = self.state.clone();
            state["generation"] = number.into();
            if number > 0 {
                state["expires_at"] = json!(Utc::now() + chrono::Duration::minutes(10));
                for slot in SLOTS[..4].iter() {
                    state["references"][slot] =
                        format!("eg_generation{number}_{slot}_0000000000").into();
                }
            }
            state["token_sha256"] =
                json!({"source_fetch":hash(fetch),"source_delivery":hash(delivery)});
            for (name, token) in [("source-fetch", fetch), ("source-delivery", delivery)] {
                let mut bytes = token.to_vec();
                bytes.push(b'\n');
                write(&folder.join(name), &bytes, 0o444);
            }
            let expires: DateTime<Utc> =
                serde_json::from_value(state["expires_at"].clone()).unwrap();
            write(
                &folder.join("expires-at"),
                format!("{}\n", expires.timestamp()).as_bytes(),
                0o444,
            );
            write(
                &folder.join("state.json"),
                &serde_json::to_vec(&state).unwrap(),
                0o444,
            );
        }
        fn point(&self, generation: u64) {
            std::os::unix::fs::symlink(
                format!("g{generation:016x}"),
                self.source.join("current.next"),
            )
            .unwrap();
            fs::rename(
                self.source.join("current.next"),
                self.source.join("current"),
            )
            .unwrap();
        }
    }

    #[test]
    fn overlay_changes_only_four_exact_credential_leases_and_never_source_or_tool_authority() {
        let f = Fixture::new();
        let original = f.value.clone();
        f.generation(1, b"synthetic-fetch-B", b"synthetic-delivery-B");
        f.point(1);
        let mut value = f.value.clone();
        let guard = apply(&mut value, &f.manifest).unwrap().unwrap();
        assert_eq!(value["leases"][4], original["leases"][4]);
        assert_eq!(value["repository"], original["repository"]);
        assert_eq!(
            value["leases"][0]["lease_id"],
            original["leases"][0]["lease_id"]
        );
        assert_ne!(
            value["leases"][0]["expires_at"],
            original["leases"][0]["expires_at"]
        );
        drop(guard);
        for field in ["run_id", "lease_id", "repository"] {
            let mut changed = f.value.clone();
            changed[field] = "changed".into();
            assert!(apply(&mut changed, &f.manifest).is_err());
        }
        let mut legacy = json!({"version":"3"});
        assert!(apply(&mut legacy, &f.manifest).unwrap().is_none());
    }

    #[test]
    fn full_assignment_loader_accepts_exact_directory_and_refuses_unbound_generation() {
        let f = Fixture::new();
        let loaded = super::super::load_assignment(&f.manifest).unwrap();
        assert_eq!(
            loaded
                .authorize_provider_execution("provider-cli", &["MODEL_ACCESS_TOKEN".to_owned()])
                .unwrap(),
            "coding-agent"
        );
        assert!(loaded
            .mounts
            .iter()
            .any(|mount| mount.scope == super::super::LeaseScope::CredentialGeneration));
        drop(loaded);
        f.generation(1, b"synthetic-fetch-B", b"synthetic-delivery-B");
        f.point(1);
        let loaded = super::super::load_assignment(&f.manifest).unwrap();
        assert_eq!(
            loaded.manifest.leases[4].expires_at,
            serde_json::from_value(f.value["leases"][4]["expires_at"].clone()).ok()
        );
        drop(loaded);
        write(&f.source.join("pending"), b"unfinished", 0o444);
        assert!(super::super::load_assignment(&f.manifest).is_err());
    }

    #[test]
    fn opaque_admission_rejects_raw_and_merged_parent_overrides() {
        let f = Fixture::new();
        let guard = apply(&mut f.value.clone(), &f.manifest).unwrap().unwrap();
        let admission = guard.admission();
        for request in [
            json!({"runArgs":["--cgroup-parent","other.slice"]}),
            json!({"mergedConfiguration":{"runArgs":["--cgroup-parent=agentifyworkloads.slice"]}}),
            json!({"services":{"dependency":{"cgroup_parent":"other.slice"}}}),
        ] {
            assert!(admission.validate_requested_json(&request).is_err());
        }
        admission
            .validate_requested_json(
                &json!({"runArgs":["--init"],"services":{"app":{"image":"fixed"}}}),
            )
            .unwrap();
    }

    #[test]
    fn pending_poison_escaping_pointer_epoch_digest_and_token_mutations_refuse() {
        for mutation in [
            "pending",
            "dangling_pending",
            "pointer",
            "epoch",
            "digest",
            "token",
            "helper",
        ] {
            let f = Fixture::new();
            match mutation {
                "pending" => write(&f.source.join("pending"), b"prepared", 0o444),
                "dangling_pending" => {
                    std::os::unix::fs::symlink("absent", f.source.join("pending")).unwrap()
                }
                "pointer" => {
                    fs::remove_file(f.source.join("current")).unwrap();
                    std::os::unix::fs::symlink("../outside", f.source.join("current")).unwrap();
                }
                "epoch" | "digest" => {
                    let mut state = f.state.clone();
                    if mutation == "epoch" {
                        state["writer_epoch"] = 8.into();
                    } else {
                        state["identity_sha256"] = "e".repeat(64).into();
                    }
                    write(
                        &f.source.join("g0000000000000000/state.json"),
                        &serde_json::to_vec(&state).unwrap(),
                        0o444,
                    );
                }
                "token" => write(
                    &f.source.join("g0000000000000000/source-delivery"),
                    b"synthetic-tampered\n",
                    0o444,
                ),
                "helper" => write(&f.source.join("gh"), b"#!/bin/sh\nexit 0\n", 0o555),
                _ => unreachable!(),
            }
            assert!(
                apply(&mut f.value.clone(), &f.manifest).is_err(),
                "{mutation}"
            );
        }
    }

    #[test]
    fn shared_operation_guard_blocks_adoption_until_actual_operation_finishes() {
        let f = Fixture::new();
        let guard = apply(&mut f.value.clone(), &f.manifest).unwrap().unwrap();
        let writer = File::open(f.source.join(".operation-lock")).unwrap();
        // SAFETY: test owns both open descriptors.
        assert_eq!(
            unsafe { libc::flock(writer.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            -1
        );
        drop(guard);
        assert_eq!(
            unsafe { libc::flock(writer.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn real_child_helpers_rebind_without_changing_parent_and_remove_ambient_token() {
        use std::process::{Command, Stdio};
        let f = Fixture::new();
        let binaries = f.source.parent().unwrap().join("fixture-bin");
        fs::create_dir(&binaries).unwrap();
        for (name, environment) in [("gh", "GH_TOKEN"), ("git", "AGENTIFY_SOURCE_TOKEN")] {
            write(
                &binaries.join(name),
                format!("#!/bin/sh\nprintf '%s' \"${{{environment}}}\" | /usr/bin/sha256sum\n")
                    .as_bytes(),
                0o555,
            );
        }
        // Only the fixed root is replaced in this nonsecret filesystem fixture.
        let gh = String::from_utf8(GH.to_vec())
            .unwrap()
            .replace(TARGET, f.source.to_str().unwrap());
        let git = String::from_utf8(GIT.to_vec())
            .unwrap()
            .replace(TARGET, f.source.to_str().unwrap());
        write(&f.source.join("gh"), gh.as_bytes(), 0o555);
        write(&f.source.join("git"), git.as_bytes(), 0o555);
        let mut child=Command::new("/bin/sh").args(["-c","echo $$; while IFS= read -r command; do case \"$command\" in gh) gh;; fetch) git -C push fetch;; push) git -C /fixture push;; quit) exit;; esac; done"])
            .env_clear().env("PATH",format!("{}:{}:/usr/bin:/bin",f.source.display(),binaries.display()))
            .env("AGENTIFY_CREDENTIAL_BASE_PATH",format!("{}:/usr/bin:/bin",binaries.display()))
            .env("GH_TOKEN","synthetic-stale-ambient").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let pid = child.id();
        let mut input = child.stdin.take().unwrap();
        use std::io::{BufRead, BufReader, Write};
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim().parse::<u32>().unwrap(), pid);
        for (command, expected) in [
            ("gh", hash(b"synthetic-delivery-A")),
            ("fetch", hash(b"synthetic-fetch-A")),
            ("push", hash(b"synthetic-delivery-A")),
        ] {
            writeln!(input, "{command}").unwrap();
            input.flush().unwrap();
            line.clear();
            output.read_line(&mut line).unwrap();
            assert!(line.starts_with(&expected));
        }
        f.generation(1, b"synthetic-fetch-B", b"synthetic-delivery-B");
        f.point(1);
        writeln!(input, "gh").unwrap();
        input.flush().unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert!(line.starts_with(&hash(b"synthetic-delivery-B")));
        assert_eq!(child.id(), pid);
        writeln!(input, "quit").unwrap();
        drop(input);
        assert!(child.wait().unwrap().success());
    }
}
