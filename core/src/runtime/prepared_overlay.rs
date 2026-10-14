//! Content-free provenance for fixed platform-generated workspace inputs.
//! Measured before container startup; consumers must independently verify the
//! trusted producer/assignment and recheck physical state before using it.
use super::InGuestRuntimeMetadata;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = "branchbox.in-guest-preparation.v1";
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
    pub projected_git_pointer_sha256: String,
    pub common_git_device: u64,
    pub common_git_inode: u64,
    pub prepared_at: String,
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
    ) -> Result<Option<Self>> {
        #[cfg(unix)]
        {
            unix::capture(assignment, repository, workspace, feature, scope)
        }
        #[cfg(not(unix))]
        {
            let _ = (assignment, repository, workspace, feature, scope);
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
    pub(super) fn capture(
        assignment: &Path,
        repository: &Path,
        workspace: &Path,
        feature: &str,
        scope: &InGuestRuntimeMetadata,
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
        let mut generated = Vec::new();
        for (index, relative) in GENERATED.iter().enumerate() {
            match root.measure(relative) {
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
                    if index == 1 && error.kind() == std::io::ErrorKind::NotFound =>
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
        Ok(Some(PreparedOverlay {
            schema_version: SCHEMA.into(),
            assignment_sha256: assignment_digest,
            run_id: scope.run_id.clone(),
            assignment_lease_id: scope.assignment_lease_id.clone(),
            outer_runtime_id: scope.outer_runtime_id.clone(),
            repository_revision: scope.repository_revision.clone(),
            work_feature: feature.into(),
            task_branch: scope.task_branch.clone(),
            workspace_path: workspace.display().to_string(),
            generated,
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
