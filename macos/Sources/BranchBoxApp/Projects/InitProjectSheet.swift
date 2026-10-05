import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation
import Observation
import SwiftUI

/// The choices of the Set Up BranchBox sheet, and the `InitRequest` they make (D-21). The repository is moved only
/// when the user picked "Move into a parent folder" after the warning; tunnels start off.
struct InitDraft: Hashable {
    enum Layout: String, Hashable, CaseIterable { case keepInPlace, moveIntoParent }

    /// Stacks `init -s` accepts; nil is automatic detection.
    static let stacks = ["rails", "nodejs", "rust", "generic"]

    var folder: URL
    var mode: InitSheetMode
    var stack: String?
    var includeDevcontainer = true
    var includeEnv = true
    var codingAgents = true
    /// Only the move confirmation sets `.moveIntoParent`.
    private(set) var layout: Layout = .keepInPlace
    var tunnelsEnabled = false
    var usesOnePassword = false
    var gitHubRef = ""
    var signingKeyRef = ""
    var verifyOnePasswordRefs = true

    init(folder: URL, mode: InitSheetMode) {
        self.folder = folder.standardizedFileURL
        self.mode = mode
    }

    /// Called from the move warning's confirm button only.
    mutating func confirmMoveIntoParent() { layout = .moveIntoParent }

    mutating func keepInPlace() { layout = .keepInPlace }

    /// Where the repository is expected after a move: `<folder>/main` (Preview shows the CLI's exact answer).
    var movedRepositoryPath: String { folder.appendingPathComponent("main").path }

    /// Where feature folders go when the repository stays put: next to it.
    var featureFolderParent: String { folder.deletingLastPathComponent().path }

    static func stackLabel(_ stack: String) -> String {
        switch stack {
        case "rails": "Rails"
        case "nodejs": "Node.js"
        case "rust": "Rust"
        case "generic": "Generic"
        default: stack.capitalized
        }
    }

    /// Problems that block Initialize (1Password references must be one-line `op://…`, §RS-3).
    var problems: [String] {
        guard usesOnePassword else { return [] }
        var problems: [String] = []
        let github = gitHubRef.trimmingCharacters(in: .whitespaces)
        if github.isEmpty {
            problems.append("Enter the 1Password reference for your GitHub token")
        } else if !Self.isOnePasswordReference(github) {
            problems.append("The GitHub token reference must start with op://")
        }
        let signing = signingKeyRef.trimmingCharacters(in: .whitespaces)
        if !signing.isEmpty, !Self.isOnePasswordReference(signing) {
            problems.append("The signing key reference must start with op://")
        }
        return problems
    }

    static func isOnePasswordReference(_ text: String) -> Bool {
        text.hasPrefix("op://") && !text.contains("\n") && text.count > 5
    }

    /// The request for `mode` (`initialize`/`update` from the sheet mode unless overridden, `validate` for Check
    /// Setup). Options a CLI can't take are left out: 1Password needs `init-json`, tunnels need `config`.
    func makeRequest(dryRun: Bool, capabilities: Set<Capability>, mode override: InitRequest.Mode? = nil) -> InitRequest {
        let mode = override ?? (self.mode == .repair ? .update : .initialize)
        var onePassword = InitRequest.OnePassword.unchanged
        if usesOnePassword, capabilities.contains(.initJSON), mode != .validate {
            let signing = signingKeyRef.trimmingCharacters(in: .whitespaces)
            onePassword = .configure(githubRef: gitHubRef.trimmingCharacters(in: .whitespaces),
                                     signingKeyRef: signing.isEmpty ? nil : signing, verify: verifyOnePasswordRefs)
        }
        let appliesTunnels = mode == .initialize && capabilities.contains(.config)
        return InitRequest(folder: folder, stack: mode == .validate ? nil : stack,
                           skipDevcontainer: !includeDevcontainer, skipEnv: !includeEnv, codingAgents: codingAgents,
                           reorganize: mode == .initialize && layout == .moveIntoParent, dryRun: dryRun, mode: mode,
                           onePassword: onePassword, tunnelsEnabled: appliesTunnels ? tunnelsEnabled : nil)
    }
}

