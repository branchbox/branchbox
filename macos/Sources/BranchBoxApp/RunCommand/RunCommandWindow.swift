import AppKit
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI
import UniformTypeIdentifiers

/// The `run` window: runs one command in a feature's runtime or dev container.
///
/// The command goes through `/bin/sh -lc` unless "Run through shell" is off, runs as one `.exec` operation, and
/// shows its output when it finishes: stdout and stderr, the exit code (a non-zero exit is a result, not an
/// error), and the duration. The initializer is final (DESIGN §4.11).
struct RunCommandWindow: View {
    let feature: FeatureRef?

    init(feature: FeatureRef?) {
        self.feature = feature
    }

    var body: some View {
        Group {
            if let feature {
                RunCommandView(feature: feature)
            } else {
                ContentUnavailableView {
                    Label("No feature chosen", systemImage: "play.rectangle")
                } description: {
                    Text("Open Run Command from a feature's toolbar or the Feature menu.")
                }
            }
        }
        .frame(minWidth: 560, idealWidth: 680, minHeight: 440, idealHeight: 560)
    }
}

/// The window's content for one feature. Render tests pin the phase and the dev container state.
struct RunCommandView: View {
    enum OutputTab: Hashable { case output, errors }

    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    let feature: FeatureRef
    var pinnedPhase: RunCommandPhase?
    var pinnedDevcontainerRunning: Bool?
    var initialDraft = RunCommandDraft()

    @State private var draft = RunCommandDraft()
    @State private var draftLoaded = false
    @State private var recordID: UUID?
    @State private var history: [String] = []
    @State private var tab: OutputTab = .output
    @State private var confirmingStop = false
    @State private var devcontainerRunning = false
    @State private var rejection: String?

    @MainActor private static let historyDefaults = AppSettings.defaultsForCurrentProcess()

