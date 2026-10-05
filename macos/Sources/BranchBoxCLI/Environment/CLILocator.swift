import BranchBoxKit
import Foundation

/// Finds the `branchbox` executable (D-8). Candidates, in order:
///
/// 1. `BRANCHBOX_CLI_PATH`;
/// 2. the Settings override (Settings › Tools › Locate…);
/// 3. each directory of the login-shell PATH, searched here rather than by a shell;
/// 4. `/opt/homebrew/bin`, `/usr/local/bin`, `~/.cargo/bin`, `~/.local/bin`;
/// 5. `Contents/Helpers/branchbox` inside the app bundle, only if the app was packaged with it.
///
/// The first executable regular file wins. Its path is kept exactly as found, never symlink-resolved, so a
/// `brew upgrade` that repoints `/opt/homebrew/bin/branchbox` is picked up. A candidate that exists but cannot
/// be used, and an override that does not work, is recorded with the reason for Diagnostics.
public struct CLILocator: Sendable {
    public static let executableName = "branchbox"
    public static let environmentOverrideKey = "BRANCHBOX_CLI_PATH"

    public struct Outcome: Sendable, Hashable {
        /// nil when no candidate qualified.
        public let resolution: CLIResolution?
        /// Every path looked at, in order (`BackendError.cliNotFound(searched:)`).
        public let searched: [String]
        public let rejected: [RejectedCandidate]
    }

    private let fileSystem: any FileSystemProbing

    public init(fileSystem: any FileSystemProbing = LocalFileSystem()) {
        self.fileSystem = fileSystem
    }

    public static func wellKnownDirectories(home: String) -> [String] {
        ["/opt/homebrew/bin", "/usr/local/bin", "\(home)/.cargo/bin", "\(home)/.local/bin"]
    }

    /// - Parameters:
    ///   - processEnvironment: the app's environment, read for `BRANCHBOX_CLI_PATH`.
    ///   - settingsOverride: `BackendSettings.cliPathOverride`.
    ///   - searchPath: the login-shell PATH (`EnvironmentProvider.baseEnvironment(for:)`).
    ///   - home: expands `~` in overrides and in the well-known directories.
    ///   - bundleURL: the running app bundle; its `Contents/Helpers/branchbox` is the last candidate.
    public func locate(processEnvironment: [String: String], settingsOverride: String?, searchPath: String?,
                       home: String, bundleURL: URL? = Bundle.main.bundleURL) -> Outcome {
        var searched: [String] = []
        var rejected: [RejectedCandidate] = []

        func consider(_ path: String, source: CLISource, explicit: Bool) -> CLIResolution? {
            guard !searched.contains(path) else { return nil }
            searched.append(path)
            let problem: String
            switch fileSystem.kind(at: path) {
            case .file(executable: true):
                return CLIResolution(path: path, source: source, rejected: rejected)
            case .file(executable: false): problem = "Not executable"
            case .directory: problem = "Is a directory"
            case .brokenSymbolicLink: problem = "Symbolic link to a missing file"
            case .missing:
                // A directory without the CLI is not worth a Diagnostics row; a configured path is.
                guard explicit else { return nil }
                problem = "Does not exist"
            }
            rejected.append(RejectedCandidate(path: path, reason: "\(problem) (\(describe(source)))"))
            return nil
        }

        func override(_ value: String?, source: CLISource) -> CLIResolution? {
            guard let value = value?.trimmingCharacters(in: .whitespaces), !value.isEmpty else { return nil }
            let path = expandingTilde(value, home: home)
            guard path.hasPrefix("/") else {
                rejected.append(RejectedCandidate(path: value, reason: "Not an absolute path (\(describe(source)))"))
                return nil
            }
            return consider(path, source: source, explicit: true)
        }

        if let found = override(processEnvironment[Self.environmentOverrideKey], source: .environmentOverride) {
            return Outcome(resolution: found, searched: searched, rejected: rejected)
        }
        if let found = override(settingsOverride, source: .settingsOverride) {
            return Outcome(resolution: found, searched: searched, rejected: rejected)
        }
        // Relative PATH entries depend on the working directory, which is `/` for an app; skip them.
        let pathDirectories = (searchPath ?? "").split(separator: ":").map(String.init).filter { $0.hasPrefix("/") }
        for directory in pathDirectories {
            if let found = consider(candidate(in: directory), source: .loginShellPath, explicit: false) {
                return Outcome(resolution: found, searched: searched, rejected: rejected)
            }
        }
        for directory in Self.wellKnownDirectories(home: home) {
            if let found = consider(candidate(in: directory), source: .wellKnownPath, explicit: false) {
                return Outcome(resolution: found, searched: searched, rejected: rejected)
            }
        }
        if let bundleURL, bundleURL.pathExtension == "app" {
            let embedded = bundleURL.appendingPathComponent("Contents/Helpers/\(Self.executableName)").path
            if let found = consider(embedded, source: .embedded, explicit: false) {
                return Outcome(resolution: found, searched: searched, rejected: rejected)
            }
        }
        return Outcome(resolution: nil, searched: searched, rejected: rejected)
    }

    /// `locate`, throwing `.cliNotFound` with every searched path when nothing qualified.
    public func resolve(processEnvironment: [String: String], settingsOverride: String?, searchPath: String?,
                        home: String, bundleURL: URL? = Bundle.main.bundleURL) throws -> CLIResolution {
        let outcome = locate(processEnvironment: processEnvironment, settingsOverride: settingsOverride,
                             searchPath: searchPath, home: home, bundleURL: bundleURL)
        guard let resolution = outcome.resolution else { throw BackendError.cliNotFound(searched: outcome.searched) }
        return resolution
    }

    private func candidate(in directory: String) -> String {
        (directory as NSString).appendingPathComponent(Self.executableName)
    }

    private func expandingTilde(_ path: String, home: String) -> String {
        if path == "~" { return home }
        guard path.hasPrefix("~/") else { return path }
        return (home as NSString).appendingPathComponent(String(path.dropFirst(2)))
    }

    private func describe(_ source: CLISource) -> String {
        switch source {
        case .environmentOverride: return "from \(Self.environmentOverrideKey)"
        case .settingsOverride: return "from Settings"
        case .loginShellPath: return "on the login-shell PATH"
        case .wellKnownPath: return "in a well-known directory"
        case .embedded: return "bundled with the app"
        }
    }
}
