import SwiftUI

struct StartFeatureSheet: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @Environment(\.dismiss) private var dismiss
    @Binding var name: String
    @State private var title: String = ""
    @State private var minimal = false
    @State private var prompt = ""
    @State private var branchPrefix = ""
    @State private var reuse = false
    @State private var skipped: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Advanced Start Options").font(.headline)
            TextField("Feature name", text: $name).textFieldStyle(.roundedBorder)
            TextField("Optional title", text: $title).textFieldStyle(.roundedBorder)

            Toggle("Minimal mode", isOn: $minimal)
            Toggle("Reuse existing worktree", isOn: $reuse)

            HStack {
                TextField("Branch prefix", text: $branchPrefix).textFieldStyle(.roundedBorder)
                TextField("Prompt seed", text: $prompt).textFieldStyle(.roundedBorder)
            }

            if !viewModel.promptHistory.isEmpty {
                HStack { Text("Recent prompts:").font(.caption)
                    ForEach(viewModel.promptHistory, id: \.self) { seed in
                        Button(seed) { prompt = seed }
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
                    let isOn = skipped.contains(module)
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
        .onAppear {
            title = viewModel.newFeatureTitle
            minimal = viewModel.useMinimalMode
            prompt = viewModel.promptSeed
            branchPrefix = viewModel.branchPrefix
            skipped = viewModel.skipModules
            reuse = viewModel.reuseExisting
        }
    }

    private func toggle(_ module: String) {
        if skipped.contains(module) { skipped.remove(module) } else { skipped.insert(module) }
    }

    private func start() {
        viewModel.newFeatureName = normalized(name)
        viewModel.newFeatureTitle = title
        viewModel.useMinimalMode = minimal
        viewModel.promptSeed = prompt
        viewModel.branchPrefix = branchPrefix
        viewModel.skipModules = skipped
        viewModel.reuseExisting = reuse
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
}
