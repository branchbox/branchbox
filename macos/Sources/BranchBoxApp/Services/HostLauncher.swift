import AppKit
import BranchBoxKit
import Darwin

/// Why a host launch failed; each case names its cause.
enum HostLaunchError: Error, Sendable, Hashable {
    case disabled(reason: String)
    case folderMissing(path: String)
    case applicationNotFound(String)
    case noApplicationForURL(URL)
    case scriptWriteFailed(path: String, reason: String)
    case openFailed(target: String, reason: String)
    case invalidTerminalTemplate(String)
    case terminalLaunchFailed(command: String, reason: String)

    var message: String {
        switch self {
        case .disabled(let reason): reason
        case .folderMissing(let path): "The folder \(path) is missing"
        case .applicationNotFound(let app): "Couldn't find \(app); is it installed?"
        case .noApplicationForURL(let url): "No app opens \(url.scheme ?? "these")://… links; is the editor installed?"
        case .scriptWriteFailed(let path, let reason): "Couldn't write the launch script \(path): \(reason)"
        case .openFailed(let target, let reason): "Couldn't open \(target): \(reason)"
        case .invalidTerminalTemplate(let reason): "The custom terminal command is invalid: \(reason)"
        case .terminalLaunchFailed(let command, let reason): "Couldn't run “\(command)”: \(reason)"
        }
    }
}

/// The side effects `HostLauncher` needs, so tests can run it without opening apps.
@MainActor protocol HostOpening {
    func applicationURL(forBundleIdentifier bundleID: String) -> URL?
    func applicationURL(toOpen url: URL) -> URL?
    func open(_ urls: [URL], withApplicationAt application: URL) async throws
    func open(_ url: URL) -> Bool
    /// Starts `/bin/sh -c command` without waiting for it (a custom terminal template).
    func runShell(_ command: String) throws
}

/// `NSWorkspace` and `Process`.
@MainActor struct WorkspaceOpener: HostOpening {
    func applicationURL(forBundleIdentifier bundleID: String) -> URL? {
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID)
    }

    func applicationURL(toOpen url: URL) -> URL? {
        NSWorkspace.shared.urlForApplication(toOpen: url)
    }

    func open(_ urls: [URL], withApplicationAt application: URL) async throws {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            NSWorkspace.shared.open(urls, withApplicationAt: application, configuration: configuration) { _, error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume() }
            }
        }
    }

    func open(_ url: URL) -> Bool {
        NSWorkspace.shared.open(url)
    }

    func runShell(_ command: String) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = ["-c", command]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        // The terminal it starts belongs to the user from here on; the app does not wait for or stop it.
        try process.run()
    }
}

/// Executes a `HostLaunchPlan`: opens a folder in an app by bundle id or path, opens a URL, or writes a terminal
/// script to `~/Library/Caches/BranchBox/launch/` (mode 0700, never overwriting) and opens it with Terminal,
/// iTerm or the user's terminal template. Every failure is thrown as a `HostLaunchError`.
@MainActor final class HostLauncher {
    static let terminalBundleID = "com.apple.Terminal"
    static let iTermBundleID = "com.googlecode.iterm2"
    /// Scripts older than this are deleted on the next launch; a terminal has long read its script by then.
    static let scriptLifetime: TimeInterval = 24 * 60 * 60

