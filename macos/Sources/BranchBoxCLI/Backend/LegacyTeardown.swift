import BranchBoxKit
import Foundation

/// The app preflight (DESIGN §6.5 planTeardown, CLIs without `teardown-plan`): builds the same document as
/// `feature teardown --dry-run --json` from git and the file system, with `source == .appPreflight`.
///
/// - Changes: `git status` of the worktree, classified by `WorktreeChangeClassifier`.
/// - Branch: the recorded branch (else `<config prefix>/<name>`), its existence and its `git branch -d` merge state.
/// - Worktree: existence on disk and the lock from `git worktree list --porcelain`.
/// - Blockers exactly as §5.5, with `override` naming the app's recoveries.
struct TeardownPreflight: Sendable {
    let git: GitInspector
    /// The feature's registry record, when it has one.
    let record: FeatureRecord?
    let config: ProjectConfig
    let fileSystem: any FileSystemProbing
    /// The CLI version the plan is made for (0.13.x moves the spec to main itself, so it needs no warning).
    let cliVersion: String

    func plan(for request: TeardownRequest) async throws -> TeardownPlanDocument {
        let root = request.feature.project.path
        let name = request.feature.name
        let worktreePath = record?.worktreePath ?? Paths.join(Paths.parent(root), name)
        let exists = fileSystem.kind(at: worktreePath) == .directory

        let entry = try await git.worktrees(in: root).first { Paths.same($0.path, worktreePath) }
        let worktree = TeardownPlanDocument.Worktree(path: worktreePath, exists: exists, locked: entry?.locked ?? false,
                                                     lockReason: entry?.lockReason)

        var blockers: [TeardownPlanDocument.Blocker] = []
        var changes = TeardownPlanDocument.Changes(statusAvailable: true)
        if exists {
            do {
                let entries = try await git.status(of: worktreePath)
                changes = try await classify(entries, worktree: worktreePath, root: root, request: request)
            } catch let error as BackendError {
                let cause = Self.summary(of: error)
                changes = .unavailable
                blockers.append(.init(kind: "status_unavailable",
                                      message: "git status failed in \(worktreePath), so what a removal would lose is unknown: \(cause)",
                                      override: "Force removal", cause: cause))
            }
        }

        let (branchName, source) = branch(for: request)
        let state = try await git.mergeState(of: branchName, in: root)
        let action: String
        switch request.branch {
        case .keep: action = "keep"
        case .deleteIfMerged: action = "delete"
        case .forceDelete: action = "force_delete"
        }
        let branch = TeardownPlanDocument.Branch(name: branchName, source: source, exists: state.exists,
                                                 upstream: state.upstream, reference: state.reference,
                                                 referenceName: state.referenceName, merged: state.merged,
                                                 mergedIntoHead: state.mergedIntoHead, ahead: state.ahead,
                                                 action: action)

        let uncovered = LegacyTeardown.uncovered(changes.user, by: request.discard)
        if !uncovered.isEmpty {
            blockers.insert(.init(kind: "uncommitted_changes",
                                  message: "\(LegacyTeardown.count(changes.user.count, "uncommitted change")) in \(worktreePath) would be lost: \(LegacyTeardown.list(changes.user.map(\.path)))",
                                  override: "Discard the changes", count: changes.user.count), at: 0)
        }
        if request.branch == .deleteIfMerged, state.exists, !state.merged {
            blockers.append(.init(kind: "unmerged_branch",
                                  message: "\(branchName) has \(LegacyTeardown.count(state.ahead, "commit")) not in \(state.referenceName)",
                                  override: "Keep the branch, or force-delete it", branch: branchName,
                                  ahead: state.ahead))
        }
        if worktree.locked {
            blockers.append(.init(kind: "worktree_locked",
                                  message: "\(worktreePath) is locked" + (worktree.lockReason.map { ": \($0)" } ?? ""),
                                  override: "Force removal"))
        }
        return TeardownPlanDocument(source: .appPreflight, workFeature: name, registered: record != nil,
                                    status: record?.status, worktree: worktree, changes: changes, branch: branch,
                                    defaults: .init(deleteBranchByDefault: config.deleteBranchByDefault,
                                                    forceDeleteUnmergedByDefault: config.forceDeleteUnmergedByDefault),
                                    runtime: record.map { .init(provider: $0.runtime.provider.raw,
                                                                runtimeID: $0.runtime.runtimeID) },
                                    tunnel: record?.tunnel.map { .init(status: $0.status) }, blockers: blockers,
                                    warnings: [])
    }

