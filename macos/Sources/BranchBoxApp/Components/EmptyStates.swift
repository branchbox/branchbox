import BranchBoxKit
import BranchBoxPreview
import SwiftUI

// Empty and blocking states, each with one primary action (DESIGN §9 state catalogue).
//
// They share `EmptyStateLayout` rather than `ContentUnavailableView`: on macOS the latter stretches its action
// buttons to equal widths (three of them overflow a 620 pt pane) and greys its title in some hosts. Here the
// title stays in the primary label colour, buttons keep their natural width, and a row of actions that does not
// fit stacks vertically.

/// The shared layout: a symbol, a title, a description and an action row centred in the available space.
struct EmptyStateLayout<Description: View, Actions: View>: View {
    let title: String
    let systemImage: String
    var tint: Color = .secondary
    @ViewBuilder var description: Description
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: systemImage)
                .font(.system(size: 44, weight: .light))
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(tint)
                .accessibilityHidden(true)
            VStack(spacing: 8) {
                Text(title)
                    .font(.title2.weight(.semibold))
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.center)
                    .accessibilityAddTraits(.isHeader)
                description
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 8) { actions }
                    .fixedSize()
                VStack(spacing: 8) { actions }
                    .fixedSize()
            }
            .padding(.top, 4)
        }
        .frame(maxWidth: 460)
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }
}

/// Paths the CLI locator tried, one per line: monospaced, selectable, and truncated in the middle so a path never
/// wraps at a slash.
struct SearchedPathsList: View {
    let paths: [String]
    var title = "BranchBox looked in:"

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            ForEach(Array(paths.enumerated()), id: \.offset) { _, path in
                Text(path)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .help(path)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 6))
        .multilineTextAlignment(.leading)
        .accessibilityElement(children: .combine)
    }
}

/// A command to type in Terminal, shown monospaced and selectable on a subtle background.
struct CommandLineText: View {
    let command: String

    var body: some View {
        Text(command)
            .font(.system(.callout, design: .monospaced))
            .foregroundStyle(.primary)
            .textSelection(.enabled)
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 6))
    }
}

/// No usable CLI was found: where the app looked, [Locate…], the Homebrew command and [Re-detect].
struct CLINotFoundView: View {
    let searched: [String]
    var onLocate: () -> Void
    var onRedetect: () -> Void

    static let installCommand = "brew install branchbox/tap/branchbox"

    var body: some View {
        EmptyStateLayout(title: "BranchBox CLI not found", systemImage: "terminal") {
            VStack(spacing: 12) {
                Text("BranchBox for Mac runs the branchbox command-line tool. Install it with Homebrew, or locate it.")
                CommandLineText(command: Self.installCommand)
                if !searched.isEmpty {
                    SearchedPathsList(paths: searched)
                }
            }
        } actions: {
            Button("Locate…", action: onLocate)
                .buttonStyle(.borderedProminent)
            CopyButton(text: Self.installCommand, label: "Copy Install Command", showsTitle: true)
            Button("Re-detect", action: onRedetect)
        }
    }
}

/// The CLI is older than the minimum: its version and path, the upgrade command, [Locate…] and [Re-detect].
struct CLITooOldView: View {
    let found: SemVer
    let minimum: SemVer
    let path: String
    var onLocate: () -> Void
    var onRedetect: () -> Void

    static let upgradeCommand = "brew upgrade branchbox"

    var body: some View {
        EmptyStateLayout(title: "BranchBox CLI is too old", systemImage: "arrow.up.circle") {
            VStack(spacing: 12) {
                Text("This CLI is version \(found.description). BranchBox for Mac needs \(minimum.description) or later.")
                CommandLineText(command: Self.upgradeCommand)
                SearchedPathsList(paths: [path], title: "Found at:")
            }
        } actions: {
            CopyButton(text: Self.upgradeCommand, label: "Copy Upgrade Command", showsTitle: true)
                .buttonStyle(.borderedProminent)
            Button("Locate…", action: onLocate)
            Button("Re-detect", action: onRedetect)
        }
    }
}

