import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Settings › Tools: which branchbox CLI is used (Locate…, Use Automatic, Check Again), the PATH read from the
/// login shell (Re-capture), and extra environment variables for every command. Changes switch the CLI at once.
struct ToolsTab: View {
    @Environment(AppModel.self) private var model
    @State private var rebootstrapper = SettingsRebootstrapper()
    @State private var variables: [EnvironmentVariableRow] = []
    @State private var isRecapturing = false

    var body: some View {
        Form {
            cliSection
            shellSection
            environmentSection
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
        .onAppear { variables = EnvironmentVariableRow.rows(from: model.settings.extraEnvironment) }
    }

    // MARK: CLI

    private var cliSection: some View {
        Section {
            switch model.environment.backendState {
            case .resolving:
                LabeledContent("Status") {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.small)
                        Text("Looking for branchbox…")
                    }
                }
            case .ready(let identity):
                LabeledContent("Version") {
                    Text(identity.version.description + (identity.isLegacy ? " (older version: some features need an update)" : ""))
                        .monospacedDigit()
                }
                if let resolution = model.environment.resolution {
                    LabeledContent("Location") { pathText(resolution.path) }
                    LabeledContent("Found") { Text(WelcomeCLISummary.sourceSentence(resolution.source)) }
                }
            case .unavailable(let error):
                let presentation = error.presentation()
                LabeledContent("Status") {
                    Label(presentation.title, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                }
                SettingsCaption(presentation.message)
            }
            if let override = model.settings.cliPathOverride, !override.isEmpty {
                LabeledContent("Chosen by you") { pathText(override) }
            }
            HStack {
                Button("Locate…", action: locate)
                Button("Use Automatic") {
                    model.settings.cliPathOverride = nil
                    rebootstrapper.now(model.environment)
                }
                .disabled(model.settings.cliPathOverride == nil)
                .help("Find branchbox on your shell's PATH and the standard install locations")
                Spacer()
                if model.environment.isBootstrapping { ProgressView().controlSize(.small) }
                Button("Check Again") { rebootstrapper.now(model.environment) }
                    .disabled(model.environment.isBootstrapping)
            }
        } header: {
            Text("BranchBox command-line tool")
        }
    }

    private func pathText(_ path: String) -> some View {
        Text(path)
            .font(.callout.monospaced())
            .lineLimit(1)
            .truncationMode(.middle)
            .textSelection(.enabled)
            .help(path)
    }

    private func locate() {
        let start = model.environment.resolution.map { URL(fileURLWithPath: $0.path).deletingLastPathComponent() }
            ?? URL(fileURLWithPath: "/opt/homebrew/bin")
        guard let url = ProjectActions.chooseExecutable(title: "Locate the branchbox tool", startingAt: start) else { return }
        model.settings.cliPathOverride = url.path
        rebootstrapper.now(model.environment)
    }

    // MARK: Shell

    private var shellSection: some View {
        Section {
            if let summary = model.environment.summary {
                LabeledContent("Shell") { Text(summary.shell ?? "Unknown") }
                LabeledContent("Read") {
                    Text(Self.sourceLabel(summary) + (summary.captureDuration.map { " in \(Self.duration($0))" } ?? ""))
                }
                PathEntriesView(entries: summary.pathEntries)
            } else {
                Text("Not read yet.").foregroundStyle(.secondary)
            }
            HStack {
                SettingsCaption("Changed your shell profile? Read it again so BranchBox finds the same tools as Terminal.")
                Spacer()
                if isRecapturing { ProgressView().controlSize(.small) }
                Button("Re-capture", action: recapture)
                    .disabled(isRecapturing)
            }
        } header: {
            Text("Shell environment")
        }
    }

    private func recapture() {
        isRecapturing = true
        let environment = model.environment
        Task {
            await environment.recaptureEnvironment()
            await environment.rebootstrap()
            isRecapturing = false
        }
    }

    static func sourceLabel(_ summary: EnvironmentSummary) -> String {
        let source = switch summary.source {
        case .interactiveLogin: "From your login shell"
        case .login: "From your login shell (non-interactive)"
        case .processEnvironment: "From the app's own environment"
        case .cachedPath: "From the last capture"
        }
        return summary.isProvisional ? source + " (still reading…)" : source
    }

