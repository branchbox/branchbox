import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Starts a feature, then shows its progress and result.
///
/// One `StartFlow` per presentation (MAC-18): cancelling discards it, so the next presentation starts empty.
/// The sheet turns into the operation's progress (Run in Background / Stop) and then its result in place; it
/// never presents another sheet (chaining goes through `model.post`).
struct StartFeatureSheet: View {
    let project: ProjectRef?
    let prefill: StartFeatureRequest?

    @Environment(AppModel.self) private var model
    @State private var flow: StartFlow?

    init(project: ProjectRef?, prefill: StartFeatureRequest?) {
        self.project = project
        self.prefill = prefill
    }

    /// A sheet around an existing flow (previews and render tests).
    init(flow: StartFlow) {
        self.project = flow.draft.project
        self.prefill = nil
        _flow = State(initialValue: flow)
    }

    var body: some View {
        Group {
            if let flow {
                StartFeatureContent(flow: flow)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 600, idealWidth: 640, minHeight: 500, idealHeight: 620)
        .onAppear {
            if flow == nil { flow = StartFlow(model: model, project: project, prefill: prefill) }
        }
    }
}

private struct StartFeatureContent: View {
    @Bindable var flow: StartFlow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Group {
            if let record = flow.record {
                if record.isFinished {
                    finished(record)
                } else {
                    running(record)
                }
            } else if flow.availableProjects.isEmpty, !flow.model.hasStarted {
                // projects.json is still loading (⌘N right at launch): don't claim there are no projects.
                ProgressView().frame(maxWidth: .infinity, minHeight: 300)
            } else if flow.availableProjects.isEmpty {
                noProjects
            } else {
                StartForm(flow: flow, actions: actions, onCancel: { dismiss() })
            }
        }
        .stopConfirmation(flow.stop, model: flow.model)
        .task { await flow.prepare() }
        .onChange(of: flow.record?.isFinished == true) { _, finished in
            guard finished else { return }
            if flow.record?.needsAttention == true { flow.record?.acknowledge() }
            Task { await flow.autoLaunchIfConfigured(with: actions) }
        }
    }

    private var actions: FlowActions {
        FlowActions(model: flow.model, openWindow: { openWindow(id: $0) })
    }

    /// No project to start in: add one first (posted to the window's router, never a sheet from this sheet).
    private var noProjects: some View {
        FlowSheetLayout(title: "Start a Feature", systemImage: "plus.square.on.square") {
            if let problem = flow.backendProblem {
                ResultCard(error: problem, context: nil) { action in
                    Task { _ = await actions.perform(action) }
                }
            }
            if let missing = flow.model.projects.projects.first {
                // Projects exist, but none of their folders does (a moved or unmounted repository).
                ContentUnavailableView {
                    Label("Your projects' folders are missing", systemImage: "questionmark.folder")
                } description: {
                    Text("Features start inside a project's folder. Locate the moved folder, or add another project.")
                } actions: {
                    Button("Show Project") {
                        flow.model.post(.select(.project(path: missing.ref.path)))
                        dismiss()
                    }
                    .buttonStyle(.borderedProminent)
                    Button("Add Project…") {
                        flow.model.post(.addProject(nil))
                        dismiss()
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 300)
            } else {
                ContentUnavailableView {
                    Label("No projects yet", systemImage: "folder.badge.plus")
                } description: {
                    Text("Features start inside a project. Add a Git repository that uses BranchBox, then start your first feature.")
                } actions: {
                    Button("Add Project…") {
                        flow.model.post(.addProject(nil))
                        dismiss()
                    }
                    .buttonStyle(.borderedProminent)
                }
                .frame(maxWidth: .infinity, minHeight: 300)
            }
        } footer: {
            Spacer()
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
        }
    }

    private func running(_ record: OperationRecord) -> some View {
        FlowSheetLayout(title: "Start a Feature", subtitle: flow.projectStore?.displayName, systemImage: "plus.square.on.square") {
            OperationProgressView(record: record, capabilities: flow.model.environment.identity?.capabilities ?? [])
                .frame(minHeight: 420)
        } footer: {
            Text("You can close this; the start keeps running in Activity.")
                .font(.callout)
                .foregroundStyle(.secondary)
            Spacer()
            Button("Run in Background") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Stop…", role: .destructive) { flow.stop.request(record) }
                .keyboardShortcut(".", modifiers: .command)
        }
    }

    @ViewBuilder private func finished(_ record: OperationRecord) -> some View {
        if let summary = flow.summary {
            StartResultView(flow: flow, summary: summary, record: record, actions: actions, onDone: { dismiss() })
        } else {
            StartFailureView(flow: flow, record: record, actions: actions, onClose: { dismiss() })
        }
    }
}

// MARK: Form

private struct StartForm: View {
    @Bindable var flow: StartFlow
    let actions: FlowActions
    let onCancel: () -> Void
    @FocusState private var titleFocused: Bool

