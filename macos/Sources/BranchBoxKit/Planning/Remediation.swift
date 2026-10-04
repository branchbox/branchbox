import Foundation

public enum RemediationAction: Sendable, Hashable {
    case resumeSetup(StartFeatureRequest)            // setup interrupted → start --reuse
    case retryRetainedRuntime(StartFeatureRequest)   // failed_retained / degraded sbx → --runtime sbx --reuse-runtime
    case recreateRuntime(StartFeatureRequest)        // orphaned with worktree present → start --reuse
    case startEnvironment(FeatureRef)                // container: devcontainer up
    case teardown(FeatureRef, preselect: BranchPolicy)
    case cleanUpMissingFolder(TeardownRequest)       // forceRemoval + .keep
    case syncDevcontainers(ProjectRef)
    case runDoctor
    case deleteLeftoverBranch(String, ProjectRef)
    case rerunSetup(StartFeatureRequest)             // active with failed modules → start --reuse --devcontainer-reuse preserve
    case copyCommand(String, label: String)          // e.g. "Copy Inspect Command" → sbx exec <id> bash
    case showLog(FeatureRef)                         // the feature's most recent setup log
}

// AttentionReason / AttentionItem are declared in Kit/Backend/Recovery.swift (§4.5, SW-0).

/// Health remediation for one feature record (§9.1): whether it needs attention, the callout explaining why,
/// and the actions that fix it. Rendered by the feature detail's health callout and the menu bar.
///
/// Precedence, first match wins: removed; setup still running (a live start is never offered a cleanup, even
/// while its folder is not there yet); folder missing; setup interrupted; unknown status; failed_retained;
/// orphaned; degraded; active with failed modules. Actions that start the feature again are offered only for
/// runtimes the app can start (container, sbx, local-vm), and keeping a retained runtime only for sbx (core
/// supports `--reuse-runtime` there alone). A dev container config that is out
/// of date only adds an Update All Workspaces action. `degraded` never happens for the container runtime (core
/// only reports it for sandboxes and VMs); if it does, Start Environment is offered.
public enum Remediation {
    public static func attention(for record: FeatureRecord, folderExists: Bool) -> AttentionReason? {
        switch condition(of: record, folderExists: folderExists) {
        case .removed, .settingUp, .healthy: nil
        case .folderMissing: .folderMissing
        case .worktreeInvalid: .worktreeInvalid
        case .interrupted: .interrupted
        case .unknownStatus(let raw): .unknownStatus(raw)
        case .failedRetained: .failedRetained
        case .orphaned: .orphaned
        case .degraded: .degraded
        case .failedModules(let modules): .setupIncomplete(module: modules[0].module)
        }
    }

    public static func actions(for record: FeatureRecord, project: ProjectRef, identity: BackendIdentity?,
                               folderExists: Bool) -> [RemediationAction] {
        actions(for: record, project: project, identity: identity, folderExists: folderExists, branchExists: nil)
    }

    /// `branchExists` is whether the record's branch is still a local branch (nil when unknown); it decides the
    /// Delete Branch action of a removed feature. Without a backend (`identity == nil`) only the actions that
    /// need none are returned: copying a command, the doctor and the log.
    public static func actions(for record: FeatureRecord, project: ProjectRef, identity: BackendIdentity?,
                               folderExists: Bool, branchExists: Bool?) -> [RemediationAction] {
        let feature = FeatureRef(project: project, name: record.workFeature)
        let tearDown = RemediationAction.teardown(feature, preselect: .keep)
        var actions: [RemediationAction]
        switch condition(of: record, folderExists: folderExists) {
        case .removed:
            actions = branchExists != false && !record.branchName.isEmpty
                ? [.deleteLeftoverBranch(record.branchName, project)] : []
        case .folderMissing:
            var request = TeardownRequest(feature: feature, recordedBranch: recordedBranch(record), branch: .keep)
            request.forceRemoval = true
            actions = [.cleanUpMissingFolder(request)]
        case .worktreeInvalid:
            let command = "git -C \(HostLaunchPlan.shellQuote(record.worktreePath ?? project.path)) rev-parse --git-dir"
            actions = [.copyCommand(command, label: "Copy Git Check Command")]
        case .interrupted:
            actions = appCanStart(record.runtime.provider)
                ? [.resumeSetup(startRequest(record, project: project, reuse: .existingWorktree(.fail))), tearDown]
                : [tearDown]
        case .settingUp:
            actions = []
        case .unknownStatus:
            actions = [.runDoctor]
        case .failedRetained:
            actions = retainedRuntimeActions(record, project: project) + [tearDown]
        case .orphaned:
            actions = appCanStart(record.runtime.provider)
                ? [.recreateRuntime(startRequest(record, project: project, reuse: .existingWorktree(.fail))), tearDown]
                : [tearDown]
        case .degraded:
            switch record.runtime.provider {
            case .sbx: actions = [.retryRetainedRuntime(startRequest(record, project: project, reuse: .retainedRuntime)), tearDown]
            case .container: actions = [.startEnvironment(feature), tearDown]
            default: actions = [tearDown]
            }
        case .failedModules:
            actions = appCanStart(record.runtime.provider)
                ? [.rerunSetup(startRequest(record, project: project, reuse: .existingWorktree(.preserve))), .showLog(feature)]
                : [.showLog(feature)]
        case .healthy:
            actions = []
        }
        if record.devcontainerOutdated, record.status != .removed, folderExists {
            actions.append(.syncDevcontainers(project))
        }
        guard identity != nil else { return actions.filter(\.needsNoBackend) }
        return actions
    }

