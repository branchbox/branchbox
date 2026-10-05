import BranchBoxKit
import Foundation

/// Every `BranchBoxBackend` method, for scripting and for filtering the call log.
public enum PreviewMethod: String, Sendable, Hashable, CaseIterable {
    case identity, doctor, resolveProject, detect, readConfig, applyConfig, setTunnelCredentials, initProject
    case listFeatures, listBranches, previewName, planTeardown, devcontainerStatus
    case startFeature, teardownFeature, exec, devcontainer, syncDevcontainers, openTunnel, removeTunnel
    case deleteBranch, removeStray
}

/// One step of a scripted call. A call runs its steps in order, then returns its canned result.
public enum PreviewBehavior: Sendable, Hashable {
    /// Waits `after`, then carries on (as the last step, the call succeeds). Cancellation ends the wait at once.
    case succeed(after: Duration)
    /// Throws `error`, as the CLI backend does once it has classified a failure.
    case fail(BackendError)
    /// Waits until `resume(_:)`, `resumeAll()`, `terminateAll()` or cancellation.
    case suspendUntilResumed
    /// Sends `events`, in order, to the call's progress sink. Methods without a sink ignore them.
    case emit([ProgressEvent])
}

/// One recorded call, with the full request values it was given.
public enum PreviewCall: Sendable, Hashable {
    case identity
    case doctor(ProjectRef?)
    case resolveProject(URL)
    case detect(URL)
    case readConfig(ProjectRef)
    case applyConfig(ConfigPatch, ProjectRef, dryRun: Bool)
    case setTunnelCredentials(TunnelCredentialsRequest, ProjectRef)
    case initProject(InitRequest)
    case listFeatures(ProjectRef, includeRemoved: Bool)
    case listBranches(ProjectRef)
    case previewName(String, ProjectRef)
    case planTeardown(TeardownRequest)
    case devcontainerStatus(FeatureRef)
    case startFeature(StartFeatureRequest)
    case teardownFeature(TeardownRequest)
    case exec(ExecRequest)
    case devcontainer(DevcontainerAction, FeatureRef)
    case syncDevcontainers(SyncRequest)
    case openTunnel(FeatureRef)
    case removeTunnel(FeatureRef, force: Bool)
    case deleteBranch(String, ProjectRef, force: Bool)
    case removeStray(StrayWorktree, ProjectRef, discard: DiscardConsent?)

    public var method: PreviewMethod {
        switch self {
        case .identity: .identity
        case .doctor: .doctor
        case .resolveProject: .resolveProject
        case .detect: .detect
        case .readConfig: .readConfig
        case .applyConfig: .applyConfig
        case .setTunnelCredentials: .setTunnelCredentials
        case .initProject: .initProject
        case .listFeatures: .listFeatures
        case .listBranches: .listBranches
        case .previewName: .previewName
        case .planTeardown: .planTeardown
        case .devcontainerStatus: .devcontainerStatus
        case .startFeature: .startFeature
        case .teardownFeature: .teardownFeature
        case .exec: .exec
        case .devcontainer: .devcontainer
        case .syncDevcontainers: .syncDevcontainers
        case .openTunnel: .openTunnel
        case .removeTunnel: .removeTunnel
        case .deleteBranch: .deleteBranch
        case .removeStray: .removeStray
        }
    }

    /// The request of a `startFeature` call.
    public var startRequest: StartFeatureRequest? {
        if case .startFeature(let request) = self { request } else { nil }
    }

    /// The request of a `teardownFeature` call, e.g. to check that a first attempt carries no discard consent.
    public var teardownRequest: TeardownRequest? {
        if case .teardownFeature(let request) = self { request } else { nil }
    }
}

