import BranchBoxKit
import Foundation

/// `BranchBoxBackend` over the `branchbox` CLI (DESIGN §6).
///
/// - **Contract mode** (`identity.contractVersion != nil`) uses the 0.14 JSON surface; each feature is still gated on
///   its capability. **Legacy mode** (0.13.4+) falls back per method: text parsing, the app preflight, the app's
///   own branch step and `HostToolProbe`.
/// - Every spawn has an absolute executable, the §7.3 child environment (`.read` for reads, `.mutation` for anything
///   that changes state), stdin `/dev/null` unless a payload is sent, and an explicit `--repo`/`-p`.
/// - Every method throws only `BackendError`; cancellation throws `.cancelled` once the process group is gone.
public struct CLIBackend: BranchBoxBackend {
    /// The note on a cancelled start, or an older CLI's teardown/init, which can leave an unrecorded worktree.
    public static let partialWorktreeNote = "may have left a partial worktree; see Needs attention"

    public let executable: URL
    public let currentIdentity: BackendIdentity
    let runner: any ProcessRunning
    let environment: any EnvironmentProviding
    let settings: BackendSettings
    let fileSystem: any FileSystemProbing
    let gitExecutable: URL?
    let probe: CLIProbe?

    /// - Parameters:
    ///   - executable: the CLI, unresolved (`/opt/homebrew/bin/branchbox`).
    ///   - gitExecutable: git to run; nil finds `git` on the child PATH (else `/usr/bin/git`).
    ///   - probe: re-checks the CLI in `identity()`; nil returns `identity` as given.
    public init(executable: URL, identity: BackendIdentity, runner: any ProcessRunning,
                environment: any EnvironmentProviding, settings: BackendSettings = BackendSettings(),
                fileSystem: any FileSystemProbing = LocalFileSystem(), gitExecutable: URL? = nil,
                probe: CLIProbe? = nil) {
        self.executable = executable
        self.currentIdentity = identity
        self.runner = runner
        self.environment = environment
        self.settings = settings
        self.fileSystem = fileSystem
        self.gitExecutable = gitExecutable
        self.probe = probe
    }

    // MARK: - Modes

    var isContract: Bool { currentIdentity.contractVersion != nil }

    func supports(_ capability: Capability) -> Bool { currentIdentity.supports(capability) }

    /// Contract teardown flags need `--discard-changes`; a CLI without it gets the legacy flags and app branch step.
    var teardownMode: CLICommand.TeardownMode { supports(.teardownDiscardChanges) ? .contract : .legacy }

    /// Legacy teardown/init may leave a partial worktree. Start always uses `partialWorktreeNote`, because
    /// cancellation can interrupt git worktree add before the CLI writes its write-ahead registry entry.
    var cancelNote: String? {
        supports(.writeAheadStart) && supports(.registryLock) ? nil : Self.partialWorktreeNote
    }

    var cliVersion: String { currentIdentity.version.description }

    // MARK: - Invocation

    func childEnvironment(_ purpose: EnvironmentPurpose) async -> [String: String] {
        await environment.childEnvironment(for: purpose, settings: settings)
    }

    var redaction: RedactedCommandLine {
        RedactedCommandLine(secrets: Array(settings.extraEnvironment.values))
    }

    /// One CLI run (DESIGN §6.3's single `invoke`): runner errors mapped, the rest left to `CLIOutput`.
    func cli(_ arguments: [String], operation: String, purpose: EnvironmentPurpose, project: ProjectRef? = nil,
             workingDirectory: URL? = nil, timeout: Duration?, standardInput: Data? = nil, streamStdout: Bool = false,
             relay: ProgressRelay? = nil, cancelNote: String? = nil, environmentOverrides: [String: String] = [:],
             secrets: [String] = []) async throws -> CLIOutput {
        var environment = await childEnvironment(purpose)
        if !environmentOverrides.isEmpty {
            environment.merge(environmentOverrides) { _, override in override }
            environment.merge(settings.extraEnvironment) { _, extra in extra }   // the user's extras still win
        }
        let redaction = RedactedCommandLine(secrets: Array(settings.extraEnvironment.values), tokens: secrets)
        let invocation = ToolInvocation(runner: runner, environment: environment, redaction: redaction,
                                        cliVersion: cliVersion)
        var options = ToolInvocation.Options(operation: operation, workingDirectory: workingDirectory, timeout: timeout)
        options.standardInput = standardInput
        options.streamStdout = streamStdout
        options.cancelNote = cancelNote
        let result = try await invocation.run(executable, arguments, options) { line in relay?.line(line) }
        return CLIOutput(result: result, operation: operation, invocation: invocation.invocation(executable, arguments),
                         cliVersion: cliVersion, context: Self.context(project))
    }

