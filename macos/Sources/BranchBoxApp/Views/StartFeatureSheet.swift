import SwiftUI

struct StartFeatureSheet: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @Environment(\.dismiss) private var dismiss
    @Binding var name: String

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Advanced Start Options").font(.headline)
            TextField("Feature name", text: $name).textFieldStyle(.roundedBorder)
            TextField("Optional title", text: titleBinding).textFieldStyle(.roundedBorder)

            Toggle("Minimal mode", isOn: minimalBinding)
            Toggle("Reuse existing worktree", isOn: reuseBinding)

            HStack {
                TextField("Branch prefix", text: branchPrefixBinding).textFieldStyle(.roundedBorder)
                TextField("Prompt seed", text: promptBinding).textFieldStyle(.roundedBorder)
            }

            if !viewModel.promptHistory.isEmpty {
                HStack { Text("Recent prompts:").font(.caption)
                    ForEach(viewModel.promptHistory, id: \.self) { seed in
                        Button(seed) { viewModel.promptSeed = seed }
                            .buttonStyle(.borderless)
                            .font(.caption)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.blue.opacity(0.1))
                            .clipShape(Capsule())
                    }
                }
            }

            Menu("Skip modules") {
                ForEach(viewModel.availableModules, id: \.self) { module in
                    let isOn = viewModel.skipModules.contains(module)
                    Button(action: { toggle(module) }) {
                        Label(module.capitalized, systemImage: isOn ? "checkmark" : "")
                    }
                }
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { close() }
                Button("Start") { start() }
                    .buttonStyle(.borderedProminent)
                    .disabled(name.trimmed.isEmpty || viewModel.isWorking)
            }
        }
        .padding()
        .frame(width: 520)
    }

    private func toggle(_ module: String) {
        if viewModel.skipModules.contains(module) {
            viewModel.skipModules.remove(module)
        } else {
            viewModel.skipModules.insert(module)
        }
    }

    private func start() {
        viewModel.newFeatureName = normalized(name)
        viewModel.startFeature()
        close()
    }

    private func close() { dismiss() }

    // Optional name normalization for quick hygiene
    private func normalized(_ input: String) -> String {
        let lowered = input.lowercased()
        let allowed = lowered.map { ch -> Character in
            if ch.isLetter || ch.isNumber || ch == "-" { return ch }
            if ch.isWhitespace { return "-" }
            return "-"
        }
        let joined = String(allowed)
        // Collapse multiple dashes
        let collapsed = joined.replacingOccurrences(of: "-+", with: "-", options: .regularExpression)
        return collapsed.trimmingCharacters(in: CharacterSet(charactersIn: "-"))
    }

    private var titleBinding: Binding<String> {
        Binding(
            get: { viewModel.newFeatureTitle },
            set: { viewModel.newFeatureTitle = $0 }
        )
    }

    private var promptBinding: Binding<String> {
        Binding(
            get: { viewModel.promptSeed },
            set: { viewModel.promptSeed = $0 }
        )
    }

    private var branchPrefixBinding: Binding<String> {
        Binding(
            get: { viewModel.branchPrefix },
            set: { viewModel.branchPrefix = $0 }
        )
    }

    private var minimalBinding: Binding<Bool> {
        Binding(
            get: { viewModel.useMinimalMode },
            set: { viewModel.useMinimalMode = $0 }
        )
    }

    private var reuseBinding: Binding<Bool> {
        Binding(
            get: { viewModel.reuseExisting },
            set: { viewModel.reuseExisting = $0 }
        )
    }
}
