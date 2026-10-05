import BranchBoxKit
import Foundation
import SwiftUI

// The app's vocabulary for feature state (DESIGN §9): every status has a label and a symbol, so nothing is told
// by colour alone. Tints are semantic; views turn them into colours.

/// A semantic tint. Tests compare these; views use `color`.
enum StatusTint: Sendable, Hashable {
    case green, orange, red, purple, blue, gray

    var color: Color {
        switch self {
        case .green: .green
        case .orange: .orange
        case .red: .red
        case .purple: .purple
        case .blue: .blue
        case .gray: .secondary
        }
    }
}

extension FeatureStatus {
    /// "Active", "Failed (kept)"; an unknown status shows its raw value with underscores as spaces.
    var label: String {
        switch self {
        case .active: "Active"
        case .degraded: "Degraded"
        case .failedRetained: "Failed (kept)"
        case .orphaned: "Orphaned"
        case .removed: "Removed"
        case .unknown(let raw): raw.isEmpty ? "Unknown" : raw.replacingOccurrences(of: "_", with: " ")
        }
    }

    var symbol: String {
        switch self {
        case .active: "circle.fill"
        case .degraded: "exclamationmark.triangle.fill"
        case .failedRetained: "xmark.octagon.fill"
        case .orphaned: "diamond.fill"
        case .removed: "archivebox"
        case .unknown: "questionmark.circle"
        }
    }

    var tint: StatusTint {
        switch self {
        case .active: .green
        case .degraded: .orange
        case .failedRetained: .red
        case .orphaned: .purple
        case .removed, .unknown: .gray
        }
    }

    /// Statuses that count toward attention badges on their own.
    var needsAttention: Bool {
        switch self {
        case .degraded, .failedRetained, .orphaned, .unknown: true
        case .active, .removed: false
        }
    }
}

extension RuntimeProvider {
    var label: String {
        switch self {
        case .container: "Container"
        case .sbx: "Docker Sandbox"
        case .localVM: "Local VM"
        case .inGuest: "In-guest"
        case .unknown(let raw): raw.isEmpty ? "Unknown runtime" : raw.replacingOccurrences(of: "_", with: " ")
        }
    }

    var symbol: String {
        switch self {
        case .container: "shippingbox"
        case .sbx: "lock.shield"
        case .localVM: "cpu"
        case .inGuest: "square.dashed"
        case .unknown: "questionmark.square.dashed"
        }
    }
}

extension TunnelStatus {
    /// The JSON vocabulary in words (§5.1); never the CLI's text-mode "degraded".
    var label: String {
        switch self {
        case .pending: "Starting"
        case .active: "Online"
        case .manual: "Manual setup needed"
        case .disabled: "Off"
        case .unknown(let raw): raw.isEmpty ? "Unknown" : raw.replacingOccurrences(of: "_", with: " ")
        }
    }

    var symbol: String {
        switch self {
        case .pending: "hourglass"
        case .active: "network"
        case .manual: "hand.raised"
        case .disabled: "network.slash"
        case .unknown: "questionmark.circle"
        }
    }

    var tint: StatusTint {
        switch self {
        case .pending: .blue
        case .active: .green
        case .manual: .orange
        case .disabled, .unknown: .gray
        }
    }
}

extension ModuleStatus {
    var label: String {
        switch self {
        case .success: "OK"
        case .skipped: "Skipped"
        case .failed: "Failed"
        case .unknown(let raw): raw.isEmpty ? "Unknown" : raw.replacingOccurrences(of: "_", with: " ")
        }
    }

    var symbol: String {
        switch self {
        case .success: "checkmark.circle.fill"
        case .skipped: "minus.circle"
        case .failed: "xmark.circle.fill"
        case .unknown: "questionmark.circle"
        }
    }

    var tint: StatusTint {
        switch self {
        case .success: .green
        case .skipped, .unknown: .gray
        case .failed: .red
        }
    }
}

extension AttentionReason {
    /// The derived-attention vocabulary: "Setup incomplete", "Folder missing", "Interrupted", "Unregistered worktree".
    var label: String {
        switch self {
        case .degraded: FeatureStatus.degraded.label
        case .failedRetained: FeatureStatus.failedRetained.label
        case .orphaned: FeatureStatus.orphaned.label
        case .interrupted: "Interrupted"
        case .setupIncomplete: "Setup incomplete"
        case .folderMissing: "Folder missing"
        case .worktreeInvalid: "Git worktree broken"
        case .unknownStatus(let raw): FeatureStatus.unknown(raw).label
        case .unregisteredWorktree: "Unregistered worktree"
        }
    }

