@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation
import Testing

/// An `AppModel` on a `PreviewBackend` whose settings and storage live in a throwaway folder (never
/// ~/Library/Preferences: the suite name is an absolute path, which CFPreferences takes as the plist location).
@MainActor final class AppTestModel {
    let directory: URL
    let defaultsName: String
    let backend: PreviewBackend
    let model: AppModel

    init(_ scenario: PreviewScenario = .contract) {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests", isDirectory: true)
        directory = base.appendingPathComponent("app-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defaultsName = base.appendingPathComponent("app-settings-\(UUID().uuidString)").path
        let bootstrapper = PreviewBootstrapper(scenario: scenario)
        backend = bootstrapper.backend
        model = AppModel(settings: AppSettings(defaults: UserDefaults(suiteName: defaultsName)!), bootstrapper: bootstrapper,
                         notifier: NoopNotifier(), configuration: .isolated(in: directory))
    }

    /// Starts the model, adds the sample project (or `projects`) and waits until every project has listed.
    func start(projects: [ProjectRef] = [PreviewSamples.project]) async throws {
        await model.start()
        for project in projects { _ = await model.projects.add(folder: project.root) }
        for store in model.projects.projects { await store.refresh(.manual) }
    }

    func remove() async {
        await model.prepareForTermination()
        UserDefaults(suiteName: defaultsName)?.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(atPath: defaultsName + ".plist")
        try? FileManager.default.removeItem(at: directory)
    }
}

private let project = PreviewSamples.project
private let feature = FeatureRef(project: project, name: "prine")

@MainActor @Suite struct RouterTests {
    @Test func aPaletteIntentIsPresentedOnlyAfterTheSheetIsDismissed() {
        let router = PresentationRouter()
        router.handle(.startFeature(project: project, prefill: nil))
        router.handle(.quickOpen)
        #expect(router.sheet == .startFeature(project: project, prefill: nil))
        #expect(!router.isQuickOpenPresented)
        #expect(router.next == .quickOpen)

        router.sheet = nil                                        // the user dismissed it; then onDismiss runs
        router.promoteNext()
        #expect(router.isQuickOpenPresented)
        #expect(router.queue.isEmpty)
    }

    @Test func anItemChosenInThePaletteIsPresentedOnlyOnceThePaletteCloses() {
        let router = PresentationRouter()
        router.showQuickOpen()
        router.handle(.teardown(feature, preselect: nil))         // e.g. posted while the palette is still up
        #expect(router.sheet == nil)
        #expect(router.isQuickOpenPresented)

        router.closeQuickOpen()
        #expect(!router.isQuickOpenPresented)
        #expect(router.sheet == .teardown(feature, preselect: nil))
    }

    @Test func closingThePaletteWithAChoicePresentsItsSheet() {
        let router = PresentationRouter()
        router.showQuickOpen()
        router.closeQuickOpen(then: .prune(project))
        #expect(!router.isQuickOpenPresented)
        #expect(router.sheet == .prune(project))
    }

    @Test func twoConsecutiveIntentsNeverStack() {
        let router = PresentationRouter()
        router.handle(.startFeature(project: nil, prefill: nil))
        router.handle(.prune(project))
        router.handle(.prune(project))                            // a duplicate queues once
        #expect(router.sheet == .startFeature(project: nil, prefill: nil))
        #expect(router.queue == [.prune(project)])

        router.sheet = nil
        router.promoteNext()
        #expect(router.sheet == .prune(project))
        #expect(router.queue.isEmpty)

        router.sheet = nil
        router.promoteNext()
        #expect(router.sheet == nil)
    }

    @Test func theSameSheetAgainIsIgnored() {
        let router = PresentationRouter()
        router.handle(.prune(project))
        router.handle(.prune(project))
        #expect(router.sheet == .prune(project))
        #expect(router.queue.isEmpty)
    }

    @Test func selectionsApplyAtOnceEvenUnderASheet() {
        let router = PresentationRouter()
        router.handle(.startFeature(project: project, prefill: nil))
        router.handle(.select(.feature(projectPath: project.path, name: "oauth")))
        #expect(router.selection == .feature(projectPath: project.path, name: "oauth"))
        #expect(router.queue.isEmpty)
    }

    @Test func windowIntentsOpenTheirScenes() {
        let router = PresentationRouter()
        var opened: [String] = []
        router.openWindow = { opened.append($0) }
        router.handle(.showDiagnostics)
        router.handle(.showActivity(operation: nil))
        router.handle(.showActivity(operation: UUID()))           // unknown operation: the Activity window
        #expect(opened == [SceneID.diagnostics, SceneID.activity, SceneID.activity])
        #expect(router.sheet == nil)
    }

