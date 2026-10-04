import BranchBoxKit
import Foundation

/// The redacted Markdown behind [Copy diagnostic report] (§9.4): app version and SHA, the CLI's path, source,
/// version and capabilities, the child PATH, and for a failure its redacted argv, exit or signal, and the last 50
/// stderr lines. Secrets never reach it: token-shaped values, prompt arguments and the user's extra environment
/// values are replaced, and the home folder is written as `~`.
struct DiagnosticReport: Sendable, Hashable {
    struct AppInfo: Sendable, Hashable {
        let version: String
        let build: String?
        let gitSHA: String?

        /// The bundle's `CFBundleShortVersionString`, `CFBundleVersion` and `BranchBoxGitSHA` (set by the packaging
        /// script); "development" under `swift run`, which has no Info.plist.
        static func current(bundle: Bundle = .main) -> AppInfo {
            let info = bundle.infoDictionary ?? [:]
            let nonEmpty: (String) -> String? = { key in (info[key] as? String).flatMap { $0.isEmpty ? nil : $0 } }
            return AppInfo(version: nonEmpty("CFBundleShortVersionString") ?? "development", build: nonEmpty("CFBundleVersion"),
                           gitSHA: nonEmpty("BranchBoxGitSHA"))
        }
    }

    var app: AppInfo
    var osVersion: String
    var identity: BackendIdentity?
    var environment: EnvironmentSummary?
    var operationTitle: String?
    var error: BackendError?
    var context: OperationRequestContext?
    var generatedAt: Date
    /// Values to scrub wherever they appear, e.g. Settings › Extra environment values.
    var secrets: [String]
    var homeDirectory: String?

    init(app: AppInfo = .current(), osVersion: String = ProcessInfo.processInfo.operatingSystemVersionString,
         identity: BackendIdentity?, environment: EnvironmentSummary? = nil, operationTitle: String? = nil,
         error: BackendError? = nil, context: OperationRequestContext? = nil, generatedAt: Date = .now,
         secrets: [String] = [], homeDirectory: String? = NSHomeDirectory()) {
        self.app = app
        self.osVersion = osVersion
        self.identity = identity
        self.environment = environment
        self.operationTitle = operationTitle
        self.error = error
        self.context = context
        self.generatedAt = generatedAt
        self.secrets = secrets
        self.homeDirectory = homeDirectory
    }

    func markdown() -> String {
        var lines = ["# BranchBox diagnostic report", ""]
        lines.append("- Generated: \(generatedAt.formatted(Date.ISO8601FormatStyle()))")
        let build = [app.build.map { "build \($0)" }, app.gitSHA.map { "SHA \($0)" }].compactMap { $0 }
        lines.append("- App: BranchBox for Mac \(app.version)" + (build.isEmpty ? "" : " (\(build.joined(separator: ", ")))"))
        lines.append("- macOS: \(osVersion)")
        lines += ["", "## CLI", ""] + cliLines()
        lines += ["", "## Environment", ""] + environmentLines()
        if let error {
            lines += ["", "## Failure", ""] + failureLines(error)
        }
        return redact(lines.joined(separator: "\n") + "\n")
    }

    private func cliLines() -> [String] {
        guard let identity else { return ["- Backend: unavailable"] }
        var lines: [String] = []
        switch identity.kind {
        case .cli(let resolution):
            lines.append("- Path: \(resolution.path)")
            lines.append("- Source: \(Self.sourceLabel(resolution.source))")
            for rejected in resolution.rejected {
                lines.append("- Rejected: \(rejected.path) (\(rejected.reason))")
            }
        case .agent(let endpoint):
            lines.append("- Backend: agent at \(endpoint)")
        case .preview:
            lines.append("- Backend: preview (no CLI)")
        }
        lines.append("- Version: \(identity.version) (minimum \(BackendIdentity.minimumCLI))")
        lines.append("- Contract version: " + (identity.contractVersion.map(String.init) ?? "none (legacy CLI)"))
        let capabilities = identity.capabilities.map(\.rawValue).sorted()
        lines.append("- Capabilities: " + (capabilities.isEmpty ? "none" : capabilities.joined(separator: ", ")))
        return lines
    }

