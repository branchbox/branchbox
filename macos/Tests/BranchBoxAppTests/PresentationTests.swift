import AppKit
@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import BranchBoxTestSupport
import Foundation
import SwiftUI
import Testing

// SW-3: the presentation vocabulary (DESIGN §9), the error presentation table (§9.4), the diagnostic report, the
// log filter and HostLauncher's side effects behind a recording opener.

private let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/main"))
private let feature = FeatureRef(project: project, name: "eta")
private let diagnostics = Diagnostics(summary: "Error: git worktree remove failed", causes: ["Directory not empty"],
                                      exitCode: 1, logTail: ["Error: git worktree remove failed"],
                                      invocation: "branchbox feature teardown eta --json --keep-branch", cliVersion: "0.13.4")

private func refused(_ cause: RefusalCause, message: String = "Refusing to tear down 'eta'; nothing was removed.") -> BackendError {
    .refused(Refusal(cause: cause, message: message, diagnostics: diagnostics))
}

@Suite struct FeaturePresentationTests {
    static let statuses: [FeatureStatus] = [.active, .degraded, .failedRetained, .orphaned, .removed, .unknown("paused_by_admin")]

    @Test func everyStatusHasADistinctLabelAndSymbol() {
        let labels = Self.statuses.map(\.label)
        let symbols = Self.statuses.map(\.symbol)
        #expect(labels.allSatisfy { !$0.isEmpty })
        #expect(symbols.allSatisfy { !$0.isEmpty })
        #expect(Set(labels).count == labels.count)
        #expect(Set(symbols).count == symbols.count)
        #expect(NSImage(systemSymbolName: "circle.fill", accessibilityDescription: nil) != nil)
        for symbol in symbols {
            #expect(NSImage(systemSymbolName: symbol, accessibilityDescription: nil) != nil, "\(symbol) is not an SF Symbol")
        }
    }

    @Test(arguments: [
        (FeatureStatus.active, "Active", StatusTint.green, false),
        (.degraded, "Degraded", .orange, true),
        (.failedRetained, "Failed (kept)", .red, true),
        (.orphaned, "Orphaned", .purple, true),
        (.removed, "Removed", .gray, false),
        (.unknown("paused_by_admin"), "paused by admin", .gray, true),
        (.unknown(""), "Unknown", .gray, true),
    ])
    func statusVocabulary(status: FeatureStatus, label: String, tint: StatusTint, attention: Bool) {
        #expect(status.label == label)
        #expect(status.tint == tint)
        #expect(status.needsAttention == attention)
    }

    @Test func failedRetainedDecodedFromTheCLIReadsFailedKept() throws {
        let records = try CLIJSON.decode([FeatureRecord].self,
                                         from: Fixtures.data("cli-0.13.4/synthetic_feature_list_new_statuses.json")).value
        #expect(records.first { $0.workFeature == "retained" }?.status.label == "Failed (kept)")
    }

    @Test(arguments: [
        (TunnelStatus.pending, "Starting", StatusTint.blue), (.active, "Online", .green), (.manual, "Manual setup needed", .orange),
        (.disabled, "Off", .gray), (.unknown("rate_limited"), "rate limited", .gray),
    ])
    func tunnelLabels(status: TunnelStatus, label: String, tint: StatusTint) {
        #expect(status.label == label)
        #expect(status.tint == tint)
        #expect(NSImage(systemSymbolName: status.symbol, accessibilityDescription: nil) != nil)
    }

    /// Tints are semantic; each maps to a colour, and failures are red.
    @Test func tints() {
        #expect([ModuleStatus.success, .skipped, .failed, .unknown("x")].map(\.tint) == [.green, .gray, .red, .gray])
        let reasons: [AttentionReason] = [.degraded, .failedRetained, .orphaned, .interrupted, .setupIncomplete(module: "m"),
                                          .folderMissing, .unknownStatus("x"), .unregisteredWorktree]
        #expect(reasons.map(\.tint) == [.orange, .red, .purple, .orange, .orange, .red, .gray, .orange])
        let states: [OperationState] = [.queued(behind: "x"), .running, .succeeded, .succeededWithWarnings, .partial,
                                        .failed(.cancelled(note: nil)), .cancelled(note: nil)]
        #expect(states.map(\.tint) == [.gray, .blue, .green, .orange, .orange, .red, .gray])
        let levels: [LogLevel] = [.trace, .debug, .info, .warn, .error, .output]
        #expect(levels.map(\.tint) == [.gray, .gray, .blue, .orange, .red, .gray])
        let all: [StatusTint] = [.green, .orange, .red, .purple, .blue, .gray]
        #expect(all.map(\.color) == [.green, .orange, .red, .purple, .blue, .secondary])
    }

    @Test func runtimeAndModuleVocabulary() {
        let providers: [RuntimeProvider] = [.container, .sbx, .localVM, .inGuest, .unknown("fire_cracker")]
        #expect(providers.map(\.label) == ["Container", "Docker Sandbox", "Local VM", "In-guest", "fire cracker"])
        #expect(providers.map(\.symbol) == ["shippingbox", "lock.shield", "cpu", "square.dashed", "questionmark.square.dashed"])
        let modules: [ModuleStatus] = [.success, .skipped, .failed, .unknown("deferred")]
        #expect(modules.map(\.label) == ["OK", "Skipped", "Failed", "deferred"])
        #expect(Set(modules.map(\.symbol)).count == modules.count)
    }