    @Test func anIntentPostedWhileTheWindowWasClosedIsConsumedOnAppear() async {
        let harness = AppTestModel()
        harness.model.post(.teardown(feature, preselect: .keep))
        let router = PresentationRouter()                         // the window appears later
        router.consume(from: harness.model)
        #expect(router.sheet == .teardown(feature, preselect: .keep))
        #expect(harness.model.pendingIntent == nil)
        router.consume(from: harness.model)                       // consumed once
        #expect(router.queue.isEmpty)
        await harness.remove()
    }

    @Test func showingAnOperationSelectsItsTargetAndOpensTheInspector() async throws {
        let harness = AppTestModel()
        try await harness.start()
        let request = TeardownRequest(feature: feature, recordedBranch: "feature/prine", branch: .keep)
        guard case .started(let record) = harness.model.actions.dispatch(.teardown(request)) else {
            Issue.record("the teardown should start")
            await harness.remove()
            return
        }
        let router = PresentationRouter()
        router.operation = { harness.model.operations.record($0) }
        router.handle(.showActivity(operation: record.id))
        #expect(router.selection == .feature(projectPath: project.path, name: "prine"))
        #expect(router.isInspectorPresented)
        #expect(record.acknowledged)
        await harness.remove()
    }

    @Test func selectionRoundTripsThroughSceneStorage() {
        let selections: [SidebarSelection] = [
            .welcome, .project(path: project.path), .feature(projectPath: project.path, name: "oauth"),
            .stray(projectPath: project.path, path: "/tmp/x/spike"),
        ]
        for selection in selections {
            #expect(PresentationRouter.decode(PresentationRouter.encode(selection)) == selection)
        }
        #expect(PresentationRouter.encode(nil).isEmpty)
        #expect(PresentationRouter.decode("") == nil)
        #expect(PresentationRouter.decode("{not json") == nil)
    }

    @Test func selectionResolvesItsProjectFeatureAndInspectorTarget() {
        let selection = SidebarSelection.feature(projectPath: project.path, name: "prine")
        #expect(selection.projectRef == project)
        #expect(selection.featureRef == feature)
        #expect(selection.operationTarget == .feature(feature))
        #expect(SidebarSelection.stray(projectPath: project.path, path: "/x").operationTarget == .project(project))
        #expect(SidebarSelection.welcome.operationTarget == nil)
    }
}

// MARK: Command enablement

@MainActor @Suite struct CommandEnablementTests {
    private static let record = PreviewSamples.features[0]      // prine: active, container, has a folder

    private func context(backend: Bool = true, projects: Bool = true, project: ProjectRef? = project,
                         feature: FeatureRecord? = record, folderExists: Bool = true) -> CommandContext {
        var context = CommandContext()
        context.backendReady = backend
        context.hasProjects = projects
        context.hasMainWindow = true
        context.project = project
        context.feature = feature
        context.featureFolderExists = folderExists
        return context
    }

    @Test func everythingAppliesToAnActiveFeature() {
        let context = context()
        for command in AppCommand.allCases where command != .shareViaTunnel || Self.record.tunnel?.status != .active {
            #expect(context.state(command).isEnabled, "\(command) should be enabled: \(context.state(command).reason ?? "")")
        }
    }

