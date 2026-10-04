import BranchBoxKit
import BranchBoxStores
import Foundation
import Observation
import Testing

// Compile-only check of the DESIGN §4.10 Stores API: every member is used with its contract type, through a plain
// (non-@testable) import, so dropping a member, narrowing its access or changing its signature breaks this file.
// `apiSurface` is never called.

@MainActor private func apiSurface(model: AppModel, bootstrapper: any BackendBootstrapping, request: OperationRequestContext,
                                   recovery: RecoveryAction, ref: ProjectRef, folder: URL, record: FeatureRecord,
                                   defaults: UserDefaults) async throws {
    // AppModel
    let made: AppModel = AppModel(settings: AppSettings(defaults: defaults), bootstrapper: bootstrapper, notifier: NoopNotifier())
    let settings: AppSettings = made.settings
    let environment: EnvironmentStore = model.environment
    let projects: ProjectsStore = model.projects
    let operations: OperationStore = model.operations
    let actions: ActionDispatcher = model.actions
    let notifier: any Notifier = model.notifier
    let pending: WindowIntent? = model.pendingIntent
    let token: Int = model.intentToken
    await model.start()
    let backend: any BranchBoxBackend = try model.backend()
    model.post(.showDiagnostics)
    let taken: WindowIntent? = model.takePendingIntent()
    model.appDidBecomeActive()
    await model.prepareForTermination()
    _ = (settings, notifier, pending, token, backend, taken)

    // EnvironmentStore
    let state: BackendState = environment.backendState
    let identity: BackendIdentity? = environment.identity
    let summary: EnvironmentSummary? = environment.summary
    let doctor: DoctorReport? = environment.doctor
    await environment.rebootstrap()
    await environment.recaptureEnvironment()
    await environment.runDoctor(for: Optional(ref))
    let supported: Bool = environment.supports(.registryLock)
    _ = (state, identity, summary, doctor, supported)

    // ProjectsStore
    let list: [ProjectStore] = projects.projects
    let found: ProjectStore? = projects.project(ref)
    let outcome: AddProjectOutcome = await projects.add(folder: folder)
    projects.remove(ref)
    let relocated: AddProjectOutcome = await projects.relocate(ref, to: folder)
    projects.setPinned(ref, true)
    projects.markOpened(ref)
    projects.refreshAll(.manual)
    let attentionCount: Int = projects.attentionCount
    _ = (list, found, outcome, relocated, attentionCount)
    switch outcome {
    case .added(let added, let note): _ = (added, note as String?)
    case .alreadyPresent(let present): _ = present
    case .needsInit(let uninitialized): _ = uninitialized
    case .refused(let error): _ = error as BackendError
    }

    // ProjectStore
    guard let store = found else { return }
    let storeRef: ProjectRef = store.ref
    let id: String = store.id
    let features: [FeatureRecord] = store.features
    let strays: [StrayWorktree] = store.strays
    let dropped: Int = store.droppedRecords
    let warnings: [String] = store.listWarnings
    let loadState: LoadState = store.loadState
    let rootExists: Bool = store.rootExists
    store.includeRemoved = !store.includeRemoved
    let config: ProjectConfigDocument? = store.config
    let detect: DetectReport? = store.detect
    let named: FeatureRecord? = store.feature(named: "x")
    let exists: Bool = store.folderExists(for: record)
    store.requestRefresh(.registryChanged)
    await store.refresh(.timer)
    await store.reloadConfig()
    await store.reloadDetect()
    let attention: [AttentionItem] = store.attention
    _ = (storeRef, id, features, strays, dropped, warnings, loadState, rootExists, config, detect, named, exists, attention)
    let reasons: [RefreshReason] = [.initial, .registryChanged, .appActivated, .timer, .afterOperation, .manual, .settingsChanged]
    let states: [LoadState] = [.idle, .loading, .loaded(.now), .failed(.cancelled(note: nil), lastGood: nil)]
    let backendStates: [BackendState] = [.resolving, .unavailable(.cancelled(note: nil))]
    _ = (reasons, states, backendStates)

    // OperationStore and OperationRecord
    let records: [OperationRecord] = operations.records
    let running: [OperationRecord] = operations.running
    let targeted: [OperationRecord] = operations.records(for: .global)
    let active: OperationRecord? = operations.active(for: .project(ref))
    let admission: Admission = operations.admission(for: .start, target: .feature(FeatureRef(project: ref, name: "x")))
    switch admission {
    case .allowed, .queued(behind: _), .rejected(reason: _): break
    }
    if let first = records.first {
        operations.cancel(first.id)
        let recordID: UUID = first.id
        let kind: OperationKind = first.kind
        let target: OperationTarget = first.target
        let title: String = first.title
        let context: OperationRequestContext = first.context
        let startedAt: Date = first.startedAt
        let recordState: OperationState = first.state
        let result: OperationResult? = first.result
        let phase: OperationPhase? = first.phase
        let step: StepProgress? = first.stepProgress
        let recordWarnings: [String] = first.warnings
        let log: LogBuffer = first.log
        let lines: [LogLine] = log.lines
        let revision: Int = log.revision
        let archive: URL? = log.archiveURL
        let finishedAt: Date? = first.finishedAt
        let acknowledged: Bool = first.acknowledged
        let cancellable: Bool = first.isCancellable
        first.acknowledge()
        _ = (recordID, kind, target, title, context, startedAt, recordState, result, phase, step, recordWarnings, lines,
             revision, archive, finishedAt, acknowledged, cancellable)
    }
    await operations.cancelAll()
    _ = (running, targeted, active)
    let flags: (Bool, Bool, Bool) = (OperationKind.start.writesRegistry, OperationKind.exec.isMutating, OperationKind.prune.isProjectWide)
    let allStates: [OperationState] = [.queued(behind: "x"), .running, .succeeded, .succeededWithWarnings, .partial,
                                       .failed(.cancelled(note: nil)), .cancelled(note: nil)]
    let rows: [PruneRowOutcome] = [.refused(.cancelled(note: nil)), .failed(.cancelled(note: nil)), .skipped("x"), .cancelled]
    let pruneResult = PruneResult(rows: [PruneRow(feature: "x", outcome: .cancelled)])
    let results: [OperationResult] = [.prune(pruneResult), .message("x"), .exec(ExecResult(exitCode: 0))]
    let targets: [OperationTarget] = [.feature(FeatureRef(project: ref, name: "x")), .project(ref), .global]
    _ = (flags, allStates, rows, results, targets, StepProgress(completed: 1, total: 2))

    // ActionDispatcher
    let dispatched: DispatchResult = actions.dispatch(request)
    switch dispatched {
    case .started(let started): _ = started as OperationRecord
    case .queued(let queued, let behind): _ = (queued as OperationRecord, behind as String)
    case .rejected(let reason): _ = reason as String
    case .unavailable(let error): _ = error as BackendError
    }
    let performed: DispatchResult? = actions.perform(recovery)
    let title: String = actions.title(for: request)
    _ = (performed, title)

    // Notifier
    let note = UserNote(title: "t", body: "b", intent: .quickOpen, threadID: "x")
    let noteParts: (String, String, WindowIntent?, String) = (note.title, note.body, note.intent, note.threadID)
    let noop = NoopNotifier()
    let available: Bool = noop.isAvailable
    let authorized: Bool = await noop.requestAuthorizationIfNeeded()
    await noop.post(note)
    _ = (noteParts, available, authorized)

    // AppSettings
    let processDefaults: UserDefaults = AppSettings.defaultsForCurrentProcess()
    let cli: String? = settings.cliPathOverride
    let extra: [String: String] = settings.extraEnvironment
    let verbose: Bool = settings.verboseLogs
    let editor: EditorChoice = settings.preferredEditor
    let openMode: EditorOpenMode = settings.editorOpenMode
    let terminal: TerminalChoice = settings.preferredTerminal
    let agent: AgentChoice = settings.agentChoice
    let agentName: String? = settings.agentDisplayName
    let passPrompt: Bool = settings.passPromptToAgent
    let notificationsEnabled: Bool = settings.notificationsEnabled
    let onlyProblems: Bool = settings.notifyOnlyOnProblems
    let attentionChanges: Bool = settings.notifyAttentionChanges
    let watch: Bool = settings.watchProjectFiles
    let selectedRefresh: RefreshInterval = settings.selectedProjectRefresh
    let otherRefresh: RefreshInterval = settings.otherProjectsRefresh
    let menuBar: Bool = settings.showMenuBarIcon
    let retention: Int = settings.logRetention
    let quick: [String: [String]] = settings.quickCommands
    let prompts: [String] = settings.promptHistory
    let backendSettings: BackendSettings = settings.backendSettings
    _ = (processDefaults, cli, extra, verbose, editor, openMode, terminal, agent, agentName, passPrompt, notificationsEnabled,
         onlyProblems, attentionChanges, watch, selectedRefresh, otherRefresh, menuBar, retention, quick, prompts, backendSettings)

    // Navigation
    let selections: [SidebarSelection] = [.welcome, .project(path: "/r"), .feature(projectPath: "/r", name: "x"),
                                          .stray(projectPath: "/r", path: "/s")]
    let intents: [WindowIntent] = [
        .select(.welcome), .startFeature(project: ref, prefill: nil), .teardown(FeatureRef(project: ref, name: "x"), preselect: .keep),
        .prune(ref), .stray(ref, StrayWorktree(path: "/s", branch: nil, head: nil)), .addProject(nil),
        .initProject(folder, mode: InitSheetMode.setUp), .projectSettings(ref), .syncDevcontainers(ref),
        .showActivity(operation: nil), .showDiagnostics, .quickOpen,
    ]
    _ = (selections, intents)
}

@Suite struct StoresAPITests {
    /// The surface above compiled; this keeps the file in the test run.
    @Test func theStoresAPICompiles() {
        let surface: @MainActor (AppModel, any BackendBootstrapping, OperationRequestContext, RecoveryAction, ProjectRef, URL,
                                 FeatureRecord, UserDefaults) async throws -> Void = apiSurface
        _ = surface
    }
}