    @Test func attentionVocabulary() {
        let reasons: [AttentionReason] = [.degraded, .failedRetained, .orphaned, .interrupted, .setupIncomplete(module: "compose"),
                                          .folderMissing, .worktreeInvalid, .unknownStatus("x_y"), .unregisteredWorktree]
        #expect(reasons.map(\.label) == ["Degraded", "Failed (kept)", "Orphaned", "Interrupted", "Setup incomplete",
                                         "Folder missing", "Git worktree broken", "x y", "Unregistered worktree"])
        #expect(reasons.allSatisfy { NSImage(systemSymbolName: $0.symbol, accessibilityDescription: nil) != nil })
    }

    @Test func moduleSummary() throws {
        let prine = try #require(PreviewSamples.features.first { $0.workFeature == "prine" })
        #expect(FeaturePresentation.moduleSummary(prine.moduleOutcomes) == "3 ok · 1 skipped · 0 failed")
        #expect(FeaturePresentation.moduleSummary([]) == "No modules recorded")
        let mixed = [ModuleOutcome(module: "a", status: .failed), ModuleOutcome(module: "b", status: .unknown("later")),
                     ModuleOutcome(module: "c", status: .success)]
        #expect(FeaturePresentation.moduleSummary(mixed) == "1 ok · 0 skipped · 1 failed · 1 other")
    }

    @Test(arguments: [(0, "0 ms"), (190, "190 ms"), (1200, "1.2 s"), (5000, "5 s"), (125_000, "2 min 5 s"), (-3, "0 ms")])
    func moduleDurations(milliseconds: Int, text: String) {
        #expect(FeaturePresentation.duration(milliseconds: milliseconds) == text)
    }

    @Test func datesUseFormatStyles() {
        let english = Locale(identifier: "en_US")
        #expect(FeaturePresentation.relative(Date.now.addingTimeInterval(-7_200), locale: english) == "2 hours ago")
        #expect(FeaturePresentation.relative(Date.now.addingTimeInterval(-3 * 86_400), locale: english) == "3 days ago")
        let date = RFC3339.parse("2026-03-17T03:37:57Z")!
        #expect(FeaturePresentation.absolute(date, locale: english).contains("2026"))
    }

    @Test func subtitleAndAccessibility() throws {
        let prine = try #require(PreviewSamples.features.first { $0.workFeature == "prine" })
        #expect(FeaturePresentation.subtitle(for: prine, locale: Locale(identifier: "en_US")).hasPrefix("feature/prine from current HEAD · created "))
        #expect(FeaturePresentation.subtitle(for: FeatureRecord(workFeature: "x")) == "")
        #expect(FeaturePresentation.accessibilityLabel(for: prine, attention: nil) == "prine, Active, Container")
        let quick = FeatureRecord(workFeature: "q", status: .orphaned, startMode: "minimal", runtime: RuntimeInfo(provider: .sbx))
        #expect(FeaturePresentation.accessibilityLabel(for: quick, attention: .folderMissing)
            == "q, Orphaned, Docker Sandbox, Quick, needs attention: Folder missing")
        #expect(FeaturePresentation.accessibilityLabel(for: quick, attention: .orphaned) == "q, Orphaned, Docker Sandbox, Quick")
        #expect(FeaturePresentation.shortCommit("d3f308c112340957ce1fc42ec5383ce1b7294074") == "d3f308c")
        #expect(FeaturePresentation.shortCommit("") == nil)
    }

    @Test(arguments: [("#e67e22", true), ("e67e22", true), ("#fff", true), ("#ggg", false), ("#12345", false), (nil, false)]
          as [(String?, Bool)])
    func hexColors(hex: String?, valid: Bool) {
        #expect((HexColor.rgb(hex) != nil) == valid)
    }

    @Test func hexColorComponents() throws {
        let rgb = try #require(HexColor.rgb("#ff8000"))
        #expect(rgb.red == 1)
        #expect(abs(rgb.green - 128.0 / 255) < 0.0001)
        #expect(rgb.blue == 0)
        let short = try #require(HexColor.rgb("#f80"))
        let long = try #require(HexColor.rgb("#ff8800"))
        #expect(short == long)
    }

    @Test func attentionBadgeText() {
        #expect(AttentionBadge.accessibilityText(count: 1, noun: "feature") == "1 feature needs attention")
        #expect(AttentionBadge.accessibilityText(count: 3, noun: "item") == "3 items need attention")
    }
}

@Suite struct ErrorPresentationTests {
    struct Row: Sendable, CustomTestStringConvertible {
        let name: String
        let error: BackendError
        let context: OperationRequestContext?
        let style: ErrorPresentation.Style
        let title: String
        var testDescription: String { name }
    }