    /// The branch a teardown concerns: the recorded one, else `<config prefix>/<name>`.
    func branch(for request: TeardownRequest) -> (name: String, source: String) {
        if let recorded = request.recordedBranch, !recorded.isEmpty { return (recorded, "registry") }
        if let recorded = record?.branchName, !recorded.isEmpty { return (recorded, "registry") }
        let prefix = config.branchPrefix
        return (prefix.isEmpty ? request.feature.name : "\(prefix)/\(request.feature.name)", "config_prefix")
    }

    private func classify(_ entries: [GitStatusEntry], worktree: String, root: String,
                          request: TeardownRequest) async throws -> TeardownPlanDocument.Changes {
        var committed: [String: Data] = [:]
        if let settings = entries.first(where: { $0.path == ".vscode/settings.json" && !$0.isUntracked }) {
            committed[settings.path] = try await git.contents(of: settings.path, in: worktree)
        }
        let context = FileSystemChangeContext(worktree: worktree, main: root, committed: committed)
        return WorktreeChangeClassifier.classify(entries, context: context, completeSpec: request.completeSpec).changes
    }

    /// One readable line naming the cause, for plan warnings, blockers and branch outcomes (never an enum dump).
    static func summary(of error: BackendError) -> String {
        switch error {
        case .commandFailed(let diagnostics): return diagnostics.summary
        case .timedOut(_, let after, _): return "timed out after \(ToolInvocation.describe(after))"
        case .launchFailed(let executable, let reason): return "\(executable): \(reason)"
        case .cliNotFound: return "the BranchBox CLI was not found"
        case .cliTooOld(let found, let minimum, _): return "BranchBox CLI \(found) is older than \(minimum)"
        case .cliUnusable(_, let reason): return reason
        case .projectInvalid(.missing(let path)), .projectInvalid(.workingDirectoryMissing(let path)):
            return "the folder \(path) does not exist"
        case .projectInvalid(.notGitRepository(let path)): return "\(path) is not a git repository"
        case .projectInvalid(.notInitialized(let path)): return "BranchBox is not set up in \(path)"
        case .refused(let refusal): return refusal.diagnostics.summary
        case .partial(let failure): return failure.remaining.diagnostics.summary
        case .decodeFailed(let what, let detail, _): return "could not read \(what): \(detail)"
        case .registryCorrupted(let path, _): return "the feature registry \(path) cannot be read"
        case .unsupported(let capability, let minimumCLI):
            return "needs BranchBox CLI \(minimumCLI) or later (\(capability.rawValue))"
        case .cancelled(let note): return note ?? "cancelled"
        }
    }
}