    static func context(_ project: ProjectRef?) -> CLIErrorClassifier.Context {
        CLIErrorClassifier.Context(registryPath: project.map { Paths.join($0.path, ".branchbox/registry.json") })
    }

    func git(_ purpose: EnvironmentPurpose) async -> GitInspector {
        let environment = await childEnvironment(purpose)
        let executable = gitExecutable
            ?? ExecutableSearch.find("git", path: environment["PATH"], fileSystem: fileSystem).map(URL.init(fileURLWithPath:))
            ?? GitInspector.defaultExecutable
        return GitInspector(executable: executable,
                            invocation: ToolInvocation(runner: runner, environment: environment, redaction: redaction),
                            fileSystem: fileSystem)
    }

    // MARK: - Identity and environment

    /// The CLI's identity as probed now (cached per binary). The backend keeps operating in the mode of
    /// `currentIdentity`, the one it was bootstrapped with: a result that differs (a `brew upgrade` or downgrade
    /// replaced the binary) means the caller must bootstrap again. `EnvironmentStore.recheckIfStale` does that on
    /// app activation, so a changed CLI is picked up the next time the app comes to the front.
    public func identity() async throws -> BackendIdentity {
        guard let probe, case .cli(let resolution) = currentIdentity.kind else { return currentIdentity }
        return try await probe.identity(for: resolution, environment: await childEnvironment(.read))
    }

    /// `HostToolProbe`, merged with `doctor --json` when the CLI has it (the CLI's check wins for a shared id).
    public func doctor(_ project: ProjectRef?) async -> DoctorReport {
        let host = HostToolProbe(runner: runner, environment: await childEnvironment(.read), fileSystem: fileSystem)
        guard supports(.doctor) else { return DoctorReport(source: .app, checks: await host.checks()) }
        async let appChecks = host.checks()
        let cliChecks = try? await cli(CLICommand.doctor(repo: project?.path), operation: "doctor", purpose: .read,
                                       project: project, timeout: .seconds(60))
            .decodeInBand(DoctorPayload.self, what: "doctor report").checks
        let app = await appChecks
        guard let cliChecks else { return DoctorReport(source: .app, checks: app) }
        let ids = Set(cliChecks.map(\.id))
        return DoctorReport(source: .merged, checks: cliChecks + app.filter { !ids.contains($0.id) })
    }

    // MARK: - Projects

    public func resolveProject(at folder: URL) async throws -> ProjectResolution {
        try await git(.read).resolveProject(at: folder)
    }

    public func detect(_ folder: URL) async throws -> DetectReport {
        let path = folder.standardizedFileURL.path
        if supports(.detectJSON) {
            return try await cli(CLICommand.detect(folder: path, json: true), operation: "detect", purpose: .read,
                                 timeout: .seconds(15)).decode(DetectPayload.self, what: "detect report").value.report
        }
        let output = try await cli(CLICommand.detect(folder: path, json: false), operation: "detect", purpose: .read,
                                   timeout: .seconds(15))
        guard output.succeeded else { throw output.failure() }
        let gitRepository = (try? await git(.read).repositoryRoots(of: path)) != nil
        return TextParsers.detectReport(output.stdoutText, folder: path, gitRepository: gitRepository,
                                        initialized: fileSystem.kind(at: Paths.join(path, ".branchbox")) == .directory,
                                        hasDevcontainer: fileSystem.kind(at: Paths.join(path, ".devcontainer")) == .directory,
                                        hasEnv: fileSystem.kind(at: Paths.join(path, ".env")) != .missing)
    }

    public func readConfig(_ project: ProjectRef) async throws -> ProjectConfigDocument {
        guard supports(.config) else { return try LegacyConfig.read(root: project.path) }
        return try await cli(CLICommand.configGet(repo: project.path), operation: "config get", purpose: .read,
                             project: project, timeout: .seconds(15))
            .decode(ConfigGetPayload.self, what: "project config").value.document
    }