    /// DESIGN §9.4, one row per error kind (and per refusal cause).
    static let table: [Row] = [
        Row(name: "cli not found", error: .cliNotFound(searched: ["/opt/homebrew/bin/branchbox"]), context: nil,
            style: .blocking, title: "BranchBox CLI not found"),
        Row(name: "cli too old", error: .cliTooOld(found: SemVer(0, 13, 3), minimum: SemVer(0, 13, 4), path: "/usr/local/bin/branchbox"),
            context: nil, style: .blocking, title: "BranchBox CLI is too old"),
        Row(name: "cli unusable", error: .cliUnusable(path: "/x/branchbox", reason: "not executable"), context: nil,
            style: .blocking, title: "BranchBox CLI can't be used"),
        Row(name: "launch failed", error: .launchFailed(executable: "/usr/bin/git", reason: "ENOENT"), context: nil,
            style: .failure, title: "Couldn't run git"),
        Row(name: "project missing", error: .projectInvalid(.missing("/tmp/bbx/main")), context: nil, style: .failure,
            title: "The project folder is missing"),
        Row(name: "not git", error: .projectInvalid(.notGitRepository("/tmp")), context: nil, style: .failure,
            title: "Not a Git repository"),
        Row(name: "not initialized", error: .projectInvalid(.notInitialized("/tmp/x")), context: nil, style: .failure,
            title: "BranchBox isn't set up here"),
        Row(name: "cwd missing", error: .projectInvalid(.workingDirectoryMissing("/tmp/x")), context: nil, style: .failure,
            title: "A folder is missing"),
        Row(name: "dirty teardown", error: refused(.uncommittedChanges(files: [ChangedFile(path: "a.txt", kind: "untracked", area: "other")])),
            context: .teardown(TeardownRequest(feature: feature, recordedBranch: nil, branch: .keep)), style: .refusal,
            title: "Teardown stopped: uncommitted changes would be lost"),
        Row(name: "dirty stray", error: refused(.uncommittedChanges(files: [])),
            context: .removeStray(StrayWorktree(path: "/tmp/s", branch: nil, head: nil), project, discard: nil),
            style: .refusal, title: "Removal stopped: uncommitted changes would be lost"),
        Row(name: "generated files", error: refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: [])), context: nil,
            style: .refusal, title: "Teardown stopped: BranchBox-generated files changed"),
        Row(name: "unmerged", error: refused(.unmergedBranch(branch: "feature/eta", ahead: 2)), context: nil, style: .refusal,
            title: "Teardown stopped: feature/eta has unmerged commits"),
        Row(name: "locked", error: refused(.worktreeLocked(reason: nil)), context: nil, style: .refusal,
            title: "Teardown stopped: the worktree is locked"),
        Row(name: "status unavailable", error: refused(.statusUnavailable(cause: "x")), context: nil, style: .refusal,
            title: "Teardown stopped: couldn't check for unsaved work"),
        Row(name: "removal failed", error: refused(.worktreeRemovalFailed(cause: "busy")), context: nil, style: .refusal,
            title: "Couldn't remove the worktree"),
        Row(name: "exists", error: refused(.worktreeExists(path: "/tmp/bbx/eta")),
            context: .start(StartFeatureRequest(project: project, name: "eta", runtime: .container)), style: .refusal,
            title: "The folder eta already exists"),
        Row(name: "worktree gone", error: refused(.worktreeNotFound("eta")), context: nil, style: .refusal,
            title: "The worktree for eta is gone"),
        Row(name: "feature gone", error: refused(.featureNotFound("eta")), context: nil, style: .refusal, title: "No feature named eta"),
        Row(name: "branch exists", error: refused(.branchExists("feature/eta")), context: nil, style: .refusal,
            title: "Branch feature/eta already exists"),
        Row(name: "invalid name", error: refused(.invalidName("Bad Name")), context: nil, style: .refusal,
            title: "“Bad Name” isn't a valid feature name"),
        Row(name: "not a repo", error: refused(.notGitRepository("/")), context: nil, style: .refusal, title: "Not a Git repository"),
        Row(name: "sbx sign in", error: refused(.runtimePrerequisite(provider: "sbx", detail: "Sign in with: sbx login")),
            context: nil, style: .refusal, title: "Docker Sandbox isn't ready"),
        Row(name: "registry locked", error: refused(.registryLocked(path: "/tmp/bbx/main/.branchbox")), context: nil, style: .refusal,
            title: "Another BranchBox process is updating this project"),
        Row(name: "confirmation", error: refused(.confirmationRequired), context: .prune(PruneSelection(project: project, rows: [])),
            style: .refusal, title: "Teardown stopped: confirmation required"),
        Row(name: "config key", error: refused(.configInvalid(key: "feature.branch_prefix", detail: "bad")),
            context: .applyConfig(ConfigPatch(changes: []), project), style: .refusal, title: "Invalid setting: feature.branch_prefix"),
        Row(name: "config", error: refused(.configInvalid(key: nil, detail: "bad")), context: nil, style: .refusal,
            title: "Invalid project settings"),
        Row(name: "devcontainer source", error: refused(.devcontainerSourceMissing), context: .syncDevcontainers(SyncRequest(project: project)),
            style: .refusal, title: "The project has no .devcontainer folder"),
        Row(name: "other code", error: refused(.other(code: "validation_failed")), context: .tunnelOpen(feature), style: .refusal,
            title: "Sharing stopped: the CLI refused (validation_failed)"),
        Row(name: "partial", error: .partial(PartialFailure(completed: ["Worktree removed"],
                                                            remaining: Refusal(cause: .unmergedBranch(branch: "feature/eta", ahead: 1),
                                                                               message: "Branch could not be deleted", diagnostics: diagnostics))),
            context: nil, style: .partial, title: "Partly done: feature/eta has unmerged commits"),
        Row(name: "partial plain", error: .partial(PartialFailure(completed: [],
                                                                  remaining: Refusal(cause: .worktreeRemovalFailed(cause: "x"),
                                                                                     message: "m", diagnostics: diagnostics))),
            context: nil, style: .partial, title: "Partly done. Couldn't remove the worktree"),
        Row(name: "command failed", error: .commandFailed(diagnostics), context: nil, style: .failure, title: "The command failed"),
        Row(name: "decode failed", error: .decodeFailed(what: "feature list", detail: "bad", diagnostics: diagnostics), context: nil,
            style: .failure, title: "Couldn't read the CLI's output"),
        Row(name: "registry corrupted", error: .registryCorrupted(path: "/tmp/bbx/main/.branchbox/registry.json", diagnostics: diagnostics),
            context: nil, style: .failure, title: "The feature registry is damaged"),
        Row(name: "unsupported", error: .unsupported(.config, minimumCLI: "0.14.0"), context: nil, style: .unsupported,
            title: "Needs a newer BranchBox CLI"),
        Row(name: "timed out", error: .timedOut(operation: "command", after: .seconds(90), diagnostics: diagnostics), context: nil,
            style: .neutral, title: "Timed out"),
        Row(name: "cancelled", error: .cancelled(note: "may have left a partial worktree"), context: nil, style: .neutral,
            title: "Cancelled"),
    ]

    @Test(arguments: ErrorPresentationTests.table)
    func presentationTable(_ row: Row) {
        let presentation = row.error.presentation(context: row.context)
        #expect(presentation.style == row.style)
        #expect(presentation.title == row.title)
        #expect(!presentation.message.isEmpty)
        #expect(NSImage(systemSymbolName: presentation.symbol, accessibilityDescription: nil) != nil)
    }

    @Test func refusalsListTheirFilesAndMessage() {
        let files = [ChangedFile(path: "README.md", kind: "modified", area: "other"), ChangedFile(path: "notes", kind: "", area: "other")]
        let presentation = refused(.uncommittedChanges(files: files)).presentation()
        // App copy introduces the files; the CLI's "Refusing to tear down…" text stays in the log.
        #expect(presentation.message == "Nothing was removed. These files have changes that aren't committed:")
        #expect(refused(.uncommittedChanges(files: [files[0]])).presentation(context: nil).message
            == "Nothing was removed. This file has changes that aren't committed:")
        #expect(refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: [])).presentation().message
            == "Nothing was removed. This file that BranchBox generated was changed:")
        #expect(refused(.uncommittedChanges(files: [])).presentation().message == "Refusing to tear down 'eta'; nothing was removed.")
        #expect(presentation.details == ["README.md (modified)", "notes"])
        #expect(refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: [files[0]])).presentation().details
            == ["README.md (modified)", ".devcontainer/"])
        #expect(refused(.runtimePrerequisite(provider: "sbx", detail: "Sign in with: sbx login")).presentation().details
            == ["Sign in with: sbx login", "Directory not empty"])   // the detail, then the CLI's causes
    }

    @Test func failuresNameTheirSpecifics() {
        let registry = BackendError.registryCorrupted(path: "/r/.branchbox/registry.json", diagnostics: diagnostics).presentation()
        #expect(registry.message == "/r/.branchbox/registry.json can't be read, so every feature in this project is hidden until it is repaired.")
        #expect(registry.offersDiagnosticReport)
        #expect(registry.tint == .red)
        let failed = BackendError.commandFailed(diagnostics).presentation()
        #expect(failed.message == diagnostics.summary)
        #expect(failed.details == ["Directory not empty"])
        #expect(BackendError.partial(PartialFailure(completed: ["Worktree removed"], remaining: Refusal(
            cause: .unmergedBranch(branch: "b", ahead: 1), message: "m", diagnostics: diagnostics))).presentation().completed
            == ["Worktree removed"])
        #expect(BackendError.cancelled(note: nil).presentation().message == "The operation was stopped.")
        #expect(BackendError.cancelled(note: nil).presentation().offersDiagnosticReport == false)
        #expect(BackendError.timedOut(operation: "command", after: .seconds(90), diagnostics: Diagnostics(summary: "x"))
            .presentation().message == "The command didn't finish within 1:30.")
        #expect(BackendError.cliTooOld(found: SemVer(0, 13, 3), minimum: SemVer(0, 13, 4), path: "/b").presentation().message
            == "/b is version 0.13.3; BranchBox for Mac needs 0.13.4 or later.")
    }

    @Test func unsupportedControlsNameTheCapability() {
        #expect(BackendError.unsupportedHelp(.config) == "Requires BranchBox CLI with config editing")
        #expect(BackendError.unsupported(.tunnelCredentials, minimumCLI: "0.14.0").presentation().message
            == "Requires BranchBox CLI with tunnel credentials (0.14.0 or later).")
        let all: [Capability] = [.jsonErrorEnvelope, .registryLock, .writeAheadStart, .teardownPlan, .teardownDiscardChanges,
                                 .teardownUnmergedPreflight, .pruneJSON, .detectJSON, .devcontainerSyncJSON, .config,
                                 .tunnelCredentials, .doctor, .initJSON]
        #expect(Set(all.map(\.displayName)).count == all.count)
        #expect(Capability(rawValue: "future-thing").displayName == "“future-thing”")
    }

    @Test func recoveryActionTitles() {
        let discard = RecoveryAction.retry(.teardown(TeardownRequest(feature: feature, recordedBranch: nil, branch: .keep)),
                                           label: "Discard 2 changes and tear down…", destructive: true, confirmation: "• a\n• b")
        #expect(discard.title == "Discard 2 changes and tear down…")
        #expect(discard.isDestructive)
        #expect(discard.confirmationTitle == "Discard 2 changes and tear down")
        #expect(discard.confirmationMessage == "• a\n• b")
        let unexplained = RecoveryAction.retry(.deleteBranch("b", project, force: true), label: "Force-delete b…",
                                               destructive: true, confirmation: nil)
        #expect(unexplained.confirmationMessage == "This can't be undone.")
        let keep = RecoveryAction.retry(.tunnelOpen(feature), label: "Try Again", destructive: false, confirmation: "ignored")
        #expect(!keep.isDestructive)
        #expect(keep.confirmationMessage == nil)
        let others: [RecoveryAction] = [.runInTerminal(command: ["sbx", "login"], workingDirectory: nil, label: "Sign in"),
                                        .revealInFinder(path: "/x"), .copyCommand("brew", label: "Copy"), .openDoctor,
                                        .locateCLI, .refresh(project), .showLog(operation: UUID())]
        #expect(others.map(\.title) == ["Sign in", "Reveal in Finder", "Copy", "Open Diagnostics", "Locate…", "Refresh", "Show Log"])
        #expect(others.allSatisfy { !$0.isDestructive })
    }

    @Test func bannerCopyText() {
        let presentation = BackendError.commandFailed(diagnostics).presentation()
        #expect(ErrorBanner.copyText(presentation) == "The command failed\nError: git worktree remove failed\nDirectory not empty")
    }
}

