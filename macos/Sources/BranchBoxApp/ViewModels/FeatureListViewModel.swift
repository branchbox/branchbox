import Foundation
import SwiftUI
#if os(macOS)
import AppKit
#endif

enum TransportPreference: String, CaseIterable, Identifiable {
    case automatic
    case grpc
    case cliFallback

    var id: String { rawValue }

    var label: String {
        switch self {
        case .automatic:
            return "Automatic"
        case .grpc:
            return "Force gRPC"
        case .cliFallback:
            return "Force CLI"
        }
    }

    var overrideTransport: AgentBridge.Transport? {
        switch self {
        case .automatic:
            return nil
        case .grpc:
            return .grpc
        case .cliFallback:
            return .cliFallback
        }
    }
}

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
    @Published var teardownOptions = TeardownOptions() {
        didSet {
            persistTeardownOptions()
        }
    }
    @Published private(set) var promptHistory: [String]
    @Published var controlPlaneStatus: AgentBridge.AgentStatusSnapshot?
    @Published var transportPreference: TransportPreference
    // UI intents
    @Published var commandStartRequested: Bool = false
    @Published var syncDevcontainerRequested: Bool = false
    @Published var isCommandPalettePresented: Bool = false
    @Published var selectedSection: AppSection? = .home
    @Published var devcontainerStrategy: String
    @Published var detectOutput: String?
    @Published var isDetectSheetPresented = false

    private let bridge: AgentBridge
    private let defaults: UserDefaults
    private var hasLoadedInitially = false
    private static let workspaceDefaultsKey = "branchbox.workspace"
    private static let promptHistoryKey = "branchbox.promptHistory"
    private static let transportPreferenceKey = "branchbox.transportPreference"
    private static let teardownForceKey = "branchbox.teardown.force"
    private static let teardownCompleteSpecKey = "branchbox.teardown.completeSpec"
    private static let teardownDeleteBranchKey = "branchbox.teardown.deleteBranch"
    let availableModules = ["compose", "database", "tunnel", "specs"]

    init(bridge: AgentBridge = AgentBridge(), defaults: UserDefaults = .standard) {
        self.bridge = bridge
        self.defaults = defaults
        let storedWorkspace = defaults.string(forKey: Self.workspaceDefaultsKey) ?? bridge.workspacePath
        self.workspacePath = storedWorkspace
        self.promptHistory = defaults.stringArray(forKey: Self.promptHistoryKey) ?? []
        self.devcontainerStrategy = defaults.string(forKey: "branchbox.devcontainerStrategy") ?? "copy"
        self.detectOutput = nil
        let storedPreference = defaults
            .string(forKey: Self.transportPreferenceKey)
            .flatMap(TransportPreference.init(rawValue:)) ?? .automatic
        self.transportPreference = storedPreference
        self.teardownOptions = FeatureListViewModel.loadTeardownDefaults(from: defaults)
        self.bridge.updateWorkspacePath(storedWorkspace)
        self.bridge.setTransportOverride(storedPreference.overrideTransport)
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

    func setTransportPreference(_ preference: TransportPreference) {
        transportPreference = preference
        if preference == .automatic {
            defaults.removeObject(forKey: Self.transportPreferenceKey)
        } else {
            defaults.set(preference.rawValue, forKey: Self.transportPreferenceKey)
        }
        bridge.setTransportOverride(preference.overrideTransport)
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
            controlPlaneStatus = result.status
        } catch {
            activeAlert = FeatureAlert(
                title: "Unable to reach agent",
                message: error.localizedDescription
            )
        }
        isLoading = false
    }

    // MARK: - Derived UI state

    var activeFeature: FeatureViewData? {
        features.first { $0.status.lowercased() == "active" }
    }

    var isAgentConnected: Bool { transportStatus == .grpc }

    var isControlPlaneHealthy: Bool {
        guard let status = controlPlaneStatus else { return false }
        return status.controlPlaneConfigured && status.controlPlaneConnected
    }

    var outdatedDevcontainersCount: Int {
        features.filter { $0.devcontainerOutdated }.count
    }

    var suggestedFeatureName: String { "" }

    var workspaceDisplayName: String {
        let url = URL(fileURLWithPath: workspacePath)
        let name = url.lastPathComponent
        return name.isEmpty ? workspacePath : name
    }

    var transportStatusLabel: String {
        transportStatus == .grpc ? "gRPC" : "CLI fallback"
    }

    var transportStatusIcon: String {
        transportStatus == .grpc ? "bolt.horizontal" : "arrow.triangle.2.circlepath"
    }

    var transportStatusTint: Color {
        transportStatus == .grpc ? .green : .orange
    }

    var controlPlaneStatusLabel: String {
        isControlPlaneHealthy ? "CP connected" : "CP pending"
    }

    var controlPlaneStatusIcon: String {
        isControlPlaneHealthy ? "waveform.path.ecg" : "exclamationmark.triangle"
    }

    var controlPlaneStatusTint: Color {
        isControlPlaneHealthy ? .green : .orange
    }

    var workspaceNeedsSetup: Bool {
        !FileManager.default.fileExists(atPath: workspacePath)
    }

    struct TunnelSummary {
        let provider: String
        let status: String
        let hostname: String?
        let workFeature: String
    }

    var tunnelSummary: TunnelSummary? {
        guard let feature = (activeFeature ?? features.first(where: {
            ($0.tunnelProvider?.isEmpty == false) || ($0.tunnelStatus?.isEmpty == false)
        })) else {
            return nil
        }
        let provider = feature.tunnelProvider ?? "Unknown provider"
        let status = feature.tunnelStatus ?? "Unknown status"
        return TunnelSummary(
            provider: provider,
            status: status,
            hostname: feature.tunnelHostname,
            workFeature: feature.workFeature
        )
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
                self.newFeatureName = ""
                self.newFeatureTitle = ""
                self.promptSeed = ""
                self.useMinimalMode = false
                self.branchPrefix = ""
                self.skipModules = []
                self.reuseExisting = false
                self.recordPromptSeed(intent.promptSeed)
                await self.loadFeatures()
#if os(macOS)
                LocalNotifier.notify(title: "Feature started", body: "\(trimmedName) is ready")
#endif
            } catch {
                self.activeAlert = FeatureAlert(title: "Start failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }

    func startFeatureQuick(name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        newFeatureName = trimmed
        startFeature()
    }

    func syncDevcontainer(strategy: String? = nil, dryRun: Bool = false) {
        isWorking = true
        Task { [weak self] in
            guard let self else { return }
            do {
                let chosen = strategy ?? self.devcontainerStrategy
                try await self.bridge.syncDevcontainer(strategy: chosen, dryRun: dryRun)
                self.activeAlert = FeatureAlert(title: "Devcontainer", message: "Sync completed")
                await self.loadFeatures()
#if os(macOS)
                LocalNotifier.notify(title: "Devcontainer synced", body: "Strategy: \(chosen)\(dryRun ? " (dry-run)" : "")")
#endif
            } catch {
                self.activeAlert = FeatureAlert(title: "Sync failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }

    func openTeardownSheet(for feature: FeatureViewData) {
        pendingTeardown = feature
    }

    func cancelPendingTeardown() {
        pendingTeardown = nil
    }

    func performPendingTeardown() {
        guard let feature = pendingTeardown else { return }
        teardown(
            feature: feature,
            force: teardownOptions.force,
            completeSpec: teardownOptions.completeSpec,
            deleteBranch: teardownOptions.deleteBranch
        )
        pendingTeardown = nil
    }

    func teardown(feature: FeatureViewData, force: Bool = false, completeSpec: Bool = false, deleteBranch: Bool = false) {
        isWorking = true
        Task { [weak self] in
            guard let self else { return }
            do {
                try await self.bridge.teardownFeature(name: feature.workFeature, force: force, completeSpec: completeSpec, deleteBranch: deleteBranch)
                await self.loadFeatures()
#if os(macOS)
                LocalNotifier.notify(title: "Feature torn down", body: feature.workFeature)
#endif
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

    func openWorkspacePicker() {
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.begin { response in
            if response == .OK, let url = panel.url {
                self.updateWorkspace(to: url.path)
            }
        }
        #endif
    }

    func setDevcontainerStrategy(_ strategy: String) {
        devcontainerStrategy = strategy
        defaults.set(strategy, forKey: "branchbox.devcontainerStrategy")
    }

    func runDetect() {
        isWorking = true
        Task { [weak self] in
            guard let self else { return }
            do {
                let output = try CLICompat.detectProject(path: self.workspacePath)
                self.detectOutput = output
                self.isDetectSheetPresented = true
            } catch {
                self.activeAlert = FeatureAlert(title: "Detect failed", message: error.localizedDescription)
            }
            self.isWorking = false
        }
    }

    func copyTunnelHostname() {
        guard let hostname = tunnelSummary?.hostname, !hostname.isEmpty else {
            activeAlert = FeatureAlert(title: "No tunnel hostname", message: "Start a feature with a tunnel to copy its hostname.")
            return
        }
#if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(hostname, forType: .string)
#endif
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

    private static func loadTeardownDefaults(from defaults: UserDefaults) -> TeardownOptions {
        TeardownOptions(
            force: defaults.bool(forKey: Self.teardownForceKey),
            completeSpec: defaults.bool(forKey: Self.teardownCompleteSpecKey),
            deleteBranch: defaults.bool(forKey: Self.teardownDeleteBranchKey)
        )
    }

    private func persistTeardownOptions() {
        defaults.set(teardownOptions.force, forKey: Self.teardownForceKey)
        defaults.set(teardownOptions.completeSpec, forKey: Self.teardownCompleteSpecKey)
        defaults.set(teardownOptions.deleteBranch, forKey: Self.teardownDeleteBranchKey)
    }

#if os(macOS)
    func revealWorkspaceInFinder() {
        revealInFinder(path: workspacePath)
    }

    func openWorkspaceInTerminal() {
        openInTerminal(path: workspacePath)
    }

    func revealFeatureInFinder(_ feature: FeatureViewData) {
        guard let path = feature.worktreePath else { return }
        revealInFinder(path: path)
    }

    func openFeatureInTerminal(_ feature: FeatureViewData) {
        guard let path = feature.worktreePath else { return }
        openInTerminal(path: path)
    }

    private func revealInFinder(path: String) {
        NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: path)
    }

    private func openInTerminal(path: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/open")
        process.arguments = ["-a", "Terminal", path]
        try? process.run()
    }
#endif
}

internal extension String {
    var trimmed: String {
        trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

struct TeardownOptions {
    var force = false
    var completeSpec = false
    var deleteBranch = false
}
