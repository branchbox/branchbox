import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The top of Project detail: name, path with [Reveal] and [Terminal], "Updated n s ago" with a refresh button,
/// and chips for what `detect` found (stack, adapter, modules) plus the config's default runtime and branch
/// prefix. On a legacy CLI the detect report is parsed text; its raw output is one click away.
struct ProjectHeader: View {
    let store: ProjectStore

    @Environment(AppModel.self) private var model
    @State private var showsRawDetect = false
    @State private var launchError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "folder.fill")
                    .font(.system(size: 22))
                    .foregroundStyle(.tint)
                    .frame(width: 44, height: 44)
                    .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 3) {
                    Text(store.displayName)
                        .font(.title2.weight(.semibold))
                        .lineLimit(1)
                        .accessibilityAddTraits(.isHeader)
                    Text(InitProjectSheet.abbreviated(store.ref.path))
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                        .help(store.ref.path)
                }
                Spacer(minLength: 12)
                VStack(alignment: .trailing, spacing: 6) {
                    HStack(spacing: 6) {
                        Button {
                            ProjectActions.reveal(store.ref.path)
                        } label: {
                            Label("Reveal in Finder", systemImage: "folder")
                        }
                        .help("Show the project folder in Finder")
                        Button(action: openTerminal) {
                            Label("Open in Terminal", systemImage: "terminal")
                        }
                        .help("Open a terminal in the project folder")
                    }
                    .labelStyle(.iconOnly)
                    .disabled(!store.rootExists)
                    freshness
                }
            }
            chips
            if let launchError {
                Label(launchError, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.orange)
            }
        }
    }

    // MARK: Freshness

    private var freshness: some View {
        HStack(spacing: 6) {
            if store.isRefreshing {
                ProgressView().controlSize(.mini)
            }
            if let loaded = store.lastLoadedAt {
                TimelineView(.periodic(from: .now, by: 5)) { context in
                    Text(Self.updatedText(since: loaded, now: context.date))
                        .monospacedDigit()
                }
            }
            Button {
                store.requestRefresh(.manual)
                Task {
                    await store.reloadDetect()
                    await store.reloadConfig()
                }
            } label: {
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(.borderless)
            .help("Refresh")
            .accessibilityLabel("Refresh project")
            .disabled(store.isRefreshing)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }

    /// "Updated just now", "Updated 12 s ago", "Updated 3 min ago".
    static func updatedText(since date: Date, now: Date) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(date)))
        if seconds < 5 { return "Updated just now" }
        if seconds < 60 { return "Updated \(seconds) s ago" }
        if seconds < 3600 { return "Updated \(seconds / 60) min ago" }
        return "Updated \(seconds / 3600) h ago"
    }

    // MARK: Chips

    @ViewBuilder private var chips: some View {
        let detect = store.detect
        let config = store.config?.effective
        FlowLayout(spacing: 6) {
            if let stack = detect?.stack, !stack.isEmpty {
                ProjectInfoChip(title: "Stack", value: InitDraft.stackLabel(stack.lowercased()), systemImage: "square.stack.3d.up")
            }
            if let adapter = detect?.adapter, !adapter.isEmpty {
                ProjectInfoChip(title: "Adapter", value: adapter.capitalized, systemImage: "puzzlepiece.extension")
            }
            if let modules = detect?.modules, !modules.isEmpty {
                ProjectInfoChip(title: "Modules", value: "\(modules.count)", systemImage: "checklist")
                    .help(modules.joined(separator: ", "))
            }
            if let config {
                ProjectInfoChip(title: "Runtime", value: config.runtimeProvider.label, systemImage: config.runtimeProvider.symbol)
                ProjectInfoChip(title: "Branch prefix", value: config.branchPrefix.isEmpty ? "none" : config.branchPrefix,
                                systemImage: "arrow.triangle.branch")
            }
            if let raw = detect?.rawText, !raw.isEmpty {
                Button {
                    showsRawDetect = true
                } label: {
                    Label("Detect output", systemImage: "info.circle")
                        .font(.caption)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(.quaternary.opacity(0.6), in: Capsule())
                }
                .buttonStyle(.plain)
                .foregroundStyle(.tint)
                    .popover(isPresented: $showsRawDetect, arrowEdge: .bottom) {
                        ScrollView {
                            Text(raw)
                                .font(.callout.monospaced())
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(12)
                        }
                        .frame(width: 380, height: 240)
                    }
                    .help("What branchbox detect reported")
            }
        }
    }

    private func openTerminal() {
        launchError = nil
        let path = store.ref.path
        let terminal = model.settings.preferredTerminal
        Task {
            do {
                try await ProjectActions.openTerminal(at: path, terminal: terminal)
            } catch {
                launchError = (error as? HostLaunchError)?.message ?? error.localizedDescription
            }
        }
    }
}