    var symbol: String {
        switch self {
        case .degraded: FeatureStatus.degraded.symbol
        case .failedRetained: FeatureStatus.failedRetained.symbol
        case .orphaned: FeatureStatus.orphaned.symbol
        case .interrupted: "pause.circle.fill"
        case .setupIncomplete: "exclamationmark.circle.fill"
        case .folderMissing: "folder.badge.questionmark"
        case .worktreeInvalid: "exclamationmark.triangle.fill"
        case .unknownStatus: "questionmark.circle"
        case .unregisteredWorktree: "questionmark.folder"
        }
    }

    var tint: StatusTint {
        switch self {
        case .degraded, .setupIncomplete, .interrupted, .unregisteredWorktree: .orange
        case .failedRetained, .folderMissing, .worktreeInvalid: .red
        case .orphaned: .purple
        case .unknownStatus: .gray
        }
    }
}

/// Text helpers for feature rows, headers and cards. Every formatter is a value `FormatStyle` built per call.
enum FeaturePresentation {
    /// "3 ok · 1 skipped · 0 failed", plus "· n other" for statuses this app version does not know.
    static func moduleSummary(_ outcomes: [ModuleOutcome]) -> String {
        guard !outcomes.isEmpty else { return "No modules recorded" }
        var ok = 0, skipped = 0, failed = 0, other = 0
        for outcome in outcomes {
            switch outcome.status {
            case .success: ok += 1
            case .skipped: skipped += 1
            case .failed: failed += 1
            case .unknown: other += 1
            }
        }
        let summary = "\(ok) ok · \(skipped) skipped · \(failed) failed"
        return other == 0 ? summary : summary + " · \(other) other"
    }

    /// "2 hours ago", relative to now.
    static func relative(_ date: Date, locale: Locale = .autoupdatingCurrent) -> String {
        date.formatted(Date.RelativeFormatStyle(presentation: .named, unitsStyle: .wide, locale: locale))
    }

    /// "20 min. ago": the short form for table cells.
    static func shortRelative(_ date: Date, locale: Locale = .autoupdatingCurrent) -> String {
        date.formatted(Date.RelativeFormatStyle(presentation: .named, unitsStyle: .abbreviated, locale: locale))
    }

    /// Started with `--minimal` (shown as "Quick").
    static func isQuick(_ record: FeatureRecord) -> Bool {
        record.startMode == StartFeatureRequest.Mode.minimal.rawValue
    }

    /// The branch is just `<project prefix>/<name>`, so repeating it next to the name adds nothing.
    static func hasConventionalBranch(_ record: FeatureRecord, projectPrefix: String?) -> Bool {
        record.branchName.isEmpty || record.branchName == NameRules.branchName(prefix: projectPrefix, slug: record.workFeature)
    }

    /// "Mar 17, 2026 at 3:37 AM".
    static func absolute(_ date: Date, locale: Locale = .autoupdatingCurrent) -> String {
        date.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened, locale: locale))
    }

    /// A module's recorded duration: "0 ms", "190 ms", "1.2 s", "2 min 5 s".
    static func duration(milliseconds: Int) -> String {
        if milliseconds < 1000 { return "\(max(milliseconds, 0)) ms" }
        if milliseconds < 60_000 {
            let seconds = Double(milliseconds) / 1000
            return seconds.formatted(.number.precision(.fractionLength(0...1)).locale(Locale(identifier: "en_US"))) + " s"
        }
        let totalSeconds = milliseconds / 1000
        return "\(totalSeconds / 60) min \(totalSeconds % 60) s"
    }

    /// "feature/oauth from main · created 2 days ago"; "from current HEAD" when no base was recorded.
    static func subtitle(for record: FeatureRecord, locale: Locale = .autoupdatingCurrent) -> String {
        var parts: [String] = []
        if !record.branchName.isEmpty {
            parts.append("\(record.branchName) from \(record.baseBranch ?? "current HEAD")")
        }
        if let created = record.createdAt { parts.append("created \(relative(created, locale: locale))") }
        return parts.joined(separator: " · ")
    }

    /// One combined label for a feature row: "oauth, Active, Container, needs attention: Folder missing".
    static func accessibilityLabel(for record: FeatureRecord, attention: AttentionReason?) -> String {
        var parts = [record.workFeature, record.status.label, record.runtime.provider.label]
        if record.startMode == StartFeatureRequest.Mode.minimal.rawValue { parts.append("Quick") }
        if let attention, attention.label != record.status.label { parts.append("needs attention: \(attention.label)") }
        return parts.joined(separator: ", ")
    }

    /// A short commit for display: the first 7 characters of a full SHA.
    static func shortCommit(_ commit: String?) -> String? {
        guard let commit, !commit.isEmpty else { return nil }
        return String(commit.prefix(7))
    }
}
