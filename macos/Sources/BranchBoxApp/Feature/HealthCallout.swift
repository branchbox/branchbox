import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The health remediation callout (§9.1): what happened, in one line, what the fix does, and the actions that
/// fix it — exactly `Remediation.actions`, the first fix prominent. While an operation runs on the feature the
/// buttons wait for it; a setup still running shows its elapsed time instead.
struct HealthCallout: View {
    let record: FeatureRecord
    let folderExists: Bool
    let items: [RemediationItem]
    /// Disabled while the feature has a running operation or the CLI is unavailable.
    let availability: ActionAvailability
    let onPerform: (RemediationEffect) -> Void

    @State private var pendingConfirmation: RemediationItem?

    /// False when the record has nothing to explain. A removed feature gets the "torn down" banner.
    static func isShown(for record: FeatureRecord, folderExists: Bool) -> Bool {
        Remediation.callout(for: record, folderExists: folderExists) != nil
    }

    var body: some View {
        let attention = Remediation.attention(for: record, folderExists: folderExists)
        let style = Self.style(for: record, attention: attention)
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: style.symbol)
                .font(.title2)
                .foregroundStyle(style.tint.color)
                .frame(width: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                Text(Remediation.callout(for: record, folderExists: folderExists) ?? "")
                    .font(.headline)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                if record.setup?.state == .inProgress {
                    settingUpLine
                }
                if let explanation = RemediationPresenter.explanation(for: record, folderExists: folderExists) {
                    Text(explanation)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !items.isEmpty {
                    buttons
                        .padding(.top, 6)
                }
                if let reason = availability.disabledReason, showsWaitingReason {
                    Label(reason, systemImage: "hourglass")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(style.tint.color.opacity(0.09), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(style.tint.color.opacity(0.35)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("feature.health")
        .confirmationDialog(confirmationTitle, isPresented: confirmationShown, titleVisibility: .visible,
                            presenting: pendingConfirmation) { item in
            if case .confirmThenDispatch(let request, _, _, let label) = item.effect {
                Button(label, role: .destructive) { onPerform(.dispatch(request)) }
            }
            Button("Cancel", role: .cancel) {}
        } message: { item in
            if case .confirmThenDispatch(_, _, let message, _) = item.effect { Text(message) }
        }
    }

    /// Why the fixes can't run now; also shown when the missing CLI left no fix to offer at all.
    private var showsWaitingReason: Bool {
        if items.contains(where: \.needsBackend) { return true }
        return items.isEmpty && record.status != .removed && record.setup?.state != .inProgress
            && Remediation.attention(for: record, folderExists: folderExists) != nil
    }

    private var buttons: some View {
        FlowLayout(spacing: 8) {
            ForEach(items) { item in
                button(for: item)
            }
        }
    }

    @ViewBuilder private func button(for item: RemediationItem) -> some View {
        let disabled = item.needsBackend && !availability.isEnabled
        let identifier = "feature.action.\(Self.verb(item.title))"
        if case .copy(let text) = item.effect {
            CopyButton(text: text, label: item.title, showsTitle: true)
                .help("Copies: \(text)")
                .accessibilityIdentifier(identifier)
        } else if item.role == .primary {
            Button(item.title) { perform(item) }
                .buttonStyle(.borderedProminent)
                .disabled(disabled)
                .help(availability.disabledReason ?? item.title)
                .accessibilityIdentifier(identifier)
        } else {
            Button(item.title, role: item.isTeardown ? .destructive : nil) { perform(item) }
                .disabled(disabled)
                .help(disabled ? (availability.disabledReason ?? "") : item.title)
                .accessibilityIdentifier(identifier)
        }
    }

    private func perform(_ item: RemediationItem) {
        if case .confirmThenDispatch = item.effect {
            pendingConfirmation = item
        } else {
            onPerform(item.effect)
        }
    }

    private var settingUpLine: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                if let started = record.setup?.startedAt {
                    Text("Running for \(OperationPresentation.elapsed(.seconds(max(0, context.date.timeIntervalSince(started)))))")
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var confirmationTitle: String {
        if case .confirmThenDispatch(_, let title, _, _)? = pendingConfirmation?.effect { return title }
        return ""
    }

    private var confirmationShown: Binding<Bool> {
        Binding(get: { pendingConfirmation != nil }, set: { if !$0 { pendingConfirmation = nil } })
    }

    // MARK: Style

    struct Style: Equatable {
        let symbol: String
        let tint: StatusTint
    }

    static func style(for record: FeatureRecord, attention: AttentionReason?) -> Style {
        if record.status == .removed { return Style(symbol: "archivebox.fill", tint: .gray) }
        if record.setup?.state == .inProgress { return Style(symbol: "gearshape.2.fill", tint: .blue) }
        if let attention { return Style(symbol: attention.symbol, tint: attention.tint) }
        return Style(symbol: "info.circle.fill", tint: .blue)
    }

    /// "Resume Setup" → "resumeSetup", for accessibility identifiers.
    static func verb(_ title: String) -> String {
        let words = title.replacingOccurrences(of: "…", with: "").split { !$0.isLetter && !$0.isNumber }
        guard let first = words.first else { return "remediation" }
        return first.lowercased() + words.dropFirst().map { $0.prefix(1).uppercased() + $0.dropFirst().lowercased() }.joined()
    }
}

#Preview("Interrupted setup") {
    let record = PreviewSamples.interruptedFeature
    let actions = Remediation.actions(for: record, project: PreviewSamples.project,
                                       identity: try? PreviewScenario.contract.identity.get(), folderExists: true)
    HealthCallout(record: record, folderExists: true, items: RemediationPresenter.items(for: actions, record: record),
                  availability: .enabled, onPerform: { _ in })
        .padding()
        .frame(width: 640)
}
