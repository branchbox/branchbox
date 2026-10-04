//! Host and project health checks (`branchbox doctor`, DESIGN §5.12).
//!
//! [`run`] checks the tools BranchBox drives (git, Docker, Compose, the Dev Container CLI, the
//! optional runtimes, 1Password and GitHub CLIs), the host itself, and with a repository, that
//! repository's BranchBox state. It never fails: every problem is a check with a status, a
//! cause-naming `detail` and a `remediation`.
//!
//! Tools are run through [`probe`], which kills a tool that has not finished by the deadline
//! (a wedged Docker daemon makes `docker version` hang), and the checks run in parallel, so a
//! hung tool delays the report by about one probe timeout, not one per check. A check that runs
//! a tool twice (`docker compose` then `docker-compose`, `op --version` then `op whoami`) shares
//! one probe timeout between both runs, so no check takes longer than that.

use crate::config::BranchBoxConfig;
use crate::workflows::detect;
use crate::workflows::feature::FeatureMetadata;
use crate::workflows::init::GITIGNORE_ENTRIES;
use regex::Regex;
use serde::Serialize;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

/// `schema_version` of the `doctor --json` payload.
pub const SCHEMA_VERSION: u32 = 1;

/// How long one tool may run before it is killed and reported as timed out.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// How often a running probe is polled.
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Output kept per stream.
const MAX_CAPTURE: usize = 64 * 1024;

/// What to check.
#[derive(Debug, Clone)]
pub struct DoctorOptions {
    /// Also check this repository (`repo.*` checks).
    pub repo: Option<PathBuf>,
    /// Deadline for each tool invocation.
    pub probe_timeout: Duration,
    /// Also check that `op` and `gh` are signed in (otherwise they are presence-only).
    pub check_auth: bool,
}

impl Default for DoctorOptions {
    fn default() -> Self {
        Self {
            repo: None,
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            check_auth: false,
        }
    }
}

/// A check's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warn,
    Error,
    /// Not applicable here (an optional tool that is not installed, a platform it does not
    /// support, or a repository check whose prerequisite failed).
    Skipped,
}

/// One check of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    /// Stable id, e.g. `docker.daemon`.
    pub id: &'static str,
    pub title: &'static str,
    /// Whether an `error` makes `doctor` exit 1.
    pub required: bool,
    pub status: CheckStatus,
    pub path: Option<String>,
    pub version: Option<String>,
    pub detail: Option<String>,
    pub remediation: Option<String>,
}

impl DoctorCheck {
    fn new(id: &'static str, title: &'static str, required: bool) -> Self {
        Self {
            id,
            title,
            required,
            status: CheckStatus::Ok,
            path: None,
            version: None,
            detail: None,
            remediation: None,
        }
    }

    fn at(mut self, path: &Path) -> Self {
        self.path = Some(path.display().to_string());
        self
    }

    fn version(mut self, version: Option<String>) -> Self {
        self.version = version;
        self
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn with(mut self, status: CheckStatus, detail: impl Into<String>) -> Self {
        self.status = status;
        self.detail = Some(detail.into());
        self
    }

    fn fix(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }
}

/// The BranchBox build that produced the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CliInfo {
    pub version: &'static str,
    pub contract_version: u32,
    /// The running executable, when the OS reports it.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostInfo {
    /// `std::env::consts::OS`, e.g. `macos`.
    pub os: &'static str,
    /// `std::env::consts::ARCH`, e.g. `aarch64`.
    pub arch: &'static str,
}

/// How many checks ended in each status (skipped checks are not counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DoctorSummary {
    pub ok: usize,
    pub warn: usize,
    pub error: usize,
}

/// The `doctor --json` payload (DESIGN §5.12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub cli: CliInfo,
    pub host: HostInfo,
    pub checks: Vec<DoctorCheck>,
    pub summary: DoctorSummary,
}

impl DoctorReport {
    /// The required checks that ended in `error`; any makes `doctor` exit 1.
    pub fn failed_required(&self) -> Vec<&DoctorCheck> {
        self.checks
            .iter()
            .filter(|check| check.required && check.status == CheckStatus::Error)
            .collect()
    }
}

/// Run every check. Never fails; problems are reported as checks.
pub fn run(options: &DoctorOptions) -> DoctorReport {
    run_on(&Host::current(options), options.repo.as_deref())
}

/// Run every check against `host` (and `repo`, when given), in parallel.
fn run_on(host: &Host, repo: Option<&Path>) -> DoctorReport {
    type Job<'a> = Box<dyn FnOnce() -> Vec<DoctorCheck> + Send + 'a>;
    let mut jobs: Vec<Job<'_>> = vec![
        Box::new(|| vec![check_git(host)]),
        Box::new(|| vec![check_docker_cli(host)]),
        Box::new(|| vec![check_docker_daemon(host)]),
        Box::new(|| vec![check_docker_compose(host)]),
        Box::new(|| vec![check_devcontainer_cli(host)]),
        Box::new(|| vec![check_sbx(host)]),
        Box::new(|| vec![check_local_vm(host)]),
        Box::new(|| vec![check_op(host)]),
        Box::new(|| vec![check_gh(host)]),
        Box::new(|| vec![check_in_container(host)]),
        Box::new(|| vec![check_path_entries(host)]),
    ];
    if let Some(repo) = repo {
        jobs.push(Box::new(move || check_repo(host, repo)));
    }

    let checks: Vec<DoctorCheck> = thread::scope(|scope| {
        let handles: Vec<_> = jobs.into_iter().map(|job| scope.spawn(job)).collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });

    let count = |status| checks.iter().filter(|check| check.status == status).count();
    let summary = DoctorSummary {
        ok: count(CheckStatus::Ok),
        warn: count(CheckStatus::Warn),
        error: count(CheckStatus::Error),
    };
    DoctorReport {
        schema_version: SCHEMA_VERSION,
        cli: CliInfo {
            version: crate::VERSION,
            contract_version: crate::CONTRACT_VERSION,
            path: std::env::current_exe()
                .ok()
                .map(|path| path.display().to_string()),
        },
        host: HostInfo {
            os: host.os,
            arch: std::env::consts::ARCH,
        },
        checks,
        summary,
    }
}

