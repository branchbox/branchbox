import SwiftUI

struct MainAppView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @State private var selectedFeature: FeatureViewData?
    @State private var startName: String = ""

    var body: some View {
        NavigationSplitView {
            List(AppSection.allCases, selection: $viewModel.selectedSection) { section in
                Label(section.title, systemImage: section.icon)
            }
            .listStyle(.sidebar)
            .navigationTitle("BranchBox")
        } content: {
            switch viewModel.selectedSection ?? .home {
            case .home:
                HomeView()
            case .features:
                FeaturesView(selected: $selectedFeature)
            case .agent:
                AgentStatusView()
            case .settings:
                SettingsView()
            }
        } detail: {
            if viewModel.selectedSection == .features, let feature = selectedFeature {
                FeatureDetailView(feature: feature)
                    .environmentObject(viewModel)
            } else if viewModel.selectedSection == .features {
                PlaceholderDetail()
            } else {
                EmptyView()
            }
        }
        .task { await viewModel.loadIfNeeded() }
        .sheet(isPresented: $viewModel.isCommandPalettePresented) {
            CommandPaletteView().environmentObject(viewModel)
        }
        .sheet(isPresented: $viewModel.commandStartRequested, onDismiss: { startName = "" }) {
            StartFeatureSheet(name: $startName).environmentObject(viewModel)
        }
        .sheet(item: $viewModel.pendingTeardown) { feature in
            TeardownSheetView(
                feature: feature,
                options: $viewModel.teardownOptions,
                onConfirm: { viewModel.performPendingTeardown() },
                onCancel: { viewModel.cancelPendingTeardown() }
            )
        }
        .sheet(isPresented: $viewModel.isDetectSheetPresented, onDismiss: { viewModel.detectOutput = nil }) {
            DetectOutputView(output: viewModel.detectOutput ?? "No data") {
                viewModel.isDetectSheetPresented = false
                viewModel.detectOutput = nil
            }
        }
        .alert(item: $viewModel.activeAlert) { alert in
            Alert(title: Text(alert.title), message: Text(alert.message), dismissButton: .default(Text("OK")))
        }
        .toolbar {
            ToolbarItemGroup {
                Menu {
                    Button("Choose Workspace…") { viewModel.openWorkspacePicker() }
                    Divider()
                    Button("Reveal in Finder") { viewModel.revealWorkspaceInFinder() }
                    Button("Open in Terminal") { viewModel.openWorkspaceInTerminal() }
                } label: {
                    Label(viewModel.workspaceDisplayName, systemImage: "folder")
                }

                Picker(
                    "Transport",
                    selection: Binding(
                        get: { viewModel.transportPreference },
                        set: { viewModel.setTransportPreference($0) }
                    )
                ) {
                    ForEach(TransportPreference.allCases) { pref in
                        Text(pref.label).tag(pref)
                    }
                }
                .pickerStyle(.menu)
                .labelStyle(.titleAndIcon)

                StatusBadge(
                    title: viewModel.transportStatusLabel,
                    systemImage: viewModel.transportStatusIcon,
                    tint: viewModel.transportStatusTint
                )

                StatusBadge(
                    title: viewModel.controlPlaneStatusLabel,
                    systemImage: viewModel.controlPlaneStatusIcon,
                    tint: viewModel.controlPlaneStatusTint
                )

                Button {
                    viewModel.refresh()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh")
                .disabled(viewModel.isWorking)
            }
        }
    }
}

private struct PlaceholderDetail: View {
    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: "shippingbox")
                .font(.system(size: 42))
                .foregroundColor(.secondary)
            Text("Select an item")
                .font(.title3)
            Text("Or start a new feature from Home.")
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct StatusBadge: View {
    let title: String
    let systemImage: String
    let tint: Color

    var body: some View {
        Label(title, systemImage: systemImage)
            .foregroundColor(tint)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(tint.opacity(0.15))
            .clipShape(Capsule())
    }
}
