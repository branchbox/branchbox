import BranchBoxKit
import Foundation

/// Turns a failed CLI run into a `BackendError` (DESIGN §6.3 steps 3, 5 and 6, §6.4):
///
/// - An error envelope on stdout wins (`classify(envelope:…)`), mapped by `error.code`.
/// - Otherwise the anyhow block on stderr (the last `Error:` line and its `Caused by:` list) is matched against the
///   0.13.4 known-message table (`classify(stderr:…)`).
/// - Anything else is `.commandFailed`, whose summary is the CLI's `Error:` message — never the INFO log.
///
/// Summaries drop the `Error: ` prefix, so a legacy CLI's text and a contract CLI's envelope read the same.
public enum CLIErrorClassifier {
    public struct Context: Sendable {
        /// `<root>/.branchbox/registry.json`, named by `.registryCorrupted` (the CLI's message does not name it).
        public var registryPath: String?
        public init(registryPath: String? = nil) { self.registryPath = registryPath }
    }

    // MARK: - Envelope (§6.4)

    public static func classify(envelope: ErrorEnvelope, diagnostics base: Diagnostics,
                                context: Context = Context()) -> BackendError {
        let body = envelope.error
        var diagnostics = base
        diagnostics.summary = body.message.isEmpty ? body.code : body.message
        diagnostics.causes = body.causes
        let details = body.details?.objectValue ?? [:]
        func refused(_ cause: RefusalCause, plan: TeardownPlanDocument? = nil) -> BackendError {
            .refused(Refusal(cause: cause, message: diagnostics.summary, diagnostics: diagnostics, plan: plan))
        }
        let text = ([body.message] + body.causes).joined(separator: "\n")

        if isRegistryParseError(text) {
            return .registryCorrupted(path: context.registryPath ?? "registry.json", diagnostics: diagnostics)
        }
        switch body.code {
        case "teardown_refused":
            return teardownRefusal(details: details, diagnostics: diagnostics)
        case "worktree_not_found":
            return refused(.worktreeNotFound(details["name"]?.stringValue ?? details["path"]?.stringValue
                                             ?? diagnostics.summary))
        case "feature_not_found":
            return refused(.featureNotFound(details["name"]?.stringValue ?? diagnostics.summary))
        case "invalid_feature_name":
            return refused(.invalidName(details["name"]?.stringValue ?? diagnostics.summary))
        case "worktree_exists":
            return refused(.worktreeExists(path: details["path"]?.stringValue ?? diagnostics.summary))
        case "branch_exists":
            return refused(.branchExists(details["branch"]?.stringValue ?? diagnostics.summary))
        case "not_a_git_repository":
            return refused(.notGitRepository(details["path"]?.stringValue ?? diagnostics.summary))
        case "registry_locked":
            return refused(.registryLocked(path: details["path"]?.stringValue ?? context.registryPath ?? ""))
        case "confirmation_required":
            return refused(.confirmationRequired)
        case "config_invalid", "config_unknown_key":
            return refused(.configInvalid(key: details["key"]?.stringValue, detail: diagnostics.summary))
        case "devcontainer_source_missing":
            return refused(.devcontainerSourceMissing)
        case "validation_failed":
            if let cause = runtimePrerequisite(in: text) { return refused(cause) }
            return refused(.other(code: body.code))
        default:
            // git_failed, io_error, command_failed, module_failed, internal, internal_panic, and codes this app
            // does not know: failures, not refusals.
            return .commandFailed(diagnostics)
        }
    }