/// What the checks read from the process environment, captured once per run (so a test can
/// describe a host without touching the process environment), plus the run's options.
#[derive(Debug, Clone)]
struct Host {
    /// Deadline for each tool invocation.
    timeout: Duration,
    check_auth: bool,
    /// The search path tools are looked up on (`PATH`).
    path: Option<OsString>,
    /// `BRANCHBOX_SBX_PATH`: the Docker Sandboxes CLI to use instead of `sbx` on `PATH`.
    sbx_path: Option<PathBuf>,
    /// `BRANCHBOX_LOCAL_VM_DRIVER_PATH`: the local-vm driver to use.
    local_vm_driver: Option<PathBuf>,
    /// The running executable's directory; the local-vm driver may sit beside it.
    exe_dir: Option<PathBuf>,
    /// `std::env::consts::OS`.
    os: &'static str,
    /// Why this looks like a container, if it does.
    container_marker: Option<&'static str>,
    /// `BRANCHBOX_SKIP_HOST_VALIDATION` is set.
    skip_host_validation: bool,
}

impl Host {
    fn current(options: &DoctorOptions) -> Self {
        let container_marker = if Path::new("/.dockerenv").exists() {
            Some("/.dockerenv exists")
        } else if std::env::var_os("DOCKER_CONTAINER").is_some() {
            Some("DOCKER_CONTAINER is set")
        } else {
            None
        };
        Self {
            timeout: options.probe_timeout,
            check_auth: options.check_auth,
            path: std::env::var_os("PATH"),
            sbx_path: std::env::var_os("BRANCHBOX_SBX_PATH").map(PathBuf::from),
            local_vm_driver: std::env::var_os("BRANCHBOX_LOCAL_VM_DRIVER_PATH").map(PathBuf::from),
            exe_dir: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf)),
            os: std::env::consts::OS,
            container_marker,
            skip_host_validation: std::env::var_os("BRANCHBOX_SKIP_HOST_VALIDATION").is_some(),
        }
    }

    /// `program` as found on this host's `PATH`.
    fn find(&self, program: &str) -> Option<PathBuf> {
        let path = self.path.as_ref()?;
        which::which_in(program, Some(path), Path::new("/")).ok()
    }

    fn probe(&self, program: &Path, args: &[&str]) -> ProbeOutcome {
        probe(program, args, self.timeout)
    }

    /// The deadline of a check that starts now: one probe timeout for all of its runs.
    fn check_deadline(&self) -> Instant {
        Instant::now() + self.timeout
    }

    /// Run `program` with whatever is left until `deadline`; at or past it, the run is reported
    /// as timed out without starting.
    fn probe_by(&self, program: &Path, args: &[&str], deadline: Instant) -> ProbeOutcome {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return ProbeOutcome::TimedOut(self.timeout);
        }
        probe(program, args, remaining)
    }
}

// --- probe ---------------------------------------------------------------------------------

/// What running a tool produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The tool exited on its own.
    Exited {
        success: bool,
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    /// The tool was still running at the deadline and was killed (with its process group).
    TimedOut(Duration),
    /// The tool could not be started.
    LaunchFailed(String),
}

impl ProbeOutcome {
    /// The trimmed stdout of a successful run.
    fn success_stdout(&self) -> Option<&str> {
        match self {
            ProbeOutcome::Exited {
                success: true,
                stdout,
                ..
            } => Some(stdout.trim()),
            _ => None,
        }
    }

    /// Why a run did not succeed, in a few words: the tool's first error line, the exit status,
    /// `timed out after 3s` or why it could not be started.
    fn failure(&self) -> String {
        match self {
            ProbeOutcome::Exited {
                code,
                stdout,
                stderr,
                ..
            } => first_line(stderr)
                .or_else(|| first_line(stdout))
                .map(str::to_string)
                .unwrap_or_else(|| match code {
                    Some(code) => format!("exited with status {code}"),
                    None => "terminated by a signal".to_string(),
                }),
            ProbeOutcome::TimedOut(after) => format!("timed out after {}", format_duration(*after)),
            ProbeOutcome::LaunchFailed(reason) => reason.clone(),
        }
    }
}

/// Run `program args…` with stdin closed and both outputs captured, killing it (and on Unix
/// every process it started in its process group) if it is still running after `timeout`.
///
/// Output goes to anonymous temp files rather than pipes: a background process the tool leaves
/// behind cannot hold the result back, and a chatty tool never blocks on a full pipe.
pub fn probe(program: &Path, args: &[&str], timeout: Duration) -> ProbeOutcome {
    let captures = tempfile::tempfile().and_then(|stdout| {
        let stderr = tempfile::tempfile()?;
        Ok((stdout, stderr))
    });
    let (stdout, stderr) = match captures {
        Ok(files) => files,
        Err(err) => {
            return ProbeOutcome::LaunchFailed(format!(
                "could not run {}: no temporary file for its output: {err}",
                program.display()
            ))
        }
    };
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null());
    match (stdout.try_clone(), stderr.try_clone()) {
        (Ok(out), Ok(err)) => {
            command.stdout(out).stderr(err);
        }
        (Err(err), _) | (_, Err(err)) => {
            return ProbeOutcome::LaunchFailed(format!(
                "could not run {}: no temporary file for its output: {err}",
                program.display()
            ))
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own group, so a timeout also kills what it started.
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ProbeOutcome::LaunchFailed(format!(
                "could not run {}: {err}",
                program.display()
            ))
        }
    };

    let status = match wait_with_deadline(&mut child, timeout) {
        Ok(Some(status)) => status,
        Ok(None) => return ProbeOutcome::TimedOut(timeout),
        Err(err) => {
            return ProbeOutcome::LaunchFailed(format!(
                "could not wait for {}: {err}",
                program.display()
            ))
        }
    };
    ProbeOutcome::Exited {
        success: status.success(),
        code: status.code(),
        stdout: read_capture(stdout),
        stderr: read_capture(stderr),
    }
}

