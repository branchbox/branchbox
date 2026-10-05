import BranchBoxKit
import BranchBoxStores
import Foundation
import Observation

/// The Start Feature sheet's state for ONE presentation (MAC-18): the `StartDraft`, the debounced name preview,
/// the base branches, the runtime choices and, once started, the operation. A new presentation makes a new flow,
/// so nothing typed for a start that was cancelled reaches the next one. Advanced options are remembered per
/// project, and only when a start is actually dispatched.
@MainActor @Observable final class StartFlow {
    /// One runtime the sheet offers, with what the doctor knows about it.
    struct RuntimeOption: Identifiable, Hashable {
        let provider: RuntimeProvider
        let detail: String
        let isEnabled: Bool
        /// Docker Sandboxes is installed but signed out: the row offers [Sign in…].
        let needsSignIn: Bool
        var id: String { provider.raw }
    }

    let model: AppModel
    private(set) var draft: StartDraft
    /// What the user typed as the title; never sent (the request carries only the resolved slug).
    private(set) var title = ""
    /// The slug typed under "Edit name"; nil while the name follows the title.
    private(set) var nameOverride: String?
    private(set) var branches: BranchList?
    private(set) var branchesError: String?
    private(set) var isPreviewPending = false
    /// Start was pressed once: validation shows even for fields the user has not touched.
    private(set) var attemptedStart = false
    private(set) var dispatchError: String?
    /// The running or finished start; nil while editing.
    private(set) var record: OperationRecord?
    /// The request of the last dispatch, for [Edit and Retry] and recoveries.
    private(set) var lastRequest: StartFeatureRequest?
    private(set) var launchError: String?
    var showsAdvanced: Bool
    let stop = StopConfirmationState()

    @ObservationIgnored private var previewTask: Task<Void, Never>?
    @ObservationIgnored private var runtimeTouched = false
    @ObservationIgnored private var didAutoLaunch = false
    @ObservationIgnored private let memory: StartAdvancedMemory
    @ObservationIgnored let debounce: Duration

    /// `prefill` (Edit and Retry, a remediation) wins over the remembered advanced options.
    init(model: AppModel, project: ProjectRef?, prefill: StartFeatureRequest?,
         memory: StartAdvancedMemory = StartAdvancedMemory(), debounce: Duration = .milliseconds(250)) {
        self.model = model
        self.memory = memory
        self.debounce = debounce
        let available = Self.availableProjects(in: model)
        let chosen = prefill?.project ?? project ?? (available.count == 1 ? available.first : nil)
        let config = chosen.flatMap { model.projects.project($0)?.config?.effective }
        if let prefill {
            draft = StartDraft(prefill: prefill, config: config)
            title = prefill.name
            runtimeTouched = true
            showsAdvanced = false
        } else {
            draft = StartDraft(project: chosen, config: config)
            let remembered = chosen.flatMap(memory.load)
            showsAdvanced = remembered?.showsAdvanced ?? false
            if let remembered { Self.apply(remembered, to: &draft) }
        }
    }

    // MARK: Projects

    /// Projects whose folder exists, in sidebar order.
    var availableProjects: [ProjectRef] { Self.availableProjects(in: model) }

    /// More than one project and none chosen by the caller: the sheet shows a picker.
    var needsProjectPicker: Bool { availableProjects.count > 1 }

    var projectStore: ProjectStore? { draft.project.flatMap { model.projects.project($0) } }

    /// Why nothing can start at all: the CLI is missing, too old or unusable.
    var backendProblem: BackendError? {
        if case .unavailable(let error) = model.environment.backendState { return error }
        return nil
    }

    var config: ProjectConfig { projectStore?.config?.effective ?? .defaults }

    func selectProject(_ project: ProjectRef) {
        guard project != draft.project else { return }
        var next = StartDraft(project: project, config: model.projects.project(project)?.config?.effective)
        carryUserInput(into: &next)
        if let remembered = memory.load(project) { Self.apply(remembered, to: &next) }
        draft = next
        branches = nil
        schedulePreview()
        Task { await self.loadProjectData() }
    }

    /// Loads what the sheet needs from the backend: the project's config (runtime and prefix defaults), its
    /// branches, and the doctor report (runtime availability) when there is none yet.
    func prepare() async {
        await loadProjectData()
        if model.environment.doctor == nil, !model.environment.isRunningDoctor {
            await model.environment.runDoctor(for: draft.project)
        }
        applyRuntimeFallback()
    }

