import Foundation
import GRPC
import NIOCore
import NIOPosix
import OSLog

struct AgentConfiguration {
    let grpcHost: String
    let grpcPort: Int
    var workspacePath: String
    let includeRemoved: Bool

    static func detect(userDefaults: UserDefaults = .standard) -> AgentConfiguration {
        let env = ProcessInfo.processInfo.environment
        let address = env["BRANCHBOX_AGENT_GRPC_ADDR"] ?? "127.0.0.1:50515"
        let parts = address.split(separator: ":")
        let host = parts.first.map(String.init) ?? "127.0.0.1"
        let port = parts.last.flatMap { Int($0) } ?? 50515
        let workspace = env["BRANCHBOX_WORKSPACE"]
            ?? userDefaults.string(forKey: "branchbox.workspace")
            ?? FileManager.default.currentDirectoryPath
        let includeRemoved = env["BRANCHBOX_SHOW_REMOVED"] == "1"

        return AgentConfiguration(
            grpcHost: host,
            grpcPort: port,
            workspacePath: workspace,
            includeRemoved: includeRemoved
        )
    }

    func withWorkspace(_ path: String) -> AgentConfiguration {
        var copy = self
        copy.workspacePath = path
        return copy
    }
}

enum AgentBridgeError: Error {
    case invalidFeatureName
    case cliUnavailable(String)
}

extension AgentBridgeError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .invalidFeatureName:
            return "Feature name is required."
        case .cliUnavailable(let reason):
            return "CLI fallback failed: \(reason)"
        }
    }
}

final class AgentBridge {
    enum Transport: String {
        case grpc
        case cliFallback
    }

    struct FeatureFetchResult {
        let features: [FeatureViewData]
        let transport: Transport
        let status: AgentStatusSnapshot
    }

    struct AgentStatusSnapshot {
        let controlPlaneConfigured: Bool
        let controlPlaneConnected: Bool
        let lastDeliveryAt: String?
        let lastFailureAt: String?
        let lastError: String?
        let lastAckEventID: Int64?

        init(
            controlPlaneConfigured: Bool,
            controlPlaneConnected: Bool,
            lastDeliveryAt: String?,
            lastFailureAt: String?,
            lastError: String?,
            lastAckEventID: Int64?
        ) {
            self.controlPlaneConfigured = controlPlaneConfigured
            self.controlPlaneConnected = controlPlaneConnected
            self.lastDeliveryAt = lastDeliveryAt
            self.lastFailureAt = lastFailureAt
            self.lastError = lastError
            self.lastAckEventID = lastAckEventID
        }

        init(grpc status: Branchbox_Agent_AgentStatus) {
            controlPlaneConfigured = status.controlPlaneConfigured
            controlPlaneConnected = status.controlPlaneConnected
            lastDeliveryAt = status.hasLastDeliveryAt ? status.lastDeliveryAt : nil
            lastFailureAt = status.hasLastFailureAt ? status.lastFailureAt : nil
            lastError = status.hasLastError ? status.lastError : nil
            lastAckEventID = status.hasLastAckEventID ? Int64(status.lastAckEventID) : nil
        }

        init(cli status: CLICompat.AgentStatusRecord) {
            controlPlaneConfigured = status.controlPlaneConfigured
            controlPlaneConnected = status.controlPlaneConnected
            lastDeliveryAt = status.lastDeliveryAt
            lastFailureAt = status.lastFailureAt
            lastError = status.lastError
            lastAckEventID = status.lastAckEventId.map { Int64($0) }
        }
    }

    private var configuration: AgentConfiguration
    private let group: EventLoopGroup
    private var client: Branchbox_Agent_FeatureServiceAsyncClient?
    private var connection: ClientConnection?
    private let logger = Logger(subsystem: "dev.branchbox.app", category: "agent")

    init(configuration: AgentConfiguration = .detect()) {
        self.configuration = configuration
        self.group = MultiThreadedEventLoopGroup(numberOfThreads: 1)
    }

    deinit {
        // Close the connection gracefully if present
        if let connection {
            _ = try? connection.close().wait()
        }
        try? group.syncShutdownGracefully()
    }

    var workspacePath: String { configuration.workspacePath }

    func updateWorkspacePath(_ path: String) {
        guard !path.isEmpty, configuration.workspacePath != path else { return }
        configuration = configuration.withWorkspace(path)
        resetConnection()
    }

