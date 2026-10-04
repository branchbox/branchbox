import BranchBoxKit
import BranchBoxStores
import SwiftUI

// The sidebar's rows (DESIGN §9 Sidebar). They take values, not stores, so they render the same inside the
// sidebar `List` and in plain stacks (previews, snapshot tests). Every row is one accessibility element with a
// combined label; status is told by glyph shape and word as well as colour.

/// A project: its name, attention badge, refresh spinner and stale or missing-folder marker.
struct ProjectRow: View {
    let name: String
    var path: String?
    var attentionCount = 0
    var isRefreshing = false
    /// The last refresh failed; the rows below are from an earlier one.
    var isStale = false
    var rootExists = true
    var isPinned = false

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: rootExists ? "folder.fill" : "folder.badge.questionmark")
                .foregroundStyle(rootExists ? Color.accentColor : .secondary)
                .accessibilityHidden(true)
            Text(name)
                .fontWeight(.semibold)
                .lineLimit(1)
                .truncationMode(.middle)
                .foregroundStyle(rootExists ? .primary : .secondary)
            if isPinned {
                Image(systemName: "pin.fill")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
            Spacer(minLength: 4)
            if isRefreshing {
                ProgressView()
                    .controlSize(.mini)
                    .accessibilityHidden(true)
            } else if isStale {
                Image(systemName: "clock.badge.exclamationmark")
                    .foregroundStyle(.orange)
                    .help("Couldn't refresh; showing earlier data")
                    .accessibilityHidden(true)
            }
            if !rootExists {
                Text("Missing")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            AttentionBadge(count: attentionCount)
        }
        .help(path ?? name)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(accessibilityText)
    }

    var accessibilityText: String {
        var parts = ["Project \(name)"]
        if !rootExists { parts.append("folder missing") }
        if attentionCount > 0 { parts.append(AttentionBadge.accessibilityText(count: attentionCount, noun: "item")) }
        if isRefreshing { parts.append("refreshing") } else if isStale { parts.append("showing earlier data") }
        return parts.joined(separator: ", ")
    }
}

/// A feature: colour tag, name, Quick capsule, operation spinner and runtime glyph over its status (or what
/// needs attention) and branch. The branch shows only when it isn't `<prefix>/<name>`, and the runtime glyph
/// only when it differs from the project's default, so the status stays the loudest signal.
struct FeatureRow: View {
    /// The leading column every row kind shares, so names line up.
    static let leadingWidth: CGFloat = 14

