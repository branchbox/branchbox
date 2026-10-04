import AppKit
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// What a doctor row offers to fix its problem (onboarding step 2 and Diagnostics).
enum DoctorFix: Sendable, Hashable {
    /// `open -b com.docker.docker`.
    case openDockerDesktop
    /// A web page, e.g. the Docker Desktop download.
    case openURL(URL, label: String)
    /// `sbx login` in the user's terminal.
    case signInToSandboxes
    /// A command to paste into Terminal, e.g. `xcode-select --install`.
    case copyCommand(String, label: String)

    static let dockerBundleID = "com.docker.docker"
    static let dockerDownload = URL(string: "https://www.docker.com/products/docker-desktop/")!

    var title: String {
        switch self {
        case .openDockerDesktop: "Open Docker Desktop"
        case .openURL(_, let label): label
        case .signInToSandboxes: "Sign In…"
        case .copyCommand(_, let label): label
        }
    }

    /// The fix for a check that needs one; nil for passing and skipped checks, and for problems without a fix the
    /// app can offer (their remediation text is still shown).
    static func fix(for check: DoctorCheck) -> DoctorFix? {
        guard check.status == .error || check.status == .warn else { return nil }
        let remediation = check.remediation ?? ""
        switch check.id {
        case "docker.daemon":
            return .openDockerDesktop
        case "docker.cli":
            return .openURL(dockerDownload, label: "Get Docker Desktop…")
        case "git":
            return .copyCommand("xcode-select --install", label: "Copy Install Command")
        case "runtime.sbx" where remediation.contains("sbx login"):
            return .signInToSandboxes
        default:
            break
        }
        if let command = command(in: remediation) { return .copyCommand(command, label: "Copy Command") }
        return nil
    }

    /// A shell command named by a remediation ("npm install -g @devcontainers/cli", "Install …: brew install gh").
    static func command(in remediation: String) -> String? {
        let candidate = remediation.split(separator: ":", maxSplits: 1).last.map(String.init) ?? remediation
        let trimmed = candidate.trimmingCharacters(in: .whitespaces)
        let tools = ["brew ", "npm ", "xcode-select ", "sbx ", "gh ", "op ", "curl "]
        return tools.contains(where: trimmed.hasPrefix) ? trimmed : nil
    }
}

/// Doctor rows grouped into Required and Optional, each with its status symbol and word, version or detail, and
/// a fix button when the app can offer one. Notifications can join the optional group (onboarding step 2).
struct DoctorChecklist: View {
    let checks: [DoctorCheck]
    /// Show the "Allow Notifications" row (the notifier is available and not yet authorized).
    var offersNotifications = false
    var onAllowNotifications: (() -> Void)?

    @Environment(AppModel.self) private var model
    @State private var launchError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            group("Required", checks.filter(\.required))
            let optional = checks.filter { !$0.required }
            if !optional.isEmpty || offersNotifications {
                VStack(alignment: .leading, spacing: 0) {
                    groupHeader("Optional")
                    ForEach(Array(optional.enumerated()), id: \.element.id) { index, check in
                        if index > 0 { Divider().padding(.leading, 30) }
                        DoctorCheckRow(check: check, onFix: perform)
                    }
                    if offersNotifications {
                        if !optional.isEmpty { Divider().padding(.leading, 30) }
                        notificationsRow
                    }
                }
            }
            if let launchError {
                Label(launchError, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
        }
    }