@Suite struct OperationPresentationTests {
    @Test func stateVocabulary() {
        let states: [OperationState] = [.queued(behind: "Starting oauth"), .running, .succeeded, .succeededWithWarnings, .partial,
                                        .failed(.cancelled(note: nil)), .cancelled(note: nil)]
        #expect(states.map(\.label) == ["Waiting for Starting oauth", "Running", "Done", "Done with warnings", "Partly done",
                                        "Failed", "Stopped"])
        #expect(Set(states.map(\.symbol)).count == states.count)
        #expect(states.allSatisfy { NSImage(systemSymbolName: $0.symbol, accessibilityDescription: nil) != nil })
    }

    @Test func kindSymbolsExist() {
        let kinds: [OperationKind] = [.start, .teardown, .prune, .exec, .devcontainerUp, .devcontainerDown, .devcontainerRebuild,
                                      .devcontainerBuild, .syncDevcontainers, .tunnelOpen, .tunnelRemove, .initProject,
                                      .applyConfig, .tunnelCredentials, .deleteBranch, .removeStray]
        for kind in kinds {
            #expect(NSImage(systemSymbolName: kind.symbol, accessibilityDescription: nil) != nil, "\(kind)")
        }
    }

    @Test(arguments: [
        (OperationPhase.preparing, "Preparing"), (.module("compose"), "Setting up compose"), (.runtime("sbx"), "Preparing the sbx runtime"),
        (.item(index: 2, of: 5, name: "oauth"), "2 of 5: oauth"), (.step("Copying files"), "Copying files"),
        (.removingWorktree, "Removing the worktree"), (.deletingBranch, "Deleting the branch"),
    ])
    func phaseLabels(phase: OperationPhase, label: String) {
        #expect(phase.label == label)
    }

    @Test(arguments: [(Duration.seconds(0), "0:00"), (.seconds(42), "0:42"), (.seconds(725), "12:05"), (.seconds(3725), "1:02:05")])
    func elapsed(duration: Duration, text: String) {
        #expect(OperationPresentation.elapsed(duration) == text)
    }

    @Test func summaryStatusLine() {
        let start = Date(timeIntervalSince1970: 1_000)
        let running = OperationSummary(kind: .start, title: "Starting oauth", state: .running, startedAt: start, phase: .module("compose"))
        #expect(OperationPresentation.statusLine(running, now: start.addingTimeInterval(42)) == "Running · Setting up compose · 0:42")
        let done = OperationSummary(kind: .start, title: "Starting oauth", state: .succeeded, startedAt: start,
                                    finishedAt: start.addingTimeInterval(61), phase: .module("compose"))
        #expect(OperationPresentation.statusLine(done, now: start.addingTimeInterval(9_999)) == "Done · 1:01")
        #expect(!done.isRunning)
        #expect(OperationSummary(kind: .exec, title: "x", state: .queued(behind: "y"), startedAt: start).isRunning)
    }

    /// D-18: the corruption warning appears for registry writers unless the CLI has both safety capabilities.
    @Test(arguments: [
        (.start, [], true),
        (.start, [.registryLock], true),
        (.start, [.writeAheadStart], true),
        (.start, [.registryLock, .writeAheadStart], false),
        (.teardown, [], true),
        (.syncDevcontainers, [], true),
        (.devcontainerUp, [], false),
        (.exec, [], false),
    ] as [(OperationKind, Set<Capability>, Bool)])
    func cancelConfirmation(kind: OperationKind, capabilities: Set<Capability>, warns: Bool) {
        let confirmation = CancelConfirmation(kind: kind, title: "Starting oauth", capabilities: capabilities)
        #expect(confirmation.title == "Stop “Starting oauth”?")
        #expect(confirmation.warnsAboutCorruption == warns)
        #expect(confirmation.message.contains("partial worktree") == (warns || kind == .start))
        #expect(confirmation.message.contains(".branchbox/registry.json") == warns)
        #expect(confirmation.stopLabel == "Stop")
        #expect(confirmation.keepLabel == "Keep Running")
        if kind == .exec {
            #expect(confirmation.message == "The command is interrupted. Any output already captured is kept, "
                + "but interrupted commands may not return output.")
        }
    }

    private static let lines = [
        LogLine(timestamp: Date(timeIntervalSince1970: 0.25), level: .info, source: .stderr, target: "core::compose", message: "Starting compose"),
        LogLine(timestamp: nil, level: .warn, source: .stderr, target: nil, message: "Tunnel disabled"),
        LogLine(timestamp: nil, level: .error, source: .stderr, target: nil, message: "Café failed"),
        LogLine(timestamp: nil, level: .output, source: .stdout, target: nil, message: "✓ ready"),
    ]

    @Test func logFilter() {
        #expect(LogFilter().apply(to: Self.lines).map(\.index) == [0, 1, 2, 3])
        #expect(LogFilter().apply(to: Self.lines).map(\.id) == [0, 1, 2, 3])
        #expect(LogFilter(warningsOnly: true).apply(to: Self.lines).map(\.index) == [1, 2])
        #expect(LogFilter(query: "cafe").apply(to: Self.lines).map(\.index) == [2])
        #expect(LogFilter(query: " COMPOSE ").apply(to: Self.lines).map(\.index) == [0])
        #expect(LogFilter(warningsOnly: true, query: "compose").apply(to: Self.lines).isEmpty)
        #expect(LogFilter(query: "zzz").apply(to: []).isEmpty)
        // After the ring dropped lines, ids keep naming the same lines.
        #expect(LogFilter().apply(to: Self.lines, firstIndex: 10_000).map(\.id) == [10_000, 10_001, 10_002, 10_003])
        #expect(LogFilter(warningsOnly: true).apply(to: Self.lines, firstIndex: 5).map(\.index) == [6, 7])
    }

    @Test func logPlainText() throws {
        let utc = try #require(TimeZone(identifier: "UTC"))
        #expect(LogFilter.plainText(Self.lines, timestamps: true, timeZone: utc)
            == "00:00:00.250 INFO core::compose: Starting compose\nWARN Tunnel disabled\nERROR Café failed\n✓ ready")
        #expect(LogFilter.plainText(Array(Self.lines.prefix(1)), timestamps: false) == "INFO core::compose: Starting compose")
        #expect(LogFilter.timestamp(Date(timeIntervalSince1970: 3_723.5), timeZone: utc) == "01:02:03.500")
    }

    @Test func logLevelSymbolsExist() {
        let levels: [LogLevel] = [.trace, .debug, .info, .warn, .error, .output]
        #expect(levels.allSatisfy { NSImage(systemSymbolName: $0.symbol, accessibilityDescription: nil) != nil })
    }
}

