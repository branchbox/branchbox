import SwiftUI
#if os(macOS)
import AppKit
#endif

struct CommandPaletteView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @Environment(\.dismiss) private var dismiss
    @State private var query: String = ""

    struct ActionItem: Identifiable {
        let id = UUID()
        let title: String
        let subtitle: String?
        let systemImage: String
        let perform: () -> Void
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: "magnifyingglass")
                TextField("Type a command…", text: $query)
                    .textFieldStyle(.plain)
            }
            .padding(12)
            .background(.ultraThickMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 10))

            List(filteredItems) { item in
                Button(action: { item.perform(); dismiss() }) {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: item.systemImage)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(item.title)
                            if let sub = item.subtitle { Text(sub).font(.caption).foregroundColor(.secondary) }
                        }
                        Spacer()
                    }
                }
                .buttonStyle(.plain)
            }
            .listStyle(.plain)
            .frame(minHeight: 180, maxHeight: 320)
        }
        .padding(16)
        .frame(width: 540)
    }

    private var items: [ActionItem] {
        var result: [ActionItem] = [
            ActionItem(title: "Start Feature…", subtitle: nil, systemImage: "plus.circle.fill") {
                viewModel.commandStartRequested = true
            },
            ActionItem(title: "Switch Workspace…", subtitle: nil, systemImage: "folder") {
                viewModel.openWorkspacePicker()
            },
            ActionItem(title: "Run detect", subtitle: nil, systemImage: "text.magnifyingglass") {
                viewModel.runDetect()
            },
            ActionItem(title: "Refresh", subtitle: nil, systemImage: "arrow.clockwise") {
                viewModel.refresh()
            },
            ActionItem(title: "Sync Devcontainer", subtitle: nil, systemImage: "arrow.triangle.2.circlepath") {
                viewModel.syncDevcontainer()
            },
            ActionItem(title: "Go to Home", subtitle: nil, systemImage: "house") {
                viewModel.selectedSection = .home
            },
            ActionItem(title: "Go to Features", subtitle: nil, systemImage: "list.bullet") {
                viewModel.selectedSection = .features
            },
            ActionItem(title: "Go to Agent", subtitle: nil, systemImage: "waveform.path.ecg") {
                viewModel.selectedSection = .agent
            },
            ActionItem(title: "Go to Settings", subtitle: nil, systemImage: "gearshape") {
                viewModel.selectedSection = .settings
            },
        ]
        if let active = viewModel.activeFeature, let urlStr = active.featureURL, let url = URL(string: urlStr) {
            result.append(ActionItem(title: "Open Active Feature", subtitle: active.workFeature, systemImage: "safari") {
                #if os(macOS)
                NSWorkspace.shared.open(url)
                #endif
            })
            result.append(ActionItem(title: "Teardown Active Feature…", subtitle: active.workFeature, systemImage: "trash") {
                viewModel.openTeardownSheet(for: active)
            })
        }
        return result
    }

    private var filteredItems: [ActionItem] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { return items }
        return items.filter { $0.title.localizedCaseInsensitiveContains(q) || ($0.subtitle?.localizedCaseInsensitiveContains(q) ?? false) }
    }
}