    private func environmentLines() -> [String] {
        guard let environment else { return ["- Not captured yet"] }
        var lines = ["- Source: \(environment.source.rawValue)" + (environment.isProvisional ? " (provisional)" : "")]
        if let shell = environment.shell { lines.append("- Shell: \(shell)") }
        if let duration = environment.captureDuration {
            lines.append("- Capture took: \(duration.formatted(.units(allowed: [.seconds, .milliseconds], width: .narrow)))")
        }
        lines.append("- Child PATH: \(environment.pathEntries.joined(separator: ":"))")
        return lines
    }

    private func failureLines(_ error: BackendError) -> [String] {
        let presentation = error.presentation(context: context)
        var lines: [String] = []
        if let operationTitle { lines.append("- Operation: \(operationTitle)") }
        lines.append("- Error: \(presentation.title)")
        lines.append("- Message: \(presentation.message)")
        if !presentation.completed.isEmpty { lines.append("- Completed: \(presentation.completed.joined(separator: "; "))") }
        guard let diagnostics = presentation.diagnostics else {
            lines += presentation.details.map { "- Detail: \($0)" }
            return lines
        }
        if diagnostics.summary != presentation.message { lines.append("- Summary: \(diagnostics.summary)") }
        lines += diagnostics.causes.map { "- Caused by: \($0)" }
        if let invocation = diagnostics.invocation { lines.append("- Invocation: `\(invocation)`") }
        if let exitCode = diagnostics.exitCode { lines.append("- Exit code: \(exitCode)") }
        if let signal = diagnostics.signal { lines.append("- Signal: \(signal)") }
        if let cliVersion = diagnostics.cliVersion { lines.append("- CLI version: \(cliVersion)") }
        let tail = diagnostics.logTail.suffix(Diagnostics.logTailLimit)
        if !tail.isEmpty {
            let fence = tail.contains { $0.contains("```") } ? "~~~~" : "```"
            lines += ["", "Last \(tail.count) stderr lines:", "", fence] + tail + [fence]
        }
        return lines
    }

    /// Scrubs the configured secrets, token-shaped values and prompt arguments, then shortens the home folder.
    func redact(_ text: String) -> String {
        var result = text
        for secret in secrets where secret.count >= 4 {
            result = result.replacingOccurrences(of: secret, with: "<redacted>")
        }
        for (pattern, template) in Self.redactions {
            let regex: NSRegularExpression
            do {
                regex = try NSRegularExpression(pattern: pattern)
            } catch {
                // The patterns are literals covered by PresentationTests; a typo there is a programming error.
                preconditionFailure("Bad redaction pattern \(pattern): \(error)")
            }
            result = regex.stringByReplacingMatches(in: result, range: NSRange(result.startIndex..., in: result),
                                                    withTemplate: template)
        }
        if let home = homeDirectory, home.count > 1 {
            result = result.replacingOccurrences(of: home, with: "~")
        }
        return result
    }

    /// (pattern, replacement template) pairs, compiled when a report is made.
    private static let redactions: [(String, String)] = [
        // `--prompt 'text'`, `--prompt=text`: SW-1 already redacts argv; this is the backstop.
        (#"(--prompt[= ])('[^']*'|"[^"]*"|\S+)"#, "$1'<redacted>'"),
        // KEY=value and `key: value` where the key names a secret.
        (#"(?i)\b([A-Z0-9_]*(TOKEN|SECRET|PASSWORD|PASSWD|API_?KEY|PRIVATE_KEY|CREDENTIALS?)[A-Z0-9_]*)(\s*[=:]\s*)("[^"]*"|'[^']*'|\S+)"#,
         "$1$3<redacted>"),
        (#"(?i)\b(bearer|token)\s+[A-Za-z0-9._~+/=-]{8,}"#, "$1 <redacted>"),
        (#"\bgh[pousr]_[A-Za-z0-9]{20,}\b"#, "<redacted>"),
        (#"\bgithub_pat_[A-Za-z0-9_]{20,}\b"#, "<redacted>"),
        (#"\b(sk|xox[abprs])-[A-Za-z0-9-]{16,}\b"#, "<redacted>"),
    ]

    static func sourceLabel(_ source: CLISource) -> String {
        switch source {
        case .environmentOverride: "BRANCHBOX_CLI_PATH"
        case .settingsOverride: "Settings override"
        case .loginShellPath: "login-shell PATH"
        case .wellKnownPath: "well-known location"
        case .embedded: "embedded in the app"
        }
    }
}