    var body: some View {
        FlowSheetLayout(title: "Start a Feature", subtitle: subtitle, systemImage: "plus.square.on.square") {
            if let problem = flow.backendProblem {
                FlowNotice(style: .error, text: problem.oneLine)
            }
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 16) {
                if flow.needsProjectPicker {
                    GridRow {
                        label("Project")
                        Picker("Project", selection: Binding(
                            get: { flow.draft.project },
                            set: { if let project = $0 { flow.selectProject(project) } }
                        )) {
                            if flow.draft.project == nil { Text("Choose a project").tag(ProjectRef?.none) }
                            ForEach(flow.availableProjects, id: \.self) { project in
                                Text(flow.model.projects.project(project)?.displayName ?? project.displayName)
                                    .tag(ProjectRef?.some(project))
                            }
                        }
                        .labelsHidden()
                        .frame(maxWidth: 280, alignment: .leading)
                    }
                }
                GridRow {
                    label("Name")
                    nameField
                }
                GridRow {
                    label("Start from")
                    BasePicker(flow: flow)
                }
                GridRow {
                    label("Runtime")
                    VStack(alignment: .leading, spacing: 6) {
                        RuntimePicker(flow: flow, actions: actions)
                        ForEach(errors(.runtime), id: \.self) { FlowFieldMessage(style: .error, text: $0) }
                    }
                }
                GridRow {
                    label("Setup")
                    VStack(alignment: .leading, spacing: 4) {
                        Picker("Setup", selection: Binding(get: { flow.draft.mode }, set: { flow.setMode($0) })) {
                            Text("Full").tag(StartFeatureRequest.Mode.full)
                            Text("Quick").tag(StartFeatureRequest.Mode.minimal)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .fixedSize()
                        Text(flow.draft.mode == .full
                             ? "Every module: dev container, Compose services, database, tunnel and specs."
                             : "Just the worktree and its dev container; modules are skipped.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                GridRow {
                    label("Prompt")
                    VStack(alignment: .leading, spacing: 6) {
                        PromptEditor(flow: flow)
                        ForEach(errors(.prompt), id: \.self) { FlowFieldMessage(style: .error, text: $0) }
                    }
                }
            }
            AdvancedOptions(flow: flow)
            if let error = flow.dispatchError {
                FlowNotice(style: .error, text: error)
            }
        } footer: {
            copyCommandButton
            Spacer()
            if let behind = flow.queuedBehind, flow.canStart {
                Text("Waits for “\(behind)”")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Button("Cancel", action: onCancel)
                .keyboardShortcut(.cancelAction)
            Button(flow.queuedBehind != nil && flow.canStart ? "Queue Start" : "Start") { flow.start() }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!flow.canStart)
                .help(flow.canStart ? "" : (flow.validationErrors.first ?? "Waiting for the name check"))
        }
        .onAppear { titleFocused = true }
    }

    private var subtitle: String {
        flow.projectStore?.displayName ?? (flow.draft.project?.displayName ?? "Choose a project")
    }

    private func label(_ text: String) -> some View {
        Text(text)
            .foregroundStyle(.secondary)
            .gridColumnAlignment(.trailing)
            .frame(minWidth: 72, alignment: .trailing)
    }

    @ViewBuilder private var nameField: some View {
        VStack(alignment: .leading, spacing: 8) {
            if flow.nameOverride == nil {
                TextField("What are you working on? e.g. “Add OAuth login”", text: Binding(
                    get: { flow.title }, set: { flow.setTitle($0) }
                ))
                .textFieldStyle(.roundedBorder)
                .font(.title3)
                .focused($titleFocused)
                .accessibilityIdentifier("sheet.start.title")
            } else {
                HStack(spacing: 8) {
                    TextField("feature-name", text: Binding(
                        get: { flow.nameOverride ?? "" }, set: { flow.setNameOverride($0) }
                    ))
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.title3, design: .monospaced))
                    .accessibilityIdentifier("sheet.start.name")
                    Button("Use Title") { flow.useTitleForName() }
                        .help("Name the feature after the title again")
                }
            }
            NamePreviewLine(flow: flow)
            ForEach(errors(.name), id: \.self) { error in
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    FlowFieldMessage(style: .error, text: error)
                    if let existing = flow.collidingFeature, error.contains("already exists in this project") {
                        Button("Show \(existing.workFeature)") {
                            flow.model.post(.select(.feature(projectPath: flow.draft.project?.path ?? "",
                                                             name: existing.workFeature)))
                            onCancel()
                        }
                        .buttonStyle(.link)
                        .font(.callout)
                    }
                }
            }
            ForEach(flow.notices, id: \.self) { notice in
                FlowFieldMessage(style: .info, text: notice)
            }
        }
    }

