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

    private static func run(arguments: [String], workspacePath: String) throws -> String {
        let cliBinary = ProcessInfo.processInfo.environment["BRANCHBOX_CLI_PATH"] ?? "branchbox"
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
            throw AgentBridgeError.cliUnavailable(error.localizedDescription)
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

    struct FeatureRecord: Decodable {
        let work_feature: String
        let branch_name: String
        let status: String
        let feature_url: String?
        let prompt_seed: String?
        let start_mode: String?
        let updated_at: String?
        let tunnel_status: String?
        let tunnel_provider: String?
        let module_outcomes: [ModuleOutcomeRecord]?
    }

    struct ModuleOutcomeRecord: Decodable {
        let module: String
        let status: String
    }
}
