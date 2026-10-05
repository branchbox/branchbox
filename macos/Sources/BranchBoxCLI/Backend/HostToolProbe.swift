import BranchBoxKit
import Foundation

/// Finds a tool on a child environment's PATH, searched here rather than by a shell. The path is kept as found
/// (not symlink-resolved), like the CLI's own.
enum ExecutableSearch {
    static func find(_ name: String, path: String?, fileSystem: any FileSystemProbing) -> String? {
        for directory in (path ?? "").split(separator: ":").map(String.init) where directory.hasPrefix("/") {
            let candidate = Paths.join(directory, name)
            if fileSystem.kind(at: candidate) == .file(executable: true) { return candidate }
        }
        return nil
    }
}

/// The app-side doctor (DESIGN §6.2 doctor): the whole report on legacy CLIs, merged with `doctor --json` on contract
/// ones. Each probe runs a tool found on the child PATH with a 5 s limit; all run concurrently.
///
/// | id | probe | required |
/// |---|---|---|
/// | `git` | `git --version` | yes |
/// | `docker.cli` | `docker --version` | yes |
/// | `docker.daemon` | `docker info --format {{.ServerVersion}}` | yes |
/// | `docker.compose` | `docker compose version` | no |
/// | `devcontainer.cli` | `devcontainer --version` | no |
/// | `runtime.sbx` | `BRANCHBOX_SBX_PATH` or PATH, then `sbx ls --quiet` | no |
/// | `op` | `op --version` | no |
/// | `gh` | `gh --version` | no |
public struct HostToolProbe: Sendable {
    public static let probeTimeout: Duration = .seconds(5)
    public static let checkOrder = ["git", "docker.cli", "docker.daemon", "docker.compose", "devcontainer.cli",
                                    "runtime.sbx", "op", "gh"]

    let runner: any ProcessRunning
    let environment: [String: String]
    let fileSystem: any FileSystemProbing
    let timeout: Duration

    public init(runner: any ProcessRunning, environment: [String: String],
                fileSystem: any FileSystemProbing = LocalFileSystem(), timeout: Duration = HostToolProbe.probeTimeout) {
        self.runner = runner
        self.environment = environment
        self.fileSystem = fileSystem
        self.timeout = timeout
    }

    public func checks() async -> [DoctorCheck] {
        let docker = locate("docker")
        let checks = await withTaskGroup(of: DoctorCheck.self, returning: [DoctorCheck].self) { group in
            group.addTask { await self.versionCheck(id: "git", title: "Git", tool: "git", required: true,
                                                    remediation: "Install the Xcode Command Line Tools: xcode-select --install") }
            group.addTask { await self.versionCheck(id: "docker.cli", title: "Docker CLI", tool: "docker", required: true,
                                                    remediation: "Install Docker Desktop") }
            group.addTask { await self.dockerDaemon(docker) }
            group.addTask { await self.dockerCompose(docker) }
            group.addTask { await self.versionCheck(id: "devcontainer.cli", title: "Dev Container CLI",
                                                    tool: "devcontainer", required: false,
                                                    remediation: "npm install -g @devcontainers/cli") }
            group.addTask { await self.sandboxes() }
            group.addTask { await self.versionCheck(id: "op", title: "1Password CLI", tool: "op", required: false,
                                                    remediation: "Install the 1Password CLI", missingStatus: .skipped) }
            group.addTask { await self.versionCheck(id: "gh", title: "GitHub CLI", tool: "gh", required: false,
                                                    remediation: "Install the GitHub CLI: brew install gh",
                                                    missingStatus: .skipped) }
            var checks: [DoctorCheck] = []
            for await check in group { checks.append(check) }
            return checks
        }
        return checks.sorted { (Self.checkOrder.firstIndex(of: $0.id) ?? .max) < (Self.checkOrder.firstIndex(of: $1.id) ?? .max) }
    }

    // MARK: - Probes

    private func versionCheck(id: String, title: String, tool: String, required: Bool, remediation: String,
                              missingStatus: DoctorCheck.Status? = nil) async -> DoctorCheck {
        guard let path = locate(tool) else {
            return DoctorCheck(id: id, title: title, required: required,
                               status: missingStatus ?? (required ? .error : .warn),
                               detail: "\(tool) was not found on PATH", remediation: remediation)
        }
        switch await probe(path, ["--version"]) {
        case .success(let output):
            return DoctorCheck(id: id, title: title, required: required, status: .ok, path: path,
                               version: Self.version(in: output))
        case .failure(let detail):
            return DoctorCheck(id: id, title: title, required: required, status: required ? .error : .warn, path: path,
                               detail: detail, remediation: remediation)
        }
    }