    private enum ErrorPlace { case name, runtime, prompt }

    /// Each visible problem under the control it is about.
    private func errors(_ place: ErrorPlace) -> [String] {
        flow.visibleErrors.filter { error in
            let isPrompt = error.localizedCaseInsensitiveContains("prompt")
            let isRuntime = !isPrompt && (error.localizedCaseInsensitiveContains("runtime")
                || error.localizedCaseInsensitiveContains("sandbox") || error.localizedCaseInsensitiveContains("environment"))
            switch place {
            case .prompt: return isPrompt
            case .runtime: return isRuntime
            case .name: return !isPrompt && !isRuntime
            }
        }
    }

    @ViewBuilder private var copyCommandButton: some View {
        let command = flow.draft.makeRequest().flatMap { request in
            (try? flow.model.backend())?.previewCommandLine(.start(request))
        }
        if let command {
            CopyButton(text: command, label: "Copy as Command", showsTitle: true)
        } else {
            Button {} label: { Label("Copy as Command", systemImage: "doc.on.doc") }
                .disabled(true)
                .help(flow.draft.makeRequest() == nil ? "Fix the problems above first"
                                                       : "Only available with the BranchBox CLI")
        }
    }
}

/// What the title resolves to: the feature's name, branch and folder (each truncated in the middle, never cut
/// short to a guess), with [Edit Name].
private struct NamePreviewLine: View {
    let flow: StartFlow

    var body: some View {
        if let slug = flow.draft.resolvedName {
            HStack(alignment: .top, spacing: 12) {
                Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 2) {
                    row("Feature", slug, emphasized: true)
                    row("Branch", flow.draft.branchName ?? slug)
                    if let path = flow.worktreePath {
                        row("Folder", (path as NSString).abbreviatingWithTildeInPath)
                    }
                }
                Spacer(minLength: 0)
                if flow.isPreviewPending {
                    ProgressView().controlSize(.mini)
                }
                if flow.nameOverride == nil {
                    Button("Edit Name") { flow.beginEditingName() }
                        .buttonStyle(.link)
                        .font(.callout)
                }
            }
            .padding(.vertical, 2)
            .accessibilityElement(children: .combine)
        } else {
            Text("The feature's name, branch and folder come from what you type.")
                .font(.callout)
                .foregroundStyle(.tertiary)
        }
    }

    private func row(_ label: String, _ value: String, emphasized: Bool = false) -> some View {
        GridRow {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .gridColumnAlignment(.trailing)
            Text(value)
                .font(.system(.callout, design: .monospaced).weight(emphasized ? .semibold : .regular))
                .lineLimit(1)
                .truncationMode(.middle)
                .textSelection(.enabled)
                .help(value)
        }
    }
}

/// Current HEAD by default; local and remote branches in a searchable popover.
private struct BasePicker: View {
    let flow: StartFlow
    @State private var showsList = false
    @State private var query = ""

