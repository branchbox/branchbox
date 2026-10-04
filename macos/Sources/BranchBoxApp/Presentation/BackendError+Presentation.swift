import BranchBoxKit
import Foundation

/// How a `BackendError` is shown (DESIGN §9.4): where it goes, its cause-naming title and message, the files or
/// causes behind it, and whether a diagnostic report helps.
struct ErrorPresentation: Sendable, Hashable {
    enum Style: Sendable, Hashable {
        case blocking       // the CLI is missing, too old or unusable: EnvironmentGate's ContentUnavailableView
        case refusal        // a card with the cause, its files and its recoveries
        case partial        // orange card: completed steps plus the remaining refusal
        case failure        // red card: summary, causes, Show Log, Copy diagnostic report
        case unsupported    // a disabled control explaining which CLI it needs
        case neutral        // grey card: cancelled or timed out
    }

    let style: Style
    let title: String
    let message: String
    /// File paths ("README.md (modified)") for refusals about files, else the CLI's "Caused by" lines.
    let details: [String]
    /// Steps that did finish before a partial failure.
    let completed: [String]
    let diagnostics: Diagnostics?

    /// The red failure card offers a diagnostic report; so does any error that carries a CLI invocation.
    var offersDiagnosticReport: Bool { style == .failure || diagnostics?.invocation != nil }

    var symbol: String {
        switch style {
        case .blocking: "exclamationmark.triangle.fill"
        case .refusal: "hand.raised.fill"
        case .partial: "exclamationmark.circle.fill"
        case .failure: "xmark.octagon.fill"
        case .unsupported: "arrow.up.circle"
        case .neutral: "stop.circle"
        }
    }

    var tint: StatusTint {
        switch style {
        case .blocking, .failure: .red
        case .refusal, .partial: .orange
        case .unsupported, .neutral: .gray
        }
    }
}

extension BackendError {
    /// The presentation of this error; `context` (the request that failed) sharpens refusal titles.
    func presentation(context: OperationRequestContext? = nil) -> ErrorPresentation {
        switch self {
        case .cliNotFound(let searched):
            let looked = searched.isEmpty ? "" : " BranchBox looked in: \(searched.joined(separator: ", "))."
            return ErrorPresentation(style: .blocking, title: "BranchBox CLI not found",
                                     message: "Install the BranchBox CLI with Homebrew, or locate it.\(looked)",
                                     details: [], completed: [], diagnostics: nil)
        case .cliTooOld(let found, let minimum, let path):
            return ErrorPresentation(style: .blocking, title: "BranchBox CLI is too old",
                                     message: "\(path) is version \(found); BranchBox for Mac needs \(minimum) or later.",
                                     details: [], completed: [], diagnostics: nil)
        case .cliUnusable(let path, let reason):
            let place = path.isEmpty ? "The BranchBox CLI" : path
            return ErrorPresentation(style: .blocking, title: "BranchBox CLI can't be used", message: "\(place): \(reason)",
                                     details: [], completed: [], diagnostics: nil)
        case .launchFailed(let executable, let reason):
            let name = URL(fileURLWithPath: executable).lastPathComponent
            return ErrorPresentation(style: .failure, title: "Couldn't run \(name)", message: "\(executable): \(reason)",
                                     details: [], completed: [], diagnostics: nil)
        case .projectInvalid(let problem):
            return Self.presentation(of: problem)
        case .refused(let refusal):
            return ErrorPresentation(style: .refusal, title: Self.title(of: refusal.cause, context: context),
                                     message: Self.message(of: refusal, context: context), details: Self.details(of: refusal),
                                     completed: [], diagnostics: refusal.diagnostics)
        case .partial(let partial):
            let title = switch Self.reason(of: partial.remaining.cause) {
            case .stopped(let phrase): "Partly done: \(phrase)"
            case .plain(let text): "Partly done. \(text)"
            }
            return ErrorPresentation(style: .partial, title: title,
                                     message: partial.remaining.message, details: Self.details(of: partial.remaining),
                                     completed: partial.completed, diagnostics: partial.remaining.diagnostics)
        case .commandFailed(let diagnostics):
            return ErrorPresentation(style: .failure, title: "The command failed", message: diagnostics.summary,
                                     details: diagnostics.causes, completed: [], diagnostics: diagnostics)
        case .decodeFailed(let what, let detail, let diagnostics):
            return ErrorPresentation(style: .failure, title: "Couldn't read the CLI's output",
                                     message: "The \(what) output couldn't be decoded: \(detail)",
                                     details: diagnostics.causes, completed: [], diagnostics: diagnostics)
        case .registryCorrupted(let path, let diagnostics):
            return ErrorPresentation(style: .failure, title: "The feature registry is damaged",
                                     message: "\(path) can't be read, so every feature in this project is hidden until it is repaired.",
                                     details: diagnostics.causes, completed: [], diagnostics: diagnostics)
        case .unsupported(let capability, let minimumCLI):
            return ErrorPresentation(style: .unsupported, title: "Needs a newer BranchBox CLI",
                                     message: "\(Self.unsupportedHelp(capability)) (\(minimumCLI) or later).",
                                     details: [], completed: [], diagnostics: nil)
        case .timedOut(let operation, let after, let diagnostics):
            return ErrorPresentation(style: .neutral, title: "Timed out",
                                     message: "The \(operation) didn't finish within \(OperationPresentation.elapsed(after)).",
                                     details: diagnostics.causes, completed: [], diagnostics: diagnostics)
        case .cancelled(let note):
            return ErrorPresentation(style: .neutral, title: "Cancelled", message: note ?? "The operation was stopped.",
                                     details: [], completed: [], diagnostics: nil)
        }
    }