/// The sheet's state: the draft, what `detect` found, and the preview and initialize operations.
@MainActor @Observable final class InitSheetModel {
    var draft: InitDraft
    private(set) var detected: DetectReport?
    /// The dry run behind [Preview].
    private(set) var preview: OperationRecord?
    /// The real run (or Check Setup).
    private(set) var run: OperationRecord?
    private(set) var dispatchProblem: String?
    /// The preview log is showing instead of the form.
    var showsPreview = false

    init(folder: URL, mode: InitSheetMode, detected: DetectReport? = nil) {
        draft = InitDraft(folder: folder, mode: mode)
        self.detected = detected
    }

    func loadDetect(using model: AppModel) async {
        guard detected == nil, let backend = try? model.backend() else { return }
        detected = try? await backend.detect(draft.folder)
    }

    func startPreview(using model: AppModel) {
        let request = draft.makeRequest(dryRun: true, capabilities: model.environment.identity?.capabilities ?? [])
        if let record = dispatch(.initProject(request), using: model) {
            preview = record
            showsPreview = true
        }
    }

    func initialize(using model: AppModel, mode: InitRequest.Mode? = nil) {
        let request = draft.makeRequest(dryRun: false, capabilities: model.environment.identity?.capabilities ?? [], mode: mode)
        if let record = dispatch(.initProject(request), using: model) { run = record }
    }

    /// D-21: the repository moves only after a dry run of exactly this request succeeded, so the user saw the
    /// CLI's real target (the form's path is only an estimate). Nil when Set Up may run; otherwise the reason.
    func moveNeedsPreview(capabilities: Set<Capability>) -> String? {
        guard draft.mode == .setUp, draft.layout == .moveIntoParent else { return nil }
        let wanted = draft.makeRequest(dryRun: true, capabilities: capabilities)
        guard let preview, case .initProject(let previewed) = preview.context, previewed == wanted else {
            return "Preview first: moving the repository needs a dry run of exactly these settings"
        }
        switch preview.state {
        case .succeeded, .succeededWithWarnings: return nil
        case .queued, .running: return "Wait for the preview to finish"
        default: return "The preview didn't succeed; fix the problem and preview again"
        }
    }

    /// Back to the form after a failure or a preview.
    func edit() {
        showsPreview = false
        if let run, !OperationSummary(run).isRunning { self.run = nil }
    }

    private func dispatch(_ request: OperationRequestContext, using model: AppModel) -> OperationRecord? {
        dispatchProblem = nil
        switch model.actions.dispatch(request) {
        case .started(let record), .queued(let record, _):
            return record
        case .rejected(let reason):
            dispatchProblem = reason
        case .unavailable(let error):
            dispatchProblem = error.presentation().message
        }
        return nil
    }
}

/// Sets up (or repairs) BranchBox in a folder through `branchbox init -y`: stack, what to add, where the repository
/// lives, tunnels and 1Password; [Preview] runs a dry run and shows its log; Initialize runs it and shows the
/// stack, modules, warnings and next steps. The app never writes the repository's files itself (D-19).
struct InitProjectSheet: View {
    let folder: URL
    let mode: InitSheetMode

    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var sheet: InitSheetModel
    @State private var confirmingMove = false
    @State private var confirmingStop = false
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    init(folder: URL, mode: InitSheetMode) {
        self.folder = folder
        self.mode = mode
        _sheet = State(initialValue: InitSheetModel(folder: folder, mode: mode))
    }

    /// Additive: a sheet with prepared state (render tests, previews).
    init(model: InitSheetModel) {
        folder = model.draft.folder
        mode = model.draft.mode
        _sheet = State(initialValue: model)
    }

