import SwiftUI
#if os(macOS)
import AppKit
#endif

struct SettingsView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel

    var body: some View {
        Form {
            Section("Workspace") {
                HStack {
                    Text(viewModel.workspacePath).lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Choose…") { chooseWorkspace() }
                }
                Button("Run detect") {
                    viewModel.runDetect()
                }
                .disabled(viewModel.isWorking)
            }
            Section("Devcontainer") {
                Picker("Sync strategy", selection: Binding(
                    get: { viewModel.devcontainerStrategy },
                    set: { viewModel.setDevcontainerStrategy($0) }
                )) {
                    Text("Copy").tag("copy")
                    Text("Symlink").tag("symlink")
                }
                .pickerStyle(.segmented)
                Text("Used for menu bar and Agent syncs.")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
        }
        .padding(24)
        .navigationTitle("Settings")
    }

    private func chooseWorkspace() {
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.begin { response in
            if response == .OK, let url = panel.url {
                viewModel.updateWorkspace(to: url.path)
            }
        }
        #endif
    }
}