    private func loadProjectData() async {
        guard let project = draft.project, let store = model.projects.project(project) else { return }
        if store.config == nil {
            await store.reloadConfig()
            if let config = store.config?.effective, draft.project == project { rebase(on: config) }
        }
        do {
            let list = try await model.backend().listBranches(in: project)
            guard draft.project == project else { return }
            branches = list
            branchesError = nil
        } catch {
            branchesError = BackendError.normalize(error).oneLine
        }
    }

    // MARK: Name

    func setTitle(_ text: String) {
        guard text != title else { return }
        title = text
        if nameOverride == nil { setInput(text) }
    }

    /// "Edit name": the slug field starts from the name the title resolves to.
    func beginEditingName() {
        nameOverride = draft.resolvedName ?? ""
        setInput(nameOverride ?? "")
    }

    func setNameOverride(_ text: String) {
        nameOverride = text
        setInput(text)
    }

    /// Back to the name derived from the title.
    func useTitleForName() {
        nameOverride = nil
        setInput(title)
    }

    private func setInput(_ text: String) {
        draft.input = text
        schedulePreview()
    }

    /// Asks `previewName` 250 ms after the last keystroke; an answer for an older input is ignored.
    private func schedulePreview() {
        previewTask?.cancel()
        let input = draft.input
        guard let project = draft.project, !input.trimmingCharacters(in: .whitespaces).isEmpty else {
            isPreviewPending = false
            return
        }
        isPreviewPending = true
        let debounce = self.debounce
        previewTask = Task { [weak self] in
            if debounce > .zero {
                do { try await Task.sleep(for: debounce) } catch { return }
            }
            await self?.fetchPreview(input, in: project)
        }
    }

    private func fetchPreview(_ input: String, in project: ProjectRef) async {
        let preview: NamePreview?
        do {
            preview = try await model.backend().previewName(input, in: project)
        } catch {
            preview = nil                                   // the local rule stands in (StartDraft.resolvedName)
        }
        guard !Task.isCancelled, draft.input == input, draft.project == project else { return }
        draft.preview = preview
        isPreviewPending = false
    }

    /// Waits for the preview in flight (tests).
    func waitForPreview() async {
        await previewTask?.value
    }

    /// The would-be worktree folder: the CLI's answer, else the layout rule.
    var worktreePath: String? {
        guard let project = draft.project, let slug = draft.resolvedName else { return nil }
        return draft.currentPreview?.worktreePath ?? NameRules.worktreePath(for: slug, in: project)
    }

    /// The registry entry the name collides with, for [Show].
    var collidingFeature: FeatureRecord? {
        guard let slug = draft.resolvedName else { return nil }
        return projectStore?.features.first { $0.workFeature == slug && $0.status != .removed }
    }

    // MARK: Validation

    var validationErrors: [String] {
        let folderExists = worktreePath.map { FileManager.default.fileExists(atPath: $0) } ?? false
        var errors = draft.validationErrors(existing: projectStore?.features ?? [], folderExists: folderExists)
        if let option = runtimeOptions.first(where: { $0.provider == draft.runtime }), !option.isEnabled {
            let detail = option.detail.prefix(1).lowercased() + option.detail.dropFirst()
            errors.append("\(option.provider.label) can't be used: \(detail). Choose another runtime.")
        }
        return errors
    }

    var notices: [String] {
        draft.notices(existing: projectStore?.features ?? [], branches: branches?.local ?? [])
    }

    /// Errors worth showing now: none on a fresh sheet, all once the user typed a name or pressed Start.
    var visibleErrors: [String] {
        guard attemptedStart || !draft.input.trimmingCharacters(in: .whitespaces).isEmpty else {
            return validationErrors.filter { !$0.hasPrefix("Choose a project") && !isNamePrompt($0) }
        }
        return validationErrors
    }

    var canStart: Bool {
        record == nil && !isPreviewPending && validationErrors.isEmpty && draft.makeRequest() != nil
    }

    /// Whether a start now would wait behind another operation (D-16), and behind what.
    var queuedBehind: String? {
        guard let request = draft.makeRequest() else { return nil }
        let target = OperationRequestContext.start(request).operationTarget
        if case .queued(let behind) = model.operations.admission(for: .start, target: target) { return behind }
        return nil
    }