/// Poll `child` until it exits or `timeout` passes; on timeout kill it and return `None`.
fn wait_with_deadline(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) if started.elapsed() >= timeout => {
                kill_process_tree(child);
                return Ok(None);
            }
            Ok(None) => thread::sleep(PROBE_POLL_INTERVAL.min(timeout)),
            Err(err) => {
                kill_process_tree(child);
                return Err(err);
            }
        }
    }
}

fn kill_process_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: kill(2) with a negative pid signals the process group the child leads (it was
        // spawned with `process_group(0)`); it has no memory-safety preconditions.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// At most [`MAX_CAPTURE`] bytes of a capture file, as (lossy) UTF-8.
fn read_capture(mut file: File) -> String {
    let mut kept = Vec::new();
    let read = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.take(MAX_CAPTURE as u64).read_to_end(&mut kept));
    if let Err(err) = read {
        tracing::debug!("Failed to read a probe's output: {err}");
    }
    String::from_utf8_lossy(&kept).into_owned()
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn format_duration(duration: Duration) -> String {
    if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

/// The first dotted version number in a tool's output (`git version 2.50.1` → `2.50.1`).
fn parse_version(text: &str) -> Option<String> {
    static VERSION: OnceLock<Regex> = OnceLock::new();
    VERSION
        .get_or_init(|| Regex::new(r"\d+\.\d+(?:\.\d+)?").expect("valid version pattern"))
        .find(text)
        .map(|found| found.as_str().to_string())
}

// --- host checks ---------------------------------------------------------------------------

const INSTALL_DOCKER: &str =
    "Install Docker Desktop (https://www.docker.com/products/docker-desktop/)";

fn check_git(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("git", "Git", true);
    let Some(git) = host.find("git") else {
        return check
            .with(CheckStatus::Error, "git is not on PATH")
            .fix("Install Git (on macOS: xcode-select --install)");
    };
    let check = check.at(&git);
    let outcome = host.probe(&git, &["--version"]);
    match outcome.success_stdout() {
        Some(stdout) => check.version(parse_version(stdout)),
        None => check
            .with(CheckStatus::Error, outcome.failure())
            .fix("Reinstall Git"),
    }
}

fn check_docker_cli(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("docker.cli", "Docker CLI", true);
    let Some(docker) = host.find("docker") else {
        return check
            .with(CheckStatus::Error, "docker is not on PATH")
            .fix(INSTALL_DOCKER);
    };
    let check = check.at(&docker);
    let outcome = host.probe(&docker, &["--version"]);
    match outcome.success_stdout() {
        Some(stdout) => check.version(parse_version(stdout)),
        None => check
            .with(CheckStatus::Error, outcome.failure())
            .fix(INSTALL_DOCKER),
    }
}

fn check_docker_daemon(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("docker.daemon", "Docker daemon", true);
    let Some(docker) = host.find("docker") else {
        return check.with(CheckStatus::Skipped, "Docker CLI not found");
    };
    let check = check.at(&docker);
    let outcome = host.probe(&docker, &["version", "--format", "{{.Server.Version}}"]);
    match outcome.success_stdout() {
        Some(version) if !version.is_empty() => check.version(Some(version.to_string())),
        Some(_) => check
            .with(
                CheckStatus::Error,
                format!(
                    "`docker version` reported no server version{}",
                    match &outcome {
                        ProbeOutcome::Exited { stderr, .. } => first_line(stderr)
                            .map(|line| format!(": {line}"))
                            .unwrap_or_default(),
                        _ => String::new(),
                    }
                ),
            )
            .fix("Start Docker Desktop (or the Docker daemon) and retry"),
        None => check
            .with(CheckStatus::Error, outcome.failure())
            .fix("Start Docker Desktop (or the Docker daemon) and retry"),
    }
}

fn check_docker_compose(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("docker.compose", "Docker Compose", true);
    let fix = "Install Docker Desktop, or the Docker Compose plugin";
    let deadline = host.check_deadline();

    let plugin_failure = match host.find("docker") {
        Some(docker) => {
            let outcome = host.probe_by(&docker, &["compose", "version", "--short"], deadline);
            if let Some(stdout) = outcome.success_stdout() {
                return check.at(&docker).version(parse_version(stdout));
            }
            format!("`docker compose` failed: {}", outcome.failure())
        }
        None => "docker is not on PATH".to_string(),
    };

    // BranchBox also drives the standalone `docker-compose`.
    let Some(standalone) = host.find("docker-compose") else {
        return check
            .with(
                CheckStatus::Error,
                format!(
                    "Docker Compose is not available: {plugin_failure}; docker-compose is not \
                     on PATH"
                ),
            )
            .fix(fix);
    };
    let outcome = host.probe_by(&standalone, &["version", "--short"], deadline);
    match outcome.success_stdout() {
        Some(stdout) => check.at(&standalone).version(parse_version(stdout)),
        None => check
            .at(&standalone)
            .with(
                CheckStatus::Error,
                format!(
                    "Docker Compose is not available: {plugin_failure}; docker-compose failed: {}",
                    outcome.failure()
                ),
            )
            .fix(fix),
    }
}

fn check_devcontainer_cli(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("devcontainer.cli", "Dev Container CLI", false);
    let fix = "npm install -g @devcontainers/cli";
    let Some(cli) = host.find("devcontainer") else {
        return check.with(CheckStatus::Warn, "Not installed").fix(fix);
    };
    let check = check.at(&cli);
    let outcome = host.probe(&cli, &["--version"]);
    match outcome.success_stdout() {
        Some(stdout) => check.version(parse_version(stdout)),
        None => check.with(CheckStatus::Warn, outcome.failure()).fix(fix),
    }
}

/// Docker Sandboxes: optional; when installed it must be signed in (`sbx ls` fails otherwise).
fn check_sbx(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("runtime.sbx", "Docker Sandboxes", false);
    let sbx = match &host.sbx_path {
        Some(path) => {
            if !path.is_file() {
                return check
                    .at(path)
                    .with(
                        CheckStatus::Warn,
                        format!(
                            "BRANCHBOX_SBX_PATH names {}, which does not exist",
                            path.display()
                        ),
                    )
                    .fix("Point BRANCHBOX_SBX_PATH at the sbx executable, or unset it");
            }
            path.clone()
        }
        None => match host.find("sbx") {
            Some(path) => path,
            None => return check.with(CheckStatus::Skipped, "sbx is not installed"),
        },
    };
    let check = check.at(&sbx);
    let outcome = host.probe(&sbx, &["ls", "--quiet"]);
    if outcome.success_stdout().is_some() {
        return check.detail("Signed in");
    }
    let failure = outcome.failure();
    let lowered = failure.to_ascii_lowercase();
    if ["auth", "sign in", "signed in", "login", "log in"]
        .iter()
        .any(|hint| lowered.contains(hint))
    {
        check
            .with(CheckStatus::Warn, format!("Not signed in: {failure}"))
            .fix("Run: sbx login")
    } else {
        check
            .with(
                CheckStatus::Warn,
                format!("Docker Sandboxes is unavailable: {failure}"),
            )
            .fix("Check Docker Desktop's Docker Sandboxes setup, or use --runtime container")
    }
}

/// The Firecracker `local-vm` runtime: Linux only, and only when its driver is installed.
fn check_local_vm(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("runtime.local_vm", "Local VM (Firecracker)", false);
    if host.os != "linux" {
        return check.with(CheckStatus::Skipped, "local-vm requires a Linux host");
    }
    let Some(driver) = local_vm_driver(host) else {
        return check.with(CheckStatus::Skipped, "branchbox-local-vm is not installed");
    };
    let check = check.at(&driver);
    let outcome = host.probe(&driver, &["validate"]);
    match outcome.success_stdout() {
        Some(_) => check,
        None => check
            .with(
                CheckStatus::Warn,
                format!("local-vm preflight failed: {}", outcome.failure()),
            )
            .fix("Run: branchbox-local-vm validate"),
    }
}

/// Where the local-vm runtime looks for its driver: `BRANCHBOX_LOCAL_VM_DRIVER_PATH`, beside the
/// BranchBox executable, then `PATH`.
fn local_vm_driver(host: &Host) -> Option<PathBuf> {
    if let Some(path) = &host.local_vm_driver {
        return Some(path.clone());
    }
    host.exe_dir
        .as_ref()
        .map(|dir| dir.join("branchbox-local-vm"))
        .filter(|adjacent| adjacent.is_file())
        .or_else(|| host.find("branchbox-local-vm"))
}

/// An optional CLI that may need a sign-in: presence and version, plus the sign-in with
/// `check_auth`.
struct SignInTool {
    id: &'static str,
    title: &'static str,
    program: &'static str,
    install: &'static str,
    status_args: &'static [&'static str],
    sign_in: &'static str,
}

fn check_signed_in_tool(host: &Host, tool: &SignInTool) -> DoctorCheck {
    let check = DoctorCheck::new(tool.id, tool.title, false);
    let Some(path) = host.find(tool.program) else {
        return check
            .with(
                CheckStatus::Skipped,
                format!("{} is not installed", tool.program),
            )
            .fix(tool.install);
    };
    let check = check.at(&path);
    let deadline = host.check_deadline();
    let outcome = host.probe_by(&path, &["--version"], deadline);
    let Some(stdout) = outcome.success_stdout() else {
        return check
            .with(CheckStatus::Warn, outcome.failure())
            .fix(tool.install);
    };
    let check = check.version(parse_version(stdout));
    if !host.check_auth {
        return check.detail("Sign-in not checked (run doctor with --check-auth)");
    }
    let outcome = host.probe_by(&path, tool.status_args, deadline);
    if outcome.success_stdout().is_some() {
        check.detail("Signed in")
    } else {
        check
            .with(
                CheckStatus::Warn,
                format!("Not signed in: {}", outcome.failure()),
            )
            .fix(tool.sign_in)
    }
}

fn check_op(host: &Host) -> DoctorCheck {
    check_signed_in_tool(
        host,
        &SignInTool {
            id: "op",
            title: "1Password CLI",
            program: "op",
            install: "Install the 1Password CLI (brew install 1password-cli); needed only for \
                      1Password secret references",
            status_args: &["whoami"],
            sign_in: "Run: op signin",
        },
    )
}

fn check_gh(host: &Host) -> DoctorCheck {
    check_signed_in_tool(
        host,
        &SignInTool {
            id: "gh",
            title: "GitHub CLI",
            program: "gh",
            install: "Install the GitHub CLI (brew install gh)",
            status_args: &["auth", "status"],
            sign_in: "Run: gh auth login",
        },
    )
}

/// BranchBox drives worktrees and containers from the host; inside a container (the rule
/// `feature start` enforces) it refuses unless `BRANCHBOX_SKIP_HOST_VALIDATION` is set.
fn check_in_container(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("host.in_container", "Running on the host", true);
    match host.container_marker {
        None => check,
        Some(marker) if host.skip_host_validation => check.with(
            CheckStatus::Warn,
            format!(
                "Running inside a container ({marker}); host validation is skipped because \
                 BRANCHBOX_SKIP_HOST_VALIDATION is set"
            ),
        ),
        Some(marker) => check
            .with(
                CheckStatus::Error,
                format!(
                    "Running inside a container ({marker}); BranchBox commands must run on the \
                     host machine"
                ),
            )
            .fix("Run BranchBox on the host"),
    }
}

/// The search path tools are found on. A GUI-launched process often has a minimal `PATH`.
fn check_path_entries(host: &Host) -> DoctorCheck {
    let check = DoctorCheck::new("env.path_entries", "PATH", false);
    let entries: Vec<String> = host
        .path
        .as_ref()
        .map(|path| {
            std::env::split_paths(path)
                .filter(|entry| !entry.as_os_str().is_empty())
                .map(|entry| entry.display().to_string())
                .collect()
        })
        .unwrap_or_default();
    if entries.is_empty() {
        return check
            .with(CheckStatus::Warn, "PATH is empty, so no tools can be found")
            .fix("Run BranchBox from a login shell, or set PATH");
    }
    check.detail(format!("{} entries: {}", entries.len(), entries.join(":")))
}

// --- repository checks ---------------------------------------------------------------------

fn check_repo(host: &Host, repo: &Path) -> Vec<DoctorCheck> {
    let repo = std::path::absolute(repo).unwrap_or_else(|_| repo.to_path_buf());
    let (git_check, root) = check_repo_git(host, &repo);
    let Some(root) = root else {
        let reason = format!("{} is not a git repository", repo.display());
        let skipped = |id, title, required| {
            DoctorCheck::new(id, title, required).with(CheckStatus::Skipped, reason.clone())
        };
        return vec![
            git_check,
            skipped("repo.initialized", "BranchBox set up", true),
            skipped("repo.config", "Project config", true),
            skipped("repo.registry", "Feature registry", true),
            skipped("repo.gitignore", "Git ignore rules", false),
        ];
    };
    let initialized = detect::is_initialized(&root);
    vec![
        git_check,
        check_repo_initialized(&root, initialized),
        check_repo_config(&root),
        check_repo_registry(&root, initialized),
        check_repo_gitignore(&root, initialized),
    ]
}

/// `repo.git`, and the main worktree the other checks inspect.
fn check_repo_git(host: &Host, repo: &Path) -> (DoctorCheck, Option<PathBuf>) {
    let check = DoctorCheck::new("repo.git", "Git repository", true).at(repo);
    if !repo.is_dir() {
        return (
            check
                .with(
                    CheckStatus::Error,
                    format!("{} is not a directory", repo.display()),
                )
                .fix("Pass the folder of a git repository"),
            None,
        );
    }
    let Some(git) = host.find("git") else {
        return (
            check
                .with(CheckStatus::Error, "git is not on PATH")
                .fix("Install Git (on macOS: xcode-select --install)"),
            None,
        );
    };
    let repo_arg = repo.to_string_lossy();
    let mut args = vec!["-C", repo_arg.as_ref()];
    args.extend_from_slice(detect::MAIN_WORKTREE_QUERY);
    let outcome = host.probe(&git, &args);
    let root = outcome
        .success_stdout()
        .and_then(|stdout| detect::parse_main_worktree_root(repo, stdout));
    if let Some(root) = root {
        return (check.at(&root), Some(root));
    }
    let reason = match outcome.success_stdout() {
        Some(_) => "not inside a work tree".to_string(),
        None => outcome.failure(),
    };
    (
        check
            .with(
                CheckStatus::Error,
                format!("Not a git repository: {} ({reason})", repo.display()),
            )
            .fix("Pass the folder of a git repository"),
        None,
    )
}

fn check_repo_initialized(root: &Path, initialized: bool) -> DoctorCheck {
    let check = DoctorCheck::new("repo.initialized", "BranchBox set up", true).at(root);
    if initialized {
        return check;
    }
    check
        .with(
            CheckStatus::Warn,
            format!(
                "BranchBox is not set up in {} (no .branchbox/registry.json)",
                root.display()
            ),
        )
        .fix("Run: branchbox init")
}

fn check_repo_config(root: &Path) -> DoctorCheck {
    let path = BranchBoxConfig::path(root);
    let check = DoctorCheck::new("repo.config", "Project config", true).at(&path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return check.detail("No config.json; defaults apply");
        }
        Err(err) => {
            return check
                .with(
                    CheckStatus::Error,
                    format!("Cannot read {}: {err}", path.display()),
                )
                .fix("Fix the file's permissions");
        }
    };
    match serde_json::from_str::<BranchBoxConfig>(&text) {
        Ok(_) => check,
        Err(err) => check
            .with(
                CheckStatus::Error,
                format!(
                    "{} is invalid (line {}, column {}): {err}",
                    path.display(),
                    err.line(),
                    err.column()
                ),
            )
            .fix("Fix .branchbox/config.json (strict JSON; comments are not supported)"),
    }
}

