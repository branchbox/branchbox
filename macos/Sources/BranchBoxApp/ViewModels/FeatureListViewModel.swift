import Foundation
import SwiftUI

@MainActor
final class FeatureListViewModel: ObservableObject {
    @Published var features: [FeatureViewData] = []
    @Published var isLoading = false
    @Published var isWorking = false
    @Published var activeAlert: FeatureAlert?
    @Published var newFeatureName = ""
    @Published var newFeatureTitle = ""
    @Published var useMinimalMode = false
    @Published var promptSeed = ""
    @Published var branchPrefix = ""
    @Published var skipModules: Set<String> = []
    @Published var reuseExisting = false
    @Published var workspacePath: String
    @Published var transportStatus: AgentBridge.Transport = .grpc
    @Published var pendingTeardown: FeatureViewData?
    @Published var teardownOptions = TeardownOptions()
    @Published private(set) var promptHistory: [String]

    private let bridge: AgentBridge
    private let defaults: UserDefaults
    private var hasLoadedInitially = false
    private static let workspaceDefaultsKey = "branchbox.workspace"
    private static let promptHistoryKey = "branchbox.promptHistory"
    let availableModules = ["compose", "database", "tunnel", "specs"]

    init(bridge: AgentBridge = AgentBridge(), defaults: UserDefaults = .standard) {
        self.bridge = bridge
        self.defaults = defaults
        let storedWorkspace = defaults.string(forKey: Self.workspaceDefaultsKey) ?? bridge.workspacePath
        self.workspacePath = storedWorkspace
        self.promptHistory = defaults.stringArray(forKey: Self.promptHistoryKey) ?? []
        self.bridge.updateWorkspacePath(storedWorkspace)
    }

    func loadIfNeeded() async {
        if hasLoadedInitially {
            return
        }
        hasLoadedInitially = true
        await loadFeatures()
    }

    func refresh() {
        Task {
            await loadFeatures()
        }
    }

    func loadFeatures() async {
        isLoading = true
        do {
            let result = try await bridge.listFeatures(includeRemoved: nil)
            features = result.features
            transportStatus = result.transport
        } catch {
            activeAlert = FeatureAlert(
                title: "Unable to reach agent",
                message: error.localizedDescription
            )
        }
        isLoading = false
    }

    func startFeature() {
        let trimmedName = newFeatureName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedName.isEmpty else {
            activeAlert = FeatureAlert(title: "Missing name", message: "Enter a feature name before starting")
            return
        }

        let intent = FeatureStartIntent(
            name: trimmedName,
            title: newFeatureTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : newFeatureTitle.trimmed,
            minimal: useMinimalMode,
            promptSeed: promptSeed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : promptSeed.trimmed,
            branchPrefix: branchPrefix.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : branchPrefix.trimmed,
            skipModules: skipModules.sorted(),
            reuseExisting: reuseExisting
        )

        isWorking = true
        Task { [weak self] in
            guard let self else { return }
            do {
                try await self.bridge.startFeature(intent)
                await MainActor.run {
                    self.newFeatureName = ""
                    self.newFeatureTitle = ""
                    self.promptSeed = ""
                    self.useMinimalMode = false
                    self.branchPrefix = ""
                    self.skipModules = []
                    self.reuseExisting = false
                }
                self.recordPromptSeed(intent.promptSeed)
                await self.loadFeatures()
            } catch {
                self.activeAlert = FeatureAlert(title: "Start failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }

    func openTeardownSheet(for feature: FeatureViewData) {
        pendingTeardown = feature
        teardownOptions = TeardownOptions()
    }

    func cancelPendingTeardown() {
        pendingTeardown = nil
    }

    func performPendingTeardown() {
        guard let feature = pendingTeardown else { return }
        teardown(feature: feature, force: teardownOptions.force, completeSpec: teardownOptions.completeSpec)
        pendingTeardown = nil
    }

    func teardown(feature: FeatureViewData, force: Bool = false, completeSpec: Bool = false) {
        isWorking = true
        Task { [weak self] in
            guard let self else { return }
            do {
                try await self.bridge.teardownFeature(name: feature.workFeature, force: force, completeSpec: completeSpec)
                await self.loadFeatures()
            } catch {
                self.activeAlert = FeatureAlert(title: "Teardown failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }

    func updateWorkspace(to path: String) {
        workspacePath = path
        defaults.set(path, forKey: Self.workspaceDefaultsKey)
        bridge.updateWorkspacePath(path)
        Task {
            await loadFeatures()
        }
    }

    func applyPromptHistory(_ seed: String) {
        promptSeed = seed
    }

    private func recordPromptSeed(_ seed: String?) {
        guard let seed = seed?.trimmingCharacters(in: .whitespacesAndNewlines), !seed.isEmpty else {
            return
        }
        promptHistory.removeAll { $0 == seed }
        promptHistory.insert(seed, at: 0)
        if promptHistory.count > 5 {
            promptHistory = Array(promptHistory.prefix(5))
        }
        defaults.set(promptHistory, forKey: Self.promptHistoryKey)
    }
}

private extension String {
    var trimmed: String {
        trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

struct TeardownOptions {
    var force = false
    var completeSpec = false
}