    private var capabilities: Set<Capability> { model.environment.identity?.capabilities ?? [] }
    private var isContract: Bool { model.environment.identity?.isLegacy == false }
    private var projectName: String {
        folder.lastPathComponent == "main" ? folder.deletingLastPathComponent().lastPathComponent : folder.lastPathComponent
    }

    var body: some View {
        ProjectSheetScaffold(title: mode == .repair ? "Repair BranchBox Setup" : "Set Up BranchBox",
                             subtitle: folder.path,
                             systemImage: mode == .repair ? "wrench.and.screwdriver" : "sparkles") {
            content
        } footer: {
            footer
        }
        .frame(minWidth: 600, idealWidth: 600, minHeight: 560, idealHeight: 640)
        .task { await sheet.loadDetect(using: model) }
        .confirmationDialog(stopConfirmation.title, isPresented: $confirmingStop, titleVisibility: .visible) {
            Button(stopConfirmation.stopLabel, role: .destructive) {
                if let run = sheet.run { model.operations.cancel(run.id) }
            }
            Button(stopConfirmation.keepLabel, role: .cancel) {}
        } message: {
            Text(stopConfirmation.message)
        }
        .confirmationDialog("Move the repository into a parent folder?", isPresented: $confirmingMove, titleVisibility: .visible) {
            Button("Move Repository") { sheet.draft.confirmMoveIntoParent() }
            Button("Keep It Where It Is", role: .cancel) {}
        } message: {
            Text("BranchBox will move \(folder.path) into a new folder and keep it there as “main”. Editors, terminals and other tools that use the old location will need to reopen it. Use Preview to see the exact location first.")
        }
    }

    // MARK: Content

    @ViewBuilder private var content: some View {
        if let run = sheet.run {
            runView(run)
        } else if sheet.showsPreview, let preview = sheet.preview {
            previewView(preview)
        } else {
            form
        }
    }

    private var form: some View {
        Form {
            if let problem = sheet.dispatchProblem {
                Section { ProjectNotice(style: .error, title: "Couldn't start", message: problem) }
            }
            Section {
                Picker("Project type", selection: $sheet.draft.stack) {
                    Text(automaticStackLabel).tag(String?.none)
                    Divider()
                    ForEach(InitDraft.stacks, id: \.self) { Text(InitDraft.stackLabel($0)).tag(String?.some($0)) }
                }
                Toggle(isOn: $sheet.draft.includeDevcontainer) {
                    Text("Dev container")
                    Text("Adds .devcontainer so every feature runs in its own container.")
                }
                Toggle(isOn: $sheet.draft.includeEnv) {
                    Text("Environment file")
                    Text("Creates .env from your project's template, with ports for each feature.")
                }
                Toggle(isOn: $sheet.draft.codingAgents) {
                    Text("Coding agent support")
                    Text("Shares your Claude Code and Codex settings with feature containers.")
                }
            } header: {
                Text("What to add")
            }
            if mode == .setUp {
                layoutSection
                tunnelsSection
            }
            onePasswordSection
        }
        .formStyle(.grouped)
    }

    private var automaticStackLabel: String {
        if let stack = sheet.detected?.stack, !stack.isEmpty {
            return "Automatic (\(InitDraft.stackLabel(stack.lowercased())))"
        }
        return "Automatic"
    }