    /// `teardown_refused`: the cause comes from the plan's first blocker. A null plan is the legacy dirty-module
    /// refusal (`details.files`). `changed_anything` means the run stopped part-way, so it is a partial failure.
    static func teardownRefusal(details: [String: JSONValue], diagnostics: Diagnostics) -> BackendError {
        let plan = details["plan"].flatMap(decodePlan)
        let cause: RefusalCause
        if let plan {
            cause = teardownCause(plan: plan)
        } else {
            let files = details["files"]?.arrayValue?.compactMap(\.stringValue) ?? []
            cause = .moduleFilesDirty(files: files, userChanges: [])
        }
        let refusal = Refusal(cause: cause, message: diagnostics.summary, diagnostics: diagnostics, plan: plan)
        if details["changed_anything"]?.boolValue == true {
            let completed = details["completed_steps"]?.arrayValue?.compactMap(\.stringValue) ?? []
            return .partial(PartialFailure(completed: completed, remaining: refusal))
        }
        return .refused(refusal)
    }

    /// The `RefusalCause` a plan's first blocker names.
    public static func teardownCause(plan: TeardownPlanDocument) -> RefusalCause {
        guard let blocker = plan.blockers.first else { return .other(code: "teardown_refused") }
        switch blocker.kind {
        case "uncommitted_changes":
            return .uncommittedChanges(files: plan.changes.user)
        case "unmerged_branch":
            return .unmergedBranch(branch: blocker.branch ?? plan.branch?.name ?? "", ahead: blocker.ahead)
        case "worktree_locked":
            return .worktreeLocked(reason: plan.worktree.lockReason)
        case "status_unavailable":
            return .statusUnavailable(cause: blocker.cause ?? blocker.message)
        case "worktree_removal_failed":
            return .worktreeRemovalFailed(cause: blocker.cause ?? blocker.message)
        default:
            return .other(code: blocker.kind)
        }
    }

    private static func decodePlan(_ value: JSONValue) -> TeardownPlanDocument? {
        guard case .object = value, let data = try? JSONEncoder().encode(value) else { return nil }
        return try? CLIJSON.decoder().decode(TeardownPlanDocument.self, from: data)
    }

    // MARK: - Text (§6.3 step 5)

    /// Classifies a failed run without an envelope from its stderr (the anyhow block) and stdout (the 0.13.x
    /// dirty-module banner). `diagnostics.summary` becomes the `Error:` message.
    public static func classify(stderr: [String], stdout: String, diagnostics base: Diagnostics,
                                context: Context = Context()) -> BackendError {
        var diagnostics = base
        if let block = anyhowBlock(in: stderr) {
            diagnostics.summary = block.summary
            diagnostics.causes = block.causes
        }
        let text = ([diagnostics.summary] + diagnostics.causes).joined(separator: "\n")
        func refused(_ cause: RefusalCause) -> BackendError {
            .refused(Refusal(cause: cause, message: diagnostics.summary, diagnostics: diagnostics))
        }

        if text.contains("Devcontainer/compose changes detected") {
            let files = TextParsers.dirtyBannerPaths(stdout + "\n" + stderr.joined(separator: "\n"))
            return refused(.moduleFilesDirty(files: files, userChanges: []))
        }
        if let branch = capture(after: "Branch '", until: "' could not be deleted without force", in: text) {
            let remaining = Refusal(cause: .unmergedBranch(branch: branch, ahead: nil), message: diagnostics.summary,
                                    diagnostics: diagnostics)
            return .partial(PartialFailure(completed: ["Worktree removed"], remaining: remaining))
        }
        if isRegistryParseError(text) {
            return .registryCorrupted(path: context.registryPath ?? "registry.json", diagnostics: diagnostics)
        }
        if let path = value(after: "Not a git repository: ", in: text) { return refused(.notGitRepository(path)) }
        if let path = value(after: "Worktree already exists at: ", in: text) { return refused(.worktreeExists(path: path)) }
        if let name = value(after: "Worktree not found: ", in: text) { return refused(.worktreeNotFound(name)) }
        if text.contains("Invalid feature name") {
            return refused(.invalidName(value(after: "Invalid feature name: ", in: text) ?? ""))
        }
        if text.contains("Branch already exists") {
            return refused(.branchExists(value(after: "Branch already exists: ", in: text) ?? ""))
        }
        if text.contains("Refusing to prune in non-interactive mode") { return refused(.confirmationRequired) }
        if let cause = runtimePrerequisite(in: text) { return refused(cause) }
        if text.contains("Devcontainer directory not found") { return refused(.devcontainerSourceMissing) }
        return .commandFailed(diagnostics)
    }

