import Foundation

public struct Diagnostics: Sendable, Hashable {
    public var summary: String                       // the CLI's own cause line (or envelope message); never raw stderr
    public var causes: [String]                      // anyhow "Caused by:" / envelope causes
    public var exitCode: Int32?
    public var signal: Int32?
    public var logTail: [String]                     // last ≤ 50 ANSI-stripped stderr lines
    public var invocation: String?                   // redacted argv (prompt/env/token values removed; args > 200 chars truncated)
    public var cliVersion: String?
    /// Most stderr lines kept in `logTail`.
    public static let logTailLimit = 50
    public init(summary: String, causes: [String] = [], exitCode: Int32? = nil, signal: Int32? = nil,
                logTail: [String] = [], invocation: String? = nil, cliVersion: String? = nil) {
        self.summary = summary
        self.causes = causes
        self.exitCode = exitCode
        self.signal = signal
        self.logTail = Array(logTail.suffix(Diagnostics.logTailLimit))
        self.invocation = invocation
        self.cliVersion = cliVersion
    }
}

public struct ChangedFile: Sendable, Hashable, Codable {
    public let path: String
    public let kind: String                          // untracked|modified|added|deleted|typechange|conflicted|staged
    public let area: String                          // devcontainer|compose|vscode|spec|env|other
    public init(path: String, kind: String, area: String) {
        self.path = path
        self.kind = kind
        self.area = area
    }

    private enum CodingKeys: String, CodingKey { case path, kind, area }

    /// `path` is the identity key (R-2); a missing classification reads as an unclassified change.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        path = try c.decode(String.self, forKey: .path)
        kind = c.lenient(String.self, forKey: .kind) ?? ""
        area = c.lenient(String.self, forKey: .area) ?? "other"
    }
}

public enum RefusalCause: Sendable, Hashable {
    case uncommittedChanges(files: [ChangedFile])
    case moduleFilesDirty(files: [String], userChanges: [ChangedFile])   // 0.13.x banner; userChanges = re-preflight result
    case unmergedBranch(branch: String, ahead: Int?)
    case worktreeLocked(reason: String?)
    case statusUnavailable(cause: String)
    case worktreeRemovalFailed(cause: String)
    case worktreeExists(path: String)
    case worktreeNotFound(String)
    case featureNotFound(String)
    case branchExists(String)
    case invalidName(String)
    case notGitRepository(String)
    case runtimePrerequisite(provider: String, detail: String)   // "Sign in with: sbx login", "local-vm requires a Linux host"
    case registryLocked(path: String)
    case confirmationRequired
    case configInvalid(key: String?, detail: String)
    case devcontainerSourceMissing
    case other(code: String)
}

public struct Refusal: Sendable, Hashable {
    public let cause: RefusalCause
    public let message: String                       // cause-naming text shown to the user
    public let diagnostics: Diagnostics
    public let plan: TeardownPlanDocument?           // present for teardown refusals (0.14 details.plan or app preflight)
    public init(cause: RefusalCause, message: String, diagnostics: Diagnostics, plan: TeardownPlanDocument? = nil) {
        self.cause = cause
        self.message = message
        self.diagnostics = diagnostics
        self.plan = plan
    }
}

public struct PartialFailure: Sendable, Hashable {
    public let completed: [String]                   // e.g. ["Worktree removed"]
    public let remaining: Refusal
    public init(completed: [String], remaining: Refusal) {
        self.completed = completed
        self.remaining = remaining
    }
}

public enum ProjectProblem: Sendable, Hashable {
    case missing(String)
    case notGitRepository(String)
    case notInitialized(String)
    case workingDirectoryMissing(String)
}

public enum BackendError: Error, Sendable, Hashable {
    case cliNotFound(searched: [String])
    case cliTooOld(found: SemVer, minimum: SemVer, path: String)
    case cliUnusable(path: String, reason: String)
    case launchFailed(executable: String, reason: String)
    case projectInvalid(ProjectProblem)
    case refused(Refusal)
    case partial(PartialFailure)
    case commandFailed(Diagnostics)
    case decodeFailed(what: String, detail: String, diagnostics: Diagnostics)
    case registryCorrupted(path: String, diagnostics: Diagnostics)
    case unsupported(Capability, minimumCLI: String)        // e.g. (.config, "0.14.0")
    case timedOut(operation: String, after: Duration, diagnostics: Diagnostics)
    case cancelled(note: String?)                            // e.g. "may have left a partial worktree"
    /// CancellationError → .cancelled(nil); BackendError passthrough; anything else → .commandFailed.
    ///
    /// `ProcessRunError` is mapped case by case so a cancelled or timed-out process keeps its meaning
    /// even when a caller lets the runner's error escape unclassified.
    public static func normalize(_ error: any Error) -> BackendError {
        switch error {
        case let error as BackendError:
            return error
        case is CancellationError:
            return .cancelled(note: nil)
        case let error as ProcessRunError:
            return normalize(error)
        default:
            return .commandFailed(Diagnostics(summary: String(describing: error)))
        }
    }

    private static func normalize(_ error: ProcessRunError) -> BackendError {
        switch error {
        case .cancelled:
            return .cancelled(note: nil)
        case .workingDirectoryMissing(let path):
            return .projectInvalid(.workingDirectoryMissing(path))
        case .launchFailed(let executable, let reason):
            return .launchFailed(executable: executable, reason: reason)
        case .stdoutTooLarge(let limit):
            return .commandFailed(Diagnostics(summary: "The command printed more than \(limit) bytes on stdout"))
        case .timedOut(let after, let partial):
            let diagnostics = Diagnostics(summary: "The command did not finish within \(after)",
                                          logTail: partial.stderrTail)
            return .timedOut(operation: "command", after: after, diagnostics: diagnostics)
        }
    }
}
