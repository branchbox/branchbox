import BranchBoxKit
import Foundation

/// One finished CLI run and how to read it (DESIGN §6.3; step 1, the runner's own errors, is `ToolInvocation`):
///
/// 2. Exit 0: decode stdout with `CLIJSON.decode`; a preamble is returned for the caller to surface as a warning.
/// 3. Non-zero: an error envelope on stdout is classified by its code.
/// 4. In-band commands (exec, devcontainer, doctor, sync) print their payload and exit 1; after the envelope check,
///    the payload is decoded on any exit status.
/// 5. Otherwise the stderr anyhow block is matched against the 0.13.4 known-message table.
/// 6. Anything else is `.commandFailed`, summarized by the CLI's `Error:` line.
/// 7. A signal is `.commandFailed("terminated by signal N")`.
struct CLIOutput: Sendable {
    let result: ProcessResult
    /// What the run was (operation, invocation, CLI version); `summary` is filled per failure.
    let operation: String
    let invocation: String
    let cliVersion: String?
    let context: CLIErrorClassifier.Context

    var stdoutText: String { String(decoding: result.stdout, as: UTF8.self) }

    var succeeded: Bool { result.termination == .exited(0) }

    func diagnostics(summary: String) -> Diagnostics {
        Diagnostics(summary: summary, exitCode: ToolInvocation.exitCode(of: result.termination),
                    signal: ToolInvocation.signal(of: result.termination), logTail: result.stderrTail,
                    invocation: invocation, cliVersion: cliVersion)
    }

    /// The payload of a command that only prints one on success.
    func decode<T: Decodable>(_ type: T.Type, what: String) throws -> (value: T, preamble: String?) {
        guard succeeded else { throw failure() }
        do {
            return try CLIJSON.decode(type, from: result.stdout)
        } catch {
            throw decodeFailed(what: what, error: error)
        }
    }

    /// The payload of an in-band command, read on any exit status once the envelope check passed. `accept` rejects
    /// a document that decodes but is not this payload (every field of some models is optional).
    func decodeInBand<T: Decodable>(_ type: T.Type, what: String, accept: (T) -> Bool = { _ in true }) throws -> T {
        if case .signaled = result.termination { throw failure() }
        if !succeeded, let envelope = envelope() {
            throw CLIErrorClassifier.classify(envelope: envelope, diagnostics: diagnostics(summary: ""),
                                              context: context)
        }
        var decodeError: (any Error)?
        do {
            let value = try CLIJSON.decode(type, from: result.stdout).value
            if accept(value) { return value }
        } catch {
            decodeError = error
        }
        guard succeeded else { throw textFailure() }
        throw decodeFailed(what: what, error: decodeError)
    }

    /// Why a run that did not succeed failed (steps 3, 5, 6 and 7).
    func failure() -> BackendError {
        if case .signaled(let signal) = result.termination {
            return .commandFailed(diagnostics(summary: "\(operation) was terminated by signal \(signal)"))
        }
        if let envelope = envelope() {
            return CLIErrorClassifier.classify(envelope: envelope, diagnostics: diagnostics(summary: ""),
                                               context: context)
        }
        return textFailure()
    }

    private func textFailure() -> BackendError {
        CLIErrorClassifier.classify(stderr: result.stderrTail, stdout: stdoutText,
                                    diagnostics: diagnostics(summary: fallbackSummary()), context: context)
    }

    /// Used when stderr has no `Error:` line: the message of a legacy `{"error": "…"}` payload or of an
    /// `{"outcome": "error", "message": "…"}` one, else clap's `error: …` usage line (exit 2: an unknown flag, a
    /// value read as a flag), else the exit status.
    private func fallbackSummary() -> String {
        if let object = try? CLIJSON.decode(JSONValue.self, from: result.stdout).value.objectValue {
            if let message = object["error"]?.stringValue, !message.isEmpty { return message }
            if object["outcome"]?.stringValue == "error", let message = object["message"]?.stringValue,
               !message.isEmpty { return message }
        }
        if let usage = CLIErrorClassifier.clapError(in: result.stderrTail) { return usage }
        switch result.termination {
        case .exited(let code): return "\(operation) exited with status \(code)"
        case .signaled(let signal): return "\(operation) was terminated by signal \(signal)"
        }
    }

    /// The §5.2 error envelope, if stdout holds one (`schema_version` plus an `error` object).
    func envelope() -> ErrorEnvelope? {
        try? CLIJSON.decode(ErrorEnvelope.self, from: result.stdout).value
    }

    private func decodeFailed(what: String, error: (any Error)?) -> BackendError {
        let version = cliVersion.map { " from branchbox \($0)" } ?? ""
        let detail = error.map(Self.describe) ?? "The output was not a \(what)"
        return .decodeFailed(what: what, detail: detail,
                             diagnostics: diagnostics(summary: "Could not read the \(what)\(version): \(detail)"))
    }

    /// "key 'work_feature' not found at the top level" rather than a multi-line `DecodingError` dump.
    static func describe(_ error: any Error) -> String {
        guard let error = error as? DecodingError else { return String(describing: error) }
        func path(_ context: DecodingError.Context) -> String {
            let keys = context.codingPath.map { $0.intValue.map { "[\($0)]" } ?? $0.stringValue }
            return keys.isEmpty ? "the top level" : keys.joined(separator: ".")
        }
        switch error {
        case .keyNotFound(let key, let context): return "key '\(key.stringValue)' not found at \(path(context))"
        case .typeMismatch(let type, let context): return "expected \(type) at \(path(context))"
        case .valueNotFound(let type, let context): return "missing \(type) at \(path(context))"
        case .dataCorrupted(let context):
            return context.codingPath.isEmpty ? "the output is not valid JSON" : "corrupted value at \(path(context))"
        @unknown default: return String(describing: error)
        }
    }
}
