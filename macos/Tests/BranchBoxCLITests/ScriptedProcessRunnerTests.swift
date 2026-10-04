import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

// The ScriptedProcessRunner that the CLIBackend tests drive: it must behave like ProcessRunner (see
// ProcessRunnerTests) for matching, line streaming and cancellation.

/// Collects streamed lines from the runner's `@Sendable` callback.
private final class LineSink: @unchecked Sendable {
    private let lock = NSLock()
    private var collected: [OutputLine] = []
    var lines: [OutputLine] { lock.withLock { collected } }
    func append(_ line: OutputLine) { lock.withLock { collected.append(line) } }
}

private func spec(_ executable: String, _ arguments: [String], streamStdout: Bool = false) -> ProcessSpec {
    var spec = ProcessSpec(executable: URL(fileURLWithPath: executable), arguments: arguments, environment: [:],
                           workingDirectory: nil)
    spec.streamStdout = streamStdout
    return spec
}

private func isCancelled(_ error: any Error) -> Bool {
    if case .cancelled? = error as? ProcessRunError { return true }
    return false
}

@Suite struct ScriptedProcessRunnerTests {
    @Test func matchesArgvPrefixAndRecordsSpecs() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: "[]"),
            .exit(["/usr/bin/git", "-C", ScriptedProcessRunner.anyArgument, "status"], stdout: "clean"),
        ])
        let list = try await runner.run(spec("/opt/homebrew/bin/branchbox", ["feature", "list", "--json", "--repo", "/r"])) { _ in }
        let status = try await runner.run(spec("/usr/bin/git", ["-C", "/r/eta", "status", "--porcelain"])) { _ in }

        #expect(list.termination == .exited(0))
        #expect(String(decoding: list.stdout, as: UTF8.self) == "[]")
        #expect(String(decoding: status.stdout, as: UTF8.self) == "clean")
        #expect(runner.invocations == [
            ["/opt/homebrew/bin/branchbox", "feature", "list", "--json", "--repo", "/r"],
            ["/usr/bin/git", "-C", "/r/eta", "status", "--porcelain"],
        ])
        #expect(runner.launched.count == 2)
    }

    @Test func firstMatchingRuleWinsAndTimesRunOut() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "start"], 1, stderr: ["Error: boom"], times: 1),
            .exit(["branchbox", "feature"], 0),
        ])
        let first = try await runner.run(spec("/b/branchbox", ["feature", "start", "x"])) { _ in }
        let second = try await runner.run(spec("/b/branchbox", ["feature", "start", "x"])) { _ in }
        #expect(first.termination == .exited(1))
        #expect(first.stderrTail == ["Error: boom"])
        #expect(second.termination == .exited(0))
    }

    @Test func unmatchedRunFailsNamingItsArgv() async {
        let runner = ScriptedProcessRunner()
        do {
            _ = try await runner.run(spec("/b/branchbox", ["detect"])) { _ in }
            Issue.record("expected launchFailed")
        } catch let ProcessRunError.launchFailed(executable, reason) {
            #expect(executable == "/b/branchbox")
            #expect(reason.contains("/b/branchbox detect"))
        } catch {
            Issue.record("unexpected \(error)")
        }
        #expect(runner.specs.count == 1)
        #expect(runner.launched.isEmpty)
    }

    @Test func streamsStderrAlwaysAndStdoutOnlyWhenAsked() async throws {
        let lines = [OutputLine(channel: .stdout, text: "out"), OutputLine(channel: .stderr, text: "err")]
        let runner = ScriptedProcessRunner([ScriptedProcessRunner.Rule(["sh"], lines: lines, outcome: .exit(0))])

        let quiet = LineSink()
        _ = try await runner.run(spec("/bin/sh", [])) { quiet.append($0) }
        #expect(quiet.lines == [OutputLine(channel: .stderr, text: "err")])

        let streaming = LineSink()
        _ = try await runner.run(spec("/bin/sh", [], streamStdout: true)) { streaming.append($0) }
        #expect(streaming.lines == lines)
    }

    @Test func returnsTheScriptedError() async {
        let runner = ScriptedProcessRunner([ScriptedProcessRunner.Rule(["git"], outcome: .error(.workingDirectoryMissing("/gone")))])
        await #expect(throws: ProcessRunError.self) {
            _ = try await runner.run(spec("/usr/bin/git", ["status"])) { _ in }
        }
    }

    @Test func alreadyCancelledRunNeverLaunches() async {
        let runner = ScriptedProcessRunner([.exit(["branchbox"])])
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await runner.run(spec("/b/branchbox", ["feature", "list"])) { _ in }
        }
        let result = await task.result
        #expect(throws: ProcessRunError.self) { try result.get() }
        if case .failure(let error) = result { #expect(isCancelled(error)) }
        #expect(runner.specs.count == 1)
        #expect(runner.launched.isEmpty)
    }

    @Test(.timeLimit(.minutes(1)))
    func cancellationEndsAHangingRunPromptly() async throws {
        let runner = ScriptedProcessRunner([ScriptedProcessRunner.Rule(["branchbox"], lines: [OutputLine(channel: .stderr, text: "working")],
                                                                       outcome: .hang)])
        let task = Task { try await runner.run(spec("/b/branchbox", ["feature", "start", "x"])) { _ in } }
        try await waitUntil { runner.launched.count == 1 }
        let clock = ContinuousClock()
        let cancelledAt = clock.now
        task.cancel()
        let result = await task.result
        // The design bound is 100 ms; shared CI runners need headroom for scheduling delays.
        #expect(clock.now - cancelledAt < .milliseconds(500))
        guard case .failure(ProcessRunError.cancelled(let partial)) = result else {
            Issue.record("expected .cancelled, got \(result)")
            return
        }
        #expect(partial.termination == .signaled(SIGINT))
        #expect(partial.stderrTail == ["working"])
    }

    @Test(.timeLimit(.minutes(1)))
    func terminateAllEndsDelayedRuns() async throws {
        let runner = ScriptedProcessRunner([.exit(["branchbox"], delay: .seconds(30))])
        let task = Task { try await runner.run(spec("/b/branchbox", ["feature", "teardown", "x"])) { _ in } }
        try await waitUntil { runner.launched.count == 1 }
        await runner.terminateAll()
        let result = await task.result
        if case .failure(let error) = result { #expect(isCancelled(error)) } else { Issue.record("expected .cancelled") }
    }

    @Test func delayedRunFinishesWithItsResult() async throws {
        let runner = ScriptedProcessRunner([.exit(["branchbox"], 3, delay: .milliseconds(20))])
        let result = try await runner.run(spec("/b/branchbox", ["feature", "exec", "x", "--", "false"])) { _ in }
        #expect(result.termination == .exited(3))
    }
}

/// Polls `condition` every millisecond, failing the test after 5 s.
private func waitUntil(_ condition: () -> Bool) async throws {
    let deadline = ContinuousClock.now + .seconds(5)
    while !condition() {
        try #require(ContinuousClock.now < deadline, "condition not met within 5 s")
        try await Task.sleep(for: .milliseconds(1))
    }
}