    @Test func withoutTheCLIOnlyLocalCommandsRun() {
        let context = context(backend: false)
        let blocked: Set<AppCommand> = [.startFeature, .addProject, .refresh, .runCommand, .startDevContainer,
                                        .stopDevContainer, .shareViaTunnel, .tearDown, .prune, .syncDevcontainers,
                                        .setUp, .repair]
        for command in AppCommand.allCases {
            let state = context.state(command)
            #expect(state.isEnabled == !blocked.contains(command), "\(command)")
            if !state.isEnabled { #expect(state.reason == CommandContext.cliUnavailable) }
        }
    }

    @Test func featureCommandsNeedASelectedFeature() {
        let context = context(feature: nil)
        let featureCommands: [AppCommand] = [.openInEditor, .terminal, .launchAgent, .openURL, .reveal, .runCommand,
                                             .copyPath, .copyBranch, .startDevContainer, .stopDevContainer,
                                             .shareViaTunnel, .tearDown]
        for command in featureCommands {
            #expect(context.state(command) == .disabled(CommandContext.noFeature), "\(command)")
        }
        #expect(context.state(.projectSettings).isEnabled)
        #expect(context.state(.startFeature).isEnabled)
    }

    @Test func projectCommandsNeedAProject() {
        let context = context(project: nil, feature: nil)
        for command: AppCommand in [.projectSettings, .prune, .syncDevcontainers, .setUp, .repair, .removeProject, .showRemoved] {
            #expect(context.state(command) == .disabled(CommandContext.noProject), "\(command)")
        }
    }

    @Test func startNeedsAProject() {
        #expect(context(projects: false, project: nil, feature: nil).state(.startFeature) == .disabled("Add a project first"))
    }

    @Test func aMissingFolderDisablesHostActionsWithItsPath() {
        let context = context(folderExists: false)
        let reason = "The folder \(Self.record.worktreePath ?? "") is missing"
        #expect(context.state(.openInEditor) == .disabled(reason))
        #expect(context.state(.terminal) == .disabled(reason))
        #expect(context.state(.reveal) == .disabled("The feature's folder is missing"))
        #expect(context.state(.copyPath).isEnabled)              // the path itself is still known
        #expect(context.state(.tearDown).isEnabled)              // cleaning up a missing folder is the fix
    }

    @Test func aRemovedFeatureCanOnlyBeCopied() {
        let removed = PreviewSamples.features.first { $0.status == .removed }
        let context = context(feature: removed)
        #expect(context.state(.tearDown) == .disabled("This feature was already torn down"))
        #expect(!context.state(.openInEditor).isEnabled)
        #expect(!context.state(.runCommand).isEnabled)
        #expect(context.state(.copyBranch).isEnabled)
    }

    @Test func sandboxTerminalsExplainWhatTheyNeed() {
        let sandbox = PreviewSamples.features.first { $0.runtime.provider == .sbx }
        let context = context(feature: sandbox)
        #expect(context.state(.terminal) == .disabled(HostLaunchPlan.sandboxShellUnavailable))
        #expect(context.state(.startDevContainer) == .disabled("Only container features have a dev container"))
        #expect(context.state(.shareViaTunnel) == .disabled("This feature is already shared"))
    }

    @Test func aBusyFeatureCantBeTornDownTwice() {
        var context = context()
        context.featureBusy = "Tearing down prine is running"
        #expect(context.state(.tearDown) == .disabled("Tearing down prine is running"))
        #expect(context.state(.openInEditor).isEnabled)
    }

    @Test func theInspectorNeedsAMainWindow() {
        var context = context()
        context.hasMainWindow = false
        #expect(!context.state(.inspector).isEnabled)
        #expect(context.state(.quickOpen).isEnabled)              // opens the window, then the palette
        #expect(context.state(.activity).isEnabled)
    }

    @Test func theLiveContextResolvesTheSelection() async throws {
        let harness = AppTestModel()
        try await harness.start()
        let model = harness.model
        let selected = CommandContext(model: model, selection: .feature(projectPath: project.path, name: "prine"),
                                      hasMainWindow: true)
        #expect(selected.backendReady)
        #expect(selected.project == project)
        #expect(selected.feature?.workFeature == "prine")
        let gone = CommandContext(model: model, selection: .feature(projectPath: project.path, name: "nope"), hasMainWindow: true)
        #expect(gone.feature == nil)
        #expect(gone.state(.tearDown) == .disabled(CommandContext.noFeature))
        let none = CommandContext(model: model, selection: nil, hasMainWindow: false)
        #expect(none.project == nil)
        #expect(none.state(.startFeature).isEnabled)
        await harness.remove()
    }
}

// MARK: Shell pieces

@MainActor @Suite struct ShellTests {
    @Test func menuBarStatusPrefersBlockedThenAttentionThenWorking() {
        #expect(MenuBarStatus.make(blocked: false, attention: 0, running: 0).state == .idle)
        #expect(MenuBarStatus.make(blocked: false, attention: 0, running: 2).state == .working)
        #expect(MenuBarStatus.make(blocked: false, attention: 3, running: 2).state == .attention(3))
        #expect(MenuBarStatus.make(blocked: true, attention: 3, running: 2).state == .blocked)
        #expect(MenuBarStatus.make(blocked: false, attention: 1, running: 1).accessibilityLabel
                == "BranchBox, 1 item needs attention, 1 operation running")
        #expect(MenuBarStatus.make(blocked: false, attention: 0, running: 0).accessibilityLabel == "BranchBox")
    }

    @Test func menuBarText() {
        #expect(MenuBarSummary.header(projects: 2, features: 5, attention: 1, running: 0)
                == "2 projects · 5 features · 1 needs attention")
        #expect(MenuBarSummary.header(projects: 1, features: 1, attention: 0, running: 2) == "1 project · 1 feature · 2 running")
        let now = Date(timeIntervalSince1970: 1_000_000)
        #expect(MenuBarSummary.updatedLine(now.addingTimeInterval(-2), now: now) == "Updated just now")
        #expect(MenuBarSummary.updatedLine(now.addingTimeInterval(-42), now: now) == "Updated 42 s ago")
        #expect(MenuBarSummary.updatedLine(now.addingTimeInterval(-300), now: now) == "Updated 5 min ago")
    }