    private func dockerDaemon(_ docker: String?) async -> DoctorCheck {
        let title = "Docker daemon"
        guard let docker else {
            return DoctorCheck(id: "docker.daemon", title: title, required: true, status: .skipped,
                               detail: "Needs the Docker CLI", remediation: "Install Docker Desktop")
        }
        switch await probe(docker, ["info", "--format", "{{.ServerVersion}}"]) {
        case .success(let output):
            return DoctorCheck(id: "docker.daemon", title: title, required: true, status: .ok, path: docker,
                               version: output.trimmingCharacters(in: .whitespacesAndNewlines))
        case .failure(let detail):
            return DoctorCheck(id: "docker.daemon", title: title, required: true, status: .error, path: docker,
                               detail: detail, remediation: "Start Docker Desktop")
        }
    }

    private func dockerCompose(_ docker: String?) async -> DoctorCheck {
        let title = "Docker Compose"
        guard let docker else {
            return DoctorCheck(id: "docker.compose", title: title, required: false, status: .skipped,
                               detail: "Needs the Docker CLI")
        }
        switch await probe(docker, ["compose", "version"]) {
        case .success(let output):
            return DoctorCheck(id: "docker.compose", title: title, required: false, status: .ok, path: docker,
                               version: Self.version(in: output))
        case .failure(let detail):
            return DoctorCheck(id: "docker.compose", title: title, required: false, status: .warn, path: docker,
                               detail: detail, remediation: "Update Docker Desktop (it ships the compose plugin)")
        }
    }

    private func sandboxes() async -> DoctorCheck {
        let title = "Docker Sandboxes (sbx)"
        let configured = environment["BRANCHBOX_SBX_PATH"].flatMap { $0.isEmpty ? nil : $0 }
        guard let sbx = configured ?? locate("sbx") else {
            return DoctorCheck(id: "runtime.sbx", title: title, required: false, status: .skipped,
                               detail: "sbx is not installed; the sbx runtime is unavailable")
        }
        switch await probe(sbx, ["ls", "--quiet"]) {
        case .success:
            return DoctorCheck(id: "runtime.sbx", title: title, required: false, status: .ok, path: sbx)
        case .failure(let detail):
            let lowered = detail.lowercased()
            let signedOut = ["sbx login", "not authenticated", "unauthorized", "401"].contains { lowered.contains($0) }
            let remediation = signedOut ? "Sign in with: sbx login" : nil
            return DoctorCheck(id: "runtime.sbx", title: title, required: false, status: .warn, path: sbx,
                               detail: detail, remediation: remediation)
        }
    }

    // MARK: - Running

    private enum ProbeResult {
        case success(String)
        case failure(String)
    }

    private func locate(_ tool: String) -> String? {
        ExecutableSearch.find(tool, path: environment["PATH"], fileSystem: fileSystem)
    }

    private func probe(_ executable: String, _ arguments: [String]) async -> ProbeResult {
        var options = ToolInvocation.Options(operation: "\((executable as NSString).lastPathComponent) "
                                                 + arguments.joined(separator: " "), timeout: timeout)
        options.interruptGrace = .seconds(1)
        options.terminateGrace = .seconds(1)
        options.stdoutLimit = 1 << 20
        let invocation = ToolInvocation(runner: runner, environment: environment)
        do {
            let result = try await invocation.run(URL(fileURLWithPath: executable), arguments, options)
            let stdout = String(decoding: result.stdout, as: UTF8.self)
            guard result.termination == .exited(0) else {
                let lines = (result.stderrTail + stdout.split(whereSeparator: \.isNewline).map(String.init))
                    .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
                let status: String
                switch result.termination {
                case .exited(let code): status = "exited with status \(code)"
                case .signaled(let signal): status = "was terminated by signal \(signal)"
                }
                return .failure(lines.first { $0.lowercased().contains("error") } ?? lines.last ?? status)
            }
            return .success(stdout)
        } catch BackendError.timedOut(_, let after, _) {
            return .failure("timed out after \(ToolInvocation.describe(after))")
        } catch BackendError.launchFailed(_, let reason) {
            return .failure(reason)
        } catch {
            return .failure(String(describing: BackendError.normalize(error)))
        }
    }

    /// The first version-looking token: `git version 2.39.5 (Apple Git-154)` → `2.39.5`, `Docker version 27.3.1,
    /// build ce12230` → `27.3.1`, `Docker Compose version v2.29.7` → `2.29.7`.
    static func version(in output: String) -> String? {
        let firstLine = output.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        for token in firstLine.split(separator: " ") {
            var candidate = Substring(token)
            if candidate.hasPrefix("v") { candidate = candidate.dropFirst() }
            candidate = candidate.prefix { $0.isNumber || $0 == "." || $0 == "-" || $0.isLetter }
            while let last = candidate.last, !last.isNumber && !last.isLetter { candidate = candidate.dropLast() }
            if candidate.first?.isNumber == true, candidate.contains(".") { return String(candidate) }
        }
        let trimmed = firstLine.trimmingCharacters(in: .whitespaces)
        return trimmed.isEmpty ? nil : trimmed
    }
}