    let record: FeatureRecord
    var attention: AttentionReason?
    /// The title of the operation running on it ("Tearing down oauth"), if any.
    var runningOperation: String?
    /// The project's branch prefix; nil when its config hasn't loaded (the CLI's default, "feature", applies).
    var projectPrefix: String?
    /// The project's default runtime; nil while its config loads (the CLI's default, Container, applies).
    var projectRuntime: RuntimeProvider?

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            // The feature's colour as a slim tag, so it never reads as another status dot.
            Capsule()
                .fill(ColorSwatch.color(record.color).opacity(0.6))
                .frame(width: 4, height: 26)
                .frame(width: Self.leadingWidth)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(record.workFeature)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .foregroundStyle(record.status == .removed ? .secondary : .primary)
                    if isQuick {
                        QuickCapsule()
                    }
                    Spacer(minLength: 2)
                    if runningOperation != nil {
                        ProgressView()
                            .controlSize(.mini)
                    }
                    if showsRuntime {
                        RuntimeBadge(provider: record.runtime.provider, style: .glyph)
                    }
                }
                statusLine
            }
        }
        .padding(.vertical, 2)
        .help(runningOperation ?? record.worktreePath ?? record.workFeature)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(accessibilityText)
    }

    private var statusLine: some View {
        HStack(spacing: 4) {
            Image(systemName: symbol)
                .font(.system(size: 8, weight: .bold))
                .foregroundStyle(tint)
            Text(statusText)
                .foregroundStyle(attention == nil || runningOperation != nil ? Color.secondary : tint)
                .lineLimit(1)
            if showsBranch, runningOperation == nil {
                Text("· \(record.branchName)")
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
        .font(.caption)
    }

    private var isQuick: Bool { FeaturePresentation.isQuick(record) }

    var showsBranch: Bool { !FeaturePresentation.hasConventionalBranch(record, projectPrefix: projectPrefix) }

    var showsRuntime: Bool { (projectRuntime ?? ProjectConfig.defaults.runtimeProvider) != record.runtime.provider }

    private var statusText: String {
        if let runningOperation {
            // "Tearing down oauth" under the name oauth reads as "Tearing down…".
            // Keep the status word next to its glyph: "Degraded · Tearing down…".
            let suffix = " " + record.workFeature
            let running = runningOperation.hasSuffix(suffix) ? String(runningOperation.dropLast(suffix.count)) + "…"
                : runningOperation
            return "\(attention?.label ?? record.status.label) · \(running)"
        }
        return attention?.label ?? record.status.label
    }

    private var symbol: String { attention?.symbol ?? record.status.symbol }
    private var tint: Color { (attention?.tint ?? record.status.tint).color }

    var accessibilityText: String {
        var label = FeaturePresentation.accessibilityLabel(for: record, attention: attention)
        if let runningOperation { label += ", \(runningOperation)" }
        return label
    }
}

/// A worktree in the project's layout that the registry doesn't know.
struct StrayRow: View {
    let stray: StrayWorktree

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: AttentionReason.unregisteredWorktree.symbol)
                .font(.callout)
                .foregroundStyle(AttentionReason.unregisteredWorktree.tint.color)
                .frame(width: FeatureRow.leadingWidth)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
        .padding(.vertical, 2)
        .help(stray.path)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(name), Unregistered worktree\(stray.branch.map { ", branch \($0)" } ?? "")")
    }

    private var name: String { URL(fileURLWithPath: stray.path).lastPathComponent }

    private var subtitle: String {
        var parts = ["Unregistered worktree"]
        if stray.locked { parts.append("locked") }
        if stray.prunable { parts.append("prunable") }
        return parts.joined(separator: " · ")
    }
}

/// A feature being started that the registry doesn't list yet.
struct ProvisionalFeatureRow: View {
    let name: String
    var state = "starting…"

    var body: some View {
        HStack(spacing: 8) {
            ProgressView()
                .controlSize(.mini)
                .frame(width: FeatureRow.leadingWidth)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundStyle(.secondary)
                Text(state)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(name): \(state)")
    }
}

/// A quiet one-line state inside a project: loading, empty, or a failed refresh with [Retry].
struct SidebarStateRow: View {
    enum Kind: Equatable {
        case loading
        case empty
        case failed(String)
    }

    let kind: Kind
    var onAction: (() -> Void)?

    var body: some View {
        HStack(spacing: 6) {
            switch kind {
            case .loading:
                ProgressView().controlSize(.mini)
                Text("Loading features…").foregroundStyle(.secondary)
            case .empty:
                Text("No features yet").foregroundStyle(.secondary)
                Spacer(minLength: 4)
                if let onAction {
                    Button("Start…", action: onAction)
                        .buttonStyle(.link)
                        .help("Start a feature in this project")
                }
            case .failed(let summary):
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .accessibilityHidden(true)
                Text("Couldn't refresh")
                    .foregroundStyle(.secondary)
                    .help(summary)
                Spacer(minLength: 4)
                if let onAction {
                    Button("Retry", action: onAction)
                        .buttonStyle(.link)
                }
            }
        }
        .font(.callout)
        .padding(.vertical, 1)
        .accessibilityElement(children: .combine)
    }
}

/// The footer of a project's rows: shows or hides removed features.
struct ShowRemovedRow: View {
    let includeRemoved: Bool
    /// How many removed features are listed (known only while they are shown).
    var removedCount = 0
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            Label(title, systemImage: includeRemoved ? "eye.slash" : "archivebox")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .buttonStyle(.plain)
        .help(includeRemoved ? "Hide torn-down features" : "List torn-down features too")
    }

    private var title: String {
        includeRemoved ? "Hide removed (\(removedCount))" : "Show removed"
    }
}
