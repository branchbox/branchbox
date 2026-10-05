import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// Onboarding in the detail area: (1) the BranchBox command-line tool, (2) the tools it needs on this Mac,
/// (3) the first project. Each step says what it is for in plain words, shows when it is done, and offers one
/// obvious next action. Shown when there are no projects or the CLI is unavailable, and again from Diagnostics.
struct WelcomeView: View {
    @Environment(AppModel.self) private var model
    @State private var notificationsRequested = false
    @State private var locateError: String?
    @State private var showsAllChecks = false

    init() {}

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header
                    .padding(.bottom, 8)
                OnboardingStep(number: 1, title: "Install the BranchBox command-line tool",
                               summary: "BranchBox for Mac runs the branchbox tool to create and remove features.",
                               state: cliStepState) {
                    cliStep
                }
                OnboardingStep(number: 2, title: "Check your Mac",
                               summary: "Features run in containers, so BranchBox needs Git and Docker. The rest is optional.",
                               state: prerequisitesStepState) {
                    prerequisitesStep
                }
                OnboardingStep(number: 3, title: "Add your first project",
                               summary: "Pick a Git repository you work on. BranchBox adds it to the sidebar and helps you set it up.",
                               state: projectStepState) {
                    projectStep
                }
            }
            .frame(maxWidth: 640, alignment: .leading)
            .padding(.horizontal, 32)
            .padding(.vertical, 32)
            .frame(maxWidth: .infinity)
        }
        .dropDestination(for: URL.self) { urls, _ in
            guard isReady, let folder = urls.first(where: \.hasDirectoryPath) ?? urls.first else { return false }
            model.post(.addProject(folder))
            return true
        }
        .task(id: isReady) {
            if isReady, model.environment.doctor == nil { await model.environment.runDoctor(for: nil) }
        }
        .accessibilityIdentifier("welcome")
    }

    // MARK: Header

    private var header: some View {
        HStack(alignment: .center, spacing: 16) {
            Image(systemName: "shippingbox.fill")
                .font(.system(size: 30, weight: .medium))
                .foregroundStyle(.white)
                .frame(width: 60, height: 60)
                .background(LinearGradient(colors: [.accentColor, .accentColor.opacity(0.75)], startPoint: .top, endPoint: .bottom),
                            in: RoundedRectangle(cornerRadius: 14))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text("Welcome to BranchBox")
                    .font(.largeTitle.weight(.bold))
                    .accessibilityAddTraits(.isHeader)
                Text("Work on several features at once. Each one gets its own folder, branch and dev environment, so switching takes a click.")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    // MARK: Step 1: CLI

    private var isReady: Bool {
        if case .ready = model.environment.backendState { return true }
        return false
    }

    private var cliStepState: OnboardingStepState {
        switch model.environment.backendState {
        case .resolving: .inProgress
        case .ready: .done
        case .unavailable: .needsAction
        }
    }

    @ViewBuilder private var cliStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch model.environment.backendState {
            case .resolving:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Looking for the branchbox tool…").foregroundStyle(.secondary)
                }
            case .ready(let identity):
                WelcomeCLISummary(identity: identity, resolution: model.environment.resolution)
                if identity.isLegacy {
                    ProjectNotice(style: .info, title: "An update is available",
                                 message: "This version works, but editing project settings and safer clean-up need a newer branchbox.") {
                        CopyButton(text: CLITooOldView.upgradeCommand, label: "Copy Update Command", showsTitle: true)
                    }
                }
                HStack(spacing: 8) {
                    Button("Use a Different Copy…", action: locate)
                    Spacer()
                }
            case .unavailable(let error):
                cliProblem(error)
            }
            if let locateError {
                Label(locateError, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.orange)
            }
        }
    }

    @ViewBuilder private func cliProblem(_ error: BackendError) -> some View {
        switch error {
        case .cliNotFound(let searched):
            Text("Install it with Homebrew in Terminal, then check again:")
            OnboardingCommandBox(command: CLINotFoundView.installCommand)
            cliButtons
            if !searched.isEmpty {
                DisclosureGroup("Where BranchBox looked") {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(searched, id: \.self) { Text($0).font(.caption.monospaced()).textSelection(.enabled) }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 4)
                }
                .font(.callout)
                .foregroundStyle(.secondary)
            }
        case .cliTooOld(let found, let minimum, let path):
            Text("The copy at \(path) is version \(found.description); BranchBox for Mac needs \(minimum.description) or later. Update it in Terminal:")
                .fixedSize(horizontal: false, vertical: true)
            OnboardingCommandBox(command: CLITooOldView.upgradeCommand)
            cliButtons
        default:
            let presentation = error.presentation()
            ProjectNotice(style: .error, title: presentation.title, message: presentation.message)
            cliButtons
        }
    }

    private var cliButtons: some View {
        HStack(spacing: 8) {
            Button("Check Again") { Task { await model.environment.rebootstrap() } }
                .buttonStyle(.borderedProminent)
                .disabled(model.environment.isBootstrapping)
            Button("Locate…", action: locate)
            if model.environment.isBootstrapping { ProgressView().controlSize(.small) }
            Spacer()
        }
    }

    private func locate() {
        locateError = nil
        guard let url = ProjectActions.chooseExecutable(title: "Locate the branchbox tool",
                                                        startingAt: URL(fileURLWithPath: "/opt/homebrew/bin")) else { return }
        model.settings.cliPathOverride = url.path
        Task { await model.environment.rebootstrap() }
    }

    // MARK: Step 2: prerequisites

    private var visibleChecks: [DoctorCheck] {
        (model.environment.doctor?.hostChecks ?? []).filter { $0.id != "branchbox.cli" }
    }

    private var prerequisitesStepState: OnboardingStepState {
        guard isReady else { return .upcoming }
        guard let doctor = model.environment.doctor else { return .inProgress }
        return doctor.blockingChecks.isEmpty ? .done : .needsAction
    }

    @ViewBuilder private var prerequisitesStep: some View {
        if !isReady {
            Text("Available once the branchbox tool is installed.")
                .foregroundStyle(.secondary)
        } else if model.environment.doctor == nil {
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Checking Git, Docker and the other tools…").foregroundStyle(.secondary)
            }
        } else {
            VStack(alignment: .leading, spacing: 12) {
                let blocking = (model.environment.doctor?.blockingChecks ?? []).filter { $0.id != "branchbox.cli" }
                if blocking.isEmpty {
                    // Done: one line, with the full list one click away.
                    let optional = visibleChecks.filter { !$0.required && $0.status == .warn }.count
                    Label {
                        Text(optional == 0 ? "Git and Docker are ready."
                             : "Git and Docker are ready. \(optional) optional tool\(optional == 1 ? "" : "s") can be added later.")
                    } icon: {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                    }
                    DisclosureGroup("Show all checks", isExpanded: $showsAllChecks) {
                        checklist.padding(.top, 8)
                    }
                    .font(.callout)
                } else {
                    Text(blocking.count == 1
                         ? "\(blocking[0].title) needs attention before you can start features."
                         : "\(blocking.count) required tools need attention before you can start features.")
                        .fontWeight(.medium)
                    checklist
                }
                HStack(spacing: 8) {
                    Button("Check Again") { Task { await model.environment.runDoctor(for: nil) } }
                        .disabled(model.environment.isRunningDoctor)
                    if model.environment.isRunningDoctor { ProgressView().controlSize(.small) }
                    Spacer()
                }
            }
        }
    }

    private var checklist: some View {
        DoctorChecklist(checks: visibleChecks,
                        offersNotifications: model.notifier.isAvailable && !notificationsRequested,
                        onAllowNotifications: allowNotifications)
            .font(.body)
    }

    private func allowNotifications() {
        notificationsRequested = true
        let notifier = model.notifier
        Task { _ = await notifier.requestAuthorizationIfNeeded() }
    }

    // MARK: Step 3: first project

    private var projectStepState: OnboardingStepState {
        if !model.projects.projects.isEmpty { return .done }
        return isReady ? .needsAction : .upcoming
    }

    @ViewBuilder private var projectStep: some View {
        let projects = model.projects.projects
        if projects.isEmpty {
            HStack(spacing: 12) {
                Button {
                    model.post(.addProject(nil))
                } label: {
                    Label("Add Project…", systemImage: "plus")
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(!isReady)
                .accessibilityIdentifier("welcome.addProject")
                Text(isReady ? "or drop a folder onto this window" : "Available once the branchbox tool is installed.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
            }
        } else {
            HStack(spacing: 8) {
                Text(projects.count == 1 ? "\(projects[0].displayName) is in your sidebar."
                     : "\(projects.count) projects are in your sidebar.")
                Spacer()
                if let first = projects.first {
                    Button("Show \(projects.count == 1 ? first.displayName : "Projects")") {
                        model.post(.select(.project(path: first.ref.path)))
                    }
                }
                Button("Add Another…") { model.post(.addProject(nil)) }
                    .disabled(!isReady)
            }
        }
    }
}

/// Where an onboarding step stands.
enum OnboardingStepState: Hashable { case upcoming, inProgress, needsAction, done }

/// One numbered onboarding step as a card: a status badge (number, check, spinner or "!"), title, a sentence on
/// why it matters, and the step's content. Upcoming steps are dimmed but readable.
struct OnboardingStep<Content: View>: View {
    typealias State = OnboardingStepState

    let number: Int
    let title: String
    let summary: String
    let state: State
    @ViewBuilder var content: Content

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            badge
            VStack(alignment: .leading, spacing: 10) {
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(title)
                            .font(.headline)
                            .accessibilityAddTraits(.isHeader)
                        if state == .done {
                            Text("Done")
                                .font(.caption.weight(.medium))
                                .foregroundStyle(.green)
                        }
                    }
                    Text(summary)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(16)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(borderColor, lineWidth: state == .needsAction ? 1.5 : 1))
        .opacity(state == .upcoming ? 0.65 : 1)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Step \(number): \(title), \(stateLabel)")
    }

    private var borderColor: Color {
        state == .needsAction ? Color.accentColor.opacity(0.6) : Color(nsColor: .separatorColor)
    }

    private var stateLabel: String {
        switch state {
        case .upcoming: "not started"
        case .inProgress: "in progress"
        case .needsAction: "needs your action"
        case .done: "done"
        }
    }

    @ViewBuilder private var badge: some View {
        ZStack {
            switch state {
            case .done:
                Circle().fill(Color.green)
                Image(systemName: "checkmark").font(.system(size: 13, weight: .bold)).foregroundStyle(.white)
            case .inProgress:
                Circle().strokeBorder(Color.accentColor, lineWidth: 2)
                ProgressView().controlSize(.mini)
            case .needsAction:
                Circle().fill(Color.accentColor)
                Text("\(number)").font(.system(size: 13, weight: .bold)).foregroundStyle(.white)
            case .upcoming:
                Circle().strokeBorder(Color.secondary.opacity(0.5), lineWidth: 1.5)
                Text("\(number)").font(.system(size: 13, weight: .semibold)).foregroundStyle(.secondary)
            }
        }
        .frame(width: 26, height: 26)
        .accessibilityHidden(true)
    }
}