    /// Whether trying the same thing again can succeed without the user changing anything first: a command that
    /// failed or timed out, a CLI that could not launch, unreadable output, or a registry another process holds.
    /// Only these get a generic [Retry]; refusals and partial failures get their `RecoveryPlanner` recoveries
    /// instead, because repeating the request would be refused again (additive, SW-4).
    var isTransient: Bool {
        switch self {
        case .commandFailed, .timedOut, .launchFailed, .decodeFailed:
            true
        case .refused(let refusal):
            if case .registryLocked = refusal.cause { true } else { false }
        case .cliNotFound, .cliTooOld, .cliUnusable, .projectInvalid, .partial, .registryCorrupted, .unsupported, .cancelled:
            false
        }
    }

    /// `.help(…)` text for a control disabled by `.unsupported`.
    static func unsupportedHelp(_ capability: Capability) -> String {
        "Requires BranchBox CLI with \(capability.displayName)"
    }

    /// A cause-naming title: "Teardown stopped: uncommitted changes would be lost".
    static func title(of cause: RefusalCause, context: OperationRequestContext?) -> String {
        switch reason(of: cause) {
        case .stopped(let phrase): "\(verb(for: context)) stopped: \(phrase)"
        case .plain(let text): text
        }
    }

    /// Why a refusal stopped: a phrase completing "<Operation> stopped: …", or a title of its own.
    enum RefusalReason: Equatable {
        case stopped(String)
        case plain(String)
    }

    static func reason(of cause: RefusalCause) -> RefusalReason {
        switch cause {
        case .uncommittedChanges: .stopped("uncommitted changes would be lost")
        case .moduleFilesDirty(_, let userChanges):
            .stopped(userChanges.isEmpty ? "BranchBox-generated files changed" : "uncommitted changes would be lost")
        case .unmergedBranch(let branch, _): .stopped("\(branch) has unmerged commits")
        case .worktreeLocked: .stopped("the worktree is locked")
        case .statusUnavailable: .stopped("couldn't check for unsaved work")
        case .confirmationRequired: .stopped("confirmation required")
        case .other(let code): .stopped(code.isEmpty ? "the CLI refused" : "the CLI refused (\(code))")
        case .worktreeRemovalFailed: .plain("Couldn't remove the worktree")
        case .worktreeExists(let path): .plain("The folder \(URL(fileURLWithPath: path).lastPathComponent) already exists")
        case .worktreeNotFound(let name): .plain("The worktree for \(name) is gone")
        case .featureNotFound(let name): .plain("No feature named \(name)")
        case .branchExists(let branch): .plain("Branch \(branch) already exists")
        case .invalidName(let name): .plain("“\(name)” isn't a valid feature name")
        case .notGitRepository: .plain("Not a Git repository")
        case .runtimePrerequisite(let provider, _): .plain("\(RuntimeProvider(raw: provider).label) isn't ready")
        case .registryLocked: .plain("Another BranchBox process is updating this project")
        case .configInvalid(let key, _): .plain(key.map { "Invalid setting: \($0)" } ?? "Invalid project settings")
        case .devcontainerSourceMissing: .plain("The project has no .devcontainer folder")
        }
    }

    private static func verb(for context: OperationRequestContext?) -> String {
        switch context {
        case .start?: "Start"
        case .removeStray?: "Removal"
        case .deleteBranch?: "Branch deletion"
        case .exec?: "Command"
        case .devcontainer?: "Dev container"
        case .syncDevcontainers?: "Update"
        case .tunnelOpen?, .tunnelRemove?, .tunnelCredentials?: "Sharing"
        case .initProject?: "Setup"
        case .applyConfig?: "Saving settings"
        case .teardown?, .prune?, nil: "Teardown"
        }
    }