@Suite struct DiagnosticReportTests {
    private static let identity = BackendIdentity(
        kind: .cli(CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .loginShellPath,
                                 rejected: [RejectedCandidate(path: "/usr/local/bin/branchbox", reason: "version 0.12.0 is too old")])),
        version: SemVer(0, 14, 0, prerelease: "dev"), contractVersion: 1, capabilities: [.registryLock, .config])
    private static let environment = EnvironmentSummary(source: .interactiveLogin, shell: "/bin/zsh", captureDuration: .milliseconds(420),
                                                        pathEntries: ["/opt/homebrew/bin", "/usr/bin", "/bin"], capturedAt: nil,
                                                        isProvisional: false)

    private func report(error: BackendError?, secrets: [String] = [], identity: BackendIdentity? = DiagnosticReportTests.identity) -> String {
        DiagnosticReport(app: .init(version: "1.2.0", build: "45", gitSHA: "abc1234"), osVersion: "Version 15.0", identity: identity,
                         environment: Self.environment, operationTitle: "Tearing down eta", error: error,
                         context: .teardown(TeardownRequest(feature: feature, recordedBranch: nil, branch: .keep)),
                         generatedAt: Date(timeIntervalSince1970: 0), secrets: secrets, homeDirectory: "/Users/dev").markdown()
    }

    @Test func containsTheCLIEnvironmentAndFailure() {
        let tail = (1...60).map { "stderr line \($0)" }
        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: boom", causes: ["Caused by: disk full"], exitCode: 1,
                                                             signal: 15, logTail: tail, invocation: "branchbox feature teardown eta --json",
                                                             cliVersion: "0.14.0-dev"))
        let text = report(error: failure)
        for expected in [
            "# BranchBox diagnostic report", "- Generated: 1970-01-01T00:00:00Z", "- App: BranchBox for Mac 1.2.0 (build 45, SHA abc1234)",
            "- macOS: Version 15.0", "- Path: /opt/homebrew/bin/branchbox", "- Source: login-shell PATH",
            "- Rejected: /usr/local/bin/branchbox (version 0.12.0 is too old)", "- Version: 0.14.0-dev (minimum 0.13.4)",
            "- Contract version: 1", "- Capabilities: config, registry-lock", "- Child PATH: /opt/homebrew/bin:/usr/bin:/bin",
            "- Shell: /bin/zsh", "- Operation: Tearing down eta", "- Error: The command failed", "- Message: Error: boom",
            "- Caused by: Caused by: disk full", "- Invocation: `branchbox feature teardown eta --json`", "- Exit code: 1",
            "- Signal: 15", "- CLI version: 0.14.0-dev", "Last 50 stderr lines:", "stderr line 60",
        ] {
            #expect(text.contains(expected), "missing \(expected)")
        }
        #expect(!text.contains("stderr line 10\n"))      // Diagnostics keeps only the last 50
    }

    @Test func legacyAndMissingBackends() {
        let legacy = BackendIdentity(kind: .preview, version: SemVer(0, 13, 4), contractVersion: nil, capabilities: [])
        let text = report(error: nil, identity: legacy)
        #expect(text.contains("- Backend: preview (no CLI)"))
        #expect(text.contains("- Contract version: none (legacy CLI)"))
        #expect(text.contains("- Capabilities: none"))
        #expect(!text.contains("## Failure"))
        #expect(report(error: .cliNotFound(searched: [])).contains("- Detail:") == false)
        #expect(report(error: nil, identity: nil).contains("- Backend: unavailable"))
        let agent = BackendIdentity(kind: .agent(endpoint: "unix:///tmp/agent.sock"), version: SemVer(0, 14, 0), contractVersion: 1,
                                    capabilities: [])
        #expect(report(error: nil, identity: agent).contains("- Backend: agent at unix:///tmp/agent.sock"))
    }

    @Test func secretsAreRedacted() {
        let failure = BackendError.commandFailed(Diagnostics(
            summary: "Error: CLOUDFLARE_API_TOKEN=cf-secret-123 rejected",
            causes: ["Authorization: Bearer abcdefghijklmnop", "password: hunter22", "token ghp_abcdefghijklmnopqrstuvwxyz0123"],
            logTail: ["--prompt 'build the secret thing'", "--prompt=quick", "github_pat_ABCDEFGHIJKLMNOPQRSTUV_123",
                      "path /Users/dev/projects/eta", "MY_EXTRA=sk-live-supersecretvalue1", "custom value hush-hush-42"],
            invocation: "branchbox feature start eta --prompt '<redacted 812 chars>'"))
        let text = report(error: failure, secrets: ["hush-hush-42", "abc"])
        for leaked in ["cf-secret-123", "abcdefghijklmnop", "hunter22", "ghp_abcdefghijklmnopqrstuvwxyz0123", "build the secret thing",
                       "quick", "github_pat_", "sk-live-supersecretvalue1", "hush-hush-42", "/Users/dev"] {
            #expect(!text.contains(leaked), "leaked \(leaked)")
        }
        #expect(text.contains("CLOUDFLARE_API_TOKEN=<redacted>"))
        #expect(text.contains("path ~/projects/eta"))
        #expect(text.contains("custom value <redacted>"))
        #expect(text.contains("SHA abc1234"))            // secrets shorter than 4 characters are not scrubbed
    }

    @Test func appInfoComesFromTheBundle() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bbx-report-\(UUID().uuidString)")
        defer { _ = try? FileManager.default.removeItem(at: root) }
        let bundleURL = root.appendingPathComponent("Fake.bundle")
        try FileManager.default.createDirectory(at: bundleURL.appendingPathComponent("Contents"), withIntermediateDirectories: true)
        let plist: [String: Any] = ["CFBundleIdentifier": "dev.branchbox.tests.\(UUID().uuidString)", "CFBundleShortVersionString": "0.14.0",
                                    "CFBundleVersion": "7", "BranchBoxGitSHA": "deadbee"]
        try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
            .write(to: bundleURL.appendingPathComponent("Contents/Info.plist"))
        let bundle = try #require(Bundle(url: bundleURL))
        #expect(DiagnosticReport.AppInfo.current(bundle: bundle) == .init(version: "0.14.0", build: "7", gitSHA: "deadbee"))

        let bare = root.appendingPathComponent("Bare.bundle")
        try FileManager.default.createDirectory(at: bare, withIntermediateDirectories: true)
        let bareBundle = try #require(Bundle(url: bare))
        #expect(DiagnosticReport.AppInfo.current(bundle: bareBundle) == .init(version: "development", build: nil, gitSHA: nil))
    }
}

