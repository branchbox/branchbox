import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Stands in front of the detail column (DESIGN §9 EnvironmentGate): when the CLI is missing, too old or unusable
/// it shows the blocking state with its fixes instead of the content. While the CLI is being located the content
/// still shows when there are projects (they load before the bootstrap). A legacy 0.13.x CLI gets a dismissible
/// banner above the content.
struct EnvironmentGate<Content: View>: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    /// The version whose legacy banner was dismissed in this window.
    @SceneStorage("legacyBannerDismissed") private var dismissedLegacyVersion = ""
    @ViewBuilder var content: Content

    var body: some View {
        switch model.environment.backendState {
        case .unavailable(let error):
            BlockingEnvironmentView(error: error, isRechecking: model.environment.isBootstrapping,
                                    onLocate: { FeatureCommands.locateCLI(model: model) },
                                    onRedetect: redetect,
                                    onDiagnostics: { openWindow(id: SceneID.diagnostics) })
        case .resolving where model.projects.projects.isEmpty:
            VStack(spacing: 12) {
                ProgressView()
                Text("Locating the BranchBox CLI…")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .resolving:
            content
        case .ready(let identity):
            VStack(spacing: 0) {
                if identity.isLegacy, dismissedLegacyVersion != identity.version.description {
                    LegacyCLIBanner(version: identity.version) { dismissedLegacyVersion = identity.version.description }
                    Divider()
                }
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    private func redetect() {
        let environment = model.environment
        Task { await environment.rebootstrap() }
    }
}

/// The blocking variant for an unavailable backend.
struct BlockingEnvironmentView: View {
    let error: BackendError
    var isRechecking = false
    var onLocate: () -> Void
    var onRedetect: () -> Void
    var onDiagnostics: () -> Void

    var body: some View {
        Group {
            switch error {
            case .cliNotFound(let searched):
                CLINotFoundView(searched: searched, onLocate: onLocate, onRedetect: onRedetect)
            case .cliTooOld(let found, let minimum, let path):
                CLITooOldView(found: found, minimum: minimum, path: path, onLocate: onLocate, onRedetect: onRedetect)
            case .cliUnusable(let path, let reason):
                CLIUnusableView(path: path, reason: reason, onLocate: onLocate, onRedetect: onRedetect,
                                onDiagnostics: onDiagnostics)
            default:
                let presentation = error.presentation()
                CLIUnusableView(path: "", reason: "\(presentation.title). \(presentation.message)", onLocate: onLocate,
                                onRedetect: onRedetect, onDiagnostics: onDiagnostics)
            }
        }
        .overlay(alignment: .bottom) {
            if isRechecking {
                HStack(spacing: 6) {
                    ProgressView().controlSize(.small)
                    Text("Checking again…").foregroundStyle(.secondary)
                }
                .padding(.bottom, 24)
            }
        }
    }
}

/// "BranchBox CLI 0.13.4: some features need a newer CLI (…)", with the upgrade command and a close button.
struct LegacyCLIBanner: View {
    let version: SemVer
    var onDismiss: () -> Void

    static let upgradeCommand = "brew upgrade branchbox"

    static func text(version: SemVer) -> String {
        "BranchBox CLI \(version.description): some features need a newer CLI (config editing, safe teardown in core, "
            + "interrupted-start tracking, …)."
    }

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: "info.circle.fill")
                .foregroundStyle(.blue)
                .accessibilityHidden(true)
            // Let the split view constrain the banner; an ideal-height override can expand its native host offscreen.
            Text(Self.text(version: version))
                .font(.callout)
            Spacer(minLength: 8)
            CopyButton(text: Self.upgradeCommand, label: "Copy Upgrade Command", showsTitle: true)
                .controlSize(.small)
            Button(action: onDismiss) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.borderless)
            .help("Dismiss")
            .accessibilityLabel("Dismiss")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Color.blue.opacity(0.08))
        .accessibilityElement(children: .contain)
    }
}