/// The CLI was found but can't be used (it crashed, printed nothing usable, or isn't executable).
struct CLIUnusableView: View {
    let path: String
    let reason: String
    var onLocate: () -> Void
    var onRedetect: () -> Void
    var onDiagnostics: (() -> Void)?

    var body: some View {
        EmptyStateLayout(title: "BranchBox CLI can't be used", systemImage: "exclamationmark.triangle", tint: .orange) {
            VStack(spacing: 12) {
                Text(reason)
                    .textSelection(.enabled)
                if !path.isEmpty {
                    SearchedPathsList(paths: [path], title: "Found at:")
                }
            }
        } actions: {
            Button("Re-detect", action: onRedetect)
                .buttonStyle(.borderedProminent)
            Button("Locate…", action: onLocate)
            if let onDiagnostics {
                Button("Open Diagnostics", action: onDiagnostics)
            }
        }
    }
}

/// A project's folder is gone: [Locate…] it or [Remove] the project (never deletes files).
struct ProjectMissingView: View {
    let path: String
    var onLocate: () -> Void
    var onRemove: () -> Void

    var body: some View {
        EmptyStateLayout(title: "Project folder is missing", systemImage: "folder.badge.questionmark") {
            VStack(spacing: 12) {
                Text("If you moved it, locate it. Removing the project only takes it out of this list.")
                SearchedPathsList(paths: [path], title: "Expected at:")
            }
        } actions: {
            Button("Locate…", action: onLocate)
                .buttonStyle(.borderedProminent)
            Button("Remove from List", action: onRemove)
        }
    }
}

/// A project without features: [Start Feature…].
struct NoFeaturesView: View {
    var projectName: String?
    var onStart: () -> Void

    var body: some View {
        EmptyStateLayout(title: "No features yet", systemImage: "shippingbox") {
            Text(projectName.map { "Start a feature to get a worktree and environment of its own in \($0)." }
                 ?? "Start a feature to get a worktree and environment of its own.")
        } actions: {
            Button("Start Feature…", action: onStart)
                .buttonStyle(.borderedProminent)
        }
    }
}

/// The selected feature no longer exists (torn down elsewhere, or the registry changed).
struct FeatureGoneView: View {
    let name: String
    var onShowProject: (() -> Void)?
    /// Lists removed features too, so a torn-down feature's record can be read (additive, SW-4).
    var onShowRemoved: (() -> Void)?

    var body: some View {
        EmptyStateLayout(title: "This feature no longer exists", systemImage: "questionmark.folder") {
            Text("\(name) isn't in the project's registry anymore. It may have been torn down, here or from the command line.")
        } actions: {
            if let onShowProject {
                Button("Show Project", action: onShowProject)
                    .buttonStyle(.borderedProminent)
            }
            if let onShowRemoved {
                Button("Show Removed Features", action: onShowRemoved)
            }
        }
    }
}

#Preview("CLI not found") {
    CLINotFoundView(searched: PreviewSamples.searchedPaths, onLocate: {}, onRedetect: {})
        .frame(width: 600, height: 400)
}

#Preview("CLI too old") {
    CLITooOldView(found: SemVer(0, 13, 3), minimum: BackendIdentity.minimumCLI, path: "/opt/homebrew/bin/branchbox",
                  onLocate: {}, onRedetect: {})
        .frame(width: 600, height: 400)
}

#Preview("Project missing") {
    ProjectMissingView(path: PreviewSamples.project.path, onLocate: {}, onRemove: {})
        .frame(width: 600, height: 400)
}

#Preview("No features") {
    NoFeaturesView(projectName: PreviewSamples.project.displayName, onStart: {})
        .frame(width: 600, height: 400)
}

#Preview("Feature gone") {
    FeatureGoneView(name: PreviewSamples.features[2].workFeature, onShowProject: {}, onShowRemoved: {})
        .frame(width: 600, height: 400)
}