    static func duration(_ duration: Duration) -> String {
        duration.formatted(.units(allowed: [.seconds, .milliseconds], width: .narrow, maximumUnitCount: 1))
    }

    // MARK: Extra environment

    private var environmentSection: some View {
        Section {
            if variables.isEmpty {
                Text("No extra variables.").foregroundStyle(.secondary)
            }
            ForEach($variables) { $row in
                // A long name is truncated in the middle (its start and end both stay readable; wrapping would
                // hyphenate it, and a hyphen isn't valid in a name) and the help shows it in full. The name column
                // grows with the pane up to a cap; a long value wraps onto a few lines.
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    TextField("Name", text: $row.name, prompt: Text("NAME"))
                        .textFieldStyle(.roundedBorder)
                        .font(.body.monospaced())
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(minWidth: 150, idealWidth: 220, maxWidth: 260)
                        .labelsHidden()
                        .help(row.name.isEmpty ? "Variable name" : row.name)
                    TextField("Value", text: $row.value, prompt: Text("value"), axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .font(.body.monospaced())
                        .lineLimit(1...4)
                        .frame(maxWidth: .infinity)
                        .labelsHidden()
                        .help(row.value.isEmpty ? "Value" : row.value)
                    Button {
                        variables.removeAll { $0.id == row.id }
                        commitVariables()
                    } label: {
                        Image(systemName: "minus.circle.fill").foregroundStyle(.secondary)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Remove \(row.name.isEmpty ? "variable" : row.name)")
                }
                .onChange(of: row) { _, _ in commitVariables() }
                let trimmed = row.name.trimmingCharacters(in: .whitespaces)
                if !trimmed.isEmpty, !EnvironmentVariableRow.isValidName(trimmed) {
                    // Rows with invalid names are not passed to the CLI; say so rather than drop them silently.
                    Label("Not used: names use letters, digits and _ and can't start with a digit.",
                          systemImage: "exclamationmark.triangle")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }
            HStack {
                Button {
                    variables.append(EnvironmentVariableRow(name: "", value: ""))
                } label: {
                    Label("Add Variable", systemImage: "plus")
                }
                Spacer()
            }
        } header: {
            Text("Extra environment variables")
        } footer: {
            Label("Not for secrets: these are passed to every branchbox command. Use 1Password references or the project's own .env for tokens.",
                  systemImage: "exclamationmark.shield")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private func commitVariables() {
        let environment = EnvironmentVariableRow.environment(from: variables)
        guard environment != model.settings.extraEnvironment else { return }
        model.settings.extraEnvironment = environment
        rebootstrapper.schedule(model.environment)
    }
}

/// One editable extra environment variable.
struct EnvironmentVariableRow: Identifiable, Hashable {
    let id = UUID()
    var name: String
    var value: String

    static func rows(from environment: [String: String]) -> [EnvironmentVariableRow] {
        environment.keys.sorted().map { EnvironmentVariableRow(name: $0, value: environment[$0] ?? "") }
    }

    /// Rows with a valid name (`[A-Za-z_][A-Za-z0-9_]*`); a later duplicate wins.
    static func environment(from rows: [EnvironmentVariableRow]) -> [String: String] {
        var environment: [String: String] = [:]
        for row in rows {
            let name = row.name.trimmingCharacters(in: .whitespaces)
            guard isValidName(name) else { continue }
            environment[name] = row.value
        }
        return environment
    }

    static func isValidName(_ name: String) -> Bool {
        guard let first = name.unicodeScalars.first, first == "_" || (first.isASCII && first.properties.isAlphabetic) else {
            return false
        }
        return name.unicodeScalars.allSatisfy { $0 == "_" || ($0.isASCII && ($0.properties.isAlphabetic || ("0"..."9").contains($0))) }
    }
}

/// The shell's PATH, one entry per line in a small scrolling box with [Copy PATH]. Shared by Settings › Tools
/// and Diagnostics so the same data reads the same way.
struct PathEntriesView: View {
    let entries: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("PATH")
                Spacer()
                CopyButton(text: entries.joined(separator: ":"), label: "Copy PATH")
                    .buttonStyle(.borderless)
            }
            ScrollView {
                Text(entries.joined(separator: "\n"))
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .frame(height: 96)
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color(nsColor: .separatorColor)))
        }
    }
}
