import BranchBoxKit
import Foundation
import os

/// Learns a located CLI's version and capabilities (D-9, DESIGN §7.4).
///
/// - `branchbox version --json` answers on 0.14+. A legacy CLI rejects the subcommand with clap's exit 2, and
///   output that does not decode is treated the same way: both fall back to `branchbox --version`, and the
///   identity then has no contract version and no capabilities (legacy mode).
/// - A CLI older than 0.13.4 is `.cliTooOld`.
/// - Results are cached in memory and in Application Support (`cli-probe-cache.json`), keyed by the unresolved
///   path plus the inode, modification time and size of the file it resolves to. A `brew upgrade` changes the
///   key, so calling `identity(for:environment:)` on app activation re-probes exactly when needed.
public actor CLIProbe {
    public static let timeout: Duration = .seconds(10)
    static let cacheFileName = "cli-probe-cache.json"
    static let maxCachedEntries = 16

    /// What a probe learned about one version of one file.
    struct Entry: Codable, Hashable, Sendable {
        var path: String
        var file: FileIdentity
        var version: String
        var contractVersion: Int?
        var capabilities: [String]

        private enum CodingKeys: String, CodingKey {
            case path, file, version, capabilities
            case contractVersion = "contract_version"
        }
    }

    private struct CacheDocument: Codable {
        var entries: [Entry]
    }

    private struct Key: Hashable {
        var path: String
        var file: FileIdentity
    }

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "cli-probe")

    private let runner: any ProcessRunning
    private let fileSystem: any FileSystemProbing
    private let cacheDirectory: URL?
    private let timeout: Duration
    private var entries: [Entry] = []                 // most recent first
    private var loadedCache = false

    /// - Parameter cacheDirectory: where `cli-probe-cache.json` lives; nil caches in memory only.
    public init(runner: any ProcessRunning, fileSystem: any FileSystemProbing = LocalFileSystem(),
                cacheDirectory: URL? = ApplicationSupport.directory, timeout: Duration = CLIProbe.timeout) {
        self.runner = runner
        self.fileSystem = fileSystem
        self.cacheDirectory = cacheDirectory
        self.timeout = timeout
    }

    /// The identity of the CLI at `resolution.path`, probed with `environment` (a `.read` child environment)
    /// unless the cache already knows this exact file. Throws only `BackendError`: `.cliTooOld`, `.cliUnusable`
    /// or `.cancelled`.
    public func identity(for resolution: CLIResolution, environment: [String: String]) async throws -> BackendIdentity {
        guard let file = fileSystem.identity(at: resolution.path) else {
            throw BackendError.cliUnusable(path: resolution.path, reason: "The file no longer exists")
        }
        let key = Key(path: resolution.path, file: file)
        let entry = try await entry(for: key, environment: environment)
        guard let version = SemVer(parsing: entry.version) else {
            throw BackendError.cliUnusable(path: resolution.path, reason: "Unreadable version \"\(entry.version)\"")
        }
        guard version >= BackendIdentity.minimumCLI else {
            throw BackendError.cliTooOld(found: version, minimum: BackendIdentity.minimumCLI, path: resolution.path)
        }
        return BackendIdentity(kind: .cli(resolution), version: version, contractVersion: entry.contractVersion,
                               capabilities: Set(entry.capabilities.map(Capability.init(rawValue:))))
    }

    /// Drops every cached result, in memory and on disk (Settings › Recheck).
    public func forgetCachedResults() {
        entries = []
        loadedCache = true
        save()
    }

    // MARK: - Cache

    private func entry(for key: Key, environment: [String: String]) async throws -> Entry {
        loadCache()
        if let cached = entries.first(where: { $0.path == key.path && $0.file == key.file }) { return cached }
        let entry = try await Self.probe(key, environment: environment, runner: runner, timeout: timeout)
        entries.removeAll { $0.path == key.path }
        entries.insert(entry, at: 0)
        if entries.count > Self.maxCachedEntries { entries.removeLast(entries.count - Self.maxCachedEntries) }
        save()
        return entry
    }

    private var cacheFile: URL? {
        cacheDirectory?.appendingPathComponent(Self.cacheFileName)
    }

    private func loadCache() {
        guard !loadedCache else { return }
        loadedCache = true
        guard let file = cacheFile, let data = try? Data(contentsOf: file),
              let document = try? JSONDecoder().decode(CacheDocument.self, from: data) else { return }
        entries = document.entries
    }

    private func save() {
        guard let file = cacheFile else { return }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        do {
            try ApplicationSupport.write(try encoder.encode(CacheDocument(entries: entries)), to: file)
        } catch {
            Self.logger.error(
                "Could not write \(file.path, privacy: .public): \(error.localizedDescription, privacy: .public)")
        }
    }

    // MARK: - Probing

    private static func probe(_ key: Key, environment: [String: String], runner: any ProcessRunning,
                              timeout: Duration) async throws -> Entry {
        let versionJSON = try await run(["version", "--json"], key: key, environment: environment, runner: runner,
                                        timeout: timeout)
        switch versionJSON.termination {
        case .exited(0):
            if let info = try? CLIJSON.decode(VersionInfo.self, from: versionJSON.stdout).value,
               SemVer(parsing: info.version) != nil {
                return Entry(path: key.path, file: key.file, version: info.version,
                             contractVersion: info.contractVersion, capabilities: info.capabilities)
            }
        case .exited(2):
            break                                         // clap: unknown subcommand, a pre-0.14 CLI
        default:
            throw BackendError.cliUnusable(path: key.path, reason: failure(["version", "--json"], versionJSON))
        }

        let legacy = try await run(["--version"], key: key, environment: environment, runner: runner,
                                   timeout: timeout)
        let text = String(decoding: legacy.stdout, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard legacy.termination == .exited(0) else {
            throw BackendError.cliUnusable(path: key.path, reason: failure(["--version"], legacy))
        }
        guard let version = SemVer(parsing: text) else {
            let shown = text.isEmpty ? "nothing" : "\"\(text.prefix(200))\""
            throw BackendError.cliUnusable(path: key.path, reason: "\(command(["--version"])) printed \(shown)")
        }
        return Entry(path: key.path, file: key.file, version: version.description, contractVersion: nil,
                     capabilities: [])
    }

    private static func run(_ arguments: [String], key: Key, environment: [String: String],
                            runner: any ProcessRunning, timeout: Duration) async throws -> ProcessResult {
        var spec = ProcessSpec(executable: URL(fileURLWithPath: key.path), arguments: arguments,
                               environment: environment, workingDirectory: nil)
        spec.timeout = timeout
        spec.interruptGrace = .seconds(2)
        spec.terminateGrace = .seconds(1)
        spec.stdoutLimit = 1 << 20
        spec.stderrTailLines = 20
        do {
            return try await runner.run(spec, onLine: { _ in })
        } catch ProcessRunError.launchFailed(_, let reason) {
            throw BackendError.cliUnusable(path: key.path, reason: reason)
        } catch ProcessRunError.timedOut(let after, _) {
            throw BackendError.cliUnusable(path: key.path,
                                           reason: "\(command(arguments)) did not answer within \(after)")
        } catch ProcessRunError.stdoutTooLarge {
            throw BackendError.cliUnusable(path: key.path, reason: "\(command(arguments)) printed too much output")
        } catch {
            throw BackendError.normalize(error)
        }
    }

    private static func command(_ arguments: [String]) -> String {
        "`branchbox \(arguments.joined(separator: " "))`"
    }

    /// "`branchbox --version` exited with status 1: Error: …", quoting the CLI's `Error:` (or clap's `error:`) line,
    /// else its last stderr line that is not a tracing log line (never an INFO line), else only the status.
    static func failure(_ arguments: [String], _ result: ProcessResult) -> String {
        let status: String
        switch result.termination {
        case .exited(let code): status = "exited with status \(code)"
        case .signaled(let signal): status = "was killed by signal \(signal)"
        }
        let lines = result.stderrTail.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        let detail = lines.last(where: { $0.hasPrefix("Error:") || $0.hasPrefix("error:") })
            ?? lines.last(where: { TracingLineParser.parse($0, source: .stderr).level == .output })
        return "\(command(arguments)) \(status)" + (detail.map { ": \($0)" } ?? "")
    }
}