    private var layoutSection: some View {
        Section {
            Picker("Location", selection: layoutBinding) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Keep the repository where it is")
                    Text("Feature folders are created next to it, in \(Self.abbreviated(sheet.draft.featureFolderParent)).")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .tag(InitDraft.Layout.keepInPlace)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Move it into a parent folder")
                    Text("The repository becomes “main” inside a new folder that also holds its features.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .tag(InitDraft.Layout.moveIntoParent)
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            if sheet.draft.layout == .moveIntoParent {
                ProjectNotice(style: .warning, title: "The repository will move",
                             message: "Expected new location: \(Self.abbreviated(sheet.draft.movedRepositoryPath)). Reopen it in your editor and terminals afterwards.")
            }
        } header: {
            Text("Where the repository lives")
        }
    }

    /// Picking "Move" asks first; the draft changes only on confirmation.
    private var layoutBinding: Binding<InitDraft.Layout> {
        Binding(get: { sheet.draft.layout }, set: { layout in
            switch layout {
            case .keepInPlace: sheet.draft.keepInPlace()
            case .moveIntoParent: confirmingMove = true
            }
        })
    }

    @ViewBuilder private var tunnelsSection: some View {
        Section {
            if capabilities.contains(.config) {
                Toggle(isOn: $sheet.draft.tunnelsEnabled) {
                    Text("Share features through public tunnels")
                    Text("Needs a Cloudflare account. You can turn this on later in Project Settings.")
                }
            } else {
                LabeledContent("Public tunnels") {
                    Text("Set up later from Terminal")
                        .foregroundStyle(.secondary)
                }
                Text("This version of branchbox can't choose tunnels during setup. Update it to change this here.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } header: {
            Text("Sharing")
        }
    }

    @ViewBuilder private var onePasswordSection: some View {
        if capabilities.contains(.initJSON) {
            Section {
                Toggle(isOn: $sheet.draft.usesOnePassword) {
                    Text("Use 1Password for Git credentials")
                    Text("Containers read your GitHub token from 1Password instead of a file.")
                }
                if sheet.draft.usesOnePassword {
                    TextField("GitHub token", text: $sheet.draft.gitHubRef, prompt: Text("op://Private/GitHub/token"))
                    TextField("Signing key (optional)", text: $sheet.draft.signingKeyRef, prompt: Text("op://Private/SSH Key/private key"))
                    Toggle("Check the references with the 1Password CLI", isOn: $sheet.draft.verifyOnePasswordRefs)
                    ForEach(sheet.draft.problems, id: \.self) { problem in
                        Label(problem, systemImage: "exclamationmark.circle.fill")
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
            } header: {
                Text("1Password")
            }
        }
    }

    // MARK: Preview and run

    private func previewView(_ record: OperationRecord) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            let summary = OperationSummary(record)
            HStack(spacing: 8) {
                if summary.isRunning {
                    ProgressView().controlSize(.small)
                    Text("Previewing… nothing in the repository changes.")
                } else if case .failed(let error) = summary.state {
                    Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
                    Text(error.presentation(context: record.context).message).textSelection(.enabled)
                } else {
                    Image(systemName: "eye").foregroundStyle(.secondary)
                    Text("This is what Set Up would do. Nothing has changed yet.")
                }
                Spacer()
            }
            .font(.callout)
            LogView(lines: record.log.lines, archiveURL: record.log.archiveURL, firstIndex: record.log.droppedLines,
                    revision: record.log.revision)
                .background(.background, in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
        }
        .padding(20)
    }

    @ViewBuilder private func runView(_ record: OperationRecord) -> some View {
        let summary = OperationSummary(record)
        if summary.isRunning {
            // Run in Background and Stop live in the sheet's footer.
            OperationProgressView(record: record, capabilities: capabilities)
                .padding(20)
        } else if case .initProject(let report)? = record.result {
            ProjectInitResultView(report: report, mode: InitProjectSheet.initMode(of: record.context), warnings: record.warnings)
        } else if case .failed(let error) = summary.state {
            ScrollView {
                ResultCard(error: error, context: record.context, operationID: record.id) { action in
                    perform(action, record: record)
                }
                .padding(20)
            }
        } else if case .cancelled(let note) = summary.state {
            ProjectNotice(style: .warning, title: "Setup was stopped", message: note)
                .padding(20)
        }
    }

    // MARK: Footer

    @ViewBuilder private var footer: some View {
        if let run = sheet.run {
            let summary = OperationSummary(run)
            if summary.isRunning {
                Spacer()
                Button("Run in Background") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Stop", role: .destructive) { confirmingStop = true }
                    .keyboardShortcut(".", modifiers: .command)
                    .disabled(!run.isCancellable)
            } else if case .initProject(let report)? = run.result, InitProjectSheet.initMode(of: run.context) != .validate {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Start Your First Feature…") {
                    let root = report.workspacePath.map { URL(fileURLWithPath: $0, isDirectory: true) } ?? folder
                    model.post(.startFeature(project: ProjectRef(root: root), prefill: nil))
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            } else if run.result != nil {
                Spacer()
                Button("Done") { dismiss() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            } else {
                Button("Back") { sheet.edit() }
                Spacer()
                Button("Close") { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }
        } else if sheet.showsPreview {
            Button("Back") { sheet.edit() }
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
            initializeButton
        } else {
            if mode == .repair {
                Button("Check Setup") { sheet.initialize(using: model, mode: .validate) }
                    .help("Checks the setup without changing anything")
            }
            Button("Preview") { sheet.startPreview(using: model) }
                .help("Shows what would change, without changing anything")
                .disabled(!sheet.draft.problems.isEmpty)
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
            initializeButton
        }
    }

    private var initializeButton: some View {
        Button(mode == .repair ? "Repair" : "Set Up") { sheet.initialize(using: model) }
            .buttonStyle(.borderedProminent)
            .keyboardShortcut(.defaultAction)
            .disabled(!sheet.draft.problems.isEmpty || model.environment.identity == nil || movePreviewProblem != nil)
            .help(movePreviewProblem ?? (mode == .repair ? "Repair the setup" : "Set up BranchBox in this folder"))
            .accessibilityIdentifier("initProject.run")
    }

    private var movePreviewProblem: String? {
        sheet.moveNeedsPreview(capabilities: model.environment.identity?.capabilities ?? [])
    }

    private var stopConfirmation: CancelConfirmation {
        CancelConfirmation(kind: .initProject, title: sheet.run?.title ?? "Set Up", capabilities: capabilities)
    }

    /// Recoveries the sheet can perform: the log and Diagnostics in their windows, Locate… in Settings.
    private func perform(_ action: RecoveryAction, record: OperationRecord) {
        switch action {
        case .showLog:
            model.post(.showActivity(operation: record.id))
            dismiss()
        case .openDoctor:
            openWindow(id: SceneID.diagnostics)
        case .locateCLI:
            openSettings()
        case .retry:
            _ = model.actions.perform(action)
        case .revealInFinder(let path):
            ProjectActions.reveal(path)
        case .runInTerminal(let command, let directory, _):
            let terminal = model.settings.preferredTerminal
            Task { try? await ProjectActions.runInTerminal(HostLaunchPlan.shellCommand(command), workingDirectory: directory,
                                                          terminal: terminal) }
        case .refresh(let project):
            model.projects.project(project)?.requestRefresh(.manual)
        case .copyCommand:
            break                                                 // ResultCard copies in place
        }
    }

    /// The init mode of an `.initProject` request; `.initialize` otherwise.
    static func initMode(of context: OperationRequestContext) -> InitRequest.Mode {
        if case .initProject(let request) = context { return request.mode }
        return .initialize
    }

    static func abbreviated(_ path: String) -> String {
        (path as NSString).abbreviatingWithTildeInPath
    }
}

/// A finished init: where the project is, what BranchBox found and added, warnings and next steps.
struct ProjectInitResultView: View {
    let report: InitReport
    let mode: InitRequest.Mode
    var warnings: [String] = []

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                ProjectNotice(style: allWarnings.isEmpty ? .success : .warning, title: title,
                             message: report.workspacePath.map { InitProjectSheet.abbreviated($0) })
                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 8) {
                    if let stack = report.stack {
                        GridRow {
                            Text("Project type").foregroundStyle(.secondary)
                            Text(InitDraft.stackLabel(stack.lowercased()))
                        }
                    }
                    if let adapter = report.adapter, !Self.sameName(adapter, report.stack.map { InitDraft.stackLabel($0.lowercased()) }) {
                        GridRow {
                            Text("Adapter").foregroundStyle(.secondary)
                            Text(adapter)
                        }
                    }
                    if report.reorganized {
                        GridRow {
                            Text("Location").foregroundStyle(.secondary)
                            Text("Moved into a parent folder")
                        }
                    }
                    if let status = report.onePasswordStatus {
                        GridRow {
                            Text("1Password").foregroundStyle(.secondary)
                            Text(status.capitalized)
                        }
                    }
                }
                if !report.modules.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Modules").font(.headline)
                        FlowLayout(spacing: 6) {
                            ForEach(report.modules, id: \.self) { module in
                                Label(module, systemImage: "checkmark.circle.fill")
                                    .font(.callout)
                                    .labelStyle(ModuleChipStyle())
                            }
                        }
                    }
                }
                if !allWarnings.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Warnings").font(.headline)
                        ForEach(allWarnings, id: \.self) { warning in
                            Label(warning, systemImage: "exclamationmark.triangle")
                                .foregroundStyle(.orange)
                                .textSelection(.enabled)
                        }
                    }
                }
                let steps = Self.nextSteps(report.nextSteps, offersStart: mode != .validate)
                if !steps.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Next steps").font(.headline)
                        ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                Text(steps.count == 1 ? step.text : "\(index + 1). \(step.text)")
                                    .textSelection(.enabled)
                                    .fixedSize(horizontal: false, vertical: true)
                                if let command = step.command {
                                    CopyButton(text: command, label: "Copy Command")
                                        .buttonStyle(.borderless)
                                }
                            }
                        }
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// "Generic" twice (project type and adapter) says nothing new.
    static func sameName(_ adapter: String, _ stack: String?) -> Bool {
        guard let stack else { return false }
        return adapter.caseInsensitiveCompare(stack) == .orderedSame
    }

    struct NextStep: Equatable {
        let text: String
        var command: String?
    }

    /// The CLI's next steps in the app's words. Starting a feature is dropped when the footer offers it, and
    /// `cd` into the workspace means nothing here; a git command becomes a sentence with the command to copy.
    static func nextSteps(_ steps: [String], offersStart: Bool) -> [NextStep] {
        steps.compactMap { raw in
            let step = raw.trimmingCharacters(in: .whitespaces)
            let lower = step.lowercased()
            if step.isEmpty || lower.hasPrefix("cd ") { return nil }
            if lower.contains("feature start") || lower.contains("first feature") {
                return offersStart ? nil : NextStep(text: "Start your first feature.")
            }
            if lower.hasPrefix("git add") {
                return NextStep(text: "Commit the new setup files so everyone working on the project gets them.", command: step)
            }
            if lower.hasPrefix("open in vs code") {
                return NextStep(text: "To work on the main checkout itself, open it in VS Code or Cursor and reopen it in its dev container.")
            }
            return NextStep(text: step)
        }
    }

    private var allWarnings: [String] {
        var seen = Set<String>()
        return (report.warnings + warnings).filter { seen.insert($0).inserted }
    }

    private var title: String {
        switch mode {
        case .initialize: "BranchBox is set up"
        case .update: "Setup repaired"
        case .validate: allWarnings.isEmpty ? "The setup looks good" : "The setup needs attention"
        }
    }
}

private struct ModuleChipStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) {
            configuration.icon.foregroundStyle(.green)
            configuration.title
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(.quaternary.opacity(0.6), in: Capsule())
    }
}


#Preview("Set up") {
    InitProjectSheet(folder: URL(fileURLWithPath: "/Users/dev/projects/acme"), mode: .setUp)
        .environment(ProjectsPreviewModel.model())
}
