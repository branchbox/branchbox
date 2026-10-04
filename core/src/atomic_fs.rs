//! Registry integrity: the `.branchbox` state-directory lock and crash-safe writes
//!
//! Every BranchBox process that changes project state does so with read-modify-write cycles on
//! small JSON files (`registry.json`, `config.json`, devcontainer baselines). Two guarantees keep
//! those files whole when several processes (CLI runs, the macOS app, the agent) work at once:
//!
//! - [`lock_state_dir`] serializes writers. It takes an exclusive advisory lock with
//!   `File::try_lock`: on Unix on the `.branchbox` directory itself, so nothing new appears in
//!   `git status`; elsewhere on `.branchbox/.lock`. The OS releases the lock when the holder
//!   exits, so a crashed process never wedges the project. Readers take no lock. On a
//!   filesystem without advisory locks (some NFS, SMB and FUSE mounts) writers go on unlocked,
//!   as BranchBox 0.13 always did, after one warning.
//! - [`write_atomic`] replaces a file in one rename, so a reader sees the old document or the new
//!   one, never a torn write, and a crash mid-write leaves the old file intact.

use crate::{Error, Result};
use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{self, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Capabilities this module provides, reported by `branchbox version --json`. `registry-lock`:
/// every registry update takes [`lock_state_dir`] and lands through [`write_atomic`].
pub(crate) const CAPABILITIES: &[&str] = &["registry-lock"];

/// How long a writer waits for another process to release the state-directory lock.
pub(crate) const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Temp files from a crashed writer older than this are removed by later writes.
const STALE_TEMP_AGE: Duration = Duration::from_secs(60 * 60);

const TEMP_PREFIX: &str = ".registry.";
const TEMP_SUFFIX: &str = ".tmp";

thread_local! {
    /// State directories this thread holds the lock for. A nested `lock_state_dir` on the same
    /// directory must not wait on itself: the OS lock belongs to the first file handle, and a
    /// second handle in the same process would block until the timeout.
    static HELD_LOCKS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// Exclusive hold on a `.branchbox` state directory, released on drop.
#[derive(Debug)]
#[must_use = "the lock is released as soon as the guard is dropped"]
pub(crate) struct StateDirLock {
    /// `None` for a nested acquisition on a thread that already holds the lock, and on a
    /// filesystem that cannot lock at all (see [`unlocked_guard_if_unsupported`]).
    file: Option<File>,
    dir: PathBuf,
    /// The thread-local bookkeeping is per thread, so the guard must stay on its thread.
    _not_send: PhantomData<*const ()>,
}

#[cfg(test)]
impl StateDirLock {
    /// The (canonicalized) state directory this guard holds.
    fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether this guard is a nested acquisition that leaves the lock to the outer guard.
    fn is_reentrant(&self) -> bool {
        self.file.is_none()
    }
}

impl Drop for StateDirLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            if let Err(err) = file.unlock() {
                // Closing the handle below releases the lock regardless.
                tracing::debug!("Failed to unlock {}: {err}", self.dir.display());
            }
            HELD_LOCKS.with(|held| {
                let mut held = held.borrow_mut();
                if let Some(index) = held.iter().position(|dir| dir == &self.dir) {
                    held.remove(index);
                }
            });
        }
    }
}

/// Take the exclusive lock on the state directory `dir` (normally `<repo>/.branchbox`), creating
/// the directory if needed. Polls every 25 ms for up to `timeout`, then fails with
/// [`Error::RegistryLocked`] naming the lock path. A nested call on a thread that already holds
/// the lock returns immediately with a guard that leaves the lock in place.
pub(crate) fn lock_state_dir(dir: &Path, timeout: Duration) -> Result<StateDirLock> {
    fs::create_dir_all(dir).map_err(|err| io_error_at("create the state directory", dir, err))?;
    let dir = dir
        .canonicalize()
        .map_err(|err| io_error_at("resolve the state directory", dir, err))?;

    if HELD_LOCKS.with(|held| held.borrow().contains(&dir)) {
        return Ok(StateDirLock {
            file: None,
            dir,
            _not_send: PhantomData,
        });
    }

    let lock_path = lock_path_for(&dir);
    let file =
        open_lock_file(&lock_path).map_err(|err| io_error_at("open the lock", &lock_path, err))?;
    let started = Instant::now();
    let mut announced = false;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(fs::TryLockError::WouldBlock) => {
                let waited = started.elapsed();
                if waited >= timeout {
                    return Err(Error::RegistryLocked {
                        path: lock_path,
                        waited_secs: waited.as_secs(),
                    });
                }
                if !announced {
                    // Say once why this process is idle; a wait can last up to `timeout`.
                    announced = true;
                    tracing::info!(
                        "Waiting for another BranchBox process to release {}",
                        lock_path.display()
                    );
                }
                std::thread::sleep(LOCK_POLL_INTERVAL.min(timeout - waited));
            }
            Err(fs::TryLockError::Error(err)) => {
                return unlocked_guard_if_unsupported(dir, &lock_path, err);
            }
        }
    }

    HELD_LOCKS.with(|held| held.borrow_mut().push(dir.clone()));
    Ok(StateDirLock {
        file: Some(file),
        dir,
        _not_send: PhantomData,
    })
}