    var body: some View {
        HStack(spacing: 8) {
            Button {
                showsList.toggle()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "arrow.triangle.branch").foregroundStyle(.secondary)
                    Text(title).lineLimit(1).truncationMode(.middle)
                    Image(systemName: "chevron.up.chevron.down").font(.caption2).foregroundStyle(.secondary)
                }
                .frame(minWidth: 200, alignment: .leading)
            }
            .popover(isPresented: $showsList, arrowEdge: .bottom) { list }
            .accessibilityLabel("Start from: \(title)")
            if let error = flow.branchesError {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .help("Couldn't list branches: \(error)")
            }
        }
    }

    private var title: String {
        if let base = flow.draft.base { return base }
        return flow.branches?.current.map { "Current HEAD (\($0))" } ?? "Current HEAD"
    }

    private var list: some View {
        let local = filtered(flow.branches?.local ?? [])
        let remote = filtered(flow.branches?.remote ?? [])
        return VStack(alignment: .leading, spacing: 0) {
            TextField("Search branches", text: $query)
                .textFieldStyle(.roundedBorder)
                .padding(10)
            Divider()
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    row(title: flow.branches?.current.map { "Current HEAD (\($0))" } ?? "Current HEAD", value: nil)
                    if !local.isEmpty { header("Local") }
                    ForEach(local, id: \.self) { row(title: $0, value: $0) }
                    if !remote.isEmpty { header("Remote") }
                    ForEach(remote, id: \.self) { row(title: $0, value: $0) }
                    if flow.branches == nil {
                        Text(flow.branchesError ?? "Loading branches…")
                            .foregroundStyle(.secondary)
                            .padding(10)
                    }
                }
                .padding(.vertical, 4)
            }
        }
        .frame(width: 300, height: 320)
    }

    private func filtered(_ names: [String]) -> [String] {
        let needle = query.trimmingCharacters(in: .whitespaces)
        return needle.isEmpty ? names : names.filter { $0.localizedCaseInsensitiveContains(needle) }
    }

    private func header(_ text: String) -> some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.top, 8)
            .padding(.bottom, 2)
    }

    private func row(title: String, value: String?) -> some View {
        Button {
            flow.setBase(value)
            showsList = false
        } label: {
            HStack {
                Image(systemName: "checkmark")
                    .opacity(flow.draft.base == value ? 1 : 0)
                    .accessibilityHidden(true)
                Text(title).lineLimit(1).truncationMode(.middle)
                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// Radio rows: Container (with Docker's state) and Docker Sandboxes (with its sign-in state).
private struct RuntimePicker: View {
    let flow: StartFlow
    let actions: FlowActions

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(flow.runtimeOptions) { option in
                row(option)
            }
            if let note = flow.runtimeNote {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Runtime")
    }

    private func row(_ option: StartFlow.RuntimeOption) -> some View {
        let selected = flow.draft.runtime == option.provider
        return HStack(spacing: 10) {
            Button {
                flow.setRuntime(option.provider)
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: selected ? "largecircle.fill.circle" : "circle")
                        .foregroundStyle(selected ? Color.accentColor : .secondary)
                        .accessibilityHidden(true)
                    Image(systemName: option.provider.symbol)
                        .frame(width: 18)
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    Text(option.provider.label)
                    Text(option.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!option.isEnabled)
            .opacity(option.isEnabled ? 1 : 0.5)
            .accessibilityAddTraits(selected ? [.isSelected] : [])
            .accessibilityLabel("\(option.provider.label), \(option.detail)")
            if option.needsSignIn {
                Button("Sign In…") {
                    Task {
                        _ = await actions.perform(.runInTerminal(command: ["sbx", "login"], workingDirectory: nil,
                                                                 label: "Sign in to Docker Sandboxes"))
                    }
                }
                .controlSize(.small)
                .help("Opens Terminal to run sbx login")
            }
        }
        .frame(maxWidth: 460, alignment: .leading)
    }
}

/// The prompt with its counter (hard limit 2,000), recent prompts and the Quick-only default prompt.
private struct PromptEditor: View {
    let flow: StartFlow

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextEditor(text: Binding(get: { flow.draft.prompt }, set: { flow.setPrompt($0) }))
                .font(.body)
                .scrollContentBackground(.hidden)
                .padding(6)
                .frame(height: 84)
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6)
                    .strokeBorder(flow.promptOverLimit ? Color.red : Color(nsColor: .separatorColor)))
                .overlay(alignment: .topLeading) {
                    if flow.draft.prompt.isEmpty {
                        Text(flow.draft.useDefaultPrompt ? "Using the project's default prompt"
                                                         : "Optional: what the coding agent should work on")
                            .foregroundStyle(.tertiary)
                            .padding(.horizontal, 11)
                            .padding(.vertical, 6)
                            .allowsHitTesting(false)
                    }
                }
                .disabled(flow.draft.useDefaultPrompt)
                .accessibilityLabel("Prompt")
            HStack(spacing: 12) {
                Menu("Recent") {
                    ForEach(flow.recentPrompts, id: \.self) { prompt in
                        Button(prompt.count > 60 ? String(prompt.prefix(60)) + "…" : prompt) { flow.setPrompt(prompt) }
                    }
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .disabled(flow.recentPrompts.isEmpty || flow.draft.useDefaultPrompt)
                Toggle("Use the default prompt", isOn: Binding(
                    get: { flow.draft.useDefaultPrompt }, set: { flow.setUseDefaultPrompt($0) }
                ))
                .disabled(flow.draft.mode != .minimal)
                .help(flow.draft.mode == .minimal ? "Let BranchBox seed the agent with its default prompt"
                                                  : "Only available in Quick setup")
                Spacer()
                Text("\(flow.draft.promptLength.formatted()) / 2,000")
                    .font(.caption)
                    .monospacedDigit()
                    .foregroundStyle(flow.promptOverLimit ? Color.red : .secondary)
                    .accessibilityLabel("\(flow.draft.promptLength) of 2,000 characters")
            }
            .font(.callout)
        }
    }
}