    @ViewBuilder private func group(_ title: String, _ checks: [DoctorCheck]) -> some View {
        if !checks.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                groupHeader(title)
                ForEach(Array(checks.enumerated()), id: \.element.id) { index, check in
                    if index > 0 { Divider().padding(.leading, 30) }
                    DoctorCheckRow(check: check, onFix: perform)
                }
            }
        }
    }

    private func groupHeader(_ title: String) -> some View {
        Text(title.uppercased())
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.bottom, 4)
            .accessibilityAddTraits(.isHeader)
    }

    private var notificationsRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: "bell.badge")
                .foregroundStyle(.secondary)
                .frame(width: 20)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text("Notifications")
                Text("Hear when a long setup or teardown finishes while you work elsewhere.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            if let onAllowNotifications {
                Button("Allow Notifications…", action: onAllowNotifications)
            }
        }
        .padding(.vertical, 8)
        .accessibilityElement(children: .contain)
    }

    private func perform(_ fix: DoctorFix) {
        launchError = nil
        switch fix {
        case .openDockerDesktop:
            if let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: DoctorFix.dockerBundleID) {
                NSWorkspace.shared.openApplication(at: app, configuration: NSWorkspace.OpenConfiguration())
            } else {
                launchError = "Docker Desktop isn't installed. Get it from docker.com, then check again."
            }
        case .openURL(let url, _):
            NSWorkspace.shared.open(url)
        case .signInToSandboxes:
            let terminal = model.settings.preferredTerminal
            Task {
                do {
                    try await ProjectActions.runInTerminal("sbx login", terminal: terminal)
                } catch {
                    launchError = (error as? HostLaunchError)?.message ?? error.localizedDescription
                }
            }
        case .copyCommand(let command, _):
            Pasteboard.general.copy(command)
            AccessibilityNotification.Announcement("Copied \(command)").post()
        }
    }
}

/// One doctor check: status symbol, title, version or detail, remediation, fix button.
struct DoctorCheckRow: View {
    let check: DoctorCheck
    var onFix: ((DoctorFix) -> Void)?
    /// Vertical padding; a Form row already has its own.
    var verticalPadding: CGFloat = 7

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: Self.symbol(check.status))
                .foregroundStyle(Self.tint(check.status))
                .frame(width: 20)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(check.title)
                    if let version = check.version, check.status == .ok {
                        Text(version)
                            .font(.caption)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
                if let line = secondaryLine {
                    Text(line)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
            }
            Spacer(minLength: 8)
            if check.status != .ok {
                Text(Self.label(check.status))
                    .font(.caption)
                    .foregroundStyle(Self.tint(check.status))
            }
            if let fix = DoctorFix.fix(for: check), let onFix {
                if case .copyCommand(let command, let label) = fix {
                    CopyButton(text: command, label: label, showsTitle: true)
                } else {
                    Button(fix.title) { onFix(fix) }
                }
            }
        }
        .padding(.vertical, verticalPadding)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(check.title), \(check.required ? "required" : "optional"), \(Self.label(check.status))")
        .accessibilityIdentifier("doctor.\(check.id)")
    }

    private var secondaryLine: String? {
        switch check.status {
        case .ok: return check.path
        case .warn, .error, .skipped:
            // The remediation text only when no button already offers it; a Copy Command button names the
            // command it copies, so it can be read before it is pasted into a terminal.
            let fix = DoctorFix.fix(for: check)
            let extra: String? = if case .copyCommand(let command, _)? = fix { command } else if fix == nil { check.remediation } else { nil }
            return [check.detail, extra].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " — ")
        }
    }

    static func symbol(_ status: DoctorCheck.Status) -> String {
        switch status {
        case .ok: "checkmark.circle.fill"
        case .warn: "exclamationmark.triangle.fill"
        case .error: "xmark.octagon.fill"
        case .skipped: "minus.circle"
        }
    }

    static func tint(_ status: DoctorCheck.Status) -> Color {
        switch status {
        case .ok: .green
        case .warn: .orange
        case .error: .red
        case .skipped: .secondary
        }
    }

    static func label(_ status: DoctorCheck.Status) -> String {
        switch status {
        case .ok: "Ready"
        case .warn: "Needs attention"
        case .error: "Not ready"
        case .skipped: "Not available"
        }
    }
}

extension DoctorReport {
    /// Required checks that fail; onboarding step 2 is done when this is empty.
    var blockingChecks: [DoctorCheck] { checks.filter { $0.required && $0.status == .error } }

    /// Host checks only (no `repo.*`), for onboarding.
    var hostChecks: [DoctorCheck] { checks.filter { !$0.id.hasPrefix("repo.") } }
}

#Preview("Doctor checklist") {
    DoctorChecklist(checks: PreviewSamples.hostChecks, offersNotifications: true, onAllowNotifications: {})
        .environment(ProjectsPreviewModel.model())
        .frame(width: 520)
        .padding()
}