    public func applyConfig(_ patch: ConfigPatch, to project: ProjectRef, dryRun: Bool) async throws -> ConfigApplyResult {
        guard supports(.config) else { throw BackendError.unsupported(.config, minimumCLI: "0.14.0") }
        let body = try Self.encode(LegacyConfig.mergePatch(patch))
        return try await cli(CLICommand.configApply(repo: project.path, dryRun: dryRun), operation: "config apply",
                             purpose: .mutation, project: project, timeout: .seconds(30), standardInput: body)
            .decode(ConfigApplyResult.self, what: "config apply result").value
    }

    /// The token goes on stdin only; it is also redacted from diagnostics in case a CLI echoes it.
    public func setTunnelCredentials(_ request: TunnelCredentialsRequest,
                                     in project: ProjectRef) async throws -> TunnelCredentialsResult {
        guard supports(.tunnelCredentials) else {
            throw BackendError.unsupported(.tunnelCredentials, minimumCLI: "0.14.0")
        }
        let token = request.clear ? nil : request.apiToken?.value
        return try await cli(CLICommand.tunnelCredentials(request, repo: project.path), operation: "tunnel credentials set",
                             purpose: .mutation, project: project, timeout: .seconds(30),
                             standardInput: token.map { Data($0.utf8) }, secrets: token.map { [$0] } ?? [])
            .decode(TunnelCredentialsResult.self, what: "tunnel credentials result").value
    }

    // MARK: - Feature reads

    public func listFeatures(in project: ProjectRef, includeRemoved: Bool) async throws -> FeatureListing {
        let git = await git(.read)
        // 0.13.4's `feature list --repo <feature worktree>` answers [], so always ask from the main worktree.
        let root = try await git.mainRoot(of: project)
        let (records, preamble) = try await records(in: root, includeRemoved: includeRemoved)
        var warnings = preamble.map { [$0] } ?? []
        var strays: [StrayWorktree] = []
        var registeredWorktrees: [WorktreeEntry]?
        do {
            let worktrees = try await git.worktrees(in: root.path)
            registeredWorktrees = worktrees
            strays = StrayDetector.strays(in: worktrees, mainRoot: root.path, records: records.compactMap(\.value),
                                          configPrefix: LegacyConfig.effective(root: root.path).branchPrefix)
        } catch let error as BackendError {
            warnings.append("Unregistered worktrees were not checked: \(TeardownPreflight.summary(of: error))")
        }
        let listing = FeatureListing(decoding: records, strays: strays, warnings: warnings)
        let checked = listing.features.map { record in
            var record = record
            record.worktreeIssue = WorktreeHealthInspector.issue(for: record, in: root.path,
                                                                 worktrees: registeredWorktrees, fileSystem: fileSystem)
            return record
        }
        return FeatureListing(features: checked, strays: strays, droppedRecords: listing.droppedRecords,
                              warnings: listing.warnings)
    }

    func records(in project: ProjectRef, includeRemoved: Bool) async throws -> ([Lossy<FeatureRecord>], String?) {
        let output = try await cli(CLICommand.listFeatures(repo: project.path, includeRemoved: includeRemoved),
                                   operation: "feature list", purpose: .read, project: project, timeout: .seconds(30))
        return try output.decode([Lossy<FeatureRecord>].self, what: "feature list")
    }

    /// The registry record of `feature`, if it has one.
    func record(for feature: FeatureRef) async throws -> FeatureRecord? {
        try await records(in: feature.project, includeRemoved: false).0.compactMap(\.value)
            .first { $0.workFeature == feature.name }
    }

    /// The feature's worktree: the registry's `worktree_path`, else BranchBox's layout (`<parent of main>/<name>`).
    func worktreePath(for feature: FeatureRef) async throws -> String {
        try await record(for: feature)?.worktreePath ?? Self.layoutPath(for: feature)
    }

    static func layoutPath(for feature: FeatureRef) -> String {
        Paths.join(Paths.parent(feature.project.path), feature.name)
    }

    public func listBranches(in project: ProjectRef) async throws -> BranchList {
        try await git(.read).branches(in: project)
    }