/// Records what HostLauncher asks of the system instead of opening anything.
@MainActor private final class RecordingOpener: HostOpening {
    struct Failure: Error, LocalizedError { var errorDescription: String? { "boom" } }

    var applications: [String: URL] = [
        HostLauncher.terminalBundleID: URL(fileURLWithPath: "/System/Applications/Utilities/Terminal.app"),
        HostLauncher.iTermBundleID: URL(fileURLWithPath: "/Applications/iTerm.app"),
        HostLaunchPlan.vscodeBundleID: URL(fileURLWithPath: "/Applications/Visual Studio Code.app"),
    ]
    var schemes: Set<String> = ["vscode"]
    var failOpen = false
    var failShell = false
    var refuseURL = false
    private(set) var opened: [(urls: [URL], application: URL)] = []
    private(set) var openedURLs: [URL] = []
    private(set) var shellCommands: [String] = []

    func applicationURL(forBundleIdentifier bundleID: String) -> URL? { applications[bundleID] }
    func applicationURL(toOpen url: URL) -> URL? {
        schemes.contains(url.scheme ?? "") ? URL(fileURLWithPath: "/Applications/Handler.app") : nil
    }
    func open(_ urls: [URL], withApplicationAt application: URL) async throws {
        if failOpen { throw Failure() }
        opened.append((urls, application))
    }
    func open(_ url: URL) -> Bool {
        openedURLs.append(url)
        return !refuseURL
    }
    func runShell(_ command: String) throws {
        if failShell { throw Failure() }
        shellCommands.append(command)
    }
}

@MainActor @Suite struct HostLauncherTests {
    private let root = FileManager.default.temporaryDirectory.appendingPathComponent("bbx-launch-\(UUID().uuidString)")

    private func launcher(_ opener: RecordingOpener, now: @escaping () -> Date = Date.init) -> HostLauncher {
        HostLauncher(opener: opener, launchDirectory: root.appendingPathComponent("launch"), now: now)
    }

    private func cleanUp() {
        do { try FileManager.default.removeItem(at: root) } catch {}
    }

    private func permissions(_ url: URL) throws -> Int {
        try #require(try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int)
    }

    private func terminalPlan(_ terminal: TerminalChoice, folder: String = "/tmp/bbx/eta") -> HostLaunchPlan {
        HostLaunchPlan.terminal(terminal, record: FeatureRecord(workFeature: "eta", worktreePath: folder), folderExists: true)
    }

    @Test func terminalScriptsAreWrittenPrivatelyAndOpenedWithTerminal() async throws {
        defer { cleanUp() }
        let opener = RecordingOpener()
        let plan = terminalPlan(.terminal, folder: "/tmp/it's here")
        try await launcher(opener).launch(plan)
        let call = try #require(opener.opened.first)
        #expect(call.application.lastPathComponent == "Terminal.app")
        let script = try #require(call.urls.first)
        #expect(script.deletingLastPathComponent().lastPathComponent == "launch")
        #expect(script.pathExtension == "command")
        #expect(try permissions(script) == 0o700)
        #expect(try permissions(script.deletingLastPathComponent()) == 0o700)
        guard case .terminalScript(let content, _) = plan.kind else { Issue.record("not a script plan"); return }
        #expect(try String(contentsOf: script, encoding: .utf8) == content)
        #expect(content.contains("cd '/tmp/it'\\''s here' && exec "))
    }