    var body: some View {
        let store = model.projects.project(feature.project)
        let record = store?.feature(named: feature.name)
        let availability = record.map { makeAvailability(for: $0, store: store) }
        let phase = pinnedPhase ?? RunCommandPhase.phase(of: recordID.flatMap(model.operations.record))
        VStack(alignment: .leading, spacing: 14) {
            header(record: record)
            if let reason = availability?.runCommand.disabledReason ?? (record == nil ? "\(feature.name) is no longer listed" : nil) {
                Label(reason, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
            }
            commandField(phase: phase, enabled: availability?.runCommand.isEnabled ?? false)
            options(record: record, availability: availability, running: phase.isRunning)
            if let rejection {
                Label(rejection, systemImage: "hand.raised.fill")
                    .foregroundStyle(.orange)
                    .font(.callout)
            }
            output(phase, terminal: availability?.terminal)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(20)
        .navigationTitle("Run in \(feature.name)")
        .onAppear {
            guard !draftLoaded else { return }
            draftLoaded = true
            draft = initialDraft
            history = RunCommandHistory.load(for: feature, from: Self.historyDefaults)
        }
        .task(id: record?.updatedAt) { await loadDevcontainer(record) }
        .confirmationDialog(stopConfirmation.title, isPresented: $confirmingStop, titleVisibility: .visible) {
            Button(stopConfirmation.stopLabel, role: .destructive) { if let recordID { model.operations.cancel(recordID) } }
            Button(stopConfirmation.keepLabel, role: .cancel) {}
        } message: {
            Text(stopConfirmation.message)
        }
    }

    // MARK: Parts

    private func header(record: FeatureRecord?) -> some View {
        HStack(alignment: .center, spacing: 10) {
            ColorSwatch(hex: record?.color, size: 12)
            VStack(alignment: .leading, spacing: 2) {
                Text("Run in \(feature.name)")
                    .font(.title2.weight(.semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .accessibilityAddTraits(.isHeader)
                Text(record?.worktreePath.map { OverviewCard.abbreviated($0) } ?? feature.project.displayName)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 8)
            if let record {
                RuntimeBadge(provider: record.runtime.provider)
                    .fixedSize()
            }
        }
    }

    private func commandField(phase: RunCommandPhase, enabled: Bool) -> some View {
        HStack(spacing: 8) {
            TextField("Command", text: $draft.text, prompt: Text("make test"))
                .textFieldStyle(.roundedBorder)
                .font(.system(.body, design: .monospaced))
                .onSubmit { if enabled, !phase.isRunning { run() } }
                .accessibilityIdentifier("run.command")
            commandMenu
            if phase.isRunning {
                Button("Stop", role: .destructive) { confirmingStop = true }
                    .keyboardShortcut(".", modifiers: .command)
                    .accessibilityIdentifier("run.stop")
            } else {
                Button {
                    run()
                } label: {
                    Label("Run", systemImage: "play.fill")
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(!enabled || draft.argv == nil)
                .help(draft.argv == nil && !draft.trimmed.isEmpty ? "The command has an unterminated quote" : "Run (⌘↩)")
                .accessibilityIdentifier("run.run")
            }
        }
        .controlSize(.large)
    }

    private var commandMenu: some View {
        let quick = model.settings.quickCommands[feature.project.path] ?? []
        return Menu {
            if !quick.isEmpty {
                Section("Quick Commands") {
                    ForEach(quick, id: \.self) { command in Button(command) { draft.text = command } }
                }
            }
            if !history.isEmpty {
                Section("Recent in \(feature.name)") {
                    ForEach(history, id: \.self) { command in Button(command) { draft.text = command } }
                }
            }
            if quick.isEmpty && history.isEmpty {
                Text("No commands yet")
            }
            Divider()
            Button("Save as Quick Command") { saveQuickCommand() }
                .disabled(draft.trimmed.isEmpty || quick.contains(draft.trimmed))
        } label: {
            Image(systemName: "clock.arrow.circlepath")
        }
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Quick commands for \(feature.project.displayName) and recent commands")
        .accessibilityLabel("Quick and recent commands")
    }

    /// While a command runs, the output area offers [Open in Terminal Instead] itself, so the options row drops its
    /// copy rather than showing the same action twice.
    private func options(record: FeatureRecord?, availability: FeatureActionAvailability?, running: Bool) -> some View {
        let containerAvailable = record?.runtime.provider == .container && (pinnedDevcontainerRunning ?? devcontainerRunning)
        return HStack(spacing: 16) {
            Toggle("Run through shell", isOn: $draft.runThroughShell)
                .toggleStyle(.checkbox)
                .help("On: runs /bin/sh -lc with your text, so pipes, globs and && work. Off: runs the words directly.")
            Picker("Run in", selection: $draft.target) {
                Text("Feature runtime").tag(ExecRequest.Target.featureRuntime)
                Text("Dev container").tag(ExecRequest.Target.devcontainer)
                    .selectionDisabled(!containerAvailable)
            }
            .pickerStyle(.segmented)
            .fixedSize()
            .help(containerAvailable ? "Where the command runs"
                  : "Dev container: only for container-runtime features whose dev container is running")
            Spacer(minLength: 8)
            if let availability, !running {
                let plan = availability.terminal
                Button("Open in Terminal Instead") { HostLaunchFeedback.shared.launch(plan, for: feature) }
                    .buttonStyle(.link)
                    .disabled(!plan.isEnabled)
                    .help(plan.disabledReason ?? "Open a shell in the feature's folder")
                    .accessibilityIdentifier("run.openTerminal")
            }
        }
        .onChange(of: containerAvailable) { _, available in
            if !available { draft.target = .featureRuntime }
        }
    }

    // MARK: Output

    @ViewBuilder private func output(_ phase: RunCommandPhase, terminal: HostLaunchPlan?) -> some View {
        switch phase {
        case .idle:
            placeholder(symbol: "text.alignleft", text: "Output appears here when the command finishes · up to 32 MiB")
        case .queued(let behind):
            placeholder(symbol: "hourglass", text: behind.isEmpty ? "Waiting to start…" : "Waiting for “\(behind)”…",
                        spinning: true)
        case .running(let since):
            TimelineView(.periodic(from: .now, by: 1)) { context in
                let elapsed = since.map { OperationPresentation.elapsed(.seconds(max(0, context.date.timeIntervalSince($0)))) }
                // No live output while it runs: say so, and offer a terminal (live output) right here.
                placeholder(symbol: "play.circle", text: "Running\(elapsed.map { " for \($0)" } ?? "")…",
                            caption: "Output appears when the command finishes. For live output, use a terminal.",
                            spinning: true) {
                    if let terminal {
                        Button("Open in Terminal Instead") { HostLaunchFeedback.shared.launch(terminal, for: feature) }
                            .disabled(!terminal.isEnabled)
                            .help(terminal.disabledReason ?? "Open a shell in the feature's folder and run it there")
                            .accessibilityIdentifier("run.running.openTerminal")
                    }
                }
            }
        case .finished(let output):
            finished(output)
        case .failed(let error):
            ScrollView {
                ResultCard(error: error, context: recordID.flatMap(model.operations.record)?.context,
                           operationID: recordID) { recovery in
                    HostLaunchFeedback.shared.perform(recovery, for: feature, model: model, openWindow: openWindow)
                }
            }
        case .cancelled(let note):
            placeholder(symbol: "stop.circle", text: "Stopped. " + (note ?? "No output was captured."))
        }
    }

    private func placeholder(symbol: String, text: String, caption: String? = nil, spinning: Bool = false) -> some View {
        placeholder(symbol: symbol, text: text, caption: caption, spinning: spinning) { EmptyView() }
    }

    private func placeholder<Actions: View>(symbol: String, text: String, caption: String? = nil, spinning: Bool = false,
                                            @ViewBuilder actions: () -> Actions) -> some View {
        VStack(spacing: 10) {
            if spinning {
                ProgressView()
            } else {
                Image(systemName: symbol)
                    .font(.largeTitle)
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
            Text(text)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            if let caption {
                Text(caption)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .multilineTextAlignment(.center)
            }
            actions()
                .padding(.top, 4)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .textBackgroundColor).opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor).opacity(0.6)))
    }

    private func finished(_ output: ExecOutput) -> some View {
        let text = tab == .output ? output.result.stdout : output.result.stderr
        let errorLines = output.result.stderr.split(whereSeparator: \.isNewline).count
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Picker("Show", selection: $tab) {
                    Text("Output").tag(OutputTab.output)
                    // "Stderr (3 lines)": a line count that can't be misread as the exit code next to it.
                    Text(Self.stderrTabTitle(lines: errorLines)).tag(OutputTab.errors)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                Tag(title: output.exitLabel, systemImage: output.succeeded ? "checkmark.circle.fill" : "xmark.circle.fill",
                    tint: output.tint.color)
                    .accessibilityLabel("Exit code \(output.exitCode)")
                if let duration = output.durationLabel {
                    Text(duration)
                        .font(.callout.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                CopyButton(text: text, label: tab == .output ? "Copy Output" : "Copy Stderr")
                    .buttonStyle(.borderless)
                Button {
                    save(output)
                } label: {
                    Image(systemName: "square.and.arrow.down")
                }
                .buttonStyle(.borderless)
                .help("Save Output…")
                .accessibilityLabel("Save Output…")
            }
            let shown = Self.displayed(text)
            if shown.isClipped {
                Label("Showing the last \(Self.displayLimit / 1024) KB. Copy and Save… include all of it.",
                      systemImage: "text.append")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            ScrollView(.vertical) {
                Text(text.isEmpty ? (tab == .output ? "No output" : "No errors") : shown.text)
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(text.isEmpty ? .secondary : .primary)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    .padding(10)
            }
            .frame(maxHeight: .infinity)
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor).opacity(0.6)))
            if output.isTruncated {
                Label("Output was cut at 32 MiB.", systemImage: "scissors")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    static func stderrTabTitle(lines: Int) -> String {
        switch lines {
        case 0: "Stderr"
        case 1: "Stderr (1 line)"
        default: "Stderr (\(lines) lines)"
        }
    }

    /// SwiftUI lays a `Text` out in one pass on the main thread, so megabytes of output would stall the app. The
    /// view shows the last `displayLimit` bytes (from a line start); Copy and Save keep the full text.
    static let displayLimit = 256 * 1024

    static func displayed(_ text: String) -> (text: String, isClipped: Bool) {
        let utf8 = text.utf8
        guard utf8.count > displayLimit else { return (text, false) }
        var start = utf8.index(utf8.endIndex, offsetBy: -displayLimit)
        if let newline = text[start...].firstIndex(of: "\n") { start = text.index(after: newline) }
        return (String(text[start...]), true)
    }

    // MARK: Actions

    private func makeAvailability(for record: FeatureRecord, store: ProjectStore?) -> FeatureActionAvailability {
        let backendReady: Bool = if case .ready = model.environment.backendState { true } else { false }
        return FeatureActionAvailability(
            record: record, folderExists: store?.folderExists(for: record) ?? false, backendReady: backendReady,
            preferences: LaunchPreferences(settings: model.settings, projectDefaultAgent: store?.config?.effective.editorDefaultAgent))
    }

    private var stopConfirmation: CancelConfirmation {
        CancelConfirmation(kind: .exec, title: recordID.flatMap(model.operations.record)?.title ?? "the command",
                           capabilities: model.environment.identity?.capabilities ?? [])
    }

    private func run() {
        guard let request = draft.makeRequest(for: feature) else { return }
        rejection = nil
        tab = .output
        switch model.actions.dispatch(.exec(request)) {
        case .started(let record), .queued(let record, _):
            recordID = record.id
            history = RunCommandHistory.record(draft.trimmed, for: feature, in: Self.historyDefaults)
        case .rejected(let reason):
            rejection = reason
        case .unavailable(let error):
            rejection = error.presentation().title
        }
    }

    private func saveQuickCommand() {
        var commands = model.settings.quickCommands
        let command = draft.trimmed
        guard !command.isEmpty else { return }
        commands[feature.project.path, default: []].append(command)
        model.settings.quickCommands = commands
    }

    private func save(_ output: ExecOutput) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(feature.name)-output.txt"
        panel.allowedContentTypes = [.plainText]
        let text = output.result.stdout + (output.result.stderr.isEmpty ? "" : "\n--- stderr ---\n" + output.result.stderr)
        panel.begin { response in
            guard response == .OK, let url = panel.url else { return }
            do {
                try text.write(to: url, atomically: true, encoding: .utf8)
            } catch {
                HostLaunchFeedback.shared.report("Couldn't save the output: \(error.localizedDescription)", for: feature)
            }
        }
    }

    private func loadDevcontainer(_ record: FeatureRecord?) async {
        guard pinnedDevcontainerRunning == nil, let record, record.runtime.provider == .container,
              record.status != .removed else { return }
        do {
            devcontainerRunning = try await model.backend().devcontainerStatus(for: feature).state == .running
        } catch {
            devcontainerRunning = false
        }
    }
}

#Preview("Run Command, exit 3") {
    RunCommandView(feature: FeatureRef(project: PreviewSamples.project, name: PreviewSamples.features[0].workFeature),
                   pinnedPhase: .finished(ExecOutput(result: ExecResult(exitCode: 3, stdout: "running checks\n2 failed\n",
                                                                        stderr: "error: lint failed\n"),
                                                     duration: .milliseconds(2400))),
                   initialDraft: RunCommandDraft(text: "sh -c 'exit 3'"))
        .environment(FeaturePreviewModel.make())
        .frame(width: 680, height: 520)
}
