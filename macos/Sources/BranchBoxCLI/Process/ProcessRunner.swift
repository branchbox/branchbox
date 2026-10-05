import BranchBoxKit
import Darwin
import Foundation
import os

/// Runs child processes (the CLI, git, docker) without ever blocking a thread (DESIGN §7.1).
///
/// - A Task that is already cancelled gets `.cancelled` and nothing is spawned.
/// - The working directory and the executable path are checked before spawning.
/// - stdout and stderr are drained from the moment the child starts, so neither pipe can fill up and stall it.
///   stdout is collected whole (it carries the JSON); stderr is split into lines and the last
///   `stderrTailLines` are kept.
/// - Lines reach `onLine` ANSI-stripped and one at a time, never concurrently: stderr always, stdout only when
///   `spec.streamStdout` is set.
/// - The run finishes once the child has exited and both pipes reached EOF, or `drainGrace` after the child
///   exited when a grandchild still holds a pipe. Leftover group members of a successful run are left
///   running, with a logged warning.
/// - Cancellation, `timeout`, a stdout overflow and `terminateAll()` stop the whole process group (see
///   `Escalator`); the error is thrown only after the group is gone, or after the escalation's bound.
public actor ProcessRunner: ProcessRunning {
    private let registry = ChildRegistry()
    private let hooks: LaunchHooks

    public init() {
        self.hooks = LaunchHooks()
    }

    init(hooks: LaunchHooks) {
        self.hooks = hooks
    }

    /// Runs in flight, including ones still being spawned.
    public nonisolated var liveRunCount: Int { registry.count }

    public func run(_ spec: ProcessSpec,
                    onLine: @escaping @Sendable (OutputLine) -> Void) async throws -> ProcessResult {
        let started = ContinuousClock.now
        if Task.isCancelled { throw ProcessRunError.cancelled(partial: .notStarted(since: started)) }
        try Self.validate(spec)

        let escalator = Escalator(interruptGrace: spec.interruptGrace, terminateGrace: spec.terminateGrace)
        let registration = registry.register(escalator)
        defer { registry.unregister(registration) }
        return try await Self.execute(spec, escalator: escalator, hooks: hooks, started: started, onLine: onLine)
    }

    public func terminateAll() async {
        await registry.terminateAll()
    }

    // MARK: - Running

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "process")

    private static func validate(_ spec: ProcessSpec) throws {
        if let directory = spec.workingDirectory {
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: directory.path, isDirectory: &isDirectory),
                  isDirectory.boolValue else {
                throw ProcessRunError.workingDirectoryMissing(directory.path)
            }
        }
        guard spec.executable.isFileURL, spec.executable.path.hasPrefix("/") else {
            throw ProcessRunError.launchFailed(executable: spec.executable.relativeString,
                                               reason: "The executable path is not absolute")
        }
        // Foundation reports a file that is not executable as missing; name the real cause instead.
        let path = spec.executable.path
        var info = stat()
        guard stat(path, &info) == 0 else {
            throw ProcessRunError.launchFailed(executable: path, reason: String(cString: strerror(errno)))
        }
        if info.st_mode & S_IFMT == S_IFDIR {
            throw ProcessRunError.launchFailed(executable: path, reason: "It is a directory")
        }
        guard access(path, X_OK) == 0 else {
            throw ProcessRunError.launchFailed(executable: path, reason: "The file is not executable")
        }
    }

    /// Nonisolated, so the cancellation handler is installed outside the actor and the run never holds it.
    private static func execute(_ spec: ProcessSpec, escalator: Escalator, hooks: LaunchHooks,
                                started: ContinuousClock.Instant,
                                onLine: @escaping @Sendable (OutputLine) -> Void) async throws -> ProcessResult {
        try await withTaskCancellationHandler {
            try await launchAndCollect(spec, escalator: escalator, hooks: hooks, started: started, onLine: onLine)
        } onCancel: {
            escalator.begin(.cancelled)
        }
    }

    private static func launchAndCollect(_ spec: ProcessSpec, escalator: Escalator, hooks: LaunchHooks,
                                         started: ContinuousClock.Instant,
                                         onLine: @escaping @Sendable (OutputLine) -> Void) async throws
        -> ProcessResult {
        let process = Process()
        process.executableURL = spec.executable
        process.arguments = spec.arguments
        process.environment = spec.environment
        process.currentDirectoryURL = spec.workingDirectory
        let stdinPipe = spec.standardInput.map { _ in Pipe() }
        process.standardInput = stdinPipe ?? FileHandle.nullDevice
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        let (exits, exitContinuation) = AsyncStream.makeStream(of: Termination.self)
        process.terminationHandler = { process in
            escalator.leaderDidExit()
            exitContinuation.yield(process.terminationReason == .uncaughtSignal
                                   ? .signaled(process.terminationStatus) : .exited(process.terminationStatus))
            exitContinuation.finish()
        }
        let stdoutReader = PipeReader(stdoutPipe.fileHandleForReading)
        let stderrReader = PipeReader(stderrPipe.fileHandleForReading)

        // A stop that arrived once the cancellation handler was installed still prevents the spawn.
        if let reason = escalator.reason {
            stdoutReader.finish()
            stderrReader.finish()
            throw stopError(for: reason, spec: spec, partial: .notStarted(since: started))
        }
        do {
            try process.run()
        } catch {
            stdoutReader.finish()
            stderrReader.finish()
            throw ProcessRunError.launchFailed(executable: spec.executable.path, reason: launchFailureReason(error))
        }
        let pid = process.processIdentifier
        hooks.afterSpawn?(pid)
        escalator.publish(pid: pid)
        if let stdinPipe, let input = spec.standardInput {
            StandardInputWriter.write(input, to: stdinPipe.fileHandleForWriting)
        }

        let timer = spec.timeout.map { limit in
            Task.detached {
                try? await Task.sleep(for: limit)
                // A child that already exited is only draining, which drainGrace bounds.
                if !Task.isCancelled, !escalator.leaderExited { escalator.begin(.timedOut) }
            }
        }
        defer { timer?.cancel() }

        // The collector is unstructured so that cancelling the caller does not end the AsyncStream iteration
        // early: cancellation only reaches the escalator, and the drain runs until the group is done.
        let emitter = LineEmitter(onLine)
        let collector = Task.detached {
            await collect(stdout: stdoutReader, stderr: stderrReader, exits: exits, spec: spec,
                          escalator: escalator, emitter: emitter)
        }
        let collected = await collector.value
        withExtendedLifetime(process) {}                 // the Process (and its pipes) outlive the child
        let result = ProcessResult(termination: collected.termination, stdout: collected.stdout,
                                   stderrTail: collected.stderrTail, duration: ContinuousClock.now - started)

        guard let reason = escalator.reason else {
            if escalator.groupAlive {
                let name = spec.executable.lastPathComponent
                logger.warning(
                    "\(name, privacy: .public) exited; group \(pid, privacy: .public) still has members, left running")
            }
            escalator.retire()
            return result
        }
        if !(await escalator.awaitGroupGone()) {
            logger.error("Process group \(pid, privacy: .public) survived SIGKILL; giving up waiting for it")
        }
        escalator.retire()
        throw stopError(for: reason, spec: spec, partial: result)
    }

    private struct Collected: Sendable {
        var termination: Termination
        var stdout: Data
        var stderrTail: [String]
    }

    private static func collect(stdout: PipeReader, stderr: PipeReader, exits: AsyncStream<Termination>,
                                spec: ProcessSpec, escalator: Escalator, emitter: LineEmitter) async -> Collected {
        async let stdoutData = drainStdout(stdout, spec: spec, escalator: escalator, emitter: emitter)
        async let stderrTail = drainStderr(stderr, tailLines: spec.stderrTailLines, emitter: emitter)
        var termination = Termination.exited(-1)
        for await exit in exits { termination = exit }
        // The leader is gone; a grandchild that inherited a pipe may keep it open indefinitely.
        let drainGrace = spec.drainGrace
        let drainDeadline = Task.detached {
            try? await Task.sleep(for: drainGrace)
            guard !Task.isCancelled else { return }
            stdout.finish()
            stderr.finish()
        }
        let collected = Collected(termination: termination, stdout: await stdoutData, stderrTail: await stderrTail)
        drainDeadline.cancel()
        return collected
    }

    private static func drainStdout(_ reader: PipeReader, spec: ProcessSpec, escalator: Escalator,
                                    emitter: LineEmitter) async -> Data {
        var data = Data()
        var splitter = LineSplitter()
        var overflowed = false
        for await chunk in reader.chunks {
            guard !overflowed else { continue }
            guard data.count + chunk.count <= spec.stdoutLimit else {
                overflowed = true
                escalator.begin(.stdoutTooLarge)
                continue
            }
            data.append(chunk)
            if spec.streamStdout {
                for line in splitter.append(chunk) { emitter.emit(.stdout, line) }
            }
        }
        if spec.streamStdout, !overflowed, let last = splitter.flush() { emitter.emit(.stdout, last) }
        return data
    }

    private static func drainStderr(_ reader: PipeReader, tailLines: Int, emitter: LineEmitter) async -> [String] {
        var tail = LineTail(limit: tailLines)
        var splitter = LineSplitter()
        for await chunk in reader.chunks {
            for line in splitter.append(chunk) {
                if let text = emitter.emit(.stderr, line) { tail.append(text) }
            }
        }
        if let last = splitter.flush(), let text = emitter.emit(.stderr, last) { tail.append(text) }
        return tail.lines
    }

    private static func stopError(for reason: Escalator.Reason, spec: ProcessSpec,
                                  partial: ProcessResult) -> ProcessRunError {
        switch reason {
        case .cancelled, .shutdown: return .cancelled(partial: partial)
        case .timedOut: return .timedOut(after: spec.timeout ?? .zero, partial: partial)
        case .stdoutTooLarge: return .stdoutTooLarge(limit: spec.stdoutLimit)
        }
    }

    /// Foundation's description, e.g. "The operation couldn’t be completed. Exec format error".
    private static func launchFailureReason(_ error: any Error) -> String {
        (error as NSError).localizedDescription
    }
}