/// A scriptable, in-memory `BranchBoxBackend` for SwiftUI previews, the `BRANCHBOX_BACKEND=preview` dev loop and
/// the store tests.
///
/// - An unscripted call succeeds at once with a canned result built from the scenario and the request.
///   `startFeature` and `teardownFeature` also update that project's listing, so a refresh shows the change.
/// - `script(_:_:)` queues steps for the NEXT call of a method; each call takes one queued entry, in order.
/// - Every call is appended to `calls`, cancelled ones included.
/// - Every call honours Task cancellation: it throws `.cancelled`, and a waiting step ends at once.
/// - In a scenario without a CLI (`cliMissing`), every throwing call throws the scenario's error.
/// - A scenario's dirty features refuse a teardown (with the plan, as the CLI and the app preflight do) unless its
///   discard consent names exactly their changed files; its unmerged branches are reported by `planTeardown` and
///   refuse (contract) or fail to delete (legacy) under "delete if merged".
public actor PreviewBackend: BranchBoxBackend {
    public nonisolated let scenario: PreviewScenario
    public private(set) var calls: [PreviewCall] = []

    private var listings: [String: FeatureListing] = [:]          // by project path; the scenario's until changed
    private var resolutions: [String: ProjectResolution] = [:]    // by requested folder path
    private var scripts: [PreviewMethod: [[PreviewBehavior]]] = [:]
    private var waits: [PendingWait] = []

    public init(scenario: PreviewScenario = .contract) {
        self.scenario = scenario
    }

    // MARK: Scripting and inspection

    /// Queues `behavior` as the only step of the next `method` call.
    public func script(_ method: PreviewMethod, _ behavior: PreviewBehavior) {
        script(method, steps: [behavior])
    }

    /// Queues `steps` for the next `method` call, e.g. `[.emit(events), .suspendUntilResumed]`.
    public func script(_ method: PreviewMethod, steps: [PreviewBehavior]) {
        scripts[method, default: []].append(steps)
    }

    /// Replaces what `listFeatures` returns for `project` (removed records are still filtered unless asked for).
    public func setListing(_ listing: FeatureListing, for project: ProjectRef) {
        listings[project.path] = listing
    }

    /// Makes `resolveProject(at: folder)` return `resolution`, e.g. an uninitialized repository or a container folder.
    public func setResolution(_ resolution: ProjectResolution, for folder: URL) {
        resolutions[folder.standardizedFileURL.path] = resolution
    }

    /// What `listFeatures` currently returns for `project` with `--all`.
    public func currentListing(for project: ProjectRef) -> FeatureListing {
        listing(for: project)
    }

    public func calls(to method: PreviewMethod) -> [PreviewCall] {
        calls.filter { $0.method == method }
    }

    public func clearCalls() {
        calls.removeAll()
    }

    /// Lets the oldest `method` call waiting in `.suspendUntilResumed` carry on. False when none is waiting.
    @discardableResult public func resume(_ method: PreviewMethod) -> Bool {
        guard let index = waits.firstIndex(where: { $0.method == method && $0.resumable && !$0.wait.hasEnded }) else {
            return false
        }
        waits.remove(at: index).wait.end(.resumed)
        return true
    }

    public func resumeAll() {
        for pending in waits where pending.resumable { pending.wait.end(.resumed) }
        waits.removeAll { $0.resumable }
    }

    /// Ends every waiting step; each of those calls throws `.cancelled`, as after the CLI's process groups are killed.
    public func terminateAll() {
        for pending in waits { pending.wait.end(.interrupted) }
        waits.removeAll()
    }

    /// How many `method` calls are waiting in `.suspendUntilResumed`.
    public func suspendedCount(_ method: PreviewMethod) -> Int {
        waits.filter { $0.method == method && $0.resumable && !$0.wait.hasEnded }.count
    }

    /// Waits until `count` `method` calls are suspended, so a test can act on a call that is surely in flight.
    /// False on timeout or cancellation.
    public func waitUntilSuspended(_ method: PreviewMethod, count: Int = 1, timeout: Duration = .seconds(5)) async -> Bool {
        let deadline = ContinuousClock.now + timeout
        while suspendedCount(method) < count {
            guard ContinuousClock.now < deadline else { return false }
            do { try await Task.sleep(for: .milliseconds(1)) } catch { return false }
        }
        return true
    }

    // MARK: Identity & environment

    public func identity() async throws -> BackendIdentity {
        try await perform(.identity) { try scenario.identity.get() }
    }

    public func doctor(_ project: ProjectRef?) async -> DoctorReport {
        do {
            return try await perform(.doctor(project)) { doctorReport(for: project, problem: nil) }
        } catch {
            return doctorReport(for: project, problem: BackendError.normalize(error))
        }
    }

    // MARK: Projects

    public func resolveProject(at folder: URL) async throws -> ProjectResolution {
        try await perform(.resolveProject(folder)) {
            if let resolution = resolutions[folder.standardizedFileURL.path] { return resolution }
            // A sample feature's folder normalizes to the sample project, like a real feature worktree.
            let isSampleWorktree = PreviewSamples.features.contains { $0.worktreePath == folder.standardizedFileURL.path }
            let project = isSampleWorktree ? PreviewSamples.project : ProjectRef(root: folder)
            return ProjectResolution(project: project, requested: folder,
                                     normalization: isSampleWorktree ? .fromFeatureWorktree : .none, initialized: true)
        }
    }

    public func detect(_ folder: URL) async throws -> DetectReport {
        try await perform(.detect(folder)) {
            let modules = ["devcontainer", "compose", "tunnel", "specs"]
            let rawText = supports(.detectJSON) ? nil : """
                📦 BranchBox Configuration

                Project: .
                Stack: Rails
                Adapter: Rails

                Enabled modules: \(modules.count)
                \(modules.map { "  ✓ \($0)" }.joined(separator: "\n"))
                """
            return DetectReport(project: folder.path, gitRepository: true, initialized: true, stack: "rails", adapter: "Rails",
                                modules: modules, hasDevcontainer: true, hasEnv: true, rawText: rawText)
        }
    }

    public func readConfig(_ project: ProjectRef) async throws -> ProjectConfigDocument {
        try await perform(.readConfig(project)) {
            let editable = supports(.config)
            return ProjectConfigDocument(path: "\(project.path)/.branchbox/config.json", exists: true, effective: .defaults,
                                         keys: editable ? PreviewSamples.configKeys : [], editable: editable)
        }
    }

    public func applyConfig(_ patch: ConfigPatch, to project: ProjectRef, dryRun: Bool) async throws -> ConfigApplyResult {
        try await perform(.applyConfig(patch, project, dryRun: dryRun)) {
            try require(.config)
            let current = Dictionary(PreviewSamples.configKeys.map { ($0.key, $0.value) }, uniquingKeysWith: { first, _ in first })
            let changed = patch.changes.map { ConfigApplyResult.Change(key: $0.key, old: current[$0.key] ?? nil, new: $0.value) }
            return ConfigApplyResult(changed: changed, effective: .defaults)
        }
    }

    public func setTunnelCredentials(_ request: TunnelCredentialsRequest, in project: ProjectRef) async throws -> TunnelCredentialsResult {
        try await perform(.setTunnelCredentials(request, project)) {
            try require(.tunnelCredentials)
            return TunnelCredentialsResult(credentialsPath: "\(project.path)/.branchbox/secure/cloudflared.env",
                                           accountID: request.accountID, tokenPresent: !request.clear && request.apiToken != nil)
        }
    }

    public func initProject(_ request: InitRequest, progress: @escaping ProgressSink) async throws -> InitReport {
        try await perform(.initProject(request), progress: progress) {
            let modules = request.skipDevcontainer ? ["specs"] : ["devcontainer", "compose", "specs"]
            return InitReport(workspacePath: request.folder.path, reorganized: request.reorganize, stack: request.stack ?? "generic",
                              adapter: "Generic", modules: modules, nextSteps: ["Start your first feature"],
                              onePasswordStatus: supports(.initJSON) ? "skipped" : nil,
                              log: ["Initialized BranchBox in \(request.folder.path)"])
        }
    }

    // MARK: Feature reads

    public func listFeatures(in project: ProjectRef, includeRemoved: Bool) async throws -> FeatureListing {
        try await perform(.listFeatures(project, includeRemoved: includeRemoved)) {
            let listing = listing(for: project)
            let features = includeRemoved ? listing.features : listing.features.filter { $0.status != .removed }
            return FeatureListing(features: features, strays: listing.strays, droppedRecords: listing.droppedRecords,
                                  warnings: listing.warnings)
        }
    }

    public func listBranches(in project: ProjectRef) async throws -> BranchList {
        try await perform(.listBranches(project)) {
            let branches = listing(for: project).features.filter { $0.status != .removed }.map(\.branchName)
            return BranchList(current: "main", local: ["main"] + branches.filter { !$0.isEmpty }, remote: ["origin/main"])
        }
    }

    public func previewName(_ input: String, in project: ProjectRef) async throws -> NamePreview {
        try await perform(.previewName(input, project)) {
            let slug = Self.slug(input)
            guard !slug.isEmpty else {
                return NamePreview(input: input, slug: nil, valid: false,
                                   problem: "A feature name needs at least one letter or digit")
            }
            let taken = findRecord(named: slug, in: project).map { $0.status != .removed } ?? false
            return NamePreview(input: input, slug: slug, valid: !taken, branchName: "feature/\(slug)",
                               worktreePath: Self.worktreePath(for: slug, in: project),
                               problem: taken ? "A feature named \(slug) already exists" : nil)
        }
    }

    public func planTeardown(_ request: TeardownRequest) async throws -> TeardownPlanDocument {
        try await perform(.planTeardown(request)) { plan(for: request) }
    }

    public func devcontainerStatus(for feature: FeatureRef) async throws -> DevcontainerStatus {
        try await perform(.devcontainerStatus(feature)) {
            guard let record = findRecord(named: feature.name, in: feature.project), record.status != .removed else {
                return DevcontainerStatus(state: .unknown)
            }
            guard record.runtime.provider == .container else { return DevcontainerStatus(state: .notCreated) }
            return DevcontainerStatus(state: .running, containerID: "preview-\(feature.name)",
                                      service: DevcontainerServiceInfo(serviceName: "app", port: 3000,
                                                                       serviceURL: "http://app:3000", containerUser: "vscode"))
        }
    }

    // MARK: Feature mutations

    public func startFeature(_ request: StartFeatureRequest, progress: @escaping ProgressSink) async throws -> StartSummary {
        try await perform(.startFeature(request), progress: progress) {
            let path = Self.worktreePath(for: request.name, in: request.project)
            if let existing = findRecord(named: request.name, in: request.project), existing.status != .removed {
                let message = "A feature named \(request.name) already exists at \(existing.worktreePath ?? path)"
                throw BackendError.refused(Refusal(cause: .worktreeExists(path: existing.worktreePath ?? path), message: message,
                                                   diagnostics: Diagnostics(summary: message)))
            }
            let prefix = request.branchPrefix ?? ProjectConfig.defaults.branchPrefix
            let branch = prefix.isEmpty ? request.name : "\(prefix)/\(request.name)"
            let now = Date.now
            let outcomes = ["devcontainer", "compose", "specs", "tunnel"].map { module in
                ModuleOutcome(module: module, status: request.skipModules.contains(module) ? .skipped : .success,
                              durationMs: 0, recordedAt: now)
            }
            let record = FeatureRecord(
                workFeature: request.name, branchName: branch, worktreePath: path, baseBranch: request.base,
                featureURL: "dev-\(request.name).localhost", composeProjectName: "\(request.project.displayName)-\(request.name)",
                envPath: "\(path)/.env", status: .active, createdAt: now, updatedAt: now, color: "#2ecc71",
                startMode: request.mode.rawValue, promptSeed: request.prompt, moduleOutcomes: outcomes,
                runtime: RuntimeInfo(provider: request.runtime))
            update(request.project) { listing in
                listing.features.filter { $0.workFeature != request.name } + [record]
            }
            return StartSummary(
                workFeature: record.workFeature, branchName: branch, worktreePath: path, mode: request.mode.rawValue,
                promptSeed: request.prompt, featureURL: record.featureURL, composeProjectName: record.composeProjectName,
                runtime: record.runtime, envPath: record.envPath, color: record.color, moduleOutcomes: outcomes,
                skippedModules: request.skipModules.map { SkippedModule(module: $0, reason: "Skipped by request") },
                generatedAt: now)
        }
    }

    public func teardownFeature(_ request: TeardownRequest, progress: @escaping ProgressSink) async throws -> TeardownOutcome {
        try await perform(.teardownFeature(request), progress: progress) {
            let name = request.feature.name
            guard let record = findRecord(named: name, in: request.feature.project), record.status != .removed else {
                let message = "No feature named \(name) is registered in \(request.feature.project.displayName)"
                throw BackendError.refused(Refusal(cause: .featureNotFound(name), message: message,
                                                   diagnostics: Diagnostics(summary: message)))
            }
            let dirty = scenario.dirtyFeatures[name] ?? []
            if !dirty.isEmpty, Set(request.discard?.userFiles ?? []) != Set(dirty.map(\.path)) {
                throw uncommittedChangesRefusal(request, files: dirty)
            }
            let ahead = scenario.unmergedBranches[name] ?? 0
            let contract = supports(.teardownUnmergedPreflight)
            if ahead > 0, request.branch == .deleteIfMerged, contract {
                let message = "Refusing to tear down '\(name)'; nothing was removed. \(record.branchName) has \(ahead) "
                    + "commit\(ahead == 1 ? "" : "s") not merged into main; pass --keep-branch or --force-delete-branch."
                throw BackendError.refused(Refusal(cause: .unmergedBranch(branch: record.branchName, ahead: ahead), message: message,
                                                   diagnostics: Diagnostics(summary: message), plan: plan(for: request)))
            }
            update(request.feature.project) { listing in
                listing.features.map { $0.workFeature == name ? $0.removed(at: .now) : $0 }
            }
            let branch: BranchOutcome
            switch request.branch {
            case .keep:
                branch = .kept(record.branchName)
            case .deleteIfMerged where ahead > 0:
                // Legacy CLIs always keep the branch; the app's `git branch -d` then refuses an unmerged one.
                branch = .deleteFailed(record.branchName,
                                       reason: "error: the branch '\(record.branchName)' is not fully merged")
            case .deleteIfMerged, .forceDelete:
                branch = .deleted(record.branchName, by: contract ? .cli : .app)
            }
            let deleted: Bool = if case .deleted = branch { true } else { false }
            let deleteError: String? = if case .deleteFailed(_, let reason) = branch { reason } else { nil }
            let summary = TeardownSummary(
                workFeature: name, branchName: record.branchName, worktreeRemoved: true, branchDeleted: deleted,
                runtimeTeardown: RuntimeTeardownReport(provider: record.runtime.provider.raw, runtimeID: record.runtime.runtimeID,
                                                       verified: true, residueFree: true),
                branchAction: Self.action(for: request.branch), branchDeleteError: deleteError,
                discardedChanges: request.discard == nil ? [] : dirty, registryUpdated: true)
            return TeardownOutcome(summary: summary, branch: branch, worktreeGone: true)
        }
    }

    public func exec(_ request: ExecRequest, progress: @escaping ProgressSink) async throws -> ExecResult {
        try await perform(.exec(request), progress: progress) {
            ExecResult(exitCode: 0, stdout: "Preview backend: `\(request.command.joined(separator: " "))` was not run\n")
        }
    }

    public func devcontainer(_ action: DevcontainerAction, for feature: FeatureRef,
                             progress: @escaping ProgressSink) async throws -> DevcontainerResult {
        try await perform(.devcontainer(action, feature), progress: progress) {
            let containerID = "preview-\(feature.name)"
            return switch action {
            case .up:
                DevcontainerResult(outcome: "created", containerID: containerID, remoteUser: "vscode",
                                   remoteWorkspaceFolder: "/workspaces/\(feature.name)",
                                   composeProjectName: "\(feature.project.displayName)-\(feature.name)")
            case .down:
                DevcontainerResult(outcome: "removed", removedContainers: [containerID])
            case .build:
                DevcontainerResult(outcome: "success", imageName: "vsc-\(feature.name)-preview")
            }
        }
    }

    public func syncDevcontainers(_ request: SyncRequest, progress: @escaping ProgressSink) async throws -> SyncReport {
        try await perform(.syncDevcontainers(request), progress: progress) {
            if !request.features.isEmpty { try require(.devcontainerSyncJSON) }
            let rows = listing(for: request.project).features
                .filter { $0.status == .active && (request.features.isEmpty || request.features.contains($0.workFeature)) }
                .map { record in
                    SyncReport.Row(feature: record.workFeature, worktreePath: record.worktreePath,
                                   status: request.dryRun ? .wouldSync : .synced, files: ["devcontainer.json"])
                }
            return SyncReport(dryRun: request.dryRun, strategy: (request.strategy ?? .copy).rawValue, rows: rows)
        }
    }

    public func openTunnel(_ feature: FeatureRef, progress: @escaping ProgressSink) async throws -> TunnelChange {
        try await perform(.openTunnel(feature), progress: progress) {
            TunnelChange(workFeature: feature.name,
                         state: TunnelState(provider: "cloudflared", hostname: "\(feature.name).example.dev",
                                            serviceURL: "http://app:3000", status: .active, lastUpdated: .now))
        }
    }

    public func removeTunnel(_ feature: FeatureRef, force: Bool, progress: @escaping ProgressSink) async throws -> TunnelChange {
        try await perform(.removeTunnel(feature, force: force), progress: progress) {
            TunnelChange(workFeature: feature.name, state: TunnelState(provider: "cloudflared", status: .disabled),
                         previousState: findRecord(named: feature.name, in: feature.project)?.tunnel)
        }
    }

    public func deleteBranch(_ branch: String, in project: ProjectRef, force: Bool) async throws {
        try await perform(.deleteBranch(branch, project, force: force)) {}
    }

    public func removeStray(_ stray: StrayWorktree, in project: ProjectRef, discard: DiscardConsent?) async throws {
        try await perform(.removeStray(stray, project, discard: discard)) {
            let listing = listing(for: project)
            listings[project.path] = FeatureListing(features: listing.features, strays: listing.strays.filter { $0 != stray },
                                                    droppedRecords: listing.droppedRecords, warnings: listing.warnings)
        }
    }

    /// Only the CLI backend can print a command line.
    public nonisolated func previewCommandLine(_ request: OperationRequestContext) -> String? { nil }

    // MARK: Execution

    /// Records `call`, runs its scripted steps, then returns `result()`. Every error leaves as a `BackendError`.
    private func perform<T>(_ call: PreviewCall, progress: ProgressSink? = nil, _ result: () throws -> T) async throws -> T {
        calls.append(call)
        var steps: [PreviewBehavior] = []
        if var queue = scripts[call.method], !queue.isEmpty {
            steps = queue.removeFirst()
            scripts[call.method] = queue
        }
        do {
            if case .failure(let error) = scenario.identity { throw error }
            try Task.checkCancellation()
            for step in steps {
                switch step {
                case .succeed(let delay): try await pause(call.method, for: delay)
                case .fail(let error): throw error
                case .suspendUntilResumed: try await pause(call.method, for: nil)
                case .emit(let events): for event in events { progress?(event) }
                }
            }
            try Task.checkCancellation()
            return try result()
        } catch {
            // CancellationError (including an ended wait) becomes `.cancelled(note: nil)`.
            throw BackendError.normalize(error)
        }
    }

    /// Waits `delay`, or until resumed when `delay` is nil. Throws `CancellationError` when cancelled or terminated.
    private func pause(_ method: PreviewMethod, for delay: Duration?) async throws {
        if let delay, delay <= .zero { return }
        let pending = PendingWait(method: method, resumable: delay == nil)
        waits.append(pending)
        let ending = await pending.wait.wait(timeout: delay)
        waits.removeAll { $0.id == pending.id }
        if ending == .interrupted { throw CancellationError() }
    }

    // MARK: Canned data

    private func supports(_ capability: Capability) -> Bool {
        (try? scenario.identity.get())?.supports(capability) ?? false
    }

    /// Mirrors how a legacy CLI answers a command it does not have.
    private func require(_ capability: Capability) throws {
        guard supports(capability) else { throw BackendError.unsupported(capability, minimumCLI: "0.14.0") }
    }

    private func listing(for project: ProjectRef) -> FeatureListing {
        listings[project.path] ?? scenario.listing
    }

    private func findRecord(named name: String, in project: ProjectRef) -> FeatureRecord? {
        listing(for: project).features.first { $0.workFeature == name }
    }

    private func update(_ project: ProjectRef, features: (FeatureListing) -> [FeatureRecord]) {
        let listing = listing(for: project)
        listings[project.path] = FeatureListing(features: features(listing), strays: listing.strays,
                                                droppedRecords: listing.droppedRecords, warnings: listing.warnings)
    }

    /// The teardown plan: `teardown --dry-run --json` on contract CLIs, the app preflight on legacy ones.
    private func plan(for request: TeardownRequest) -> TeardownPlanDocument {
        let name = request.feature.name
        let record = findRecord(named: name, in: request.feature.project)
        let path = record?.worktreePath ?? Self.worktreePath(for: name, in: request.feature.project)
        let branchName = request.recordedBranch ?? record?.branchName ?? "feature/\(name)"
        let dirty = scenario.dirtyFeatures[name] ?? []
        let ahead = scenario.unmergedBranches[name] ?? 0
        typealias Plan = TeardownPlanDocument
        let branch = Plan.Branch(name: branchName, source: "registry", exists: true, reference: "HEAD",
                                 referenceName: "main", merged: ahead == 0, mergedIntoHead: ahead == 0, ahead: ahead,
                                 action: Self.action(for: request.branch))
        let defaults = Plan.Defaults(deleteBranchByDefault: ProjectConfig.defaults.deleteBranchByDefault,
                                     forceDeleteUnmergedByDefault: ProjectConfig.defaults.forceDeleteUnmergedByDefault)
        var blockers: [Plan.Blocker] = []
        if !dirty.isEmpty, request.discard == nil {
            blockers.append(Plan.Blocker(kind: "uncommitted_changes",
                                         message: "\(dirty.count) uncommitted change\(dirty.count == 1 ? "" : "s") would be lost",
                                         override: "--discard-changes", count: dirty.count))
        }
        if ahead > 0, request.branch == .deleteIfMerged {
            blockers.append(Plan.Blocker(kind: "unmerged_branch", message: "\(branchName) has \(ahead) unmerged commits",
                                         override: "--keep-branch | --force-delete-branch", branch: branchName, ahead: ahead))
        }
        return Plan(source: supports(.teardownPlan) ? .cli : .appPreflight,
                    workFeature: name, registered: record != nil, status: record?.status,
                    worktree: Plan.Worktree(path: path, exists: record.map { $0.status != .removed } ?? false),
                    changes: Plan.Changes(statusAvailable: true, user: dirty), branch: branch, defaults: defaults,
                    runtime: record.map { Plan.RuntimeRef(provider: $0.runtime.provider.raw, runtimeID: $0.runtime.runtimeID) },
                    tunnel: record?.tunnel.map { Plan.TunnelRef(status: $0.status) }, blockers: blockers)
    }

    /// What the CLI (0.14) or the app's legacy preflight says about a dirty worktree: a cause-naming refusal
    /// carrying the plan, so a recovery can offer to discard exactly those files.
    private func uncommittedChangesRefusal(_ request: TeardownRequest, files: [ChangedFile]) -> BackendError {
        let name = request.feature.name
        let list = files.map(\.path).joined(separator: ", ")
        let message = "Refusing to tear down '\(name)'; nothing was removed. \(files.count) uncommitted "
            + "change\(files.count == 1 ? "" : "s") would be lost: \(list)."
        var unconsented = request
        unconsented.discard = nil
        return .refused(Refusal(cause: .uncommittedChanges(files: files), message: message,
                                diagnostics: Diagnostics(summary: message), plan: plan(for: unconsented)))
    }

    private func doctorReport(for project: ProjectRef?, problem: BackendError?) -> DoctorReport {
        let cli: DoctorCheck = switch (scenario.identity, problem) {
        case (.success(let identity), nil):
            DoctorCheck(id: "branchbox.cli", title: "BranchBox CLI", required: true, status: .ok,
                        path: "/opt/homebrew/bin/branchbox", version: identity.version.description)
        case (.failure(let error), _), (_, let error?):
            DoctorCheck(id: "branchbox.cli", title: "BranchBox CLI", required: true, status: .error,
                        detail: String(describing: error), remediation: "brew install branchbox/tap/branchbox")
        }
        var checks = [cli] + PreviewSamples.hostChecks
        if project != nil { checks += PreviewSamples.repoChecks }
        return DoctorReport(source: supports(.doctor) ? .cli : .app, checks: checks)
    }

    private static func worktreePath(for name: String, in project: ProjectRef) -> String {
        project.root.deletingLastPathComponent().appendingPathComponent(name).path
    }

    private static func action(for policy: BranchPolicy) -> String {
        switch policy {
        case .keep: "keep"
        case .deleteIfMerged: "delete"
        case .forceDelete: "force_delete"
        }
    }

    /// Lowercase letters and digits, every other run of characters one "-".
    private static func slug(_ input: String) -> String {
        var slug = ""
        for character in input.lowercased() {
            if character.isASCII, character.isLetter || character.isNumber {
                slug.append(character)
            } else if !slug.isEmpty, slug.last != "-" {
                slug.append("-")
            }
        }
        while slug.last == "-" { slug.removeLast() }
        return slug
    }
}