    @Test func quickOpenIdsAreStableAndFilteringRanksTitlePrefixesFirst() async throws {
        let harness = AppTestModel()
        try await harness.start()
        let first = QuickOpenIndex.items(model: harness.model)
        let second = QuickOpenIndex.items(model: harness.model)
        #expect(first.map(\.id) == second.map(\.id))
        #expect(Set(first.map(\.id)).count == first.count)
        #expect(first.contains { $0.id == "feature:\(project.path):prine:teardown" })
        #expect(!first.contains { $0.title.contains("coding-agents") })   // removed features are left out

        let ranked = QuickOpenIndex.filter(first, query: "tear prine")
        #expect(ranked.first?.title == "Tear Down prine…")
        let prefix = QuickOpenIndex.filter(first, query: "prine")
        #expect(prefix.first?.title == "prine")
        let primary = QuickOpenIndex.filter(first, query: "")       // before typing: no per-feature actions
        #expect(primary.count == first.filter(\.isPrimary).count)
        #expect(!primary.contains { $0.title.hasPrefix("Tear Down") })
        #expect(QuickOpenIndex.filter(first, query: "zzz").isEmpty)
        let grouped = QuickOpenIndex.grouped(QuickOpenIndex.filter(first, query: "settings"))
        #expect(grouped.last?.group == .commands)
        await harness.remove()
    }

    @Test func sidebarItemsShowStrayProvisionalAndFooterRows() async throws {
        let harness = AppTestModel(.interruptedSetup)
        try await harness.start()
        let store = try #require(harness.model.projects.projects.first)
        let items = SidebarItem.items(for: store, operations: harness.model.operations)
        #expect(items.contains { if case .stray = $0 { true } else { false } })
        #expect(items.last == .showRemoved(includeRemoved: false, removedCount: 0))
        guard case .feature(let first, let attention, _)? = items.first else {
            Issue.record("features come first")
            await harness.remove()
            return
        }
        #expect(attention != nil, "attention rows sort first: \(first.workFeature)")
        let filtered = SidebarItem.items(for: store, operations: harness.model.operations, query: "PRI")
        #expect(filtered.map(\.id) == ["feature:prine"])

        await harness.backend.script(.startFeature, .suspendUntilResumed)
        let start = StartFeatureRequest(project: project, name: "brand-new", runtime: .container)
        _ = harness.model.actions.dispatch(.start(start))
        #expect(await harness.backend.waitUntilSuspended(.startFeature))
        let starting = SidebarItem.items(for: store, operations: harness.model.operations)
        #expect(starting.first == .provisional(name: "brand-new"))
        await harness.remove()
    }

    @Test func emptyProjectsSayNoFeaturesYet() async throws {
        let harness = AppTestModel(.emptyProject)
        try await harness.start()
        let store = try #require(harness.model.projects.projects.first)
        #expect(SidebarItem.items(for: store, operations: harness.model.operations).first == .state(.empty))
        await harness.remove()
    }

    @Test func quitConfirmationNamesTheCount() {
        #expect(QuitConfirmation.title(running: 1) == "1 operation is running")
        #expect(QuitConfirmation.title(running: 3) == "3 operations are running")
        #expect(QuitConfirmation.message == "Quitting stops them and may leave partial state.")
    }

    @Test func transientErrorsAloneOfferRetry() {
        let refusal = BackendError.refused(Refusal(cause: .uncommittedChanges(files: PreviewSamples.dirtyFiles),
                                                   message: "dirty", diagnostics: Diagnostics(summary: "")))
        #expect(!refusal.isTransient)
        #expect(BackendError.commandFailed(Diagnostics(summary: "x")).isTransient)
        #expect(BackendError.refused(Refusal(cause: .registryLocked(path: "/r"), message: "", diagnostics: Diagnostics(summary: "")))
            .isTransient)
        #expect(!BackendError.cliNotFound(searched: []).isTransient)
    }

    @Test func buildBadgeNamesPreviewAndDevBuilds() {
        #if DEBUG
        #expect(CompositionRoot.buildBadge(environment: ["BRANCHBOX_BACKEND": "preview",
                                                         "BRANCHBOX_PREVIEW_SCENARIO": "showcase"]) == "PREVIEW · showcase")
        #endif
        #expect(CompositionRoot.buildBadge(environment: [:]) == "DEV")         // xctest is not the released bundle
    }
}