/// A lock attempt failed with `err`. When the filesystem cannot take advisory locks at all
/// (ENOLCK, EOPNOTSUPP/ENOTSUP, or `Unsupported`), refusing every registry and worktree change
/// would break projects that BranchBox 0.13 handled, so the writer goes on without the lock, as
/// 0.13 always did, and the first such fallback in the process logs a warning naming the path.
/// Any other error fails, naming the lock path.
fn unlocked_guard_if_unsupported(
    dir: PathBuf,
    lock_path: &Path,
    err: io::Error,
) -> Result<StateDirLock> {
    if !locking_unsupported(&err) {
        return Err(io_error_at("lock", lock_path, err));
    }
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        tracing::warn!(
            "The filesystem holding {} does not support file locks ({err}); continuing without \
             the lock, so concurrent BranchBox processes may overwrite each other's registry \
             updates",
            lock_path.display()
        );
    });
    Ok(StateDirLock {
        file: None,
        dir,
        _not_send: PhantomData,
    })
}

/// Whether `err` says the filesystem has no advisory locks, as opposed to a failure to lock.
fn locking_unsupported(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(unix)]
    {
        matches!(
            err.raw_os_error(),
            Some(code) if code == libc::ENOLCK || code == libc::EOPNOTSUPP || code == libc::ENOTSUP
        )
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn lock_path_for(dir: &Path) -> PathBuf {
    dir.to_path_buf()
}

#[cfg(not(unix))]
fn lock_path_for(dir: &Path) -> PathBuf {
    dir.join(".lock")
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> io::Result<File> {
    // A read-only handle on the directory is enough for flock(2).
    File::open(path)
}

#[cfg(not(unix))]
fn open_lock_file(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// Replace `path` with `bytes` atomically. An existing file keeps its permission bits; a new file
/// gets `new_mode` (Unix only). Refuses to write through a symlink. The parent directory must
/// exist.
///
/// The bytes go to a `.registry.*.tmp` file in the same directory, which is chmod-ed before
/// anything is written (so an owner-only file is never readable at a wider mode), fsynced, and
/// renamed over `path`; the directory is then fsynced so the rename survives a crash. Afterwards,
/// temp files that a crashed writer left behind more than an hour ago are removed.
///
/// To tighten an existing file (say a token file that was written 0644), narrow it with
/// `set_permissions` first; the rewrite then keeps the narrower mode.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8], new_mode: u32) -> Result<()> {
    crate::workflows::feature::refuse_symlink_target(path)?;
    let dir = parent_dir(path);

    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .suffix(TEMP_SUFFIX)
        .tempfile_in(dir)
        .map_err(|err| io_error_at("create a temporary file in", dir, err))?;
    set_mode(temp.as_file(), target_mode(path, new_mode)?)
        .map_err(|err| io_error_at("set permissions on", temp.path(), err))?;
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|err| io_error_at("write", temp.path(), err))?;
    temp.persist(path)
        .map_err(|err| io_error_at("replace", path, err.error))?;
    sync_dir(dir).map_err(|err| io_error_at("sync the directory", dir, err))?;

    sweep_stale_temp_files(dir, STALE_TEMP_AGE);
    Ok(())
}

fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// The mode the replacement gets: the existing file's permission bits, else `new_mode`.
#[cfg(unix)]
fn target_mode(path: &Path, new_mode: u32) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.permissions().mode() & 0o7777),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(new_mode),
        Err(err) => Err(io_error_at("read permissions of", path, err)),
    }
}

#[cfg(not(unix))]
fn target_mode(_path: &Path, new_mode: u32) -> Result<u32> {
    Ok(new_mode)
}

#[cfg(unix)]
fn set_mode(file: &File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_file: &File, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    // Windows has no directory handle to flush; the rename is durable once it returns.
    Ok(())
}

/// Remove `.registry.*.tmp` files in `dir` last modified more than `max_age` ago, returning how
/// many were removed. Best effort: anything unreadable is left for a later sweep.
pub(crate) fn sweep_stale_temp_files(dir: &Path, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !(name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX)) {
            continue;
        }
        // `DirEntry::metadata` does not follow symlinks, so only real files are considered.
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > max_age);
        if stale {
            match fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                Err(err) => tracing::debug!(
                    "Failed to remove stale temp file {}: {err}",
                    entry.path().display()
                ),
            }
        }
    }
    removed
}

