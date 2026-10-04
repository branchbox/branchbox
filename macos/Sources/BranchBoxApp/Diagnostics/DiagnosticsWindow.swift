import AppKit
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// The `diagnostics` window: the CLI in use (and the candidates that were rejected), the shell environment, every
/// tool check with its fix, the runtimes, each project's health and the recent operations, with [Run Checks
/// Again], [Copy Report] (redacted Markdown) and [Show Logs in Finder].
struct DiagnosticsWindow: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @State private var isChecking = false

    init() {}

    var body: some View {
        Form {
            cliSection
            environmentSection
            toolsSection
            runtimesSection
            projectsSection
            operationsSection
        }
        .formStyle(.grouped)
        .safeAreaInset(edge: .bottom, spacing: 0) { bottomBar }
        .frame(minWidth: 560, idealWidth: 640, minHeight: 480, idealHeight: 720)
        .task { await initialLoad() }
    }

    // MARK: CLI

    private var cliSection: some View {
        Section {
            switch model.environment.backendState {
            case .resolving:
                LabeledContent("Status") {
                    HStack(spacing: 6) { ProgressView().controlSize(.small); Text("Looking for branchbox…") }
                }
            case .unavailable(let error):
                let presentation = error.presentation()
                LabeledContent("Status") {
                    Label(presentation.title, systemImage: "xmark.octagon.fill").foregroundStyle(.red)
                }
                SettingsCaption(presentation.message)
            case .ready:
                EmptyView()
            }
            if let resolution = model.environment.resolution {
                LabeledContent("Path") { monospaced(resolution.path) }
                LabeledContent("Found") { Text(WelcomeCLISummary.sourceSentence(resolution.source)) }
            }
            if let identity = model.environment.identity {
                if case .preview = identity.kind {
                    LabeledContent("Backend") { Text("Preview (sample data)") }
                }
                LabeledContent("Version") { Text(identity.version.description).monospacedDigit() }
                LabeledContent("Minimum") { Text(BackendIdentity.minimumCLI.description).monospacedDigit() }
                LabeledContent("Contract version") {
                    Text(identity.contractVersion.map(String.init) ?? "None (older CLI)")
                }
                let capabilities = identity.capabilities.map(\.rawValue).sorted()
                if capabilities.isEmpty {
                    LabeledContent("Capabilities") { Text("None").foregroundStyle(.secondary) }
                } else {
                    // A leading-aligned wrapping row of chips, not a right-aligned paragraph.
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Capabilities")
                        FlowLayout(spacing: 4) {
                            ForEach(capabilities, id: \.self) { capability in
                                Text(capability)
                                    .font(.caption.monospaced())
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(.quaternary.opacity(0.6), in: Capsule())
                                    .textSelection(.enabled)
                            }
                        }
                    }
                    .accessibilityElement(children: .combine)
                }
            }
            if let rejected = model.environment.resolution?.rejected, !rejected.isEmpty {
                Text("Copies BranchBox skipped")
                    .font(.callout.weight(.medium))
                ForEach(rejected, id: \.self) { candidate in
                    LabeledContent {
                        Text(candidate.reason).foregroundStyle(.secondary).multilineTextAlignment(.trailing)
                    } label: {
                        Label { monospaced(candidate.path) } icon: { Image(systemName: "xmark.circle").foregroundStyle(.secondary) }
                    }
                }
            }
        } header: {
            Text("BranchBox command-line tool")
        }
    }

    // MARK: Environment

    @ViewBuilder private var environmentSection: some View {
        Section {
            if let summary = model.environment.summary {
                LabeledContent("Shell") { Text(summary.shell ?? "Unknown") }
                LabeledContent("Read") { Text(ToolsTab.sourceLabel(summary)) }
                if let duration = summary.captureDuration {
                    LabeledContent("Took") { Text(ToolsTab.duration(duration)).monospacedDigit() }
                }
                PathEntriesView(entries: summary.pathEntries)
            } else {
                Text("Not read yet.").foregroundStyle(.secondary)
            }
        } header: {
            Text("Shell environment")
        }
    }

    // MARK: Tools

    private var doctorChecks: [DoctorCheck] {
        (model.environment.doctor?.checks ?? []).filter { !$0.id.hasPrefix("repo.") && !$0.id.hasPrefix("runtime.") }
    }

    @ViewBuilder private var toolsSection: some View {
        Section {
            if model.environment.doctor == nil {
                HStack(spacing: 6) {
                    if model.environment.isRunningDoctor { ProgressView().controlSize(.small) }
                    Text(model.environment.isRunningDoctor ? "Checking…" : (model.environment.identity == nil ? "Available once the branchbox tool is found." : "Not checked yet.")).foregroundStyle(.secondary)
                }
            }
            ForEach(doctorChecks, id: \.id) { check in
                DoctorCheckRow(check: check, onFix: perform, verticalPadding: 0)
            }
            ForEach(DiagnosticsAppChecks.checks(), id: \.id) { check in
                DoctorCheckRow(check: check, verticalPadding: 0)
            }
        } header: {
            HStack {
                Text("Tools")
                Spacer()
                if let generated = model.environment.doctor?.generatedAt {
                    Text("Checked \(FeaturePresentation.relative(generated))")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textCase(nil)
                }
            }
        }
    }

    private func perform(_ fix: DoctorFix) {
        switch fix {
        case .openDockerDesktop:
            if let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: DoctorFix.dockerBundleID) {
                NSWorkspace.shared.openApplication(at: app, configuration: NSWorkspace.OpenConfiguration())
            } else {
                NSWorkspace.shared.open(DoctorFix.dockerDownload)
            }
        case .openURL(let url, _):
            NSWorkspace.shared.open(url)
        case .signInToSandboxes:
            let terminal = model.settings.preferredTerminal
            Task { try? await ProjectActions.runInTerminal("sbx login", terminal: terminal) }
        case .copyCommand(let command, _):
            Pasteboard.general.copy(command)
        }
    }

    // MARK: Runtimes

    private var runtimesSection: some View {
        Section {
            ForEach(DiagnosticsRuntimes.rows(doctor: model.environment.doctor), id: \.id) { check in
                DoctorCheckRow(check: check, onFix: perform, verticalPadding: 0)
            }
        } header: {
            Text("Runtimes")
        }
    }

    // MARK: Projects

    @ViewBuilder private var projectsSection: some View {
        Section {
            if model.projects.projects.isEmpty {
                Text("No projects yet.").foregroundStyle(.secondary)
            }
            ForEach(model.projects.projects) { store in
                ProjectHealthRow(health: ProjectHealth(store: store))
            }
        } header: {
            Text("Projects")
        }
    }

    // MARK: Operations

    @ViewBuilder private var operationsSection: some View {
        Section {
            let running = model.operations.running
            let history = model.operations.history.prefix(8)
            if running.isEmpty, history.isEmpty {
                Text("Nothing has run yet.").foregroundStyle(.secondary)
            }
            ForEach(running) { record in
                OperationRow(record: record)
            }
            ForEach(Array(history)) { entry in
                OperationRow(summary: DiagnosticsHistory.summary(entry))
            }
        } header: {
            Text("Recent operations")
        }
    }

    // MARK: Bottom bar

    private var bottomBar: some View {
        HStack(spacing: 8) {
            Button("Show Welcome") {
                model.post(.select(.welcome))
                openWindow(id: SceneID.main)
            }
            Spacer()
            Button("Show Logs in Finder") { AdvancedTab.revealLogsFolder() }
            CopyButton(label: "Copy Report", showsTitle: true) { report() }
            Button {
                Task { await runChecks() }
            } label: {
                if isChecking {
                    ProgressView().controlSize(.small)
                } else {
                    Text("Run Checks Again")
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(isChecking)
            .keyboardShortcut("r", modifiers: .command)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(.bar)
        .overlay(alignment: .top) { Divider() }
    }

    private func monospaced(_ text: String) -> some View {
        Text(text)
            .font(.callout.monospaced())
            .lineLimit(1)
            .truncationMode(.middle)
            .textSelection(.enabled)
            .help(text)
    }

    // MARK: Actions

    private func initialLoad() async {
        if model.environment.doctor == nil { await model.environment.runDoctor(for: nil) }
        for store in model.projects.projects where store.rootExists {
            if store.config == nil { await store.reloadConfig() }
            if store.detect == nil { await store.reloadDetect() }
        }
    }

    private func runChecks() async {
        isChecking = true
        defer { isChecking = false }
        await model.environment.rebootstrap()
        await model.environment.runDoctor(for: nil)
        for store in model.projects.projects {
            store.requestRefresh(.manual)
            guard store.rootExists else { continue }
            await store.reloadConfig()
            await store.reloadDetect()
        }
    }

    private func report() -> String {
        DiagnosticsReportBuilder(model: model).markdown()
    }
}

/// App-side checks the CLI can't make: the editors and terminals BranchBox opens.
@MainActor enum DiagnosticsAppChecks {
    static let apps: [(id: String, title: String, bundleID: String, required: Bool)] = [
        ("app.vscode", "Visual Studio Code", HostLaunchPlan.vscodeBundleID, false),
        ("app.cursor", "Cursor", HostLaunchPlan.cursorBundleID, false),
        ("app.terminal", "Terminal", HostLauncher.terminalBundleID, false),
        ("app.iterm", "iTerm", HostLauncher.iTermBundleID, false),
    ]

    static func checks(workspace: NSWorkspace = .shared) -> [DoctorCheck] {
        apps.map { app in
            if let url = workspace.urlForApplication(withBundleIdentifier: app.bundleID) {
                return DoctorCheck(id: app.id, title: app.title, required: app.required, status: .ok, path: url.path)
            }
            return DoctorCheck(id: app.id, title: app.title, required: app.required, status: .skipped,
                               detail: "Not installed")
        }
    }
}

/// The four runtimes and whether each can run here.
enum DiagnosticsRuntimes {
    static func rows(doctor: DoctorReport?) -> [DoctorCheck] {
        let checks = doctor?.checks ?? []
        let daemon = checks.first { $0.id == "docker.daemon" }
        let container = DoctorCheck(id: "runtime.container", title: "Container (Docker)", required: true,
                                    status: daemon?.status ?? .skipped, path: nil, version: daemon?.version,
                                    detail: daemon == nil ? "Not checked yet" : daemon?.detail,
                                    remediation: daemon?.remediation)
        var rows = [container]
        if let sbx = checks.first(where: { $0.id == "runtime.sbx" }) {
            rows.append(DoctorCheck(id: sbx.id, title: "Docker Sandbox", required: false, status: sbx.status, path: sbx.path,
                                    version: sbx.version, detail: sbx.detail, remediation: sbx.remediation))
        }
        rows += checks.filter { $0.id.hasPrefix("runtime.") && $0.id != "runtime.sbx" && $0.id != "runtime.container" }
        if !rows.contains(where: { $0.id == "runtime.local_vm" || $0.id == "runtime.local-vm" }) {
            rows.append(DoctorCheck(id: "runtime.local-vm", title: "Local VM", required: false, status: .skipped,
                                    detail: "Needs Linux with KVM"))
        }
        rows.append(DoctorCheck(id: "runtime.in-guest", title: "In-guest", required: false, status: .skipped,
                                detail: "Only inside an isolated dev container"))
        return rows
    }
}

/// One project's health for Diagnostics and the report.
struct ProjectHealth: Hashable {
    let name: String
    let path: String
    let folderExists: Bool
    let initialized: Bool?
    let configProblem: String?
    let featureCount: Int
    let droppedRecords: Int
    let lastError: String?

    @MainActor init(store: ProjectStore) {
        name = store.displayName
        path = store.ref.path
        folderExists = store.rootExists
        initialized = store.detect?.initialized
        configProblem = store.configError.map { $0.presentation().message }
        featureCount = store.features.filter { $0.status != .removed }.count
        droppedRecords = store.droppedRecords
        if case .failed(let error, _) = store.loadState { lastError = error.presentation().message } else { lastError = nil }
    }

    var problems: [String] {
        var problems: [String] = []
        if !folderExists { problems.append("Folder is missing") }
        if initialized == false { problems.append("BranchBox isn't set up") }
        if let configProblem { problems.append("Settings can't be read: \(configProblem)") }
        if droppedRecords > 0 { problems.append("\(droppedRecords) unreadable registry \(droppedRecords == 1 ? "entry" : "entries")") }
        if let lastError { problems.append(lastError) }
        return problems
    }

    var summary: String {
        var parts = [featureCount == 1 ? "1 feature" : "\(featureCount) features"]
        if initialized == true { parts.insert("Set up", at: 0) }
        if configProblem == nil, initialized != false { parts.append("settings OK") }
        return parts.joined(separator: " · ")
    }
}

struct ProjectHealthRow: View {
    let health: ProjectHealth

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: health.problems.isEmpty ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                .foregroundStyle(health.problems.isEmpty ? Color.green : .orange)
                .frame(width: 20)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(health.name)
                Text(InitProjectSheet.abbreviated(health.path))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                ForEach(health.problems, id: \.self) { problem in
                    Text(problem).font(.caption).foregroundStyle(.orange).textSelection(.enabled)
                }
            }
            Spacer(minLength: 8)
            Text(health.summary)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }
}