    func listFeatures(includeRemoved override: Bool? = nil) async throws -> FeatureFetchResult {
        let includeRemoved = override ?? configuration.includeRemoved
        do {
            let client = try ensureClient()
            var request = Branchbox_Agent_ListRequest()
            request.repoPath = configuration.workspacePath
            request.includeRemoved = includeRemoved
            let response = try await client.list(request)
            let statusResponse = try await client.status(Branchbox_Agent_StatusRequest())
            let snapshot: AgentStatusSnapshot = {
                if statusResponse.hasStatus {
                    return AgentStatusSnapshot(grpc: statusResponse.status)
                }
                return AgentStatusSnapshot(
                    controlPlaneConfigured: false,
                    controlPlaneConnected: false,
                    lastDeliveryAt: nil,
                    lastFailureAt: nil,
                    lastError: nil,
                    lastAckEventID: nil
                )
            }()
            return FeatureFetchResult(
                features: response.features.map(FeatureViewData.init(grpc:)),
                transport: .grpc,
                status: snapshot
            )
        } catch {
            logger.error("gRPC list failed: \(error.localizedDescription, privacy: .public)")
            let features = try CLICompat
                .featureList(workspacePath: configuration.workspacePath, includeRemoved: includeRemoved)
                .map(FeatureViewData.init(cli:))
            let status = CLICompat.agentStatusOrDefault(workspacePath: configuration.workspacePath)
            return FeatureFetchResult(
                features: features,
                transport: .cliFallback,
                status: AgentStatusSnapshot(cli: status)
            )
        }
    }

    func startFeature(_ intent: FeatureStartIntent) async throws {
        guard !intent.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw AgentBridgeError.invalidFeatureName
        }

        do {
            let client = try ensureClient()
            var request = Branchbox_Agent_StartRequest()
            request.repoPath = configuration.workspacePath
            request.name = intent.name
            request.title = intent.title ?? ""
            request.mode = intent.minimal ? "minimal" : "full"
            request.promptSeed = intent.promptSeed ?? ""
            request.branchPrefix = intent.branchPrefix ?? ""
            request.reuse = intent.reuseExisting
            request.skipModules = intent.skipModules
            _ = try await client.start(request)
        } catch {
            logger.error("gRPC start failed: \(error.localizedDescription, privacy: .public)")
            try CLICompat.startFeature(intent, workspacePath: configuration.workspacePath)
        }
    }

    func teardownFeature(name: String, force: Bool, completeSpec: Bool, deleteBranch: Bool) async throws {
        guard !name.isEmpty else {
            throw AgentBridgeError.invalidFeatureName
        }

        do {
            let client = try ensureClient()
            var request = Branchbox_Agent_TeardownRequest()
            request.repoPath = configuration.workspacePath
            request.name = name
            request.force = force
            request.completeSpec = completeSpec
            request.deleteBranch = deleteBranch
            _ = try await client.teardown(request)
        } catch {
            logger.error("gRPC teardown failed: \(error.localizedDescription, privacy: .public)")
            try CLICompat.teardownFeature(
                name: name,
                workspacePath: configuration.workspacePath,
                force: force,
                completeSpec: completeSpec,
                deleteBranch: deleteBranch
            )
        }
    }

    func syncDevcontainer(strategy: String? = nil, dryRun: Bool = false) async throws {
        // Currently no gRPC endpoint; use CLI compat.
        let path = configuration.workspacePath
        do {
            try await Task.detached(priority: .userInitiated) {
                try CLICompat.devcontainerSync(workspacePath: path, strategy: strategy, dryRun: dryRun)
            }.value
        } catch {
            logger.error("Devcontainer sync failed: \(error.localizedDescription, privacy: .public)")
            throw error
        }
    }

    private func ensureClient() throws -> Branchbox_Agent_FeatureServiceAsyncClient {
        if let client {
            return client
        }

        let connection = ClientConnection.insecure(group: group)
            .withConnectionBackoff(maximum: .seconds(5))
            .connect(host: configuration.grpcHost, port: configuration.grpcPort)
        self.connection = connection
        let client = Branchbox_Agent_FeatureServiceAsyncClient(channel: connection)
        self.client = client
        return client
    }

    private func resetConnection() {
        client = nil
        if let connection {
            _ = try? connection.close().wait()
        }
        connection = nil
    }
}