    @Test func iTermAndMissingTerminals() async throws {
        defer { cleanUp() }
        let opener = RecordingOpener()
        try await launcher(opener).launch(terminalPlan(.iTerm))
        #expect(opener.opened.first?.application.lastPathComponent == "iTerm.app")

        opener.applications[HostLauncher.iTermBundleID] = nil
        await #expect(throws: HostLaunchError.applicationNotFound("iTerm")) {
            try await launcher(opener).launch(terminalPlan(.iTerm))
        }
    }

    @Test func customTerminalTemplatesGetQuotedPathAndScript() async throws {
        defer { cleanUp() }
        let opener = RecordingOpener()
        try await launcher(opener).launch(terminalPlan(.custom(template: "  open -na Ghostty --args --cwd={path} -e {command} "),
                                                       folder: "/tmp/my eta"))
        let command = try #require(opener.shellCommands.first)
        #expect(command.hasPrefix("open -na Ghostty --args --cwd='/tmp/my eta' -e "))
        let scriptPath = String(command.dropFirst("open -na Ghostty --args --cwd='/tmp/my eta' -e ".count))
        #expect(FileManager.default.fileExists(atPath: scriptPath))
        #expect(opener.opened.isEmpty)

        await #expect(throws: HostLaunchError.invalidTerminalTemplate("it must contain {command}, the script to run")) {
            try await launcher(opener).launch(terminalPlan(.custom(template: "open -a Ghostty {path}")))
        }
        await #expect(throws: HostLaunchError.invalidTerminalTemplate("it is empty")) {
            try await launcher(opener).launch(terminalPlan(.custom(template: "  ")))
        }
        opener.failShell = true
        await #expect(throws: HostLaunchError.self) {
            try await launcher(opener).launch(terminalPlan(.custom(template: "x {command}")))
        }
    }

    @Test func templateExpansion() throws {
        #expect(try HostLauncher.expand(template: "wezterm start --cwd {path} -- {command}", script: "/c/s.command", workingDirectory: "/w d")
            == "wezterm start --cwd '/w d' -- /c/s.command")
        // Substituted values are not scanned again: a folder named after a token stays one quoted word.
        #expect(try HostLauncher.expand(template: "t {path} {command}", script: "/c/s.command", workingDirectory: "/w/{command}")
            == "t '/w/{command}' /c/s.command")
    }

    @Test func foldersOpenInTheChosenApp() async throws {
        defer { cleanUp() }
        let folder = root.appendingPathComponent("eta")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let opener = RecordingOpener()
        let record = FeatureRecord(workFeature: "eta", worktreePath: folder.path)
        try await launcher(opener).launch(HostLaunchPlan.editor(.vscode, mode: .folder, record: record, folderExists: true))
        #expect(opener.opened.first?.urls.first?.path == folder.path)
        #expect(opener.opened.first?.application.lastPathComponent == "Visual Studio Code.app")

        await #expect(throws: HostLaunchError.applicationNotFound("Cursor")) {
            try await launcher(opener).launch(HostLaunchPlan.editor(.cursor, mode: .folder, record: record, folderExists: true))
        }
        await #expect(throws: HostLaunchError.applicationNotFound("/Applications/Nope.app")) {
            try await launcher(opener).launch(HostLaunchPlan.editor(.custom(appPath: "/Applications/Nope.app"), mode: .folder,
                                                                    record: record, folderExists: true))
        }
        let customApp = root.appendingPathComponent("Editor.app")
        try FileManager.default.createDirectory(at: customApp, withIntermediateDirectories: true)
        try await launcher(opener).launch(HostLaunchPlan.editor(.custom(appPath: customApp.path), mode: .folder, record: record,
                                                                folderExists: true))
        #expect(opener.opened.last?.application.path == customApp.path)

        try await launcher(opener).launch(HostLaunchPlan(kind: .openFolder(appBundleID: nil, appPath: nil, path: folder.path)))
        #expect(opener.openedURLs.last?.path == folder.path)

        opener.failOpen = true
        await #expect(throws: HostLaunchError.openFailed(target: folder.path, reason: "boom")) {
            try await launcher(opener).launch(HostLaunchPlan.editor(.vscode, mode: .folder, record: record, folderExists: true))
        }
    }

    @Test func missingFoldersAndDisabledPlansAreTypedErrors() async throws {
        let opener = RecordingOpener()
        await #expect(throws: HostLaunchError.folderMissing(path: "/nonexistent/bbx/eta")) {
            try await launcher(opener).launch(HostLaunchPlan(kind: .openFolder(appBundleID: HostLaunchPlan.vscodeBundleID, appPath: nil,
                                                                                  path: "/nonexistent/bbx/eta")))
        }
        await #expect(throws: HostLaunchError.disabled(reason: "nope")) {
            try await launcher(opener).launch(HostLaunchPlan(kind: .disabled(reason: "nope")))
        }
        #expect(opener.opened.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: root.path))
    }

    @Test func urlsNeedAHandler() async throws {
        let opener = RecordingOpener()
        let link = try #require(HostLaunchPlan.devContainerDeepLink(scheme: "vscode", worktreePath: "/tmp/x", workspaceFolder: "/workspaces/x"))
        try await launcher(opener).launch(HostLaunchPlan(kind: .openURL(link)))
        #expect(opener.openedURLs == [link])

        let cursor = try #require(HostLaunchPlan.devContainerDeepLink(scheme: "cursor", worktreePath: "/tmp/x", workspaceFolder: "/w"))
        await #expect(throws: HostLaunchError.noApplicationForURL(cursor)) {
            try await launcher(opener).launch(HostLaunchPlan(kind: .openURL(cursor)))
        }
        opener.refuseURL = true
        await #expect(throws: HostLaunchError.self) {
            try await launcher(opener).launch(HostLaunchPlan(kind: .openURL(link)))
        }
    }

    @Test func expiredScriptsAreCleanedUp() async throws {
        defer { cleanUp() }
        let opener = RecordingOpener()
        let directory = root.appendingPathComponent("launch")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let old = directory.appendingPathComponent("launch-OLD.command")
        let recent = directory.appendingPathComponent("launch-NEW.command")
        let unrelated = directory.appendingPathComponent("notes.txt")
        for file in [old, recent, unrelated] { try Data("x".utf8).write(to: file) }
        let now = Date()
        try FileManager.default.setAttributes([.modificationDate: now.addingTimeInterval(-2 * 86_400)], ofItemAtPath: old.path)
        try FileManager.default.setAttributes([.modificationDate: now.addingTimeInterval(-2 * 86_400)], ofItemAtPath: unrelated.path)
        _ = try launcher(opener, now: { now }).writeScript("#!/bin/sh\nexit 0\n")
        #expect(!FileManager.default.fileExists(atPath: old.path))
        #expect(FileManager.default.fileExists(atPath: recent.path))
        #expect(FileManager.default.fileExists(atPath: unrelated.path))
    }

    @Test func scriptWriteFailuresAreTyped() throws {
        defer { cleanUp() }
        // The launch "directory" is a file, so it can't be created.
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try Data().write(to: root.appendingPathComponent("launch"))
        #expect(throws: HostLaunchError.self) {
            _ = try launcher(RecordingOpener()).writeScript("#!/bin/sh\n")
        }
    }

    @Test func errorMessagesNameTheirCause() {
        let errors: [HostLaunchError] = [
            .disabled(reason: "r"), .folderMissing(path: "/p"), .applicationNotFound("Cursor"),
            .noApplicationForURL(URL(string: "cursor://x")!), .scriptWriteFailed(path: "/s", reason: "EACCES"),
            .openFailed(target: "/t", reason: "boom"), .invalidTerminalTemplate("empty"), .terminalLaunchFailed(command: "c", reason: "x"),
        ]
        #expect(errors.map(\.message) == [
            "r", "The folder /p is missing", "Couldn't find Cursor; is it installed?",
            "No app opens cursor://… links; is the editor installed?", "Couldn't write the launch script /s: EACCES",
            "Couldn't open /t: boom", "The custom terminal command is invalid: empty", "Couldn't run “c”: x",
        ])
        #expect(HostLauncher.appName(forBundleID: "com.example.x") == "com.example.x")
        #expect(HostLauncher.defaultLaunchDirectory.path.hasSuffix("Library/Caches/BranchBox/launch"))
    }
}

