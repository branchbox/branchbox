import BranchBoxKit
import Foundation

/// A `ProcessRunning` that spawns nothing: rules matched against each spec's argv answer with emitted lines and a
/// canned `ProcessResult` or `ProcessRunError`.
///
/// - argv is the executable followed by the arguments. A rule's first element matches the executable's full path
///   or its file name (`"branchbox"`, `"git"`); later elements match arguments in order, and `"*"` matches any
///   one argument. A rule matches when its elements are a prefix of the argv.
/// - The first matching rule wins. A rule with `times` answers that many runs, then stops matching.
/// - An unmatched run throws `.launchFailed` naming its argv, so a missing rule fails loudly.
/// - Lines are emitted like the real runner's: stderr always, stdout only when the spec sets `streamStdout`.
/// - Cancellation: a run whose Task is already cancelled throws `.cancelled` without "launching" (it is in
///   `specs` but not in `launched`); a run waiting out its `delay` (or a `.hang`) throws `.cancelled` as soon as
///   its Task is cancelled or `terminateAll()` is called.
public final class ScriptedProcessRunner: ProcessRunning, @unchecked Sendable {
    public enum Outcome: Sendable {
        case result(ProcessResult)
        case error(ProcessRunError)
        /// Never finishes; only cancellation or `terminateAll()` ends the run.
        case hang

        /// Exits with `code`, `stdout` as the captured stdout and `stderrTail` as the captured stderr tail.
        public static func exit(_ code: Int32, stdout: String = "", stderrTail: [String] = []) -> Outcome {
            .result(ProcessResult(termination: .exited(code), stdout: Data(stdout.utf8), stderrTail: stderrTail,
                                  duration: .milliseconds(10)))
        }
    }

    public struct Rule: Sendable {
        public var argv: [String]
        public var lines: [OutputLine]
        public var delay: Duration
        public var times: Int?                                    // nil = every matching run
        public var outcome: Outcome

        public init(_ argv: [String], lines: [OutputLine] = [], delay: Duration = .zero, times: Int? = nil,
                    outcome: Outcome) {
            self.argv = argv
            self.lines = lines
            self.delay = delay
            self.times = times
            self.outcome = outcome
        }

        /// The common case: `stderr` is both streamed and returned as the stderr tail.
        public static func exit(_ argv: [String], _ code: Int32 = 0, stdout: String = "", stderr: [String] = [],
                                delay: Duration = .zero, times: Int? = nil) -> Rule {
            Rule(argv, lines: stderr.map { OutputLine(channel: .stderr, text: $0) }, delay: delay, times: times,
                 outcome: .exit(code, stdout: stdout, stderrTail: stderr))
        }
    }

    /// Matches any one argument in `Rule.argv`.
    public static let anyArgument = "*"

    private let lock = NSLock()
    private var rules: [Rule]
    private var recordedSpecs: [ProcessSpec] = []
    private var launchedSpecs: [ProcessSpec] = []
    private var inFlight: [UUID: RunGate] = [:]

    public init(_ rules: [Rule] = []) {
        self.rules = rules
    }

    public func add(_ rule: Rule) {
        lock.withLock { rules.append(rule) }
    }

    /// Every spec passed to `run`, in call order, including runs refused because their Task was already cancelled.
    public var specs: [ProcessSpec] { lock.withLock { recordedSpecs } }

    /// The specs that were "launched": every run except those cancelled before launch.
    public var launched: [ProcessSpec] { lock.withLock { launchedSpecs } }

    /// `specs` as argv arrays (executable path, then arguments), for easy assertions.
    public var invocations: [[String]] { specs.map(Self.argv(of:)) }

    public static func argv(of spec: ProcessSpec) -> [String] {
        [spec.executable.path] + spec.arguments
    }

