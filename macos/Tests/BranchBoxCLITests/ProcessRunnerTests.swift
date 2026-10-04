@testable import BranchBoxCLI
import BranchBoxKit
import Darwin
import Foundation
import Testing

// DESIGN §13.1 runner table. Every test spawns real processes from FakeCLI scripts; the suite is serialized so
// the timing bounds are not measured against two dozen sibling children.

@Suite(.serialized, .timeLimit(.minutes(2)))
struct ProcessRunnerTests {
    let runner = ProcessRunner()

    // MARK: - Output

    @Test func largeInterleavedOutputDoesNotDeadlockAndStreamsEveryLine() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("big", FakeScripts.bigInterleaved)
        let lines = LineCollector()
        let started = ContinuousClock.now

        let result = try await runner.run(FakeCLI.spec(script) { $0.streamStdout = true }) { lines.append($0) }

        #expect(ContinuousClock.now - started < .seconds(5))
        #expect(result.termination == .exited(0))
        #expect(result.stdout.count == 2000 * 101)
        #expect(lines.texts(.stdout) == (0..<2000).map { padded($0, to: 100) })
        #expect(lines.texts(.stderr) == (0..<2000).map { "ERR " + padded($0, to: 96) })
        #expect(result.stderrTail == Array(lines.texts(.stderr).suffix(400)))
    }

    @Test func concurrentRunsEachKeepTheirOwnOutput() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("big", FakeScripts.bigInterleaved)
        let runner = runner

        let outcomes = try await withThrowingTaskGroup(of: [Int].self) { group in
            for _ in 0..<24 {
                group.addTask {
                    let result = try await runner.run(FakeCLI.spec(script)) { _ in }
                    return [result.stdout.count, result.stderrTail.count]
                }
            }
            var outcomes: [[Int]] = []
            for try await outcome in group { outcomes.append(outcome) }
            return outcomes
        }

        #expect(outcomes == Array(repeating: [2000 * 101, 400], count: 24))
        #expect(runner.liveRunCount == 0)
    }

    @Test func stdinIsDevNullUnlessDataIsGiven() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("probe", FakeScripts.stdinProbe)

        let closed = try await runner.run(FakeCLI.spec(script)) { _ in }
        let fed = try await runner.run(FakeCLI.spec(script) { $0.standardInput = Data("hello\n".utf8) }) { _ in }

        #expect(text(closed.stdout) == "stdin-eof\n")
        #expect(text(fed.stdout) == "read:hello\n")
    }

    @Test func largeStdinIsDeliveredWholeThenClosed() async throws {
        let input = Data(repeating: UInt8(ascii: "x"), count: 1 << 20)
        let spec = FakeCLI.spec(URL(fileURLWithPath: "/usr/bin/wc"), ["-c"]) { $0.standardInput = input }

        let result = try await runner.run(spec) { _ in }

        #expect(text(result.stdout).trimmingCharacters(in: .whitespacesAndNewlines) == "1048576")
    }

    @Test func childThatNeverReadsStdinStillFinishes() async throws {
        // The writer gets EPIPE rather than a SIGPIPE that would take the test process down.
        let spec = FakeCLI.spec(URL(fileURLWithPath: "/usr/bin/true")) { $0.standardInput = Data(count: 1 << 20) }

        let result = try await runner.run(spec) { _ in }

        #expect(result.termination == .exited(0))
    }

    @Test func nonZeroExitKeepsStdoutAndTheStderrTail() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("exec-fail", FakeScripts.execFail)

        let result = try await runner.run(FakeCLI.spec(script)) { _ in }

        #expect(result.termination == .exited(1))
        #expect(text(result.stdout) == "{\"exit_code\": 3, \"stdout\": \"out\\n\", \"stderr\": \"err\\n\"}\n")
        #expect(result.stderrTail == ["Error: Runtime command exited with status 3"])
    }

    @Test func stripsEscapeSequencesAndSplitsCarriageReturns() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("ansi", FakeScripts.ansi)
        let lines = LineCollector()

        let result = try await runner.run(FakeCLI.spec(script)) { lines.append($0) }

        #expect(lines.texts(.stderr) == [
            "2026-10-01T22:50:58.796461Z  INFO worktree_core::git: Created worktree at /tmp/x",
            "link done",
            "progress 10%", "progress 50%", "progress 100%",
        ])
        #expect(text(result.stdout) == "[]")
    }

    @Test func stdoutLinesStreamOnlyWhenAsked() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("both", "echo out\necho err >&2\nprintf 'unterminated'")
        let quiet = LineCollector()
        let streaming = LineCollector()

        let first = try await runner.run(FakeCLI.spec(script)) { quiet.append($0) }
        let second = try await runner.run(FakeCLI.spec(script) { $0.streamStdout = true }) { streaming.append($0) }

        #expect(quiet.lines == [OutputLine(channel: .stderr, text: "err")])
        #expect(streaming.texts(.stdout) == ["out", "unterminated"])
        #expect(streaming.texts(.stderr) == ["err"])
        #expect(text(first.stdout) == "out\nunterminated")
        #expect(second.stdout == first.stdout)
    }

    @Test func stderrTailKeepsOnlyTheLastLines() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("noisy", "for i in 1 2 3 4 5; do echo \"line $i\" >&2; done\nprintf 'last' >&2")

        let result = try await runner.run(FakeCLI.spec(script) { $0.stderrTailLines = 3 }) { _ in }

        #expect(result.stderrTail == ["line 4", "line 5", "last"])
    }

    @Test func childLeadsItsOwnProcessGroup() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("pgid", "echo $$\nps -o pgid= -p $$")

        let result = try await runner.run(FakeCLI.spec(script)) { _ in }

        let ids = text(result.stdout).split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
        #expect(ids.count == 2)
        #expect(ids.first == ids.last)
        #expect(ids.first != String(getpgrp()))
    }

    @Test func durationIsMeasured() async throws {
        let result = try await runner.run(FakeCLI.spec(URL(fileURLWithPath: "/bin/sleep"), ["0.2"])) { _ in }

        #expect(result.duration >= .milliseconds(200))
        #expect(result.duration < .seconds(3))
    }

    // MARK: - Cancellation

    @Test func cancelBeforeLaunchNeverSpawns() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let marker = cli.file("spawned")
        let script = try cli.script("touch", "touch \"$1\"\nexec /bin/sleep 3")
        let runner = runner
        let started = ContinuousClock.now

        let result = await Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await runner.run(FakeCLI.spec(script, [marker.path])) { _ in }
        }.result

        #expect(ContinuousClock.now - started < .seconds(1))
        guard case .failure(ProcessRunError.cancelled(let partial)) = result else {
            Issue.record("expected .cancelled, got \(result)")
            return
        }
        #expect(partial.stdout.isEmpty)
        try await Task.sleep(for: .milliseconds(300))
        #expect(!FileManager.default.fileExists(atPath: marker.path))
        #expect(runner.liveRunCount == 0)
    }

    @Test func cancelDuringLaunchSignalsOnceThePIDIsPublished() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let marker = cli.file("child.pid")
        let script = try cli.script("marker", FakeScripts.markerThenSleep)
        let spawned = Locked<pid_t?>(nil)
        // The run's Task is cancelled while the child runs but before the escalator knows its pid.
        let runner = ProcessRunner(hooks: LaunchHooks(afterSpawn: { pid in
            spawned.value = pid
            withUnsafeCurrentTask { $0?.cancel() }
            let deadline = Date().addingTimeInterval(5)
            while recordedPID(in: marker) == nil, Date() < deadline { usleep(5_000) }
        }))
        let started = ContinuousClock.now

        let result = await Task { try await runner.run(FakeCLI.spec(script, [marker.path])) { _ in } }.result

        #expect(ContinuousClock.now - started < .seconds(3))
        guard case .failure(ProcessRunError.cancelled(let partial)) = result else {
            Issue.record("expected .cancelled, got \(result)")
            return
        }
        #expect(partial.termination == .signaled(SIGINT))
        let pid = try #require(spawned.value)
        #expect(recordedPID(in: marker) == pid)
        #expect(!isAlive(pid))
    }

    @Test func cancellationReapsAGrandchildThatIgnoresSIGINT() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        // A non-interactive shell starts `cmd &` with SIGINT ignored, so only SIGTERM ends the sleeper.
        let script = try cli.script("group", "/bin/sleep 300 &\necho \"grandchild=$!\" >&2\nwait")
        let spec = FakeCLI.spec(script) {
            $0.interruptGrace = .milliseconds(300)
            $0.terminateGrace = .milliseconds(300)
            $0.drainGrace = .milliseconds(200)
        }
        let lines = LineCollector()
        let runner = runner
        let task = Task { try await runner.run(spec) { lines.append($0) } }
        try await eventually { lines.texts(.stderr).contains { $0.hasPrefix("grandchild=") } }
        let grandchild = try #require(lines.texts(.stderr).first.flatMap { pid_t($0.dropFirst("grandchild=".count)) })
        let cancelledAt = ContinuousClock.now

        task.cancel()
        let result = await task.result

        #expect(ContinuousClock.now - cancelledAt < .milliseconds(600) + .seconds(1))
        guard case .failure(ProcessRunError.cancelled(let partial)) = result else {
            Issue.record("expected .cancelled, got \(result)")
            return
        }
        #expect(partial.termination != .exited(0))
        #expect(partial.stderrTail == ["grandchild=\(grandchild)"])
        #expect(!isAlive(grandchild))
    }

    // MARK: - Timeouts and limits

    @Test func timeoutEscalatesToSIGTERM() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try await cli.warmedScript("deaf", "trap '' INT\nexec /bin/sleep 30")
        let spec = FakeCLI.spec(script) {
            $0.timeout = .milliseconds(500)
            $0.interruptGrace = .milliseconds(300)
        }
        let started = ContinuousClock.now

        let result = await Task { try await runner.run(spec) { _ in } }.result

        let elapsed = ContinuousClock.now - started
        guard case .failure(ProcessRunError.timedOut(let after, let partial)) = result else {
            Issue.record("expected .timedOut, got \(result)")
            return
        }
        #expect(after == .milliseconds(500))
        #expect(partial.termination == .signaled(SIGTERM))
        #expect(elapsed >= .milliseconds(800))
        #expect(elapsed < .seconds(3))
    }

    @Test func timeoutEscalatesToSIGKILLWhenSIGTERMIsIgnoredToo() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try await cli.warmedScript("deafer", "trap '' INT TERM\nexec /bin/sleep 30")
        let spec = FakeCLI.spec(script) {
            $0.timeout = .milliseconds(500)
            $0.interruptGrace = .milliseconds(200)
            $0.terminateGrace = .milliseconds(200)
        }

        let result = await Task { try await runner.run(spec) { _ in } }.result

        guard case .failure(ProcessRunError.timedOut(_, let partial)) = result else {
            Issue.record("expected .timedOut, got \(result)")
            return
        }
        #expect(partial.termination == .signaled(SIGKILL))
    }

    @Test func grandchildHoldingStdoutDoesNotHoldTheRun() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let holderFile = cli.file("holder.pid")
        let script = try await cli.warmedScript("holder",
                                                "/bin/sleep 30 &\necho $! > \"$1\"\necho '{\"ok\":true}'\nexit 0")
        let spec = FakeCLI.spec(script, [holderFile.path]) { $0.drainGrace = .milliseconds(500) }
        let started = ContinuousClock.now

        let result = try await runner.run(spec) { _ in }

        let elapsed = ContinuousClock.now - started
        let holder = try #require(recordedPID(in: holderFile))
        defer { kill(holder, SIGKILL) }
        #expect(elapsed < .milliseconds(500) + .milliseconds(500))
        #expect(result.termination == .exited(0))
        #expect(text(result.stdout) == "{\"ok\":true}\n")
        // A successful run's leftovers are left alone (and logged), never killed.
        #expect(isAlive(holder))
    }

    @Test func stdoutPastTheLimitStopsTheChild() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("flood", "/usr/bin/head -c 1000000 /dev/zero\nexec /bin/sleep 30")
        let spec = FakeCLI.spec(script) { $0.stdoutLimit = 10_000 }
        let started = ContinuousClock.now

        let result = await Task { try await runner.run(spec) { _ in } }.result

        #expect(ContinuousClock.now - started < .seconds(5))
        guard case .failure(ProcessRunError.stdoutTooLarge(let limit)) = result else {
            Issue.record("expected .stdoutTooLarge, got \(result)")
            return
        }
        #expect(limit == 10_000)
    }

    // MARK: - Typed failures

    @Test func missingWorkingDirectoryIsTyped() async {
        let missing = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests/gone-\(UUID())")
        let spec = ProcessSpec(executable: URL(fileURLWithPath: "/bin/echo"), arguments: [],
                               environment: FakeCLI.environment, workingDirectory: missing)

        let result = await Task { try await runner.run(spec) { _ in } }.result

        guard case .failure(ProcessRunError.workingDirectoryMissing(let path)) = result else {
            Issue.record("expected .workingDirectoryMissing, got \(result)")
            return
        }
        #expect(path == missing.path)
        #expect(runner.liveRunCount == 0)
    }

    @Test(arguments: [
        (LaunchProblem.missing, "No such file or directory"),
        (.notExecutable, "The file is not executable"),
        (.directory, "It is a directory"),
        (.relative, "The executable path is not absolute"),
        (.notABinary, "Exec format error"),                     // Foundation's wording around it is localized
    ])
    func launchFailuresAreTypedAndNameTheirCause(_ problem: LaunchProblem, _ expected: String) async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let executable = try problem.executable(in: cli)

        let result = await Task { try await runner.run(FakeCLI.spec(executable)) { _ in } }.result

        guard case .failure(ProcessRunError.launchFailed(let path, let reason)) = result else {
            Issue.record("expected .launchFailed, got \(result)")
            return
        }
        #expect(path == (problem == .relative ? "branchbox" : executable.path))
        #expect(problem == .notABinary ? reason.hasSuffix(expected) : reason == expected)
        #expect(runner.liveRunCount == 0)
    }

    // MARK: - Shutdown and leaks

    @Test func terminateAllStopsEveryRunningGroup() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("marker", FakeScripts.markerThenSleep)
        let markers = (0..<3).map { cli.file("pid-\($0)") }
        let runner = ProcessRunner()
        let tasks = markers.map { marker in
            Task { try await runner.run(FakeCLI.spec(script, [marker.path])) { _ in } }
        }
        try await eventually { markers.allSatisfy { recordedPID(in: $0) != nil } }
        let pids = markers.compactMap(recordedPID(in:))
        let started = ContinuousClock.now

        await runner.terminateAll()

        #expect(ContinuousClock.now - started < .seconds(10))
        #expect(pids.allSatisfy { !isAlive($0) })
        for task in tasks {
            let result = await task.result
            guard case .failure(ProcessRunError.cancelled) = result else {
                Issue.record("expected .cancelled, got \(result)")
                continue
            }
        }
        #expect(runner.liveRunCount == 0)
    }

    @Test(arguments: [Stop.cancel, .timeout, .stdoutOverflow, .terminateAll])
    func noProcessOutlivesAStoppedRun(_ stop: Stop) async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let family = cli.file("family.pids")
        let script = try await cli.warmedScript("family", """
            /bin/sleep 30 &
            echo "$$ $!" > "$1"
            if [ "$2" = flood ]; then /usr/bin/head -c 100000 /dev/zero; fi
            wait
            """)
        let spec = FakeCLI.spec(script, [family.path, stop == .stdoutOverflow ? "flood" : "calm"]) {
            $0.interruptGrace = .milliseconds(300)
            $0.terminateGrace = .milliseconds(300)
            $0.drainGrace = .milliseconds(200)
            $0.stdoutLimit = 10_000
            if stop == .timeout { $0.timeout = .seconds(1) }
        }
        let runner = ProcessRunner()
        let task = Task { try await runner.run(spec) { _ in } }
        try await eventually { (try? String(contentsOf: family, encoding: .utf8))?.contains(" ") == true }
        let pids = try String(contentsOf: family, encoding: .utf8).split(whereSeparator: \.isWhitespace)
            .compactMap { pid_t($0) }
        #expect(pids.count == 2)

        switch stop {
        case .cancel: task.cancel()
        case .terminateAll: await runner.terminateAll()
        case .timeout, .stdoutOverflow: break
        }
        let result = await task.result

        if case .success = result { Issue.record("expected the run to be stopped") }
        #expect(pids.allSatisfy { !isAlive($0) })
        #expect(runner.liveRunCount == 0)
    }

    enum Stop: String, Sendable, CaseIterable { case cancel, timeout, stdoutOverflow, terminateAll }

    enum LaunchProblem: String, Sendable {
        case missing, notExecutable, directory, relative, notABinary

        func executable(in cli: FakeCLI) throws -> URL {
            switch self {
            case .missing:
                return cli.file("no-such-cli")
            case .notExecutable:
                let url = cli.file("plain")
                try Data("echo hi\n".utf8).write(to: url)
                return url
            case .directory:
                return cli.directory
            case .relative:
                return try #require(URL(string: "branchbox"))
            case .notABinary:
                // Executable, but neither a script nor a Mach-O the kernel accepts.
                let url = try cli.script("garbage", "")
                try Data([0xCF, 0xFA, 0xED, 0xFE, 0, 0, 0, 0]).write(to: url)
                return url
            }
        }
    }
}