private struct PendingWait {
    let id = UUID()
    let method: PreviewMethod
    let resumable: Bool                                           // false for a timed `.succeed(after:)` wait
    let wait = PreviewWait()
}

/// A one-shot wait that ends when its timeout passes, when `end(_:)` is called, or when its task is cancelled,
/// whichever comes first. Cancellation resumes it directly, without a hop through the actor.
final class PreviewWait: @unchecked Sendable {
    enum Ending: Sendable { case resumed, interrupted }

    private let lock = NSLock()
    private var continuation: CheckedContinuation<Ending, Never>?
    private var ending: Ending?

    var hasEnded: Bool { lock.withLock { ending != nil } }

    func wait(timeout: Duration?) async -> Ending {
        let timer = timeout.map { delay in
            Task { [self] in
                try? await Task.sleep(for: delay)
                end(.resumed)
            }
        }
        defer { timer?.cancel() }
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (continuation: CheckedContinuation<Ending, Never>) in
                let ended: Ending? = lock.withLock {
                    if let ending { return ending }
                    self.continuation = continuation
                    return nil
                }
                if let ended { continuation.resume(returning: ended) }
            }
        } onCancel: {
            end(.interrupted)
        }
    }

    func end(_ value: Ending) {
        let continuation: CheckedContinuation<Ending, Never>? = lock.withLock {
            guard ending == nil else { return nil }
            ending = value
            defer { self.continuation = nil }
            return self.continuation
        }
        continuation?.resume(returning: value)
    }
}

private extension FeatureRecord {
    func removed(at date: Date) -> FeatureRecord {
        FeatureRecord(workFeature: workFeature, branchName: branchName, worktreePath: worktreePath, baseBranch: baseBranch,
                      featureURL: featureURL, composeProjectName: composeProjectName, envPath: envPath, status: .removed,
                      createdAt: createdAt, updatedAt: date, removedAt: date, lastSyncAt: lastSyncAt, tunnel: tunnel,
                      color: color, lastCommit: lastCommit, prNumber: prNumber, devcontainerOutdated: devcontainerOutdated,
                      syncStrategy: syncStrategy, startMode: startMode, promptSeed: promptSeed, moduleOutcomes: moduleOutcomes,
                      adapter: adapter, runtime: runtime, defaultAgent: defaultAgent, setup: nil)
    }
}
