import Foundation

enum CLICompat {
    static func featureList(workspacePath: String, includeRemoved: Bool) throws -> [FeatureRecord] {
        var args = ["feature", "list", "--json", "--repo", workspacePath]
        if includeRemoved {
            args.append("--all")
        }
        let output = try run(arguments: args, workspacePath: workspacePath)
        guard let data = output.data(using: .utf8) else {
            return []
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([FeatureRecord].self, from: data)
    }

    static func startFeature(_ intent: FeatureStartIntent, workspacePath: String) throws {
        var args = ["feature", "start", intent.name, "--repo", workspacePath, "--json", "--no-summary"]
        if let title = intent.title, !title.isEmpty {
            args.append(contentsOf: ["--title", title])
        }
        if intent.minimal {
            args.append("--minimal")
        }
        if let branchPrefix = intent.branchPrefix, !branchPrefix.isEmpty {
            args.append(contentsOf: ["--branch-prefix", branchPrefix])
        }
        if intent.reuseExisting {
            args.append("--reuse")
        }
        for module in intent.skipModules where !module.isEmpty {
            args.append(contentsOf: ["--skip-module", module])
        }
        if let prompt = intent.promptSeed, !prompt.isEmpty {
            args.append(contentsOf: ["--prompt", prompt])
        }
        _ = try run(arguments: args, workspacePath: workspacePath)
    }

    static func teardownFeature(name: String, workspacePath: String, force: Bool, completeSpec: Bool) throws {
        var args = ["feature", "teardown", name, "--repo", workspacePath, "--json"]
        if force {
            args.append("--force")
        }
        if completeSpec {
            args.append("--complete-spec")
        }
        _ = try run(arguments: args, workspacePath: workspacePath)
    }

    static func agentStatus(workspacePath: String) throws -> AgentStatusRecord {
        let output = try run(arguments: ["agent", "status", "--json"], workspacePath: workspacePath)
        guard let data = output.data(using: .utf8) else {
            throw AgentBridgeError.cliUnavailable("agent status returned empty output")
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(AgentStatusRecord.self, from: data)
    }

    private static func run(arguments: [String], workspacePath: String) throws -> String {
        let cliBinary = resolveCLIBinary()
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = [cliBinary] + arguments
        process.currentDirectoryURL = URL(fileURLWithPath: workspacePath)

        let outputPipe = Pipe()
        let errorPipe = Pipe()
        process.standardOutput = outputPipe
        process.standardError = errorPipe

        do {
            try process.run()
        } catch {
            throw AgentBridgeError.cliUnavailable("CLI not runnable: \(error.localizedDescription)")
        }

        process.waitUntilExit()
        let stdOut = outputPipe.fileHandleForReading.readDataToEndOfFile()
        if process.terminationStatus != 0 {
            let stderr = errorPipe.fileHandleForReading.readDataToEndOfFile()
            let message = String(data: stderr, encoding: .utf8) ?? "Unknown CLI error"
            throw AgentBridgeError.cliUnavailable(message.trimmingCharacters(in: .whitespacesAndNewlines))
        }

        return String(data: stdOut, encoding: .utf8) ?? ""
    }

    private static func resolveCLIBinary() -> String {
        // 1) Explicit override via env
        if let override = ProcessInfo.processInfo.environment["BRANCHBOX_CLI_PATH"], !override.isEmpty {
            return override
        }
        #if os(macOS)
        // 2) Embedded binary inside app bundle Resources/bin/branchbox
        if let resURL = Bundle.main.resourceURL {
            let embedded = resURL.appendingPathComponent("bin/branchbox").path
            if FileManager.default.isExecutableFile(atPath: embedded) {
                return embedded
            }
        }
        #endif
        // 3) Fallback to PATH
        return "branchbox"
    }

    struct FeatureRecord: Decodable {
        let workFeature: String
        let branchName: String
        let status: String
        let featureUrl: String?
        let promptSeed: String?
        let startMode: String?
        let updatedAt: Date?
        let tunnelStatus: String?
        let tunnelProvider: String?
        let moduleOutcomes: [ModuleOutcomeRecord]?
        let adapter: AdapterRecord?
    }

    struct ModuleOutcomeRecord: Decodable {
        let module: String
        let status: String
    }

    struct AdapterRecord: Decodable {
        let name: String
        let serviceUrl: String
        let warnings: [String]?
    }

    struct AgentStatusRecord: Decodable {
        let controlPlaneConfigured: Bool
        let controlPlaneConnected: Bool
        let lastDeliveryAt: String?
        let lastFailureAt: String?
        let lastError: String?
        let lastAckEventId: Int64?
    }
}