    static var defaultLaunchDirectory: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("BranchBox", isDirectory: true)
            .appendingPathComponent("launch", isDirectory: true)
    }

    private let opener: any HostOpening
    private let launchDirectory: URL
    private let fileManager: FileManager
    private let now: () -> Date

    init(opener: any HostOpening = WorkspaceOpener(), launchDirectory: URL = HostLauncher.defaultLaunchDirectory,
         fileManager: FileManager = .default, now: @escaping () -> Date = Date.init) {
        self.opener = opener
        self.launchDirectory = launchDirectory
        self.fileManager = fileManager
        self.now = now
    }

    /// Runs `plan`. Throws `HostLaunchError` only.
    func launch(_ plan: HostLaunchPlan) async throws {
        switch plan.kind {
        case .disabled(let reason):
            throw HostLaunchError.disabled(reason: reason)
        case .openFolder(let bundleID, let appPath, let path):
            try await openFolder(path, bundleID: bundleID, appPath: appPath)
        case .openURL(let url):
            guard opener.applicationURL(toOpen: url) != nil else { throw HostLaunchError.noApplicationForURL(url) }
            guard opener.open(url) else { throw HostLaunchError.openFailed(target: url.absoluteString, reason: "macOS refused the link") }
        case .terminalScript(let script, let terminal):
            let scriptURL = try writeScript(script)
            try await openScript(scriptURL, in: terminal, workingDirectory: plan.workingDirectory)
        }
    }

    /// Writes `script` to a new `.command` file at mode 0700 and returns its URL.
    func writeScript(_ script: String) throws -> URL {
        try prepareLaunchDirectory()
        removeExpiredScripts()
        let url = launchDirectory.appendingPathComponent("launch-\(UUID().uuidString).command")
        // O_EXCL|O_NOFOLLOW: never reuse or follow an existing file; the mode is set at creation, so the prompt in
        // the script is never readable by other users, even briefly.
        let descriptor = Darwin.open(url.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o700)
        guard descriptor >= 0 else { throw HostLaunchError.scriptWriteFailed(path: url.path, reason: Self.errnoText()) }
        defer { close(descriptor) }
        guard fchmod(descriptor, 0o700) == 0 else {
            throw HostLaunchError.scriptWriteFailed(path: url.path, reason: Self.errnoText())
        }
        var bytes = Array(script.utf8)[...]
        while !bytes.isEmpty {
            let written = bytes.withUnsafeBytes { write(descriptor, $0.baseAddress, $0.count) }
            if written < 0 {
                if errno == EINTR { continue }
                throw HostLaunchError.scriptWriteFailed(path: url.path, reason: Self.errnoText())
            }
            bytes = bytes.dropFirst(written)
        }
        return url
    }

    private func openFolder(_ path: String, bundleID: String?, appPath: String?) async throws {
        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: path, isDirectory: &isDirectory), isDirectory.boolValue else {
            throw HostLaunchError.folderMissing(path: path)
        }
        let folder = URL(fileURLWithPath: path, isDirectory: true)
        let application: URL
        if let bundleID {
            guard let url = opener.applicationURL(forBundleIdentifier: bundleID) else {
                throw HostLaunchError.applicationNotFound(Self.appName(forBundleID: bundleID))
            }
            application = url
        } else if let appPath {
            guard fileManager.fileExists(atPath: appPath) else { throw HostLaunchError.applicationNotFound(appPath) }
            application = URL(fileURLWithPath: appPath)
        } else {
            guard opener.open(folder) else { throw HostLaunchError.openFailed(target: path, reason: "macOS refused the folder") }
            return
        }
        try await open([folder], with: application, target: path)
    }

    private func openScript(_ script: URL, in terminal: TerminalChoice, workingDirectory: String?) async throws {
        switch terminal {
        case .terminal, .iTerm:
            let bundleID = terminal == .terminal ? Self.terminalBundleID : Self.iTermBundleID
            guard let application = opener.applicationURL(forBundleIdentifier: bundleID) else {
                throw HostLaunchError.applicationNotFound(Self.appName(forBundleID: bundleID))
            }
            try await open([script], with: application, target: script.path)
        case .custom(let template):
            let command = try Self.expand(template: template, script: script.path,
                                          workingDirectory: workingDirectory ?? launchDirectory.path)
            do {
                try opener.runShell(command)
            } catch {
                throw HostLaunchError.terminalLaunchFailed(command: command, reason: error.localizedDescription)
            }
        }
    }

    /// A custom terminal template with `{path}` (the working folder) and `{command}` (the script to run), both
    /// shell-quoted. Without `{command}` the terminal would open but never run the script, so that is refused.
    static func expand(template: String, script: String, workingDirectory: String) throws -> String {
        let trimmed = template.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw HostLaunchError.invalidTerminalTemplate("it is empty") }
        guard trimmed.contains("{command}") else {
            throw HostLaunchError.invalidTerminalTemplate("it must contain {command}, the script to run")
        }
        // One pass: a substituted value is never scanned again, so a folder named `{command}` stays a folder.
        let values = ["{path}": HostLaunchPlan.shellQuote(workingDirectory), "{command}": HostLaunchPlan.shellQuote(script)]
        var expanded = ""
        var rest = Substring(trimmed)
        while !rest.isEmpty {
            if let (token, value) = values.first(where: { rest.hasPrefix($0.key) }) {
                expanded += value
                rest = rest.dropFirst(token.count)
            } else {
                expanded.append(rest.removeFirst())
            }
        }
        return expanded
    }

    private func open(_ urls: [URL], with application: URL, target: String) async throws {
        do {
            try await opener.open(urls, withApplicationAt: application)
        } catch {
            throw HostLaunchError.openFailed(target: target, reason: error.localizedDescription)
        }
    }

    private func prepareLaunchDirectory() throws {
        do {
            try fileManager.createDirectory(at: launchDirectory, withIntermediateDirectories: true,
                                            attributes: [.posixPermissions: 0o700])
            try fileManager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: launchDirectory.path)
        } catch {
            throw HostLaunchError.scriptWriteFailed(path: launchDirectory.path, reason: error.localizedDescription)
        }
    }

    /// Deletes this folder's scripts older than `scriptLifetime`. Best effort by design: a leftover script is
    /// harmless, and a failed cleanup must not stop the launch the user asked for.
    private func removeExpiredScripts() {
        let cutoff = now().addingTimeInterval(-Self.scriptLifetime)
        let names: [String]
        do {
            names = try fileManager.contentsOfDirectory(atPath: launchDirectory.path)
        } catch {
            return
        }
        for name in names where name.hasPrefix("launch-") && name.hasSuffix(".command") {
            let path = launchDirectory.appendingPathComponent(name).path
            do {
                let attributes = try fileManager.attributesOfItem(atPath: path)
                if let modified = attributes[.modificationDate] as? Date, modified < cutoff {
                    try fileManager.removeItem(atPath: path)
                }
            } catch {
                continue
            }
        }
    }

    static func appName(forBundleID bundleID: String) -> String {
        switch bundleID {
        case HostLaunchPlan.vscodeBundleID: "Visual Studio Code"
        case HostLaunchPlan.cursorBundleID: "Cursor"
        case terminalBundleID: "Terminal"
        case iTermBundleID: "iTerm"
        default: bundleID
        }
    }

    private static func errnoText() -> String {
        String(cString: strerror(errno))
    }
}