// MARK: - Escalator

@Suite(.timeLimit(.minutes(1)))
struct EscalatorTests {
    @Test func stopRequestedDuringLaunchIsDeliveredWhenThePIDIsPublished() async throws {
        let (process, exits) = try spawnSleeper()
        let escalator = Escalator(interruptGrace: .seconds(5), terminateGrace: .seconds(5))

        escalator.begin(.cancelled)
        try await Task.sleep(for: .milliseconds(100))
        #expect(isAlive(process.processIdentifier))
        escalator.publish(pid: process.processIdentifier)

        var iterator = exits.makeAsyncIterator()
        let termination = await iterator.next()
        #expect(termination == .signaled(SIGINT))
        #expect(await escalator.awaitGroupGone())
        escalator.retire()
    }

    @Test func firstReasonWinsAndRetiredEscalatorsStaySilent() async throws {
        let (process, exits) = try spawnSleeper()
        defer { kill(process.processIdentifier, SIGKILL) }
        let escalator = Escalator(interruptGrace: .seconds(5), terminateGrace: .seconds(5))
        escalator.publish(pid: process.processIdentifier)
        escalator.retire()

        escalator.begin(.timedOut)
        try await Task.sleep(for: .milliseconds(200))

        #expect(escalator.reason == nil)
        #expect(isAlive(process.processIdentifier))
        kill(process.processIdentifier, SIGKILL)
        var iterator = exits.makeAsyncIterator()
        _ = await iterator.next()

        let other = Escalator(interruptGrace: .seconds(1), terminateGrace: .seconds(1))
        other.begin(.timedOut)
        other.begin(.cancelled)
        #expect(other.reason == .timedOut)
    }