/// Branch prefix, skipped modules, reuse, keep-on-failure (sbx) and verbose logs; remembered per project.
private struct AdvancedOptions: View {
    @Bindable var flow: StartFlow

    private static let modules: [(id: String, title: String)] = [
        ("compose", "Compose services"), ("database", "Database"), ("tunnel", "Tunnel"), ("specs", "Specs"),
    ]

    var body: some View {
        DisclosureGroup(isExpanded: $flow.showsAdvanced) {
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 12) {
                GridRow {
                    label("Branch prefix")
                    TextField("feature", text: Binding(get: { flow.draft.branchPrefix }, set: { flow.setBranchPrefix($0) }))
                        .textFieldStyle(.roundedBorder)
                        .font(.system(.body, design: .monospaced))
                        .frame(width: 180)
                }
                GridRow {
                    label("Skip")
                    HStack(spacing: 14) {
                        ForEach(Self.modules, id: \.id) { module in
                            let locked = module.id == "tunnel" && flow.tunnelLocked
                            Toggle(module.title, isOn: Binding(
                                get: { locked || flow.draft.skipModules.contains(module.id) },
                                set: { flow.setSkipped(module.id, $0) }
                            ))
                            .disabled(locked || flow.draft.mode == .minimal)
                            .help(locked ? "Tunnels are off for this project" : "Don't set up \(module.title.lowercased())")
                        }
                    }
                }
                GridRow {
                    label("Reuse")
                    ReusePicker(flow: flow)
                }
                if flow.draft.runtime == .sbx {
                    GridRow {
                        label("On failure")
                        Toggle("Keep the sandbox so I can inspect it", isOn: Binding(
                            get: { flow.draft.keepRuntimeOnFailure }, set: { flow.setKeepRuntimeOnFailure($0) }
                        ))
                    }
                }
                GridRow {
                    label("Logging")
                    Toggle("Verbose (debug logs and telemetry)", isOn: Binding(
                        get: { flow.draft.verbose }, set: { flow.setVerbose($0) }
                    ))
                }
            }
            .padding(.top, 10)
            .padding(.leading, 4)
        } label: {
            Text("Advanced")
                .font(.headline)
        }
    }

    private func label(_ text: String) -> some View {
        Text(text)
            .foregroundStyle(.secondary)
            .gridColumnAlignment(.trailing)
            .frame(minWidth: 96, alignment: .trailing)
    }
}

private struct ReusePicker: View {
    let flow: StartFlow

    private enum Choice: Hashable { case none, folder, sandbox }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Picker("Reuse", selection: Binding(get: { choice }, set: { apply($0) })) {
                Text("Nothing (new worktree)").tag(Choice.none)
                Text("The existing folder").tag(Choice.folder)
                if flow.draft.runtime == .sbx { Text("The kept sandbox").tag(Choice.sandbox) }
            }
            .labelsHidden()
            .frame(width: 220)
            if case .existingWorktree(let policy) = flow.draft.reuse {
                Picker("If its .devcontainer differs", selection: Binding(
                    get: { policy }, set: { flow.setReuse(.existingWorktree($0)) }
                )) {
                    Text("Stop and tell me").tag(DevcontainerReusePolicy.fail)
                    Text("Keep the folder's").tag(DevcontainerReusePolicy.preserve)
                    Text("Replace with the project's").tag(DevcontainerReusePolicy.overwrite)
                    Text("Show the differences").tag(DevcontainerReusePolicy.inspect)
                }
                .frame(width: 360)
            }
        }
    }

    private var choice: Choice {
        switch flow.draft.reuse {
        case .none: .none
        case .existingWorktree: .folder
        case .retainedRuntime: .sandbox
        }
    }

    private func apply(_ choice: Choice) {
        switch choice {
        case .none: flow.setReuse(.none)
        case .folder: flow.setReuse(.existingWorktree(.fail))
        case .sandbox: flow.setReuse(.retainedRuntime)
        }
    }
}