    /// `name validate` (exit 0: valid as typed), else `name generate` (empty output: invalid). The branch and path
    /// follow the project's prefix and BranchBox's layout.
    public func previewName(_ input: String, in project: ProjectRef) async throws -> NamePreview {
        let typed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !typed.isEmpty else {
            return NamePreview(input: input, slug: nil, valid: false, problem: "Enter a feature name")
        }
        var slug: String?
        var problem: String?
        let validate = try await cli(CLICommand.nameValidate(typed), operation: "name validate", purpose: .read,
                                     timeout: .seconds(5))
        if validate.succeeded {
            slug = typed
        } else {
            problem = Self.nameProblem(validate)
            let generate = try await cli(CLICommand.nameGenerate(typed), operation: "name generate", purpose: .read,
                                         timeout: .seconds(5))
            let generated = generate.succeeded
                ? generate.stdoutText.split(whereSeparator: \.isNewline).first.map(String.init)?
                    .trimmingCharacters(in: .whitespaces) ?? ""
                : ""
            slug = generated.isEmpty ? nil : generated
        }
        if let candidate = slug, candidate.hasPrefix("-") {
            // 0.13.4 accepts it, but every later command would read it as a flag.
            slug = nil
            problem = "Feature names cannot start with “-”"
        }
        guard let slug else {
            return NamePreview(input: input, slug: nil, valid: false,
                               problem: problem ?? "“\(typed)” cannot be turned into a feature name")
        }
        let prefix = LegacyConfig.effective(root: project.path).branchPrefix
        return NamePreview(input: input, slug: slug, valid: true, branchName: prefix.isEmpty ? slug : "\(prefix)/\(slug)",
                           worktreePath: Paths.join(Paths.parent(project.path), slug))
    }

    /// `✗ Invalid feature name: X` plus its hint, without the mark.
    private static func nameProblem(_ output: CLIOutput) -> String? {
        let lines = (output.stdoutText.split(whereSeparator: \.isNewline).map(String.init) + output.result.stderrTail)
            .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        guard !lines.isEmpty else { return nil }
        return lines.prefix(2).map { $0.hasPrefix("✗ ") ? String($0.dropFirst(2)) : $0 }
            .map { $0.hasPrefix("Error: ") ? String($0.dropFirst(7)) : $0 }
            .joined(separator: ". ")
    }

    /// `docker ps -a --filter label=devcontainer.local_folder=<W>` for the container (no docker: `.unknown`), plus
    /// `devcontainer detect -p <W> --json` for the service.
    public func devcontainerStatus(for feature: FeatureRef) async throws -> DevcontainerStatus {
        let worktree = try await worktreePath(for: feature)
        let environment = await childEnvironment(.read)
        var state = DevcontainerStatus.State.unknown
        var containerID: String?
        if let docker = ExecutableSearch.find("docker", path: environment["PATH"], fileSystem: fileSystem) {
            let invocation = ToolInvocation(runner: runner, environment: environment, redaction: redaction)
            let result = try await invocation.run(URL(fileURLWithPath: docker),
                                                  ["ps", "-a", "--filter", "label=devcontainer.local_folder=\(worktree)",
                                                   "--format", "{{json .}}"],
                                                  ToolInvocation.Options(operation: "docker ps", timeout: .seconds(10)))
            if result.termination == .exited(0) {
                (state, containerID) = Self.containerState(String(decoding: result.stdout, as: UTF8.self))
            }
        }
        let detect = try? await cli(CLICommand.devcontainerDetect(worktree: worktree), operation: "devcontainer detect",
                                    purpose: .read, timeout: .seconds(10))
            .decodeInBand(DevcontainerServiceInfo.self, what: "devcontainer service", accept: \.isRecognized)
        return DevcontainerStatus(state: state, containerID: containerID, service: detect)
    }

    /// The first `docker ps --format '{{json .}}'` row: running, or stopped in any other state; no row is
    /// `.notCreated`.
    static func containerState(_ output: String) -> (DevcontainerStatus.State, String?) {
        for line in output.split(whereSeparator: \.isNewline) {
            guard let object = try? CLIJSON.decode(JSONValue.self, from: Data(line.utf8)).value.objectValue else {
                continue
            }
            let id = object["ID"]?.stringValue
            let running = object["State"]?.stringValue == "running"
            return (running ? .running : .stopped, id)
        }
        return (.notCreated, nil)
    }

    /// Sorted keys, so a request body (and its "Copy as Command" form) is the same every time.
    static func encode(_ value: JSONValue) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        do {
            return try encoder.encode(value)
        } catch {
            throw BackendError.commandFailed(Diagnostics(summary: "Could not encode the request: \(error)"))
        }
    }
}