    @Test func groupIsGoneOnceItsMembersExit() async throws {
        let (process, exits) = try spawnSleeper()
        let escalator = Escalator(interruptGrace: .milliseconds(100), terminateGrace: .milliseconds(100))
        escalator.publish(pid: process.processIdentifier)
        #expect(escalator.groupAlive)

        escalator.begin(.shutdown)
        var iterator = exits.makeAsyncIterator()
        _ = await iterator.next()
        escalator.leaderDidExit()

        #expect(await escalator.awaitGroupGone())
        #expect(!escalator.groupAlive)
    }

    @Test func terminateAllReachesARunThatIsStillLaunching() async throws {
        let registry = ChildRegistry()
        let escalator = Escalator(interruptGrace: .seconds(5), terminateGrace: .seconds(5))
        let registration = registry.register(escalator)
        defer { registry.unregister(registration) }
        let started = ContinuousClock.now

        await registry.terminateAll()

        #expect(ContinuousClock.now - started < .seconds(1))
        #expect(escalator.reason == .shutdown)
        let (process, exits) = try spawnSleeper()
        escalator.publish(pid: process.processIdentifier)
        var iterator = exits.makeAsyncIterator()
        #expect(await iterator.next() == .signaled(SIGINT))
        #expect(registry.liveGroups == [process.processIdentifier])
        escalator.retire()
    }

    /// `sleep 30` as its own process-group leader, with its termination as a one-element stream.
    private func spawnSleeper() throws -> (Process, AsyncStream<Termination>) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sleep")
        process.arguments = ["30"]
        process.standardInput = FileHandle.nullDevice
        let (exits, continuation) = AsyncStream.makeStream(of: Termination.self)
        process.terminationHandler = { process in
            continuation.yield(process.terminationReason == .uncaughtSignal
                               ? .signaled(process.terminationStatus) : .exited(process.terminationStatus))
            continuation.finish()
        }
        try process.run()
        return (process, exits)
    }
}

// MARK: - Helpers

/// A lock-protected value a `@Sendable` hook can write and the test can read.
final class Locked<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Value

    init(_ value: Value) {
        stored = value
    }

    var value: Value {
        get { lock.withLock { stored } }
        set { lock.withLock { stored = newValue } }
    }
}

private func text(_ data: Data) -> String {
    String(decoding: data, as: UTF8.self)
}

private func padded(_ number: Int, to width: Int) -> String {
    let digits = String(number)
    return String(repeating: "0", count: width - digits.count) + digits
}
