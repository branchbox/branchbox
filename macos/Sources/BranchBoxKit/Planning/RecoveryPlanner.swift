import Foundation

/// Turns a failed operation into the recoveries the UI offers (§9.4).
///
/// A recovery appears only for the refusal or partial failure it answers, and only when the request it would
/// re-run is known (`context`). Every retry is the original request with exactly one decision changed. Retries
/// that can lose work are `destructive` and carry a confirmation naming what is lost. The only place
/// `DiscardConsent` is ever built is here, from the refused file list.
public enum RecoveryPlanner {
    /// Most files a confirmation lists before summarizing the rest.
    static let confirmationFileLimit = 20

    /// Recoveries appear ONLY for the matching refusal/partial. Destructive retries carry a confirmation naming what is lost.
    public static func recoveries(for error: BackendError, after context: OperationRequestContext?) -> [RecoveryAction] {
        recoveries(for: error, after: context, operation: nil)
    }

    /// As `recoveries(for:after:)`, adding `.showLog` for the failed operation where its log explains the failure.
    public static func recoveries(for error: BackendError, after context: OperationRequestContext?,
                                  operation: UUID?) -> [RecoveryAction] {
        let showLog = operation.map { [RecoveryAction.showLog(operation: $0)] } ?? []
        switch error {
        case .cliNotFound:
            return [.locateCLI, .copyCommand("brew install branchbox/tap/branchbox", label: "Copy Install Command")]
        case .cliTooOld:
            return [.copyCommand("brew upgrade branchbox", label: "Copy Upgrade Command"), .locateCLI]
        case .cliUnusable:
            return [.locateCLI, .openDoctor]
        case .launchFailed:
            return showLog + [.openDoctor]
        case .projectInvalid(.workingDirectoryMissing):
            return context.flatMap(project(of:)).map { [.refresh($0)] } ?? []
        case .projectInvalid, .unsupported, .cancelled:
            return []
        case .refused(let refusal):
            return refused(refusal, after: context) + (isBackendFailure(refusal.cause) ? showLog : [])
        case .partial(let partial):
            return partialRecoveries(partial, after: context) + showLog
        case .commandFailed:
            if case .tunnelRemove(let feature, force: false)? = context {
                return [removeTunnelAnyway(feature)] + showLog + [.openDoctor]
            }
            return showLog + [.openDoctor]
        case .decodeFailed, .registryCorrupted:
            return showLog + [.openDoctor]
        case .timedOut:
            return tryAgain(context) + showLog
        }
    }

    // MARK: Refusals

    private static func refused(_ refusal: Refusal, after context: OperationRequestContext?) -> [RecoveryAction] {
        switch (refusal.cause, context) {
        case (.uncommittedChanges(let files), .teardown(let request)?):
            return [discardAndTearDown(request, files: files, truncated: refusal.plan?.changes.truncated == true)]
                + reveal(refusal)
        case (.uncommittedChanges(let files), .removeStray(let stray, let project, discardChanges: false)?):
            return [discardAndRemoveStray(stray, project: project, files: files), .revealInFinder(path: stray.path)]
        case (.moduleFilesDirty(let generated, let userChanges), .teardown(let request)?):
            if userChanges.isEmpty { return [discardGeneratedAndTearDown(request, generated: generated)] }
            return [discardAndTearDown(request, files: userChanges, truncated: refusal.plan?.changes.truncated == true)]
                + reveal(refusal)
        case (.unmergedBranch(let branch, let ahead), .teardown(let request)?):
            var keep = request
            keep.branch = .keep
            var force = request
            force.branch = .forceDelete
            return [
                .retry(.teardown(keep), label: "Keep \(branch) and tear down", destructive: false, confirmation: nil),
                .retry(.teardown(force), label: "Force-delete \(branch) and tear down…", destructive: true,
                       confirmation: unmergedConfirmation(branch: branch, ahead: ahead)),
            ]
        case (.unmergedBranch(let branch, let ahead), .deleteBranch(let name, let project, force: false)?):
            return [forceDeleteBranch(name.isEmpty ? branch : name, project: project, ahead: ahead)]
        case (.worktreeLocked(let reason), .teardown(let request)?):
            let why = reason.map { " (\($0))" } ?? ""
            let confirmation = "The worktree is locked\(why). Removing it anyway deletes "
                + forcedRemovalConsequence(refusal, request: request)
            return [forcedRemoval(request, label: "Remove the locked worktree…", destructive: true, confirmation: confirmation)]
        case (.statusUnavailable(let cause), .teardown(let request)?):
            let confirmation = "BranchBox couldn't read the worktree's status (\(cause)), so it can't tell whether it "
                + "holds unsaved work. Removing it deletes " + forcedRemovalConsequence(refusal, request: request)
            return [forcedRemoval(request, label: "Remove without checking for changes…", destructive: true,
                                  confirmation: confirmation)]
        case (.worktreeNotFound, .teardown(let request)?):
            // The folder is already gone, so forcing removal only cleans up the registry entry.
            return [forcedRemoval(request, label: "Clean up the missing worktree", destructive: false, confirmation: nil)]
        case (.worktreeExists(let path), .start(let request)?):
            var reuse = request
            reuse.reuse = .existingWorktree(.fail)
            return [.retry(.start(reuse), label: "Start in the existing folder", destructive: false, confirmation: nil),
                    .revealInFinder(path: path)]
        case (.runtimePrerequisite(let provider, _), _):
            guard provider == RuntimeProvider.sbx.raw else { return [.openDoctor] }
            return [.runInTerminal(command: ["sbx", "login"], workingDirectory: nil, label: "Sign in to Docker Sandboxes"),
                    .openDoctor]
        case (.registryLocked, _):
            return tryAgain(context)
        case (.featureNotFound, _):
            return context.flatMap(project(of:)).map { [.refresh($0)] } ?? []
        case (.other, .tunnelRemove(let feature, force: false)?):
            return [removeTunnelAnyway(feature)]
        default:
            return []
        }
    }