    // MARK: Options

    func setRuntime(_ runtime: RuntimeProvider) {
        runtimeTouched = true
        draft.runtime = runtime
        if runtime != .sbx {
            draft.keepRuntimeOnFailure = false
            if draft.reuse == .retainedRuntime { draft.reuse = .none }
        }
    }

    func setMode(_ mode: StartFeatureRequest.Mode) {
        draft.mode = mode
        if mode == .full { draft.useDefaultPrompt = false }
    }

    func setPrompt(_ text: String) { draft.prompt = text }
    func setUseDefaultPrompt(_ on: Bool) { draft.useDefaultPrompt = on && draft.mode == .minimal }
    func setBase(_ base: String?) { draft.base = base }
    func setBranchPrefix(_ prefix: String) { draft.branchPrefix = prefix }
    func setVerbose(_ on: Bool) { draft.verbose = on }
    func setKeepRuntimeOnFailure(_ on: Bool) { draft.keepRuntimeOnFailure = on && draft.runtime == .sbx }
    func setReuse(_ reuse: StartFeatureRequest.Reuse) { draft.reuse = reuse }

    func setSkipped(_ module: String, _ skipped: Bool) {
        if skipped { draft.skipModules.insert(module) } else { draft.skipModules.remove(module) }
    }

    /// Tunnels are off in the project's config: the tunnel module never runs, so its toggle is locked.
    var tunnelLocked: Bool { !config.tunnelEnabled }

    var promptOverLimit: Bool { draft.promptLength > StartDraft.promptLimit }

    var recentPrompts: [String] { model.settings.promptHistory }

    /// Container and (when installed) Docker Sandboxes, with what the doctor says; Local VM only (disabled) when
    /// it is already the draft's runtime; in-guest never (a supervisor starts it).
    var runtimeOptions: [RuntimeOption] {
        let doctor = model.environment.doctor
        var options: [RuntimeOption] = []
        let docker = doctor?.checks.first { $0.id == "docker.daemon" }
        let dockerDetail: String = switch docker?.status {
        case nil: doctor == nil ? "Checking Docker…" : "Docker status unknown"
        case .ok?: "Docker is running"
        case .warn?, .error?: docker?.detail.map { "Docker isn't ready: \($0)" } ?? "Docker isn't running; start Docker Desktop first"
        case .skipped?: "Docker wasn't checked"
        }
        options.append(RuntimeOption(provider: .container, detail: dockerDetail, isEnabled: true, needsSignIn: false))
        let sbx = doctor?.checks.first { $0.id == "runtime.sbx" }
        switch sbx?.status {
        case nil where doctor == nil:
            options.append(RuntimeOption(provider: .sbx, detail: "Checking Docker Sandboxes…", isEnabled: true, needsSignIn: false))
        case .ok?:
            options.append(RuntimeOption(provider: .sbx, detail: "Signed in", isEnabled: true, needsSignIn: false))
        case .warn?, .error?:
            let signedOut = (sbx?.remediation ?? "").contains("sbx login")
            options.append(RuntimeOption(provider: .sbx, detail: signedOut ? "Not signed in" : (sbx?.detail ?? "Not ready"),
                                         isEnabled: true, needsSignIn: signedOut))
        case nil, .skipped?:
            if draft.runtime == .sbx {
                options.append(RuntimeOption(provider: .sbx, detail: "Not installed on this Mac", isEnabled: false,
                                             needsSignIn: false))
            }
        }
        // Local VM needs Linux with KVM, so a Mac never offers it; it shows (disabled) only when already chosen.
        if draft.runtime == .localVM {
            options.append(RuntimeOption(provider: .localVM, detail: "Linux with KVM only", isEnabled: false, needsSignIn: false))
        }
        return options
    }

    /// Why the runtime differs from the project's default, e.g. sbx is not installed here.
    private(set) var runtimeNote: String?