    /// The health callout's text, or nil when the record has nothing to explain.
    public static func callout(for record: FeatureRecord, folderExists: Bool) -> String? {
        let name = record.workFeature
        switch condition(of: record, folderExists: folderExists) {
        case .removed:
            return "\(name) was torn down."
        case .folderMissing:
            return "The folder \(record.worktreePath ?? "of \(name)") is gone."
        case .worktreeInvalid:
            return "\(name)'s Git worktree needs repair."
        case .interrupted:
            return "Setup of \(name) was interrupted."
        case .settingUp:
            return "Setting up…"
        case .unknownStatus(let raw):
            return "Status “\(raw)” isn't recognised by this app version."
        case .failedRetained:
            return "Setup failed; the \(runtimeNoun(record.runtime.provider)) was kept so you can inspect it."
        case .orphaned:
            return "The runtime no longer exists; your files are untouched."
        case .degraded:
            return "The environment for \(name) isn't running."
        case .failedModules(let modules):
            let failures = modules.map { outcome in
                outcome.notes.first.map { "\(outcome.module) failed (\($0))" } ?? "\(outcome.module) failed"
            }
            return "Setup finished with problems: \(failures.joined(separator: "; "))."
        case .healthy:
            return record.devcontainerOutdated && folderExists ? "This workspace's dev container config is out of date." : nil
        }
    }

    // MARK: Conditions

    enum Condition: Hashable {
        case removed, folderMissing, worktreeInvalid, interrupted, settingUp, unknownStatus(String), failedRetained, orphaned, degraded
        case failedModules([ModuleOutcome])
        case healthy
    }

    static func condition(of record: FeatureRecord, folderExists: Bool) -> Condition {
        if record.status == .removed { return .removed }
        if record.setup?.state == .inProgress { return .settingUp }
        if !folderExists { return .folderMissing }
        if record.worktreeIssue != nil { return .worktreeInvalid }
        if record.setup?.state == .interrupted { return .interrupted }
        switch record.status {
        case .unknown(let raw): return .unknownStatus(raw)
        case .failedRetained: return .failedRetained
        case .orphaned: return .orphaned
        case .degraded: return .degraded
        case .active, .removed: break
        }
        let failed = record.moduleOutcomes.filter { $0.status == .failed }
        return failed.isEmpty ? .healthy : .failedModules(failed)
    }

    // MARK: Building blocks

    /// Retry keeps the retained sandbox, with the inspect command. Core accepts `--reuse-runtime` only for sbx
    /// (failed_retained comes from `--keep-runtime-on-failure`, which is sbx-only too), so other runtimes get none.
    private static func retainedRuntimeActions(_ record: FeatureRecord, project: ProjectRef) -> [RemediationAction] {
        guard record.runtime.provider == .sbx else { return [] }
        let retry = RemediationAction.retryRetainedRuntime(startRequest(record, project: project, reuse: .retainedRuntime))
        guard let id = record.runtime.runtimeID, !id.isEmpty else { return [retry] }
        return [retry, .copyCommand(HostLaunchPlan.sandboxShellCommand(runtimeID: id), label: "Copy Inspect Command")]
    }

    /// The runtimes `StartDraft` lets the app start on; in-guest features are started by a supervisor.
    private static func appCanStart(_ provider: RuntimeProvider) -> Bool {
        switch provider {
        case .container, .sbx, .localVM: true
        case .inGuest, .unknown: false
        }
    }

    /// A start request that re-runs setup for an existing feature: same name, runtime, mode, prompt and branch
    /// prefix as the record, never a new base.
    static func startRequest(_ record: FeatureRecord, project: ProjectRef, reuse: StartFeatureRequest.Reuse) -> StartFeatureRequest {
        var request = StartFeatureRequest(project: project, name: record.workFeature, runtime: record.runtime.provider)
        request.branchPrefix = record.branchPrefix
        request.mode = record.startMode == StartFeatureRequest.Mode.minimal.rawValue ? .minimal : .full
        request.prompt = record.promptSeed.flatMap { $0.isEmpty ? nil : $0 }
        request.reuse = reuse
        return request
    }

    private static func recordedBranch(_ record: FeatureRecord) -> String? {
        record.branchName.isEmpty ? nil : record.branchName
    }

    private static func runtimeNoun(_ provider: RuntimeProvider) -> String {
        switch provider {
        case .sbx: "sandbox"
        case .localVM: "VM"
        default: "runtime"
        }
    }
}

extension RemediationAction {
    /// Copying a command, the doctor and the log work without a backend.
    var needsNoBackend: Bool {
        switch self {
        case .copyCommand, .runDoctor, .showLog: true
        default: false
        }
    }
}