fn check_repo_registry(root: &Path, initialized: bool) -> DoctorCheck {
    let path = root.join(".branchbox/registry.json");
    let check = DoctorCheck::new("repo.registry", "Feature registry", true).at(&path);
    if !initialized {
        return check.with(CheckStatus::Skipped, "No registry yet");
    }
    let parsed = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read it: {err}"))
        .and_then(|text| {
            serde_json::from_str::<serde_json::Value>(&text)
                .map_err(|err| format!("it is not valid JSON: {err}"))
        })
        .and_then(|mut registry| match registry.get_mut("features") {
            Some(features) => serde_json::from_value::<Vec<FeatureMetadata>>(features.take())
                .map_err(|err| format!("a feature entry is invalid: {err}")),
            None => Err("it has no `features` list".to_string()),
        });
    match parsed {
        Ok(features) => check.detail(format!("{} registered feature(s)", features.len())),
        Err(reason) => check
            .with(
                CheckStatus::Error,
                format!(
                    "{} is unreadable: {reason}; every feature of this project is hidden until it \
                     is repaired",
                    path.display()
                ),
            )
            .fix("Restore .branchbox/registry.json from a backup or version control history"),
    }
}

fn check_repo_gitignore(root: &Path, initialized: bool) -> DoctorCheck {
    let path = root.join(".gitignore");
    let check = DoctorCheck::new("repo.gitignore", "Git ignore rules", false).at(&path);
    if !initialized {
        return check.with(CheckStatus::Skipped, "BranchBox is not set up here yet");
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let missing: Vec<&str> = GITIGNORE_ENTRIES
        .iter()
        .copied()
        .filter(|entry| !content.lines().any(|line| line.trim() == *entry))
        .collect();
    if missing.is_empty() {
        return check;
    }
    check
        .with(
            CheckStatus::Warn,
            format!(
                "Missing BranchBox entries, so secrets or generated files could be committed: {}",
                missing.join(", ")
            ),
        )
        .fix("Run: branchbox init --update (it adds them), or add them to .gitignore")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn probe_captures_output_and_status() {
        let temp = TempDir::new().unwrap();
        let tool = script(temp.path(), "tool", "echo out; echo err >&2; exit 3");
        assert_eq!(
            probe(&tool, &[], Duration::from_secs(5)),
            ProbeOutcome::Exited {
                success: false,
                code: Some(3),
                stdout: "out\n".to_string(),
                stderr: "err\n".to_string(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn probe_kills_a_hung_tool_and_its_children_at_the_deadline() {
        let temp = TempDir::new().unwrap();
        let tool = script(temp.path(), "hang", "sleep 30");
        let started = Instant::now();
        let outcome = probe(&tool, &[], Duration::from_millis(200));
        assert_eq!(outcome, ProbeOutcome::TimedOut(Duration::from_millis(200)));
        assert_eq!(outcome.failure(), "timed out after 200ms");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn probe_reports_a_missing_program() {
        let missing = Path::new("/nonexistent/branchbox-probe");
        let outcome = probe(missing, &[], Duration::from_secs(1));
        let ProbeOutcome::LaunchFailed(reason) = &outcome else {
            panic!("{outcome:?}");
        };
        assert!(reason.contains("/nonexistent/branchbox-probe"), "{reason}");
        assert_eq!(&outcome.failure(), reason);
    }

    #[cfg(unix)]
    #[test]
    fn probe_keeps_a_bounded_amount_of_output() {
        let temp = TempDir::new().unwrap();
        let tool = script(temp.path(), "chatty", "head -c 200000 /dev/zero");
        match probe(&tool, &[], Duration::from_secs(10)) {
            ProbeOutcome::Exited {
                success, stdout, ..
            } => {
                assert!(success);
                assert_eq!(stdout.len(), MAX_CAPTURE);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn failures_name_their_cause() {
        let exited = |code, stdout: &str, stderr: &str| ProbeOutcome::Exited {
            success: false,
            code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        };
        assert_eq!(exited(Some(1), "", "\n  boom \nmore").failure(), "boom");
        assert_eq!(exited(Some(1), "only stdout", "").failure(), "only stdout");
        assert_eq!(exited(Some(2), "", "").failure(), "exited with status 2");
        assert_eq!(exited(None, "", "").failure(), "terminated by a signal");
        assert_eq!(
            ProbeOutcome::TimedOut(Duration::from_secs(3)).failure(),
            "timed out after 3s"
        );
    }

    #[test]
    fn versions_are_the_first_dotted_number() {
        assert_eq!(
            parse_version("git version 2.50.1 (Apple Git-155)").as_deref(),
            Some("2.50.1")
        );
        assert_eq!(
            parse_version("Docker version 28.3.2, build 578ccf6").as_deref(),
            Some("28.3.2")
        );
        assert_eq!(parse_version("v2.38").as_deref(), Some("2.38"));
        assert_eq!(parse_version("no version"), None);
    }

    /// A host whose `PATH` is exactly `bin` (or empty), on `os`, outside any container.
    fn fake_host(bin: Option<&Path>) -> Host {
        Host {
            timeout: Duration::from_secs(5),
            check_auth: false,
            path: bin.map(|bin| bin.as_os_str().to_os_string()),
            sbx_path: None,
            local_vm_driver: None,
            exe_dir: None,
            os: "macos",
            container_marker: None,
            skip_host_validation: false,
        }
    }

    /// The test process's own `PATH`, for checks that need the real git.
    fn real_host() -> Host {
        Host {
            path: std::env::var_os("PATH"),
            ..fake_host(None)
        }
    }

    fn status_of(check: &DoctorCheck) -> (CheckStatus, &str) {
        (check.status, check.detail.as_deref().unwrap_or(""))
    }

    /// A check that runs a tool twice shares one probe timeout: a slow `--version` leaves only
    /// the rest of it for the sign-in probe, so the check never takes two timeouts.
    #[cfg(unix)]
    #[test]
    fn a_two_step_check_shares_one_probe_timeout() {
        let temp = TempDir::new().unwrap();
        script(
            temp.path(),
            "gh",
            "case \"$1\" in --version) sleep 0.6; echo 'gh version 2.60.0' ;; *) sleep 30 ;; esac",
        );
        let host = Host {
            timeout: Duration::from_secs(1),
            check_auth: true,
            ..fake_host(Some(temp.path()))
        };
        let started = Instant::now();
        let gh = check_gh(&host);
        let elapsed = started.elapsed();
        assert_eq!(gh.status, CheckStatus::Warn, "{gh:?}");
        assert!(
            gh.detail.as_deref().unwrap_or("").contains("timed out"),
            "{gh:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1800),
            "the check took {elapsed:?}, more than one 1 s probe timeout"
        );

        let past = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            host.probe_by(&temp.path().join("gh"), &["--version"], past),
            ProbeOutcome::TimedOut(Duration::from_secs(1))
        );
    }

    #[test]
    fn options_default_to_a_three_second_probe_and_the_current_host() {
        let options = DoctorOptions::default();
        assert_eq!(options.probe_timeout, Duration::from_secs(3));
        assert!(options.repo.is_none() && !options.check_auth);
        let host = Host::current(&options);
        assert_eq!(host.timeout, DEFAULT_PROBE_TIMEOUT);
        assert_eq!(host.os, std::env::consts::OS);
        assert_eq!(host.path, std::env::var_os("PATH"));
    }

    #[test]
    fn report_counts_statuses_and_finds_failed_required_checks() {
        let report = run_on(&fake_host(None), None);
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.cli.contract_version, crate::CONTRACT_VERSION);
        assert_eq!(report.host.os, "macos");
        let ids: Vec<&str> = report.checks.iter().map(|check| check.id).collect();
        assert_eq!(
            ids,
            [
                "git",
                "docker.cli",
                "docker.daemon",
                "docker.compose",
                "devcontainer.cli",
                "runtime.sbx",
                "runtime.local_vm",
                "op",
                "gh",
                "host.in_container",
                "env.path_entries",
            ]
        );
        // Nothing is on an empty PATH: git and Docker are required errors.
        let failed: Vec<&str> = report.failed_required().iter().map(|c| c.id).collect();
        assert_eq!(failed, ["git", "docker.cli", "docker.compose"]);
        assert_eq!(
            report.summary,
            DoctorSummary {
                ok: 1,
                warn: 2,
                error: 3
            }
        );
        let skipped = report
            .checks
            .iter()
            .filter(|check| check.status == CheckStatus::Skipped)
            .count();
        assert_eq!(3 + 2 + 1 + skipped, report.checks.len());
    }

    #[test]
    fn an_empty_path_names_every_missing_tool() {
        let host = fake_host(None);
        assert_eq!(
            status_of(&check_git(&host)),
            (CheckStatus::Error, "git is not on PATH")
        );
        assert_eq!(
            status_of(&check_docker_cli(&host)),
            (CheckStatus::Error, "docker is not on PATH")
        );
        assert_eq!(
            status_of(&check_docker_daemon(&host)),
            (CheckStatus::Skipped, "Docker CLI not found")
        );
        assert_eq!(
            status_of(&check_docker_compose(&host)),
            (
                CheckStatus::Error,
                "Docker Compose is not available: docker is not on PATH; docker-compose is not \
                 on PATH"
            )
        );
        assert_eq!(
            status_of(&check_devcontainer_cli(&host)),
            (CheckStatus::Warn, "Not installed")
        );
        assert_eq!(
            status_of(&check_sbx(&host)),
            (CheckStatus::Skipped, "sbx is not installed")
        );
        let path = check_path_entries(&host);
        assert_eq!(path.status, CheckStatus::Warn);
        assert!(!path.required);
        let (git_check, root) = check_repo_git(&host, Path::new("/"));
        assert_eq!(
            status_of(&git_check),
            (CheckStatus::Error, "git is not on PATH")
        );
        assert!(root.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn broken_tools_report_their_own_error() {
        let temp = TempDir::new().unwrap();
        let bin = temp.path();
        script(bin, "git", "echo 'git: broken install' >&2; exit 1");
        script(
            bin,
            "docker",
            r#"case "$1" in
  --version) exit 2 ;;
  version) echo "context deadline exceeded" >&2 ;;
  compose) echo "docker: 'compose' is not a docker command." >&2; exit 1 ;;
esac"#,
        );
        script(
            bin,
            "docker-compose",
            "echo 'docker-compose version 1.29.2'",
        );
        script(bin, "devcontainer", "exit 1");
        script(bin, "sbx", "echo 'daemon unreachable' >&2; exit 1");
        script(bin, "gh", "exit 5");
        let host = fake_host(Some(bin));

        assert_eq!(
            status_of(&check_git(&host)),
            (CheckStatus::Error, "git: broken install")
        );
        assert_eq!(
            status_of(&check_docker_cli(&host)),
            (CheckStatus::Error, "exited with status 2")
        );
        assert_eq!(
            status_of(&check_docker_daemon(&host)),
            (
                CheckStatus::Error,
                "`docker version` reported no server version: context deadline exceeded"
            )
        );
        let compose = check_docker_compose(&host);
        assert_eq!(compose.status, CheckStatus::Ok);
        assert_eq!(compose.version.as_deref(), Some("1.29.2"));
        assert!(compose.path.unwrap().ends_with("docker-compose"));
        assert_eq!(
            status_of(&check_devcontainer_cli(&host)),
            (CheckStatus::Warn, "exited with status 1")
        );
        let sbx = check_sbx(&host);
        assert_eq!(
            status_of(&sbx),
            (
                CheckStatus::Warn,
                "Docker Sandboxes is unavailable: daemon unreachable"
            )
        );
        assert_eq!(
            status_of(&check_gh(&host)),
            (CheckStatus::Warn, "exited with status 5")
        );

        script(bin, "docker-compose", "echo 'no compose here' >&2; exit 1");
        let compose = check_docker_compose(&host);
        assert_eq!(compose.status, CheckStatus::Error);
        assert_eq!(
            compose.detail.as_deref(),
            Some(
                "Docker Compose is not available: `docker compose` failed: docker: 'compose' \
                 is not a docker command.; docker-compose failed: no compose here"
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn working_optional_tools_report_their_version() {
        let temp = TempDir::new().unwrap();
        let bin = temp.path();
        script(bin, "devcontainer", "echo 0.80.3");
        let sbx = script(bin, "sandboxes", "exit 0");
        let host = Host {
            sbx_path: Some(sbx),
            ..fake_host(Some(bin))
        };
        let devcontainer = check_devcontainer_cli(&host);
        assert_eq!(devcontainer.status, CheckStatus::Ok);
        assert_eq!(devcontainer.version.as_deref(), Some("0.80.3"));
        let sbx = check_sbx(&host);
        assert_eq!(status_of(&sbx), (CheckStatus::Ok, "Signed in"));
        assert!(sbx.path.unwrap().ends_with("sandboxes"));

        let missing = Host {
            sbx_path: Some(bin.join("missing-sbx")),
            ..fake_host(Some(bin))
        };
        let sbx = check_sbx(&missing);
        assert_eq!(sbx.status, CheckStatus::Warn);
        assert!(sbx.detail.unwrap().contains("which does not exist"));
    }

    #[cfg(unix)]
    #[test]
    fn local_vm_needs_linux_and_its_driver() {
        let temp = TempDir::new().unwrap();
        let bin = temp.path();
        assert_eq!(
            status_of(&check_local_vm(&fake_host(Some(bin)))),
            (CheckStatus::Skipped, "local-vm requires a Linux host")
        );

        let linux = Host {
            os: "linux",
            ..fake_host(Some(bin))
        };
        assert_eq!(
            status_of(&check_local_vm(&linux)),
            (CheckStatus::Skipped, "branchbox-local-vm is not installed")
        );

        script(bin, "branchbox-local-vm", "exit 0");
        let on_path = check_local_vm(&linux);
        assert_eq!(on_path.status, CheckStatus::Ok);
        assert_eq!(
            on_path.path,
            Some(bin.join("branchbox-local-vm").display().to_string())
        );

        let beside = Host {
            path: None,
            exe_dir: Some(bin.to_path_buf()),
            ..linux.clone()
        };
        assert_eq!(check_local_vm(&beside).status, CheckStatus::Ok);

        let failing = script(bin, "driver", "echo 'KVM is not available' >&2; exit 1");
        let configured = Host {
            local_vm_driver: Some(failing),
            ..linux
        };
        assert_eq!(
            status_of(&check_local_vm(&configured)),
            (
                CheckStatus::Warn,
                "local-vm preflight failed: KVM is not available"
            )
        );
    }

    #[test]
    fn containers_are_an_error_unless_host_validation_is_skipped() {
        let host = fake_host(None);
        assert_eq!(check_in_container(&host).status, CheckStatus::Ok);

        let inside = Host {
            container_marker: Some("/.dockerenv exists"),
            ..host
        };
        let check = check_in_container(&inside);
        assert_eq!(check.status, CheckStatus::Error);
        assert!(check.required);
        assert!(check.detail.unwrap().contains("(/.dockerenv exists)"));

        let skipped = Host {
            skip_host_validation: true,
            ..inside
        };
        let check = check_in_container(&skipped);
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check
            .detail
            .unwrap()
            .contains("BRANCHBOX_SKIP_HOST_VALIDATION"));
    }

    #[test]
    fn path_entries_are_listed() {
        let host = Host {
            path: Some(OsString::from("/opt/bin::/usr/bin")),
            ..fake_host(None)
        };
        assert_eq!(
            status_of(&check_path_entries(&host)),
            (CheckStatus::Ok, "2 entries: /opt/bin:/usr/bin")
        );
    }

    fn init_repo(dir: &Path) {
        let status = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn repo_checks(repo: &Path) -> Vec<DoctorCheck> {
        check_repo(&real_host(), repo)
    }

    #[test]
    fn repo_checks_on_a_folder_that_is_not_a_repository() {
        let temp = TempDir::new().unwrap();
        let checks = repo_checks(temp.path());
        assert_eq!(checks[0].id, "repo.git");
        assert_eq!(checks[0].status, CheckStatus::Error);
        assert!(checks[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("Not a git repository"));
        assert!(checks[1..]
            .iter()
            .all(|check| check.status == CheckStatus::Skipped));

        let missing = repo_checks(&temp.path().join("nope"));
        assert_eq!(missing[0].status, CheckStatus::Error);
        assert!(missing[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("not a directory"));
    }

    #[test]
    fn repo_checks_on_an_uninitialized_repository() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        let checks = repo_checks(temp.path());
        let status: Vec<(&str, CheckStatus)> = checks
            .iter()
            .map(|check| (check.id, check.status))
            .collect();
        assert_eq!(
            status,
            [
                ("repo.git", CheckStatus::Ok),
                ("repo.initialized", CheckStatus::Warn),
                ("repo.config", CheckStatus::Ok),
                ("repo.registry", CheckStatus::Skipped),
                ("repo.gitignore", CheckStatus::Skipped),
            ]
        );
        assert_eq!(
            checks[1].remediation.as_deref(),
            Some("Run: branchbox init")
        );
    }

    #[test]
    fn repo_checks_on_an_initialized_repository() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        fs::write(
            temp.path().join(".branchbox/registry.json"),
            r#"{"version":"1","features":[]}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".gitignore"),
            GITIGNORE_ENTRIES.join("\n") + "\n",
        )
        .unwrap();
        fs::write(
            temp.path().join(".branchbox/config.json"),
            r#"{"version":"1","feature":{"branch_prefix":"spike"}}"#,
        )
        .unwrap();

        let checks = repo_checks(temp.path());
        assert!(
            checks.iter().all(|check| check.status == CheckStatus::Ok),
            "{checks:#?}"
        );
        assert_eq!(checks[3].detail.as_deref(), Some("0 registered feature(s)"));
    }

    #[test]
    fn repo_checks_report_broken_state() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        fs::create_dir_all(temp.path().join(".branchbox")).unwrap();
        fs::write(
            temp.path().join(".branchbox/registry.json"),
            "{\"features\": [",
        )
        .unwrap();
        fs::write(
            temp.path().join(".branchbox/config.json"),
            "{\n  // note\n}\n",
        )
        .unwrap();
        fs::write(temp.path().join(".gitignore"), ".env\n").unwrap();

        let checks = repo_checks(temp.path());
        let config = &checks[2];
        assert_eq!(config.status, CheckStatus::Error);
        assert!(
            config.detail.as_deref().unwrap().contains("line 2"),
            "{config:?}"
        );
        let registry = &checks[3];
        assert_eq!(registry.status, CheckStatus::Error);
        assert!(registry
            .detail
            .as_deref()
            .unwrap()
            .contains("not valid JSON"));
        let gitignore = &checks[4];
        assert_eq!(gitignore.status, CheckStatus::Warn);
        let detail = gitignore.detail.as_deref().unwrap();
        assert!(detail.contains(".branchbox/secure/"), "{detail}");
        let listed: Vec<&str> = detail.rsplit(": ").next().unwrap().split(", ").collect();
        assert!(
            !listed.contains(&".env"),
            "present entries are not listed: {detail}"
        );

        fs::write(temp.path().join(".branchbox/registry.json"), "{}").unwrap();
        let registry = &repo_checks(temp.path())[3];
        assert!(registry
            .detail
            .as_deref()
            .unwrap()
            .contains("no `features` list"));
        fs::write(
            temp.path().join(".branchbox/registry.json"),
            r#"{"features":[{"work_feature":1}]}"#,
        )
        .unwrap();
        let registry = &repo_checks(temp.path())[3];
        assert!(registry
            .detail
            .as_deref()
            .unwrap()
            .contains("feature entry is invalid"));
    }
}