    /// The card's sentence. When the refusal lists files, the app says what happened and introduces the list in
    /// its own words, instead of the CLI's text (which repeats the files, says "tear down" for a stray, and
    /// writes "change(s)"); that text stays in the log. Other refusals keep the CLI's message.
    static func message(of refusal: Refusal, context: OperationRequestContext?) -> String {
        let files: Int
        let generatedOnly: Bool
        switch refusal.cause {
        case .uncommittedChanges(let changed) where !changed.isEmpty:
            files = changed.count
            generatedOnly = false
        case .moduleFilesDirty(let generated, let userChanges) where !(generated.isEmpty && userChanges.isEmpty):
            files = userChanges.isEmpty ? generated.count : userChanges.count + generated.count
            generatedOnly = userChanges.isEmpty
        default:
            return refusal.message
        }
        let list = switch (generatedOnly, files == 1) {
        case (true, true): "This file that BranchBox generated was changed:"
        case (true, false): "These files that BranchBox generated were changed:"
        case (false, true): "This file has changes that aren't committed:"
        case (false, false): "These files have changes that aren't committed:"
        }
        return "\(nothingDone(context)) \(list)"
    }

    /// "Nothing was removed." for teardowns, prunes and stray removals; what else stayed as it was otherwise.
    private static func nothingDone(_ context: OperationRequestContext?) -> String {
        switch context {
        case .teardown?, .prune?, .removeStray?, nil: "Nothing was removed."
        case .deleteBranch?: "The branch wasn't deleted."
        default: "Nothing was changed."
        }
    }

    private static func details(of refusal: Refusal) -> [String] {
        switch refusal.cause {
        case .uncommittedChanges(let files): return files.map(fileLine)
        case .moduleFilesDirty(let generated, let userChanges): return userChanges.map(fileLine) + generated
        case .runtimePrerequisite(_, let detail): return [detail] + refusal.diagnostics.causes
        case .configInvalid(_, let detail): return [detail] + refusal.diagnostics.causes
        default: return refusal.diagnostics.causes
        }
    }

    private static func fileLine(_ file: ChangedFile) -> String {
        file.kind.isEmpty ? file.path : "\(file.path) (\(file.kind))"
    }

    private static func presentation(of problem: ProjectProblem) -> ErrorPresentation {
        let (title, message): (String, String) = switch problem {
        case .missing(let path): ("The project folder is missing", "\(path) no longer exists. Locate it or remove the project.")
        case .notGitRepository(let path): ("Not a Git repository", "\(path) isn't inside a Git repository.")
        case .notInitialized(let path): ("BranchBox isn't set up here", "\(path) has no .branchbox folder yet. Set up BranchBox first.")
        case .workingDirectoryMissing(let path): ("A folder is missing", "\(path) no longer exists.")
        }
        return ErrorPresentation(style: .failure, title: title, message: message, details: [], completed: [], diagnostics: nil)
    }
}

extension Capability {
    /// What the capability lets the app do, for "Requires BranchBox CLI with …".
    var displayName: String {
        switch self {
        case .jsonErrorEnvelope: "structured errors"
        case .registryLock: "registry locking"
        case .writeAheadStart: "interrupted-start tracking"
        case .teardownPlan: "teardown previews"
        case .teardownDiscardChanges: "safe discarding of changes"
        case .teardownUnmergedPreflight: "unmerged-branch checks"
        case .pruneJSON: "prune previews"
        case .detectJSON: "project detection"
        case .devcontainerSyncJSON: "per-feature workspace updates"
        case .config: "config editing"
        case .tunnelCredentials: "tunnel credentials"
        case .doctor: "doctor checks"
        case .initJSON: "1Password setup"
        default: "“\(rawValue)”"
        }
    }
}

extension RecoveryAction {
    /// The button title.
    var title: String {
        switch self {
        case .retry(_, let label, _, _), .runInTerminal(_, _, let label), .copyCommand(_, let label): label
        case .revealInFinder: "Reveal in Finder"
        case .openDoctor: "Open Diagnostics"
        case .locateCLI: "Locate…"
        case .refresh: "Refresh"
        case .showLog: "Show Log"
        }
    }

    /// Destructive recoveries are confirmed first and styled as destructive.
    var isDestructive: Bool {
        if case .retry(_, _, let destructive, _) = self { return destructive }
        return false
    }

    /// What the confirmation says is lost; a destructive retry without its own text still gets one.
    var confirmationMessage: String? {
        guard case .retry(_, _, let destructive, let confirmation) = self, destructive else { return nil }
        return confirmation ?? "This can't be undone."
    }

    /// The confirming button: the title without its trailing ellipsis.
    var confirmationTitle: String {
        title.hasSuffix("…") ? String(title.dropLast()) : title
    }
}
