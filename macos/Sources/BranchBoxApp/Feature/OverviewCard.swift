import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// Branch, base, folder (with a "Missing" state) and recorded commit; dates, compose project and env file behind
/// a "Dates and files" disclosure.
///
/// The commit is the registry's short SHA; this card does not read the worktree's current Git HEAD.
struct OverviewCard: View {
    let record: FeatureRecord
    let folderExists: Bool

    @State private var showsMore = false

    var body: some View {
        FeatureCard("Overview", systemImage: "info.circle") {
            VStack(alignment: .leading, spacing: 10) {
                FactGrid {
                    if !record.branchName.isEmpty {
                        FactRow(label: "Branch", value: record.branchName, monospaced: true) {
                            CopyButton(text: record.branchName, label: "Copy Branch").buttonStyle(.borderless)
                        }
                    }
                    FactRow(label: "Based on", value: record.baseBranch ?? "current HEAD",
                            valueStyle: record.baseBranch == nil ? .secondary : .primary)
                    if let path = record.worktreePath, !path.isEmpty {
                        folderRow(path)
                    }
                    if let commit = FeaturePresentation.shortCommit(record.lastCommit), let full = record.lastCommit {
                        FactRow(label: "Recorded commit", value: commit, monospaced: true) {
                            CopyButton(text: full, label: "Copy Recorded Commit SHA").buttonStyle(.borderless)
                        }
                        .help("Commit SHA recorded in the feature registry; current Git HEAD is not probed here.")
                    }
                    if let removed = record.removedAt {
                        dateRow("Torn down", removed)
                    }
                }
                if hasDetails {
                    // Dates, the Compose project and the env file matter less often; they sit behind a disclosure.
                    DisclosureGroup(isExpanded: $showsMore) {
                        FactGrid { details }
                            .padding(.top, 6)
                    } label: {
                        Text("Dates and files")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    .accessibilityIdentifier("feature.overview.more")
                }
            }
        }
    }

    private var hasDetails: Bool {
        record.createdAt != nil || record.updatedAt != nil || !(record.composeProjectName ?? "").isEmpty
            || !(record.envPath ?? "").isEmpty
    }

    @ViewBuilder private var details: some View {
        if let created = record.createdAt {
            dateRow("Created", created)
        }
        if let updated = record.updatedAt, updated != record.createdAt {
            dateRow("Updated", updated)
        }
        if let compose = record.composeProjectName, !compose.isEmpty {
            FactRow(label: "Compose project", value: compose, monospaced: true)
        }
        if let env = record.envPath, !env.isEmpty {
            FactRow(label: "Env file", value: Self.relative(env, to: record.worktreePath), monospaced: true) {
                CopyButton(text: env, label: "Copy Env File Path").buttonStyle(.borderless)
            }
        }
    }

    private func folderRow(_ path: String) -> some View {
        GridRow {
            Text("Folder")
                .foregroundStyle(.secondary)
                .fixedSize()
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(Self.abbreviated(path))
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(folderExists || record.status == .removed ? .primary : .secondary)
                    .strikethrough(!folderExists && record.status != .removed)
                    .lineLimit(2)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .help(path)
                if !folderExists, record.status != .removed {
                    Tag(title: "Missing", systemImage: "folder.badge.questionmark", tint: .red)
                }
                Spacer(minLength: 4)
                if folderExists, record.status != .removed {
                    Button {
                        HostLaunchFeedback.shared.reveal(path)
                    } label: {
                        Image(systemName: "arrow.right.circle")
                    }
                    .buttonStyle(.borderless)
                    .help("Reveal in Finder")
                    .accessibilityLabel("Reveal in Finder")
                    .accessibilityIdentifier("feature.action.reveal")
                }
                CopyButton(text: path, label: "Copy Path").buttonStyle(.borderless)
            }
        }
        .accessibilityElement(children: .contain)
    }

    private func dateRow(_ label: String, _ date: Date) -> some View {
        FactRow(label: label, value: "\(FeaturePresentation.relative(date)) · \(FeaturePresentation.absolute(date))")
    }

    /// `~/projects/…` for paths in the home folder.
    static func abbreviated(_ path: String, home: String = NSHomeDirectory()) -> String {
        guard !home.isEmpty, path == home || path.hasPrefix(home + "/") else { return path }
        return "~" + path.dropFirst(home.count)
    }

    /// The env file relative to the worktree (".env") when it lives inside it.
    static func relative(_ path: String, to folder: String?) -> String {
        guard let folder, !folder.isEmpty, path.hasPrefix(folder + "/") else { return abbreviated(path) }
        return String(path.dropFirst(folder.count + 1))
    }
}

#Preview("Overview") {
    VStack(spacing: 16) {
        OverviewCard(record: PreviewSamples.features[0], folderExists: true)
        OverviewCard(record: PreviewSamples.features[0], folderExists: false)
    }
    .padding()
    .frame(width: 480)
}