/// The decisions of `teardownFeature` that need no I/O (DESIGN §6.5 steps 2–4 and the forceRemoval guard), so the
/// safety rules are tested on their own.
enum LegacyTeardown {
    /// Why this request must not reach the CLI, or nil to go ahead. In order:
    ///
    /// - forceRemoval is honoured only when the worktree is gone, a `worktree_locked`/`status_unavailable` blocker
    ///   is present, or the consent covers every user change; otherwise `.uncommittedChanges`.
    /// - Step 2: user changes and no consent → `.uncommittedChanges` (in legacy mode nothing is spawned).
    /// - Step 3: consent that does not cover every current user path → `.uncommittedChanges(<the new paths>)`.
    /// - Step 4: delete-if-merged of an existing unmerged branch → `.unmergedBranch`.
    /// - A locked worktree or unreadable status without forceRemoval → `.worktreeLocked` / `.statusUnavailable`.
    static func refusal(for request: TeardownRequest, plan: TeardownPlanDocument, cliVersion: String?) -> Refusal? {
        let name = request.feature.name
        let user = plan.changes.user
        let uncoveredChanges = uncovered(user, by: request.discard)
        func refuse(_ cause: RefusalCause, _ message: String) -> Refusal {
            Refusal(cause: cause, message: message, diagnostics: Diagnostics(summary: message, cliVersion: cliVersion),
                    plan: plan)
        }
        let overridable = plan.blockers.contains { $0.kind == "worktree_locked" || $0.kind == "status_unavailable" }

        if plan.changes.truncated, request.discard?.includesUnlistedChanges != true {
            return refuse(.uncommittedChanges(files: user),
                          "Refusing to tear down '\(name)': the change list is truncated, so additional changes would be lost without your confirmation; nothing was removed")
        }

        if request.forceRemoval, plan.worktree.exists, !overridable, !uncoveredChanges.isEmpty {
            return refuse(.uncommittedChanges(files: uncoveredChanges),
                          "Refusing to force the removal of '\(name)': \(count(uncoveredChanges.count, "uncommitted change")) would be lost without your confirmation (\(list(uncoveredChanges.map(\.path)))); nothing was removed")
        }
        if request.discard == nil, !user.isEmpty {
            return refuse(.uncommittedChanges(files: user),
                          "Refusing to tear down '\(name)': \(count(user.count, "uncommitted change")) would be lost (\(list(user.map(\.path)))); nothing was removed")
        }
        if request.discard != nil, !uncoveredChanges.isEmpty {
            return refuse(.uncommittedChanges(files: uncoveredChanges),
                          "Refusing to tear down '\(name)': \(count(uncoveredChanges.count, "change")) appeared after you confirmed discarding (\(list(uncoveredChanges.map(\.path)))); nothing was removed")
        }
        if request.branch == .deleteIfMerged, let branch = plan.branch, branch.exists, !branch.merged {
            return refuse(.unmergedBranch(branch: branch.name, ahead: branch.ahead),
                          "Refusing to delete \(branch.name): it has \(count(branch.ahead, "commit")) not in \(branch.referenceName); keep the branch or force-delete it")
        }
        if !request.forceRemoval {
            if plan.worktree.locked {
                return refuse(.worktreeLocked(reason: plan.worktree.lockReason),
                              "Refusing to tear down '\(name)': its worktree is locked" + (plan.worktree.lockReason.map { " (\($0))" } ?? ""))
            }
            if let blocker = plan.blockers.first(where: { $0.kind == "status_unavailable" }) {
                let cause = blocker.cause ?? blocker.message
                return refuse(.statusUnavailable(cause: cause),
                              "Refusing to tear down '\(name)': git status failed, so what would be lost is unknown (\(cause))")
            }
        }
        return nil
    }

    /// Whether the plan's worktree holds BranchBox-generated or preserved files and nothing else: no user change,
    /// a readable and complete status, no lock and no blocker. Legacy teardown then passes `--force`, which removes
    /// exactly those files with `git worktree remove --force` (0.13.x moves the spec to main first).
    static func onlyGeneratedChanges(_ plan: TeardownPlanDocument) -> Bool {
        let changes = plan.changes
        return plan.worktree.exists && !plan.worktree.locked && plan.blockers.isEmpty && changes.statusAvailable
            && !changes.truncated && changes.droppedEntries == 0 && changes.user.isEmpty
            && !(changes.generated.isEmpty && changes.preserved.isEmpty)
    }

    /// The user changes whose paths the consent does not name; all of them without consent.
    static func uncovered(_ changes: [ChangedFile], by consent: DiscardConsent?) -> [ChangedFile] {
        guard let consent else { return changes }
        let confirmed = Set(consent.userFiles)
        return changes.filter { !confirmed.contains($0.path) }
    }

    /// The legacy branch step after `--keep-branch` (DESIGN §6.5 step 6): which `git branch` flag to run, if any.
    static func branchDeletionFlag(for policy: BranchPolicy) -> String? {
        switch policy {
        case .keep: return nil
        case .deleteIfMerged: return "-d"
        case .forceDelete: return "-D"
        }
    }

    /// 0.13.x's last-resort `remove_dir_all` after `git worktree remove` failed: the old data-loss path.
    static let manualRemovalWarning = "removed manually after git removal failed"

    static func count(_ value: Int, _ noun: String) -> String {
        "\(value) \(noun)\(value == 1 ? "" : "s")"
    }

    /// "a, b, c and 2 more".
    static func list(_ paths: [String], limit: Int = 3) -> String {
        guard paths.count > limit else { return paths.joined(separator: ", ") }
        return paths.prefix(limit).joined(separator: ", ") + " and \(paths.count - limit) more"
    }
}