    /// The last `Error:` line of an anyhow report (`Error: msg`, a blank line, `Caused by:`, then indented causes,
    /// numbered `0: …` when there are several). The prefix and numbering are dropped.
    public static func anyhowBlock(in lines: [String]) -> (summary: String, causes: [String])? {
        guard let start = lines.lastIndex(where: { $0.hasPrefix("Error:") }) else { return nil }
        let summary = lines[start].dropFirst("Error:".count).trimmingCharacters(in: .whitespaces)
        var causes: [String] = []
        var inCauses = false
        for line in lines[(start + 1)...] {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed == "Caused by:" {
                inCauses = true
                continue
            }
            guard inCauses else { continue }
            if trimmed.isEmpty { continue }
            guard line.hasPrefix(" ") || line.hasPrefix("\t") else { break }
            causes.append(stripNumbering(trimmed))
        }
        return (summary, causes)
    }

    /// clap's usage error (`error: unexpected argument '- ' found`), without the `error: ` prefix: the cause of an
    /// exit-2 failure, which has no anyhow `Error:` line.
    public static func clapError(in lines: [String]) -> String? {
        guard let line = lines.first(where: { $0.hasPrefix("error: ") }) else { return nil }
        let message = line.dropFirst("error: ".count).trimmingCharacters(in: .whitespaces)
        return message.isEmpty ? nil : message
    }

    private static func stripNumbering(_ cause: String) -> String {
        guard let colon = cause.firstIndex(of: ":"), cause[..<colon].allSatisfy(\.isNumber), !cause[..<colon].isEmpty
        else { return cause }
        return cause[cause.index(after: colon)...].trimmingCharacters(in: .whitespaces)
    }

    /// `Failed to parse feature registry: …` (core's registry reader), or any message naming `registry.json`
    /// together with a JSON, parse or EOF error.
    static func isRegistryParseError(_ text: String) -> Bool {
        if text.contains("Failed to parse feature registry") { return true }
        guard text.contains("registry.json") else { return false }
        let lowered = text.lowercased()
        return lowered.contains("json error") || lowered.contains("parse") || lowered.contains("eof")
    }

    /// "Sign in with: sbx login" (or core's "run 'sbx login'") and "local-vm requires a Linux host".
    static func runtimePrerequisite(in text: String) -> RefusalCause? {
        let lines = text.split(whereSeparator: \.isNewline).map(String.init)
        if let line = lines.first(where: { $0.contains("sbx login") }) {
            return .runtimePrerequisite(provider: "sbx", detail: line)
        }
        if let line = lines.first(where: { $0.contains("local-vm requires a Linux host") }) {
            return .runtimePrerequisite(provider: "local-vm", detail: line)
        }
        return nil
    }

    /// The rest of the first line containing `marker`, after it.
    private static func value(after marker: String, in text: String) -> String? {
        for line in text.split(whereSeparator: \.isNewline) {
            guard let range = line.range(of: marker) else { continue }
            let value = line[range.upperBound...].trimmingCharacters(in: .whitespaces)
            return value.isEmpty ? nil : value
        }
        return nil
    }

    private static func capture(after start: String, until end: String, in text: String) -> String? {
        guard let endRange = text.range(of: end),
              let startRange = text.range(of: start, options: .backwards, range: text.startIndex..<endRange.lowerBound)
        else { return nil }
        return String(text[startRange.upperBound..<endRange.lowerBound])
    }
}

extension JSONValue {
    var objectValue: [String: JSONValue]? {
        if case .object(let value) = self { return value }
        return nil
    }

    var arrayValue: [JSONValue]? {
        if case .array(let value) = self { return value }
        return nil
    }

    var stringValue: String? {
        if case .string(let value) = self { return value }
        return nil
    }

    var boolValue: Bool? {
        if case .bool(let value) = self { return value }
        return nil
    }
}
