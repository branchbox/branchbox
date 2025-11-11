import SwiftUI
#if os(macOS)
import AppKit
#endif

struct HomeView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @State private var name: String = ""
    @State private var showOptions = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header
                startCard
                recentCard
                healthRow
            }
            .padding(24)
        }
        .sheet(isPresented: $showOptions) {
            StartFeatureSheet(name: $name)
                .environmentObject(viewModel)
        }
        .onAppear { name = viewModel.suggestedFeatureName }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Welcome to BranchBox")
                .font(.largeTitle).bold()
            Text(viewModel.workspacePath)
                .foregroundColor(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
    }

    private var startCard: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                Text("Start a Feature").font(.title3).bold()
                HStack(spacing: 12) {
                    TextField("Feature name", text: $name)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit { quickStart() }
                    Button("Start") { quickStart() }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                        .disabled(viewModel.isWorking || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("Options…") { showOptions = true }
                        .disabled(viewModel.isWorking)
                }
            }
            .padding()
        }
    }

    private var recentCard: some View {
        GroupBox("Recent activity") {
            if viewModel.features.isEmpty {
                Text("No features yet — start one above.")
                    .foregroundColor(.secondary)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(viewModel.features.prefix(5)) { feature in
                        HStack {
                            Text(feature.workFeature).bold()
                            Spacer()
                            Text(feature.statusLabel)
                                .font(.caption)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(feature.status.lowercased() == "active" ? Color.green.opacity(0.2) : Color.gray.opacity(0.2))
                                .clipShape(Capsule())
                        }
                    }
                }
            }
        }
    }

    private var healthRow: some View {
        HStack(spacing: 12) {
            GroupBox {
                HStack {
                    Circle().fill(viewModel.isAgentConnected ? Color.green : Color.orange)
                        .frame(width: 10, height: 10)
                    Text(viewModel.isAgentConnected ? "Agent online" : "Agent fallback")
                    Spacer()
                }
                .padding(8)
            }
            GroupBox {
                HStack {
                    Circle().fill(viewModel.isControlPlaneHealthy ? Color.green : Color.orange)
                        .frame(width: 10, height: 10)
                    Text(viewModel.isControlPlaneHealthy ? "Control plane" : "CP pending")
                    Spacer()
                }
                .padding(8)
            }
            GroupBox {
                HStack {
                    let hasIssues = viewModel.outdatedDevcontainersCount > 0
                    Circle().fill(hasIssues ? Color.orange : Color.green)
                        .frame(width: 10, height: 10)
                    Text(hasIssues ? "\(viewModel.outdatedDevcontainersCount) devcontainer(s) outdated" : "Devcontainers synced")
                    Spacer()
                }
                .padding(8)
            }
        }
    }

    private func quickStart() {
        viewModel.startFeatureQuick(name: name)
        name = ""
    }
}