    /// The project's default runtime may be one this Mac can't start: fall back to Container and say why.
    func applyRuntimeFallback() {
        guard !runtimeTouched else { return }
        let configured = config.runtimeProvider
        let sbxInstalled = model.environment.doctor.map { report in
            report.checks.first { $0.id == "runtime.sbx" }.map { $0.status != .skipped } ?? false
        }
        switch configured {
        case .sbx where sbxInstalled == false:
            draft.runtime = .container
            runtimeNote = "This project starts features in Docker Sandboxes, which isn't installed here; using Container."
        case .localVM:
            draft.runtime = .container
            runtimeNote = "This project starts features in a Local VM, which needs Linux with KVM; using Container."
        case .inGuest, .unknown:
            draft.runtime = .container
            runtimeNote = "This project's default runtime (\(configured.label)) can't be started from the Mac; using Container."
        default:
            runtimeNote = nil
        }
    }

    // MARK: Start

    /// Dispatches the start. Returns false (and shows why) when the draft can't start.
    @discardableResult func start() -> Bool {
        attemptedStart = true
        dispatchError = nil
        guard canStart, let request = draft.makeRequest(), let project = draft.project else { return false }
        if !draft.trimmedPrompt.isEmpty { model.settings.recordPrompt(draft.trimmedPrompt) }
        memory.save(StartAdvancedMemory.Values(draft: draft, configuredPrefix: config.branchPrefix,
                                               showsAdvanced: showsAdvanced), for: project)
        return adopt(model.actions.dispatch(.start(request)), request: request)
    }

    /// A recovery's retry (e.g. "Start in the existing folder") replaces the finished record.
    func adopt(_ result: Result<OperationRecord, FlowDispatchError>?) {
        guard let result else { return }
        switch result {
        case .success(let record):
            if case .start(let request) = record.context { lastRequest = request }
            self.record = record
            didAutoLaunch = false
        case .failure(let error):
            dispatchError = error.message
        }
    }

    private func adopt(_ result: DispatchResult, request: StartFeatureRequest) -> Bool {
        switch FlowDispatch.record(of: result) {
        case .success(let record):
            self.record = record
            lastRequest = request
            didAutoLaunch = false
            return true
        case .failure(let error):
            dispatchError = error.message
            return false
        }
    }

    /// Back to the form with everything as it was ([Edit and Retry]).
    func editAndRetry() {
        if let lastRequest {
            var next = StartDraft(prefill: lastRequest, config: projectStore?.config?.effective)
            next.preview = draft.preview
            if nameOverride == nil, !title.isEmpty, NameRules.resolve(title) == lastRequest.name { next.input = title }
            draft = next
        }
        record = nil
        dispatchError = nil
        schedulePreview()
    }

    // MARK: Result

    var summary: StartSummary? {
        if case .start(let summary)? = record?.result { return summary }
        return nil
    }

    /// The feature a failed start left in the registry, for [Show Feature].
    var partialFeature: FeatureRecord? {
        guard let request = lastRequest, record?.failure != nil else { return nil }
        return model.projects.project(request.project)?.feature(named: request.name)
    }

    /// The started feature as the launch plans need it: the refreshed registry entry, else the summary.
    var startedFeature: FeatureRecord? {
        guard let summary, let request = lastRequest else { return nil }
        if let record = model.projects.project(request.project)?.feature(named: summary.workFeature) { return record }
        return FeatureRecord(workFeature: summary.workFeature, branchName: summary.branchName, worktreePath: summary.worktreePath,
                             baseBranch: request.base, featureURL: summary.featureURL, envPath: summary.envPath,
                             color: summary.color, startMode: summary.mode, promptSeed: summary.promptSeed,
                             moduleOutcomes: summary.moduleOutcomes, adapter: summary.adapter,
                             runtime: summary.runtime ?? RuntimeInfo(provider: request.runtime), defaultAgent: summary.defaultAgent)
    }

    var editorPlan: HostLaunchPlan? {
        startedFeature.map {
            HostLaunchPlan.editor(model.settings.preferredEditor, mode: model.settings.editorOpenMode, record: $0)
        }
    }

    var agentPlan: HostLaunchPlan? {
        guard let feature = startedFeature else { return nil }
        let choice = HostLaunchPlan.agentChoice(projectDefault: config.editorDefaultAgent, fallback: model.settings.agentChoice)
        return HostLaunchPlan.agent(choice, terminal: model.settings.preferredTerminal, record: feature,
                                    passPrompt: model.settings.passPromptToAgent)
    }

    /// Once per successful start: launches the coding agent when the project's config asks for it.
    func autoLaunchIfConfigured(with actions: FlowActions) async {
        guard !didAutoLaunch, summary != nil, config.autoLaunchAgentTerminal, let plan = agentPlan, plan.isEnabled else { return }
        didAutoLaunch = true
        await launch(plan, with: actions)
    }

