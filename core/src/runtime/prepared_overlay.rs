//! Content-free provenance for fixed platform-generated workspace inputs.
//! Measured before container startup; consumers must independently verify the
//! trusted producer/assignment and recheck physical state before using it.
use super::InGuestRuntimeMetadata;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = "branchbox.in-guest-preparation.v1";
const PRIVATE_SCHEMA: &str = "branchbox.in-guest-preparation.v2";
const GENERATED: [&str; 2] = [
    ".devcontainer/.devcontainer.json",
    ".devcontainer/.branchbox-sbx-compose.yaml",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedOverlay {
    pub schema_version: String,
    pub assignment_sha256: String,
    pub run_id: String,
    pub assignment_lease_id: String,
    pub outer_runtime_id: String,
    pub repository_revision: String,
    pub work_feature: String,
    pub task_branch: String,
    pub workspace_path: String,
    pub generated: Vec<PreparedOverlayFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_configuration: Option<PreparedPrivateConfiguration>,
    pub projected_git_pointer_sha256: String,
    pub common_git_device: u64,
    pub common_git_inode: u64,
    pub prepared_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedPrivateConfiguration {
    pub path: String,
    pub device: u64,
    pub inode: u64,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub files: Vec<PreparedOverlayFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedOverlayFile {
    pub path: String,
    pub present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<u64>,
}

impl PreparedOverlay {
    /// This optional producer covers only the standard managed linked layout.
    /// Repository-mode and non-Unix starts keep their previous behavior without
    /// advertising recovery provenance. No provider-home configuration is read.
    pub(crate) fn capture(
        assignment: &Path,
        repository: &Path,
        workspace: &Path,
        feature: &str,
        scope: &InGuestRuntimeMetadata,
        private_configuration: Option<&Path>,
    ) -> Result<Option<Self>> {
        #[cfg(unix)]
        {
            unix::capture_with_configuration(
                assignment,
                repository,
                workspace,
                feature,
                scope,
                private_configuration,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (
                assignment,
                repository,
                workspace,
                feature,
                scope,
                private_configuration,
            );
            Ok(None)
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs::{File, Metadata};
    use std::io::Read;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, PathBuf};

    const MAX_INPUT_BYTES: u64 = 1024 * 1024;
    struct Directory {
        file: File,
        path: PathBuf,
    }
    impl Directory {
        fn open(path: &Path) -> Result<Self> {
            if !path.is_absolute() {
                return Err(Error::validation("Preparation root must be absolute"));
            }
            let mut file = File::open("/")?;
            for component in path.components() {
                match component {
                    Component::RootDir => (),
                    Component::Normal(name) => {
                        file = open_at(&file, name, true)?;
                    }
                    _ => return Err(Error::validation("Noncanonical preparation root")),
                }
            }
            Ok(Self {
                file,
                path: path.to_path_buf(),
            })
        }
        fn verify(&self) -> Result<()> {
            let current = Self::open(&self.path)?;
            if identity(&current.file.metadata()?) != identity(&self.file.metadata()?) {
                return Err(Error::validation("Preparation directory replaced"));
            }
            Ok(())
        }
        fn child_file(&self, relative: &str) -> Result<File> {
            self.verify()?;
            let mut parts = relative.split('/').peekable();
            let mut parent = self.file.try_clone()?;
            while let Some(part) = parts.next() {
                if part.is_empty() || matches!(part, "." | "..") {
                    return Err(Error::validation("Noncanonical preparation input"));
                }
                parent = open_at(&parent, std::ffi::OsStr::new(part), parts.peek().is_some())?;
            }
            Ok(parent)
        }
        fn measure(&self, relative: &str) -> Result<(String, Metadata)> {
            let mut file = self.child_file(relative)?;
            let before = file.metadata()?;
            if !before.is_file() || before.nlink() != 1 || before.len() > MAX_INPUT_BYTES {
                return Err(Error::validation(
                    "Preparation input must be bounded single-link regular file",
                ));
            }
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_INPUT_BYTES + 1)
                .read_to_end(&mut bytes)?;
            let after = file.metadata()?;
            let current = self.child_file(relative)?.metadata()?;
            self.verify()?;
            if bytes.len() as u64 != before.len()
                || observation(&before) != observation(&after)
                || observation(&before) != observation(&current)
            {
                return Err(Error::validation(
                    "Preparation input changed during measurement",
                ));
            }
            Ok((format!("{:x}", Sha256::digest(bytes)), before))
        }
        fn contents(&self, relative: &str) -> Result<Vec<u8>> {
            let (digest, _) = self.measure(relative)?;
            let mut file = self.child_file(relative)?;
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_INPUT_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_INPUT_BYTES
                || format!("{:x}", Sha256::digest(&bytes)) != digest
                || self.measure(relative)?.0 != digest
            {
                return Err(Error::validation(
                    "Preparation contents changed during measurement",
                ));
            }
            Ok(bytes)
        }
    }
    fn open_at(parent: &File, name: &std::ffi::OsStr, directory: bool) -> Result<File> {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes())
            .map_err(|_| Error::validation("Invalid preparation input"))?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if directory { libc::O_DIRECTORY } else { 0 };
        // Only FD-relative, no-follow paths below the independently held root.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn identity(m: &Metadata) -> (u64, u64) {
        (m.dev(), m.ino())
    }
    fn observation(m: &Metadata) -> (u64, u64, u64, u32, u32, u32, i64, i64, i64, i64) {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mode(),
            m.uid(),
            m.gid(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    }
    fn capture_private_configuration(
        assignment: &Directory,
        assignment_name: &str,
        workspace: &Path,
        path: &Path,
    ) -> Result<PreparedPrivateConfiguration> {
        let manifest: serde_json::Value =
            serde_json::from_slice(&assignment.contents(assignment_name)?)?;
        let runtime_uid = unsafe { libc::geteuid() };
        let consumer_uid = manifest["workspace_consumer"]["uid"].as_u64();
        let consumer_gid = manifest["workspace_consumer"]["gid"].as_u64();
        if manifest["version"] != "3"
            || !consumer_uid
                .is_some_and(|uid| uid > 0 && uid <= u32::MAX as u64 && uid != runtime_uid as u64)
            || !consumer_gid.is_some_and(|gid| gid > 0 && gid <= u32::MAX as u64)
        {
            return Err(Error::validation(
                "Private preparation requires a v3 workspace consumer",
            ));
        }
        let workspace = std::fs::canonicalize(workspace)?;
        let workspace_parent = workspace
            .parent()
            .ok_or_else(|| Error::validation("Private preparation workspace parent missing"))?;
        if assignment.path.starts_with(workspace_parent)
            || workspace_parent.starts_with(&assignment.path)
        {
            return Err(Error::validation(
                "Private preparation must remain outside the shared workspace",
            ));
        }
        use std::os::unix::ffi::OsStrExt;
        let digest = format!("{:x}", Sha256::digest(workspace.as_os_str().as_bytes()));
        if path
            != assignment
                .path
                .join(format!("branchbox-compose-{}", &digest[..16]))
        {
            return Err(Error::validation(
                "Private preparation stage does not match its assignment and workspace",
            ));
        }
        let directory = Directory::open(path)?;
        let metadata = directory.file.metadata()?;
        if metadata.uid() != runtime_uid || metadata.mode() & 0o7777 != 0o700 {
            return Err(Error::validation(
                "Private preparation directory must remain runtime-owned and 0700",
            ));
        }
        let config_bytes = directory.contents(".devcontainer.json")?;
        let config: serde_json::Value = serde_json::from_slice(&config_bytes)?;
        let references = match config.get("dockerComposeFile") {
            None => Vec::new(),
            Some(serde_json::Value::Array(references))
                if !references.is_empty() && references.len() < 64 =>
            {
                let mut names = Vec::new();
                for (index, reference) in references.iter().enumerate() {
                    let expected = if index + 1 == references.len() {
                        ".branchbox-sbx-compose.yaml".to_string()
                    } else {
                        format!(".branchbox-sbx-compose-input-{index}.yaml")
                    };
                    if reference.as_str() != Some(expected.as_str()) {
                        return Err(Error::validation(
                            "Private preparation references must name only exact sanitized inputs",
                        ));
                    }
                    names.push(expected);
                }
                names
            }
            _ => {
                return Err(Error::validation(
                    "Private preparation Compose references are not bounded sanitized inputs",
                ))
            }
        };
        let mut expected = references;
        expected.push(".devcontainer.json".to_string());
        expected.sort();
        let names = || -> Result<Vec<String>> {
            let mut names = Vec::new();
            for item in std::fs::read_dir(path)? {
                if names.len() == 64 {
                    return Err(Error::validation(
                        "Private preparation directory exceeds its bounded file set",
                    ));
                }
                names.push(
                    item?
                        .file_name()
                        .into_string()
                        .map_err(|_| Error::validation("Private preparation filename refused"))?,
                );
            }
            names.sort();
            Ok(names)
        };
        if names()? != expected {
            return Err(Error::validation(
                "Private preparation directory differs from its sanitized references",
            ));
        }
        let mut files = Vec::new();
        for name in &expected {
            let (digest, observed) = directory.measure(name)?;
            if observed.uid() != runtime_uid
                || observed.gid() != metadata.gid()
                || observed.mode() & 0o7777 != 0o600
            {
                return Err(Error::validation(
                    "Private preparation files must remain runtime-owned and 0600",
                ));
            }
            files.push(PreparedOverlayFile {
                path: name.clone(),
                present: true,
                sha256: Some(digest),
                mode: Some(observed.mode() & 0o7777),
                uid: Some(observed.uid()),
                gid: Some(observed.gid()),
                device: Some(observed.dev()),
                inode: Some(observed.ino()),
            });
        }
        if files.iter().any(|file| {
            directory
                .measure(&file.path)
                .map_or(true, |(digest, observed)| {
                    file.sha256.as_ref() != Some(&digest)
                        || file.inode != Some(observed.ino())
                        || file.device != Some(observed.dev())
                })
        }) || files
            .iter()
            .find(|file| file.path == ".devcontainer.json")
            .and_then(|file| file.sha256.as_ref())
            != Some(&format!("{:x}", Sha256::digest(config_bytes)))
        {
            return Err(Error::validation(
                "Private preparation inputs changed during measurement",
            ));
        }
        if names()? != expected
            || observation(&metadata) != observation(&directory.file.metadata()?)
        {
            return Err(Error::validation(
                "Private preparation directory changed during measurement",
            ));
        }
        directory.verify()?;
        assignment.verify()?;
        Ok(PreparedPrivateConfiguration {
            path: path.display().to_string(),
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            mode: metadata.mode() & 0o7777,
            files,
        })
    }
    #[cfg(test)]
    fn capture(
        assignment: &Path,
        repository: &Path,
        workspace: &Path,
        feature: &str,
        scope: &InGuestRuntimeMetadata,
    ) -> Result<Option<PreparedOverlay>> {
        capture_with_configuration(assignment, repository, workspace, feature, scope, None)
    }

    pub(super) fn capture_with_configuration(
        assignment: &Path,
        repository: &Path,
        workspace: &Path,
        feature: &str,
        scope: &InGuestRuntimeMetadata,
        private_configuration: Option<&Path>,
    ) -> Result<Option<PreparedOverlay>> {
        if repository == workspace || workspace.parent() != repository.parent() {
            return Ok(None);
        }
        if feature.is_empty()
            || feature.len() > 128
            || matches!(feature, "." | "..")
            || !feature
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || workspace.file_name().and_then(|s| s.to_str()) != Some(feature)
        {
            return Err(Error::validation(
                "Preparation linked workspace identity mismatch",
            ));
        }
        let root = Directory::open(workspace)?;
        let (pointer_digest, _) = root.measure(".git")?;
        let mut pointer = root.child_file(".git")?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut pointer)
            .take(4096)
            .read_to_end(&mut bytes)?;
        if bytes != format!("gitdir: /workspaces/main/.git/worktrees/{feature}\n").as_bytes()
            || format!("{:x}", Sha256::digest(&bytes)) != pointer_digest
        {
            return Err(Error::validation("Preparation Git projection mismatch"));
        }
        let common = Directory::open(&crate::git::repository_common_git_dir(repository)?)?;
        let common_identity = identity(&common.file.metadata()?);
        let assignment_parent = Directory::open(
            assignment
                .parent()
                .ok_or_else(|| Error::validation("Preparation assignment parent missing"))?,
        )?;
        let assignment_name = assignment
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::validation("Preparation assignment name invalid"))?;
        let (assignment_digest, _) = assignment_parent.measure(assignment_name)?;
        let private_configuration = private_configuration
            .map(|stage| {
                capture_private_configuration(&assignment_parent, assignment_name, workspace, stage)
            })
            .transpose()?;
        let mut generated = Vec::new();
        for (index, relative) in GENERATED.iter().enumerate() {
            match root.measure(relative) {
                Ok(_) if private_configuration.is_some() => {
                    return Err(Error::validation(
                        "Private preparation must not authorize a workspace overlay",
                    ));
                }
                Ok((digest, m)) => generated.push(PreparedOverlayFile {
                    path: (*relative).into(),
                    present: true,
                    sha256: Some(digest),
                    mode: Some(m.mode() & 0o7777),
                    uid: Some(m.uid()),
                    gid: Some(m.gid()),
                    device: Some(m.dev()),
                    inode: Some(m.ino()),
                }),
                Err(Error::Io(error))
                    if (index == 1 || private_configuration.is_some())
                        && error.kind() == std::io::ErrorKind::NotFound =>
                {
                    generated.push(PreparedOverlayFile {
                        path: (*relative).into(),
                        present: false,
                        sha256: None,
                        mode: None,
                        uid: None,
                        gid: None,
                        device: None,
                        inode: None,
                    });
                }
                Err(error) => return Err(error),
            }
        }
        root.verify()?;
        common.verify()?;
        assignment_parent.verify()?;
        if assignment_parent.measure(assignment_name)?.0 != assignment_digest {
            return Err(Error::validation(
                "Preparation assignment changed during measurement",
            ));
        }
        Ok(Some(PreparedOverlay {
            schema_version: if private_configuration.is_some() {
                PRIVATE_SCHEMA
            } else {
                SCHEMA
            }
            .into(),
            assignment_sha256: assignment_digest,
            run_id: scope.run_id.clone(),
            assignment_lease_id: scope.assignment_lease_id.clone(),
            outer_runtime_id: scope.outer_runtime_id.clone(),
            repository_revision: scope.repository_revision.clone(),
            work_feature: feature.into(),
            task_branch: scope.task_branch.clone(),
            workspace_path: workspace.display().to_string(),
            generated,
            private_configuration,
            projected_git_pointer_sha256: pointer_digest,
            common_git_device: common_identity.0,
            common_git_inode: common_identity.1,
            prepared_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::process::Command;

        fn fixture() -> (
            tempfile::TempDir,
            PathBuf,
            PathBuf,
            PathBuf,
            InGuestRuntimeMetadata,
        ) {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let repository = root.join("repository");
            let workspace = root.join("aex-proof");
            fs::create_dir(&repository).unwrap();
            assert!(Command::new("git")
                .args(["init", "-q"])
                .arg(&repository)
                .status()
                .unwrap()
                .success());
            fs::create_dir_all(workspace.join(".devcontainer")).unwrap();
            fs::write(
                workspace.join(".git"),
                "gitdir: /workspaces/main/.git/worktrees/aex-proof\n",
            )
            .unwrap();
            fs::write(workspace.join(GENERATED[0]), "{\"name\":\"proof\"}\n").unwrap();
            let assignment = root.join("assignment.json");
            fs::write(&assignment, "{\"version\":\"3\"}\n").unwrap();
            let scope = InGuestRuntimeMetadata {
                run_id: "run-proof".into(),
                assignment_lease_id: "lease-proof".into(),
                outer_runtime_id: "outer-proof".into(),
                repository_revision: "a".repeat(40),
                task_branch: "feature/aex-proof".into(),
                tunnel_placement: super::super::super::InGuestTunnelPlacement::Outer,
                project_docker: "disabled".into(),
                leases: vec![],
                state_path: root.join("state.json"),
                prepared_overlay: None,
            };
            (temp, repository, workspace, assignment, scope)
        }

        #[test]
        fn measures_fixed_inputs_and_explicit_absence_without_exporting_content() {
            let (_temp, repo, work, assignment, scope) = fixture();
            fs::write(work.join("auth.json"), "excluded-private-canary").unwrap();
            fs::set_permissions(work.join("auth.json"), fs::Permissions::from_mode(0o000)).unwrap();
            let receipt = capture(&assignment, &repo, &work, "aex-proof", &scope)
                .unwrap()
                .unwrap();
            assert_eq!(receipt.schema_version, SCHEMA);
            assert_eq!(receipt.run_id, scope.run_id);
            assert_eq!(receipt.repository_revision, scope.repository_revision);
            assert_eq!(receipt.generated.len(), 2);
            let first = &receipt.generated[0];
            assert_eq!(first.path, GENERATED[0]);
            assert!(first.present);
            assert_eq!(
                first.sha256.as_deref(),
                Some(format!("{:x}", Sha256::digest(b"{\"name\":\"proof\"}\n")).as_str())
            );
            assert_eq!(
                first.mode,
                Some(fs::metadata(work.join(GENERATED[0])).unwrap().mode() & 0o7777)
            );
            let absent = serde_json::to_value(&receipt.generated[1]).unwrap();
            assert_eq!(
                absent,
                serde_json::json!({"path":GENERATED[1],"present":false})
            );
            let json = serde_json::to_string(&receipt).unwrap();
            assert!(!json.contains("excluded-private-canary"));
            assert!(!json.contains("auth.json"));
            assert!(receipt.prepared_at.ends_with('Z'));
        }

        struct PrivateFixture {
            _source: tempfile::TempDir,
            _run: tempfile::TempDir,
            repository: PathBuf,
            workspace: PathBuf,
            assignment: PathBuf,
            stage: PathBuf,
            scope: InGuestRuntimeMetadata,
        }

        impl PrivateFixture {
            fn new() -> Self {
                let (source, repository, workspace, _, scope) = fixture();
                fs::remove_file(workspace.join(GENERATED[0])).unwrap();
                let run = tempfile::tempdir().unwrap();
                // macOS temp paths can start at the /var symlink. The producer
                // deliberately refuses aliases, so fixture roots must be physical.
                let run_root = run.path().canonicalize().unwrap();
                let assignment = run_root.join("assignment.json");
                let runtime_uid = unsafe { libc::geteuid() };
                fs::write(
                    &assignment,
                    serde_json::to_vec(&serde_json::json!({
                        "version":"3", "workspace_consumer":{
                            "uid":if runtime_uid == 1000 { 1001 } else { 1000 }, "gid":1001
                        }
                    }))
                    .unwrap(),
                )
                .unwrap();
                use std::os::unix::ffi::OsStrExt;
                let digest = format!("{:x}", Sha256::digest(workspace.as_os_str().as_bytes()));
                let stage = run_root.join(format!("branchbox-compose-{}", &digest[..16]));
                fs::create_dir(&stage).unwrap();
                fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
                for (name, bytes) in [
                    (".devcontainer.json", "{\"dockerComposeFile\":[\".branchbox-sbx-compose-input-0.yaml\",\".branchbox-sbx-compose.yaml\"]}\n"),
                    (".branchbox-sbx-compose-input-0.yaml", "services: {app: {image: example.invalid/app}}\n"),
                    (".branchbox-sbx-compose.yaml", "services: {}\n"),
                ] {
                    fs::write(stage.join(name), bytes).unwrap();
                    fs::set_permissions(stage.join(name), fs::Permissions::from_mode(0o600)).unwrap();
                }
                Self {
                    _source: source,
                    _run: run,
                    repository,
                    workspace,
                    assignment,
                    stage,
                    scope,
                }
            }
            fn capture(&self) -> Result<Option<PreparedOverlay>> {
                capture_with_configuration(
                    &self.assignment,
                    &self.repository,
                    &self.workspace,
                    "aex-proof",
                    &self.scope,
                    Some(&self.stage),
                )
            }
        }

        #[test]
        fn private_stage_receipt_binds_real_inputs_without_authorizing_workspace_overlays() {
            let fixture = PrivateFixture::new();
            fs::write(
                fixture.assignment.parent().unwrap().join("auth.json"),
                "excluded-secret-canary",
            )
            .unwrap();
            let receipt = fixture.capture().unwrap().unwrap();
            assert_eq!(receipt.schema_version, PRIVATE_SCHEMA);
            assert!(receipt
                .generated
                .iter()
                .all(|item| !item.present && item.sha256.is_none()));
            let configuration = receipt.private_configuration.as_ref().unwrap();
            assert_eq!(configuration.path, fixture.stage.to_str().unwrap());
            assert_eq!(configuration.mode, 0o700);
            assert_eq!(configuration.files.len(), 3);
            assert!(configuration
                .files
                .iter()
                .all(|item| item.present && item.mode == Some(0o600)));
            assert_eq!(
                configuration
                    .files
                    .iter()
                    .map(|item| item.path.as_str())
                    .collect::<Vec<_>>(),
                [
                    ".branchbox-sbx-compose-input-0.yaml",
                    ".branchbox-sbx-compose.yaml",
                    ".devcontainer.json"
                ]
            );
            let serialized = serde_json::to_string(&receipt).unwrap();
            assert!(!serialized.contains("excluded-secret-canary"));
            assert!(!serialized.contains("auth.json"));
            assert!(!serialized.contains("example.invalid"));
            assert!(!fixture.workspace.join(GENERATED[0]).exists());
            assert!(!fixture.workspace.join(GENERATED[1]).exists());
            fs::write(
                fixture.stage.join(".branchbox-sbx-compose-input-0.yaml"),
                "services: {app: {image: changed.invalid/app}}\n",
            )
            .unwrap();
            let changed = fixture.capture().unwrap().unwrap();
            assert_ne!(
                configuration.files[0].sha256,
                changed.private_configuration.unwrap().files[0].sha256
            );
        }

        #[test]
        fn private_stage_refuses_symlinked_assignment_roots() {
            let fixture = PrivateFixture::new();
            let physical_run = fixture._run.path().canonicalize().unwrap();
            assert_eq!(fixture.assignment.parent().unwrap(), physical_run);
            assert_eq!(fixture.stage.canonicalize().unwrap(), fixture.stage);
            fixture.capture().unwrap().unwrap();
            let alias_root = tempfile::tempdir().unwrap();
            let alias = alias_root.path().canonicalize().unwrap().join("run-alias");
            symlink(&physical_run, &alias).unwrap();
            let error = capture_with_configuration(
                &alias.join("assignment.json"),
                &fixture.repository,
                &fixture.workspace,
                "aex-proof",
                &fixture.scope,
                Some(&fixture.stage),
            )
            .unwrap_err();
            assert!(
                matches!(error, Error::Io(ref io) if matches!(
                    io.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP)
                )),
                "aliased assignment root must fail no-follow directory traversal: {error}"
            );
            assert!(fixture.capture().unwrap().is_some());
        }

        #[test]
        fn private_stage_refuses_workspace_exceptions_and_unreferenced_or_unowned_inputs() {
            for case in 0..7 {
                let fixture = PrivateFixture::new();
                let input = fixture.stage.join(".branchbox-sbx-compose-input-0.yaml");
                match case {
                    0 => fs::write(fixture.workspace.join(GENERATED[0]), "untrusted overlay")
                        .unwrap(),
                    1 => {
                        fs::remove_file(&input).unwrap();
                        symlink(fixture.stage.join(".branchbox-sbx-compose.yaml"), &input).unwrap();
                    }
                    2 => {
                        fs::remove_file(&input).unwrap();
                        fs::hard_link(fixture.stage.join(".branchbox-sbx-compose.yaml"), &input)
                            .unwrap();
                    }
                    3 => fs::write(fixture.stage.join("unexpected.env"), "do-not-read").unwrap(),
                    4 => fs::set_permissions(&fixture.stage, fs::Permissions::from_mode(0o755))
                        .unwrap(),
                    5 => fs::set_permissions(&input, fs::Permissions::from_mode(0o640)).unwrap(),
                    6 => fs::write(
                        fixture.stage.join(".devcontainer.json"),
                        "{\"dockerComposeFile\":[\"../auth.json\"]}",
                    )
                    .unwrap(),
                    _ => unreachable!(),
                }
                assert!(
                    fixture.capture().is_err(),
                    "private-stage attack case {case}"
                );
            }
        }

        #[test]
        fn measures_present_optional_overlay_and_changes_its_digest_with_bytes() {
            let (_temp, repo, work, assignment, scope) = fixture();
            fs::write(work.join(GENERATED[1]), "services: {}\n").unwrap();
            let first = capture(&assignment, &repo, &work, "aex-proof", &scope)
                .unwrap()
                .unwrap();
            assert!(first.generated[1].present);
            fs::write(work.join(GENERATED[1]), "services: {app: {}}\n").unwrap();
            let second = capture(&assignment, &repo, &work, "aex-proof", &scope)
                .unwrap()
                .unwrap();
            assert_ne!(first.generated[1].sha256, second.generated[1].sha256);
        }

        #[test]
        fn refuses_linked_and_nonregular_inputs_without_reading_the_target() {
            for relative in GENERATED {
                let (_temp, repo, work, assignment, scope) = fixture();
                let target = work.join(relative);
                if target.exists() {
                    fs::remove_file(&target).unwrap();
                }
                let outside = repo.join("unrelated");
                fs::write(&outside, "must-remain-unchanged").unwrap();
                symlink(&outside, &target).unwrap();
                assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
                assert_eq!(fs::read(&outside).unwrap(), b"must-remain-unchanged");
                fs::remove_file(&target).unwrap();
                fs::hard_link(&outside, &target).unwrap();
                assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
                fs::remove_file(&target).unwrap();
                fs::create_dir(&target).unwrap();
                assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
            }
        }

        #[test]
        fn refuses_wrong_projection_missing_required_and_oversized_file() {
            let (_temp, repo, work, assignment, scope) = fixture();
            fs::write(
                work.join(".git"),
                "gitdir: /unrelated/.git/worktrees/aex-proof\n",
            )
            .unwrap();
            assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
            fs::write(
                work.join(".git"),
                "gitdir: /workspaces/main/.git/worktrees/aex-proof\n",
            )
            .unwrap();
            fs::remove_file(work.join(GENERATED[0])).unwrap();
            assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
            fs::write(
                work.join(GENERATED[0]),
                vec![b'x'; MAX_INPUT_BYTES as usize + 1],
            )
            .unwrap();
            assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
        }

        #[test]
        fn refuses_symlinked_parent_and_replaced_held_root() {
            let (_temp, repo, work, assignment, scope) = fixture();
            fs::rename(work.join(".devcontainer"), work.join("saved-config")).unwrap();
            symlink("saved-config", work.join(".devcontainer")).unwrap();
            assert!(capture(&assignment, &repo, &work, "aex-proof", &scope).is_err());
            let held = Directory::open(&work).unwrap();
            fs::rename(&work, work.with_file_name("replaced-workspace")).unwrap();
            fs::create_dir(&work).unwrap();
            assert!(held.verify().is_err());
        }

        #[test]
        fn preserves_repository_mode_and_old_metadata_without_receipt() {
            let (_temp, repo, _work, assignment, scope) = fixture();
            assert!(capture(&assignment, &repo, &repo, "aex-proof", &scope)
                .unwrap()
                .is_none());
            let old = serde_json::to_value(&scope).unwrap();
            assert!(old.get("prepared_overlay").is_none());
            assert!(serde_json::from_value::<InGuestRuntimeMetadata>(old)
                .unwrap()
                .prepared_overlay
                .is_none());
        }
    }
}