/// Test seams around the spawn.
struct LaunchHooks: Sendable {
    /// Runs after the child is spawned and before its pid is published to the escalator, on the launching task.
    var afterSpawn: (@Sendable (pid_t) -> Void)?
}

/// Delivers lines to `onLine` one at a time, ANSI-stripped. The stdout and stderr drains run concurrently, so
/// without the lock a sink could be entered from two threads at once.
private final class LineEmitter: Sendable {
    private let onLine: @Sendable (OutputLine) -> Void
    private let lock = NSLock()

    init(_ onLine: @escaping @Sendable (OutputLine) -> Void) {
        self.onLine = onLine
    }

    /// Strips and emits `raw`, returning the text sent. A line made only of escape sequences (a cursor move or
    /// a line clear between redraws) is dropped and returns nil.
    @discardableResult
    func emit(_ channel: OutputLine.Channel, _ raw: String) -> String? {
        let text = ANSI.strip(raw)
        if text.isEmpty, !raw.isEmpty { return nil }
        lock.withLock { onLine(OutputLine(channel: channel, text: text)) }
        return text
    }
}

private extension ProcessResult {
    /// What a run that never spawned reports.
    static func notStarted(since started: ContinuousClock.Instant) -> ProcessResult {
        ProcessResult(termination: .exited(-1), stdout: Data(), stderrTail: [], duration: ContinuousClock.now - started)
    }
}