    public func run(_ spec: ProcessSpec, onLine: @escaping @Sendable (OutputLine) -> Void) async throws -> ProcessResult {
        let clock = ContinuousClock()
        let start = clock.now
        lock.withLock { recordedSpecs.append(spec) }
        if Task.isCancelled { throw ProcessRunError.cancelled(partial: Self.interrupted(stderr: [], after: .zero)) }

        guard let rule = claimRule(for: spec) else {
            throw ProcessRunError.launchFailed(executable: spec.executable.path,
                                               reason: "ScriptedProcessRunner has no rule for: \(Self.argv(of: spec).joined(separator: " "))")
        }
        let hangs: Bool
        switch rule.outcome {
        case .hang: hangs = true
        case .result, .error: hangs = false
        }
        // The gate is registered in the same critical section that makes the run visible in `launched`, so a
        // `terminateAll()` issued after a test observes the launch always finds it.
        let waiting: (id: UUID, gate: RunGate)? = lock.withLock {
            launchedSpecs.append(spec)
            guard hangs || rule.delay > .zero else { return nil }
            let entry = (id: UUID(), gate: RunGate())
            inFlight[entry.id] = entry.gate
            return entry
        }

        var emittedStderr: [String] = []
        for line in rule.lines where line.channel == .stderr || spec.streamStdout {
            if line.channel == .stderr { emittedStderr.append(line.text) }
            onLine(line)
        }

        if let waiting {
            let finished = await waiting.gate.wait(timeout: hangs ? nil : rule.delay)
            lock.withLock { inFlight[waiting.id] = nil }
            if !finished {
                throw ProcessRunError.cancelled(partial: Self.interrupted(stderr: emittedStderr, after: clock.now - start))
            }
        }
        if Task.isCancelled {
            throw ProcessRunError.cancelled(partial: Self.interrupted(stderr: emittedStderr, after: clock.now - start))
        }

        switch rule.outcome {
        case .result(let result): return result
        case .error(let error): throw error
        case .hang: preconditionFailure("A hanging run only ends by cancellation")
        }
    }

    /// Ends every run that is waiting out a delay or hanging; each throws `.cancelled`.
    public func terminateAll() async {
        let gates = lock.withLock { Array(inFlight.values) }
        for gate in gates { gate.end(finished: false) }
    }

    private func claimRule(for spec: ProcessSpec) -> Rule? {
        lock.withLock {
            guard let index = rules.firstIndex(where: { Self.matches($0.argv, spec) }) else { return nil }
            let rule = rules[index]
            if let times = rule.times {
                if times <= 1 { rules.remove(at: index) } else { rules[index].times = times - 1 }
            }
            return rule
        }
    }

    private static func matches(_ pattern: [String], _ spec: ProcessSpec) -> Bool {
        guard let executable = pattern.first else { return true }
        guard executable == anyArgument || executable == spec.executable.path
                || executable == spec.executable.lastPathComponent else { return false }
        let arguments = pattern.dropFirst()
        guard arguments.count <= spec.arguments.count else { return false }
        return zip(arguments, spec.arguments).allSatisfy { $0 == anyArgument || $0 == $1 }
    }

    /// What a run interrupted by SIGINT leaves behind.
    private static func interrupted(stderr: [String], after duration: Duration) -> ProcessResult {
        ProcessResult(termination: .signaled(SIGINT), stdout: Data(), stderrTail: stderr, duration: duration)
    }
}

/// A one-shot wait that finishes after its timeout (nil = never) and is interrupted by Task cancellation or
/// `end(finished: false)`, whichever comes first.
private final class RunGate: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Bool, Never>?
    private var outcome: Bool?

    func wait(timeout: Duration?) async -> Bool {
        let timer = timeout.map { delay in
            Task { [self] in
                try? await Task.sleep(for: delay)
                end(finished: true)
            }
        }
        defer { timer?.cancel() }
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (continuation: CheckedContinuation<Bool, Never>) in
                let ended: Bool? = lock.withLock {
                    if let outcome { return outcome }
                    self.continuation = continuation
                    return nil
                }
                if let ended { continuation.resume(returning: ended) }
            }
        } onCancel: {
            end(finished: false)
        }
    }

    func end(finished: Bool) {
        let continuation: CheckedContinuation<Bool, Never>? = lock.withLock {
            guard outcome == nil else { return nil }
            outcome = finished
            defer { self.continuation = nil }
            return self.continuation
        }
        continuation?.resume(returning: finished)
    }
}