    func launch(_ plan: HostLaunchPlan, with actions: FlowActions) async {
        launchError = nil
        if case .failure(let error)? = await actions.launch(plan) { launchError = error.message }
    }

    // MARK: Helpers

    private static func availableProjects(in model: AppModel) -> [ProjectRef] {
        model.projects.projects.filter(\.rootExists).map(\.ref)
    }

    /// The "type a name" problems a fresh, empty sheet doesn't shout about.
    private func isNamePrompt(_ error: String) -> Bool {
        error == "Enter a feature name or title" || error.contains("has no words") || error.contains("needs at least")
    }

    /// The config arrived after the draft was made: make it again on that config, keeping what the user set.
    private func rebase(on config: ProjectConfig) {
        var next = StartDraft(project: draft.project, config: config)
        carryUserInput(into: &next)
        next.skipModules = draft.skipModules
        next.verbose = draft.verbose
        next.reuse = draft.reuse
        next.keepRuntimeOnFailure = draft.keepRuntimeOnFailure
        if runtimeTouched { next.runtime = draft.runtime }
        if draft.branchPrefix != ProjectConfig.defaults.branchPrefix { next.branchPrefix = draft.branchPrefix }
        draft = next
        applyRuntimeFallback()
    }

    private func carryUserInput(into next: inout StartDraft) {
        next.input = draft.input
        next.preview = nil
        next.base = nil
        next.mode = draft.mode
        next.prompt = draft.prompt
        next.useDefaultPrompt = draft.useDefaultPrompt
        if runtimeTouched { next.runtime = draft.runtime }
    }

    private static func apply(_ values: StartAdvancedMemory.Values, to draft: inout StartDraft) {
        if let prefix = values.branchPrefix, NameRules.branchPrefixProblem(prefix) == nil { draft.branchPrefix = prefix }
        draft.skipModules = Set(values.skipModules).intersection(NameRules.skippableModules)
        draft.verbose = values.verbose
        draft.keepRuntimeOnFailure = values.keepRuntimeOnFailure && draft.runtime == .sbx
    }
}

/// The Start sheet's advanced options, remembered per project after a start was dispatched (never on cancel).
/// Reuse is never remembered: starting in an existing folder is a per-start decision.
@MainActor struct StartAdvancedMemory {
    struct Values: Codable, Hashable, Sendable {
        var branchPrefix: String?
        var skipModules: [String]
        var verbose: Bool
        var keepRuntimeOnFailure: Bool
        var showsAdvanced: Bool

        init(branchPrefix: String? = nil, skipModules: [String] = [], verbose: Bool = false,
             keepRuntimeOnFailure: Bool = false, showsAdvanced: Bool = false) {
            self.branchPrefix = branchPrefix
            self.skipModules = skipModules
            self.verbose = verbose
            self.keepRuntimeOnFailure = keepRuntimeOnFailure
            self.showsAdvanced = showsAdvanced
        }

        /// A prefix equal to the project's configured one is not remembered, so a later config change applies.
        init(draft: StartDraft, configuredPrefix: String, showsAdvanced: Bool) {
            self.init(branchPrefix: draft.branchPrefix == configuredPrefix ? nil : draft.branchPrefix,
                      skipModules: draft.skipModules.sorted(), verbose: draft.verbose,
                      keepRuntimeOnFailure: draft.keepRuntimeOnFailure, showsAdvanced: showsAdvanced)
        }
    }

    /// Defaults for this process, as AppSettings uses (the dev suite under `swift run` and `swift test`).
    private let suiteName: String?

    init(suiteName: String? = nil) {
        self.suiteName = suiteName
    }

    private var defaults: UserDefaults {
        suiteName.flatMap(UserDefaults.init(suiteName:)) ?? AppSettings.defaultsForCurrentProcess()
    }

    static func key(for project: ProjectRef) -> String { "flows.startAdvanced.\(project.path)" }

    func load(_ project: ProjectRef) -> Values? {
        defaults.data(forKey: Self.key(for: project)).flatMap { try? JSONDecoder().decode(Values.self, from: $0) }
    }

    func save(_ values: Values, for project: ProjectRef) {
        defaults.set(try? JSONEncoder().encode(values), forKey: Self.key(for: project))
    }
}