/// The CLI that is in use: version, location and how it was found.
struct WelcomeCLISummary: View {
    let identity: BackendIdentity
    let resolution: CLIResolution?

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 4) {
            GridRow {
                Text("Version").foregroundStyle(.secondary)
                Text(identity.version.description + (identity.isLegacy ? " (older version)" : "")).monospacedDigit()
            }
            if let path = resolution?.path {
                GridRow {
                    Text("Location").foregroundStyle(.secondary)
                    Text(path)
                        .font(.callout.monospaced())
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
            }
            if let source = resolution?.source {
                GridRow {
                    Text("Found").foregroundStyle(.secondary)
                    Text(Self.sourceSentence(source))
                }
            }
        }
        .font(.callout)
    }

    static func sourceSentence(_ source: CLISource) -> String {
        switch source {
        case .environmentOverride: "from the BRANCHBOX_CLI_PATH variable"
        case .settingsOverride: "where you chose it in Settings"
        case .loginShellPath: "on your shell's PATH"
        case .wellKnownPath: "in a standard install location"
        case .embedded: "inside the app"
        }
    }
}

/// A one-line command in a monospaced box with a copy button.
struct OnboardingCommandBox: View {
    let command: String

    var body: some View {
        HStack(spacing: 8) {
            Text(command)
                .font(.body.monospaced())
                .textSelection(.enabled)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 8)
            CopyButton(text: command, label: "Copy Command")
                .buttonStyle(.borderless)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 7))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(Color(nsColor: .separatorColor)))
    }
}

#Preview("Welcome") {
    WelcomeView()
        .environment(ProjectsPreviewModel.model(.cliMissing))
        .frame(width: 820, height: 760)
}