@MainActor @Suite struct PasteboardTests {
    @Test func copiesPlainText() {
        let named = NSPasteboard(name: NSPasteboard.Name("dev.branchbox.tests.\(UUID().uuidString)"))
        defer { named.releaseGlobally() }
        let pasteboard = Pasteboard(pasteboard: named)
        pasteboard.copy("feature/eta")
        #expect(pasteboard.string == "feature/eta")
        pasteboard.copy("second")
        #expect(pasteboard.string == "second")
    }
}

/// Every component lays out and draws offscreen (ImageRenderer; no window), in the states its previews show.
@MainActor @Suite struct ComponentRenderingTests {
    private func renders<V: View>(_ view: V, width: CGFloat = 520, sourceLocation: SourceLocation = #_sourceLocation) {
        let renderer = ImageRenderer(content: view.frame(width: width))
        #expect(renderer.cgImage != nil, sourceLocation: sourceLocation)
    }

    @Test func badgesAndSwatches() {
        renders(VStack {
            ForEach(FeaturePresentationTests.statuses, id: \.raw) { StatusBadge(status: $0) }
            StatusBadge(attention: .folderMissing)
            RuntimeBadge(provider: .sbx)
            RuntimeBadge(provider: .container, style: .glyph)
            ColorSwatch(hex: "#e67e22")
            ColorSwatch(hex: nil)
            AttentionBadge(count: 3)
            AttentionBadge(count: 0)
            CopyButton(text: "feature/eta", label: "Copy Branch", showsTitle: true)
        })
    }

    @Test func listsAndLayouts() throws {
        let sandbox = try #require(PreviewSamples.features.first { $0.workFeature == "sbx-demo" })
        renders(ModuleChecklist(outcomes: PreviewSamples.features[0].moduleOutcomes + sandbox.moduleOutcomes))
        renders(ModuleChecklist(outcomes: []))
        renders(PortLinks(urls: sandbox.urls))
        renders(PortLinks(ports: []))
        renders(FlowLayout(spacing: 6) { ForEach(PreviewSamples.features) { Text($0.workFeature) } }, width: 200)
    }

    @Test func resultsAndErrors() {
        let request = TeardownRequest(feature: feature, recordedBranch: "feature/eta", branch: .deleteIfMerged)
        let files = (1...12).map { ChangedFile(path: "file\($0).txt", kind: "untracked", area: "other") }
        renders(ResultCard(error: refused(.uncommittedChanges(files: files)), context: .teardown(request), operationID: UUID(),
                           diagnosticReport: { "report" }) { _ in })
        renders(ResultCard(error: .partial(PartialFailure(completed: ["Worktree removed"], remaining: Refusal(
            cause: .unmergedBranch(branch: "feature/eta", ahead: 3), message: "m", diagnostics: diagnostics))),
                           context: .teardown(request)) { _ in })
        renders(ResultCard(error: .commandFailed(diagnostics), context: nil, diagnosticReport: { "report" }) { _ in })
        renders(ResultCard(error: .cliNotFound(searched: []), context: nil) { _ in })
        renders(ResultCard(successTitle: "Started eta", warnings: ["Tunnel disabled"]))
        renders(ResultCard(successTitle: "Started eta", warnings: []))
        renders(ErrorBanner(error: .commandFailed(diagnostics), isStale: true, onRetry: {}, onDetails: {}))
    }

    @Test func operationsAndLogs() {
        let lines = (0..<30).map { LogLine(timestamp: .now, level: $0 % 4 == 0 ? .warn : .info, source: .stderr,
                                           target: "core::compose", message: "line \($0)") }
        renders(LogView(lines: lines, archiveURL: URL(fileURLWithPath: "/tmp/x.log")).frame(height: 240))
        renders(LogView(lines: []).frame(height: 120))
        renders(OperationRow(summary: OperationPreviewData.running))
        renders(OperationRow(summary: OperationPreviewData.pruning))
        for summary in OperationPreviewData.finished { renders(OperationRow(summary: summary)) }
        renders(OperationProgressView(summary: OperationPreviewData.running, lines: lines, capabilities: [],
                                      onRunInBackground: {}, onStop: {}).frame(height: 360))
        renders(OperationProgressView(summary: OperationPreviewData.finished[1], lines: [], capabilities: []).frame(height: 300))
    }

    @Test func emptyStates() {
        renders(CLINotFoundView(searched: PreviewSamples.searchedPaths, onLocate: {}, onRedetect: {}), width: 600)
        renders(CLITooOldView(found: SemVer(0, 13, 3), minimum: BackendIdentity.minimumCLI, path: "/opt/homebrew/bin/branchbox",
                              onLocate: {}, onRedetect: {}), width: 600)
        renders(ProjectMissingView(path: project.path, onLocate: {}, onRemove: {}), width: 600)
        renders(NoFeaturesView(projectName: nil, onStart: {}), width: 600)
        renders(FeatureGoneView(name: "eta", onShowProject: nil), width: 600)
    }
}