    // MARK: Partial failures

    private static func partialRecoveries(_ partial: PartialFailure, after context: OperationRequestContext?) -> [RecoveryAction] {
        guard case .unmergedBranch(let branch, let ahead) = partial.remaining.cause,
              let project = context.flatMap(project(of:)) else { return [] }
        return [forceDeleteBranch(branch, project: project, ahead: ahead)]
    }

    // MARK: Building blocks

    /// Consent to exactly the refused files. A retry that was already discarding (new changes appeared since
    /// the user confirmed) keeps the files confirmed before and adds the new ones, which the confirmation lists.
    /// When the plan's change list was `truncated`, the teardown deletes more than the listed files, and the label
    /// and confirmation say so.
    private static func discardAndTearDown(_ request: TeardownRequest, files: [ChangedFile],
                                           truncated: Bool = false) -> RecoveryAction {
        var retry = request
        retry.discard = DiscardConsent(userFiles: consentedPaths(request.discard, adding: files.map(\.path)))
        let label: String
        if truncated {
            label = "Discard more than \(files.count) changes and tear down…"
        } else {
            label = files.count == 1 ? "Discard 1 change and tear down…" : "Discard \(files.count) changes and tear down…"
        }
        var confirmation = "These changes in \(request.feature.name) will be permanently deleted:\n" + fileList(files)
        if truncated {
            confirmation += "\nThe list is truncated: other changes in this folder are deleted too."
        }
        if let earlier = request.discard, !earlier.userFiles.isEmpty {
            let count = earlier.userFiles.count
            confirmation += "\n…along with the \(count == 1 ? "change" : "\(count) changes") you confirmed before."
        }
        return .retry(.teardown(retry), label: label, destructive: true, confirmation: confirmation)
    }

    private static func discardGeneratedAndTearDown(_ request: TeardownRequest, generated: [String]) -> RecoveryAction {
        var retry = request
        retry.discard = DiscardConsent(userFiles: consentedPaths(request.discard, adding: []))
        var confirmation = "No changes of yours were found in \(request.feature.name). "
            + "Tearing it down deletes the files BranchBox generated there"
        confirmation += generated.isEmpty ? "." : ":\n" + bulletList(generated)
        return .retry(.teardown(retry), label: "Discard BranchBox-generated files and tear down", destructive: true,
                      confirmation: confirmation)
    }

    /// The paths of `earlier` consent followed by the new ones it lacks, in order.
    private static func consentedPaths(_ earlier: DiscardConsent?, adding paths: [String]) -> [String] {
        let previous = earlier?.userFiles ?? []
        return previous + paths.filter { !previous.contains($0) }
    }

    private static func discardAndRemoveStray(_ stray: StrayWorktree, project: ProjectRef, files: [ChangedFile]) -> RecoveryAction {
        let label = files.count == 1 ? "Discard 1 change and remove the worktree…"
            : "Discard \(files.count) changes and remove the worktree…"
        return .retry(.removeStray(stray, project, discardChanges: true), label: label, destructive: true,
                      confirmation: "These changes in \(stray.path) will be permanently deleted:\n" + fileList(files))
    }