/// An IO error that names the operation and the path it failed on.
fn io_error_at(action: &str, path: &Path, err: io::Error) -> Error {
    Error::Io(io::Error::new(
        err.kind(),
        format!("Failed to {action} {}: {err}", path.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    fn temp_files_in(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX))
            .collect()
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    #[test]
    fn filesystems_without_locks_fall_back_to_an_unlocked_guard() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join(".branchbox");
        fs::create_dir(&dir).unwrap();
        for code in [libc::ENOLCK, libc::EOPNOTSUPP, libc::ENOTSUP] {
            let guard = unlocked_guard_if_unsupported(
                dir.clone(),
                &dir,
                io::Error::from_raw_os_error(code),
            )
            .unwrap_or_else(|err| panic!("errno {code} should fall back: {err}"));
            assert!(guard.file.is_none());
            // The unlocked guard is not registered, so it leaves no reentrancy record behind.
            drop(guard);
            assert!(!HELD_LOCKS.with(|held| held.borrow().contains(&dir)));
        }
        let unsupported = io::Error::new(io::ErrorKind::Unsupported, "no flock");
        assert!(unlocked_guard_if_unsupported(dir.clone(), &dir, unsupported).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn other_lock_failures_name_the_lock_path() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join(".branchbox");
        let err = unlocked_guard_if_unsupported(
            dir.clone(),
            &dir,
            io::Error::from_raw_os_error(libc::EBADF),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to lock"), "{message}");
        assert!(message.contains(&dir.display().to_string()), "{message}");
    }

    #[test]
    fn write_atomic_creates_and_replaces_content() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("registry.json");

        write_atomic(&path, b"{\"version\":\"1\"}", 0o644).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"version\":\"1\"}");

        write_atomic(&path, b"{}", 0o644).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        assert!(temp_files_in(temp.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_creates_new_files_with_the_requested_mode() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("registry.json");
        write_atomic(&path, b"{}", 0o644).unwrap();
        assert_eq!(mode_of(&path), 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_an_existing_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, b"{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&path, b"{\"a\":1}", 0o644).unwrap();
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_refuses_a_symlink_target() {
        let temp = TempDir::new().unwrap();
        let outside = temp.path().join("outside.json");
        fs::write(&outside, b"original").unwrap();
        let link = temp.path().join("registry.json");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = write_atomic(&link, b"{}", 0o644).unwrap_err();
        assert!(
            err.to_string()
                .contains("Refusing to write through symlink"),
            "{err}"
        );
        assert!(err.to_string().contains("registry.json"), "{err}");
        assert_eq!(fs::read(&outside).unwrap(), b"original");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(temp_files_in(temp.path()).is_empty());
    }

    #[test]
    fn write_atomic_names_a_missing_directory() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("missing");
        let err = write_atomic(&missing.join("registry.json"), b"{}", 0o644).unwrap_err();
        assert!(matches!(err, Error::Io(_)));
        assert!(
            err.to_string().contains(&missing.display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn write_atomic_accepts_a_bare_file_name() {
        assert_eq!(parent_dir(Path::new("registry.json")), Path::new("."));
        assert_eq!(parent_dir(Path::new("/r/registry.json")), Path::new("/r"));
    }

    #[test]
    fn sweep_removes_only_stale_registry_temp_files() {
        let temp = TempDir::new().unwrap();
        let stale = temp.path().join(".registry.abc123.tmp");
        let fresh = temp.path().join(".registry.def456.tmp");
        let unrelated = temp.path().join("notes.tmp");
        for path in [&stale, &fresh, &unrelated] {
            fs::write(path, b"x").unwrap();
        }
        let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();

        assert_eq!(sweep_stale_temp_files(temp.path(), STALE_TEMP_AGE), 1);
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(unrelated.exists());

        // A later write sweeps too.
        File::options()
            .write(true)
            .open(&fresh)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        write_atomic(&temp.path().join("registry.json"), b"{}", 0o644).unwrap();
        assert!(!fresh.exists());
        assert_eq!(
            sweep_stale_temp_files(&temp.path().join("missing"), STALE_TEMP_AGE),
            0
        );
    }

    #[test]
    fn readers_never_observe_a_partial_document() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("registry.json");
        let document = |generation: usize| {
            let features: Vec<_> = (0..1_000)
                .map(|index| {
                    serde_json::json!({
                        "work_feature": format!("feature-{index}"),
                        "generation": generation,
                        "notes": "x".repeat(150),
                    })
                })
                .collect();
            serde_json::to_vec_pretty(&serde_json::json!({ "features": features })).unwrap()
        };
        let initial = document(0);
        assert!(initial.len() > 200_000, "{} bytes", initial.len());
        write_atomic(&path, &initial, 0o644).unwrap();

        let writer_path = path.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer_stop = Arc::clone(&stop);
        let writer = std::thread::spawn(move || {
            for generation in 1.. {
                write_atomic(&writer_path, &document(generation), 0o644).unwrap();
                if writer_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
            }
        });

        for _ in 0..500 {
            let bytes = fs::read(&path).unwrap();
            let parsed: serde_json::Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|err| panic!("reader saw invalid JSON: {err}"));
            assert_eq!(parsed["features"].as_array().unwrap().len(), 1_000);
        }
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        writer.join().unwrap();
        assert!(temp_files_in(temp.path()).is_empty());
    }

    #[test]
    fn lock_serializes_read_modify_write_across_threads() {
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join(".branchbox");
        fs::create_dir_all(&state_dir).unwrap();
        let registry = state_dir.join("registry.json");
        write_atomic(&registry, b"[]", 0o644).unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["alpha", "beta"]
            .into_iter()
            .map(|name| {
                let state_dir = state_dir.clone();
                let registry = registry.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let _lock = lock_state_dir(&state_dir, LOCK_TIMEOUT).unwrap();
                    let mut entries: Vec<String> =
                        serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
                    // Without the lock, both threads read `[]` here and one entry is lost.
                    std::thread::sleep(Duration::from_millis(200));
                    entries.push(name.to_string());
                    write_atomic(&registry, &serde_json::to_vec(&entries).unwrap(), 0o644).unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }

        let mut entries: Vec<String> =
            serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
        entries.sort();
        assert_eq!(entries, ["alpha", "beta"]);
    }

    #[test]
    fn lock_timeout_reports_registry_locked_with_the_path() {
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join(".branchbox");
        let held = lock_state_dir(&state_dir, LOCK_TIMEOUT).unwrap();
        assert!(!held.is_reentrant());
        let expected_path = lock_path_for(held.dir());

        let contender_dir = state_dir.clone();
        let err = std::thread::spawn(move || {
            lock_state_dir(&contender_dir, Duration::from_millis(100)).unwrap_err()
        })
        .join()
        .unwrap();
        match &err {
            Error::RegistryLocked { path, waited_secs } => {
                assert_eq!(path, &expected_path);
                assert_eq!(*waited_secs, 0);
            }
            other => panic!("expected RegistryLocked, got {other:?}"),
        }
        assert!(
            err.to_string()
                .contains(&expected_path.display().to_string()),
            "{err}"
        );

        drop(held);
        let contender_dir = state_dir.clone();
        let reacquired = std::thread::spawn(move || {
            lock_state_dir(&contender_dir, Duration::from_millis(100)).is_ok()
        })
        .join()
        .unwrap();
        assert!(reacquired, "dropping the guard releases the lock");
    }

    #[test]
    fn nested_lock_on_the_same_thread_is_reentrant() {
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join(".branchbox");
        let outer = lock_state_dir(&state_dir, LOCK_TIMEOUT).unwrap();
        // A different spelling of the same directory is still recognised.
        let inner = lock_state_dir(&state_dir.join("."), Duration::from_millis(100)).unwrap();
        assert!(inner.is_reentrant());
        assert_eq!(inner.dir(), outer.dir());
        drop(inner);

        // Dropping the nested guard leaves the outer lock held.
        let contender_dir = state_dir.clone();
        let contended = std::thread::spawn(move || {
            lock_state_dir(&contender_dir, Duration::from_millis(100)).is_err()
        })
        .join()
        .unwrap();
        assert!(contended, "the outer guard must still hold the lock");

        drop(outer);
        let again = lock_state_dir(&state_dir, Duration::from_millis(100)).unwrap();
        assert!(
            !again.is_reentrant(),
            "the lock is released with the outer guard"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_lock_adds_nothing_to_the_state_directory() {
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join(".branchbox");
        let lock = lock_state_dir(&state_dir, LOCK_TIMEOUT).unwrap();
        assert_eq!(fs::read_dir(&state_dir).unwrap().count(), 0);
        drop(lock);
    }

    #[test]
    fn lock_names_a_state_directory_it_cannot_create() {
        let temp = TempDir::new().unwrap();
        let blocker = temp.path().join("file");
        fs::write(&blocker, b"x").unwrap();
        let err = lock_state_dir(&blocker.join(".branchbox"), LOCK_TIMEOUT).unwrap_err();
        assert!(matches!(err, Error::Io(_)));
        assert!(err.to_string().contains("state directory"), "{err}");
    }
}
