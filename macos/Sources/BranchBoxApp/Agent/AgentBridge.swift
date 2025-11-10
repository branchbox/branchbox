import Foundation
import GRPC
import NIOCore
import NIOPosix
import OSLog

struct AgentConfiguration {
    let grpcHost: String
    let grpcPort: Int
    let workspacePath: String
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
}

enum AgentBridgeError: Error {
    case invalidFeatureName
    case cliUnavailable(String)
}

final class AgentBridge {
    private let configuration: AgentConfiguration
    private let group: EventLoopGroup
    private var client: Branchbox_Agent_FeatureServiceClient?
    private var connection: ClientConnection?
    private let logger = Logger(subsystem: "dev.branchbox.app", category: "agent")

    init(configuration: AgentConfiguration = .detect()) {
        self.configuration = configuration
        self.group = MultiThreadedEventLoopGroup(numberOfThreads: 1)
    }

    deinit {
        connection?.close(promise: nil)
        try? group.syncShutdownGracefully()
    }

    func listFeatures(includeRemoved override: Bool? = nil) async throws -> [FeatureViewData] {
        let includeRemoved = override ?? configuration.includeRemoved
        do {
            let client = try ensureClient()
            var request = Branchbox_Agent_ListRequest()
            request.repoPath = configuration.workspacePath
            request.includeRemoved = includeRemoved
            let response = try await client.list(request).response.get()
            return response.features.map(FeatureViewData.init(grpc:))
        } catch {
            logger.error("gRPC list failed: %{public}@", error.localizedDescription)
            return try CLICompat.featureList(workspacePath: configuration.workspacePath, includeRemoved: includeRemoved)
                .map(FeatureViewData.init(cli:))
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
            _ = try await client.start(request).response.get()
        } catch {
            logger.error("gRPC start failed: %{public}@", error.localizedDescription)
            try CLICompat.startFeature(intent, workspacePath: configuration.workspacePath)
        }
    }

    func teardownFeature(name: String, force: Bool) async throws {
        guard !name.isEmpty else {
            throw AgentBridgeError.invalidFeatureName
        }

        do {
            let client = try ensureClient()
            var request = Branchbox_Agent_TeardownRequest()
            request.repoPath = configuration.workspacePath
            request.name = name
            request.force = force
            _ = try await client.teardown(request).response.get()
        } catch {
            logger.error("gRPC teardown failed: %{public}@", error.localizedDescription)
            try CLICompat.teardownFeature(name: name, workspacePath: configuration.workspacePath, force: force)
        }
    }

    private func ensureClient() throws -> Branchbox_Agent_FeatureServiceClient {
        if let client {
            return client
        }

        let connection = ClientConnection.insecure(group: group)
            .withConnectionBackoff(maximum: .seconds(5))
            .connect(host: configuration.grpcHost, port: configuration.grpcPort)
        self.connection = connection
        let client = Branchbox_Agent_FeatureServiceClient(channel: connection)
        self.client = client
        return client
    }
}