    private static func forcedRemoval(_ request: TeardownRequest, label: String, destructive: Bool,
                                      confirmation: String?) -> RecoveryAction {
        var retry = request
        retry.forceRemoval = true
        retry.branch = .keep                         // D-11: forced removal is always paired with --keep-branch
        return .retry(.teardown(retry), label: label, destructive: destructive, confirmation: confirmation)
    }

    private static func forceDeleteBranch(_ branch: String, project: ProjectRef, ahead: Int?) -> RecoveryAction {
        .retry(.deleteBranch(branch, project, force: true), label: "Force-delete \(branch)…", destructive: true,
               confirmation: unmergedConfirmation(branch: branch, ahead: ahead))
    }

    private static func removeTunnelAnyway(_ feature: FeatureRef) -> RecoveryAction {
        .retry(.tunnelRemove(feature, force: true), label: "Remove Anyway…", destructive: true,
               confirmation: "BranchBox couldn't remove the tunnel for \(feature.name) at its provider. Removing it anyway "
                   + "forgets it here; the provider's tunnel and DNS record may be left behind for you to delete.")
    }

    /// The same request again, offered only when repeating it cannot lose anything the user has not already
    /// confirmed in this presentation.
    private static func tryAgain(_ context: OperationRequestContext?) -> [RecoveryAction] {
        guard let context, !isDestructive(context) else { return [] }
        return [.retry(context, label: "Try Again", destructive: false, confirmation: nil)]
    }

    private static func reveal(_ refusal: Refusal) -> [RecoveryAction] {
        guard let worktree = refusal.plan?.worktree, worktree.exists else { return [] }
        return [.revealInFinder(path: worktree.path)]
    }

    static func unmergedConfirmation(branch: String, ahead: Int?) -> String {
        let commits = switch ahead {
        case nil: "commits that aren't merged"
        case 1?: "1 commit that isn't merged"
        case let count?: "\(count) commits that aren't merged"
        }
        return "\(branch) has \(commits). Force-deleting it loses them unless they were pushed or are on another branch."
    }

    /// "<folder> and everything in it. <branch> is kept."
    private static func forcedRemovalConsequence(_ refusal: Refusal, request: TeardownRequest) -> String {
        let folder = refusal.plan?.worktree.path ?? "the folder of \(request.feature.name)"
        let branch = request.recordedBranch.map { "\($0) is kept." } ?? "The branch is kept."
        return "\(folder) and everything in it. \(branch)"
    }

    static func fileList(_ files: [ChangedFile]) -> String {
        bulletList(files.map { $0.kind.isEmpty ? $0.path : "\($0.path) (\($0.kind))" })
    }

    static func bulletList(_ items: [String]) -> String {
        var lines = items.prefix(confirmationFileLimit).map { "• \($0)" }
        if items.count > confirmationFileLimit { lines.append("…and \(items.count - confirmationFileLimit) more") }
        return lines.joined(separator: "\n")
    }

    static func project(of context: OperationRequestContext) -> ProjectRef? {
        switch context {
        case .start(let request): request.project
        case .teardown(let request): request.feature.project
        case .prune(let selection): selection.project
        case .exec(let request): request.feature.project
        case .devcontainer(_, let feature), .tunnelOpen(let feature), .tunnelRemove(let feature, _): feature.project
        case .syncDevcontainers(let request): request.project
        case .initProject: nil
        case .applyConfig(_, let project), .tunnelCredentials(_, let project), .deleteBranch(_, let project, _),
             .removeStray(_, let project, _):
            project
        }
    }

    /// Whether running `context` can lose work: discarding changes, forcing removal or branch deletion, removing
    /// volumes, or forgetting a tunnel the provider still has.
    static func isDestructive(_ context: OperationRequestContext) -> Bool {
        switch context {
        case .teardown(let request): isDestructive(request)
        case .prune(let selection): selection.rows.contains(where: isDestructive)
        case .devcontainer(.down(let removeVolumes), _): removeVolumes
        case .tunnelRemove(_, let force): force
        case .deleteBranch(_, _, let force): force
        case .removeStray(_, _, let discardChanges): discardChanges
        case .initProject(let request): request.reorganize
        case .start, .exec, .devcontainer, .syncDevcontainers, .tunnelOpen, .applyConfig, .tunnelCredentials: false
        }
    }

    /// Discards changes, forces removal or force-deletes the branch.
    static func isDestructive(_ request: TeardownRequest) -> Bool {
        request.discard != nil || request.forceRemoval || request.branch == .forceDelete
    }

    /// Refusals whose log tells more than their message: the CLI or git failed rather than declining on purpose.
    static func isBackendFailure(_ cause: RefusalCause) -> Bool {
        switch cause {
        case .worktreeRemovalFailed, .statusUnavailable, .other: true
        default: false
        }
    }
}

