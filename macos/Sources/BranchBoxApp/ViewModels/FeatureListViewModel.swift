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

    private let bridge: AgentBridge
    private var hasLoadedInitially = false

    init(bridge: AgentBridge = AgentBridge()) {
        self.bridge = bridge
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
            let data = try await bridge.listFeatures(includeRemoved: nil)
            features = data
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
            promptSeed: promptSeed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : promptSeed.trimmed
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
                }
                await self.loadFeatures()
            } catch {
                self.activeAlert = FeatureAlert(title: "Start failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }

    func teardown(_ feature: FeatureViewData, force: Bool = false) {
        isWorking = true
        Task { [weak self] in
            guard let self else { return }
            do {
                try await self.bridge.teardownFeature(name: feature.workFeature, force: force)
                await self.loadFeatures()
            } catch {
                self.activeAlert = FeatureAlert(title: "Teardown failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }
}

private extension String {
    var trimmed: String {
        trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