/// Persisted history entries as the app's operation summaries.
enum DiagnosticsHistory {
    static func summary(_ entry: BranchBoxStores.OperationSummary) -> OperationSummary {
        let state: OperationState = switch entry.outcome {
        case .succeeded: .succeeded
        case .succeededWithWarnings: .succeededWithWarnings
        case .partial: .partial
        case .failed: .failed(.commandFailed(Diagnostics(summary: entry.detail ?? "Failed")))
        case .cancelled: .cancelled(note: entry.detail)
        }
        return OperationSummary(id: entry.id, kind: entry.kind, title: entry.title, state: state, startedAt: entry.startedAt,
                                finishedAt: entry.finishedAt)
    }
}

/// The redacted Markdown behind [Copy Report]: the shared `DiagnosticReport` (app, CLI, environment) plus tools,
/// runtimes, projects and recent operations.
@MainActor struct DiagnosticsReportBuilder {
    let model: AppModel

    func markdown() -> String {
        let report = DiagnosticReport(identity: model.environment.identity, environment: model.environment.summary,
                                      secrets: Array(model.settings.extraEnvironment.values))
        var lines: [String] = []
        if case .unavailable(let error) = model.environment.backendState {
            lines += ["", "## CLI status", "", "- \(error.presentation().title): \(error.presentation().message)"]
        }
        for rejected in model.environment.resolution?.rejected ?? [] where model.environment.identity == nil {
            lines.append("- Rejected: \(rejected.path) (\(rejected.reason))")
        }
        lines += ["", "## Tools", ""]
        let checks = (model.environment.doctor?.checks ?? []) + DiagnosticsAppChecks.checks()
        lines += checks.isEmpty ? ["- Not checked"] : checks.map(Self.line)
        lines += ["", "## Projects", ""]
        let projects = model.projects.projects.map(ProjectHealth.init)
        lines += projects.isEmpty ? ["- None"] : projects.map { health in
            "- \(health.name) (\(health.path)): " + ([health.summary] + health.problems).joined(separator: "; ")
        }
        lines += ["", "## Recent operations", ""]
        let history = model.operations.history.prefix(10)
        lines += history.isEmpty ? ["- None"] : history.map { "- \($0.title): \($0.outcome.rawValue)" + ($0.detail.map { " — \($0)" } ?? "") }
        return report.redact(report.markdown() + lines.joined(separator: "\n") + "\n")
    }

    static func line(_ check: DoctorCheck) -> String {
        var text = "- \(check.title) [\(check.required ? "required" : "optional")]: \(check.status.rawValue)"
        if let version = check.version { text += " \(version)" }
        if let detail = check.detail { text += " — \(detail)" }
        if let remediation = check.remediation, check.status != .ok { text += " (fix: \(remediation))" }
        return text
    }
}

#Preview("Diagnostics") {
    DiagnosticsWindow()
        .environment(ProjectsPreviewModel.model())
        .frame(width: 640, height: 720)
}
