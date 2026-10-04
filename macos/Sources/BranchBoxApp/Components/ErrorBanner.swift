import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// An inline error with its cause and [Retry] [Details] [Copy], e.g. a failed refresh above a list that keeps
/// showing its last good data (marked stale).
///
/// [Retry] appears only for transient errors (`BackendError.isTransient`): retrying a refusal is refused again.
/// A refusal or partial failure instead offers its `RecoveryPlanner` recoveries through `onRecovery`, with the
/// destructive ones confirmed first, and a [Show Changes] toggle for the files it names.
struct ErrorBanner: View {
    let error: BackendError
    var context: OperationRequestContext?
    /// The view below still shows data from before the failure.
    var isStale = false
    var onRetry: (() -> Void)?
    var onDetails: (() -> Void)?
    /// Performs a recovery; without it no recoveries are offered (additive, SW-4).
    var onRecovery: ((RecoveryAction) -> Void)?
    @State private var showsDetails = false
    @State private var pendingConfirmation: RecoveryAction?

    var body: some View {
        let presentation = error.presentation(context: context)
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: presentation.symbol)
                    .foregroundStyle(presentation.tint.color)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(presentation.title).font(.headline)
                    Text(message(presentation))
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                    if isStale {
                        Label("Showing the last loaded data", systemImage: "clock.arrow.circlepath")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Spacer(minLength: 8)
                trailingButtons(presentation)
            }
            if case .cliNotFound(let searched) = error, !searched.isEmpty {
                SearchedPathsList(paths: searched)
                    .padding(.leading, 26)
            }
            if showsDetails, showsChangesToggle(presentation) {
                detailList(presentation.details)
                    .padding(.leading, 26)
            }
            if !recoveries.isEmpty {
                FlowLayout(spacing: 8) {
                    ForEach(recoveries) { recoveryButton($0) }
                }
                .padding(.leading, 26)
            }
        }
        .padding(10)
        .background(presentation.tint.color.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .contain)
        .confirmationDialog(pendingConfirmation?.confirmationTitle ?? "", isPresented: confirmationShown,
                            titleVisibility: .visible, presenting: pendingConfirmation) { action in
            Button(action.confirmationTitle, role: .destructive) { onRecovery?(action) }
            Button("Cancel", role: .cancel) {}
        } message: { action in
            Text(action.confirmationMessage ?? "")
        }
    }

    @ViewBuilder private func trailingButtons(_ presentation: ErrorPresentation) -> some View {
        HStack(spacing: 6) {
            if showsChangesToggle(presentation) {
                Button(showsDetails ? "Hide Changes" : "Show Changes") { showsDetails.toggle() }
            }
            if let onRetry, error.isTransient {
                Button("Retry", action: onRetry)
            }
            if let onDetails {
                Button("Details", action: onDetails)
            }
            CopyButton(text: Self.copyText(presentation), label: "Copy Error")
                .buttonStyle(.borderless)
        }
        .fixedSize()
    }

    private func detailList(_ lines: [String]) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                Text(line)
                    .font(.system(.caption, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(8)
        .background(.background.opacity(0.6), in: RoundedRectangle(cornerRadius: 6))
    }

    @ViewBuilder private func recoveryButton(_ action: RecoveryAction) -> some View {
        if case .copyCommand(let command, let label) = action {
            CopyButton(text: command, label: label, showsTitle: true)
        } else if action.isDestructive {
            Button(role: .destructive) {
                pendingConfirmation = action
            } label: {
                Text(action.title).foregroundStyle(.red)
            }
        } else {
            Button(action.title) { onRecovery?(action) }
        }
    }

    /// The planner's recoveries for refusals and partial failures; a transient error's recovery is [Retry].
    private var recoveries: [RecoveryAction] {
        guard onRecovery != nil, !error.isTransient else { return [] }
        if case .cliNotFound = error { return [] }                // EnvironmentGate owns those actions
        return RecoveryPlanner.recoveries(for: error, after: context)
    }

    /// File lists (a refusal's changes) collapse behind [Show Changes]; other details go to [Details].
    private func showsChangesToggle(_ presentation: ErrorPresentation) -> Bool {
        guard !presentation.details.isEmpty else { return false }
        switch presentation.style {
        case .refusal, .partial: return true
        case .blocking, .failure, .unsupported, .neutral: return false
        }
    }

    /// The paths of a missing CLI are listed below instead of inside the sentence, where they wrapped mid-path.
    private func message(_ presentation: ErrorPresentation) -> String {
        if case .cliNotFound = error { return "Install the BranchBox CLI with Homebrew, or locate it." }
        return presentation.message
    }

    private var confirmationShown: Binding<Bool> {
        Binding(get: { pendingConfirmation != nil }, set: { if !$0 { pendingConfirmation = nil } })
    }

    /// Title, message and details as plain text.
    nonisolated static func copyText(_ presentation: ErrorPresentation) -> String {
        ([presentation.title, presentation.message] + presentation.details).filter { !$0.isEmpty }.joined(separator: "\n")
    }
}

#Preview("Banners") {
    VStack(spacing: 12) {
        ErrorBanner(error: .registryCorrupted(path: "\(PreviewSamples.project.path)/.branchbox/registry.json",
                                              diagnostics: Diagnostics(summary: "expected value at line 1 column 1")),
                    isStale: true, onRetry: {}, onDetails: {})
        ErrorBanner(error: .commandFailed(Diagnostics(summary: "Error: Docker is not available", causes: ["connect: no such file"])),
                    onRetry: {})
        ErrorBanner(error: .unsupported(.config, minimumCLI: "0.14.0"))
    }
    .frame(width: 560)
    .padding()
}
