import BranchBoxCLI
import BranchBoxKit
import Darwin
import Foundation
import Testing

/// A per-test temporary directory of executable shell scripts that stand in for the CLI, a login shell or a
/// tool. Scripts are written at test time, so nothing executable is checked in and no SwiftPM resource rule is
/// needed. Call `remove()` in a `defer`.
struct FakeCLI {
    /// The launchd PATH a Finder-launched app gets, and nothing else.
    static let environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": NSHomeDirectory(), "LANG": "en_US.UTF-8"]

    let directory: URL

    init(_ label: String = #function) throws {
        let name = label.filter { $0.isLetter || $0.isNumber }.prefix(40)
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("branchbox-tests/cli-\(name)-\(UUID().uuidString.prefix(8))", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    /// Writes `#!/bin/sh` + `body` to `name` with mode 0755.
    @discardableResult
    func script(_ name: String, _ body: String) throws -> URL {
        let url = file(name)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(("#!/bin/sh\n" + body + "\n").utf8).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        return url
    }

    /// Like `script`, then runs it once with the single argument `__warm__`, on which it exits at once. The
    /// first exec of a freshly written file can take a few hundred milliseconds (the system scans it); tests
    /// that time a run warm their script first so they measure the runner, not the scan.
    @discardableResult
    func warmedScript(_ name: String, _ body: String) async throws -> URL {
        let url = try script(name, "[ \"$1\" = __warm__ ] && exit 0\n" + body)
        _ = try await ProcessRunner().run(Self.spec(url, ["__warm__"])) { _ in }
        return url
    }

    func file(_ name: String) -> URL {
        directory.appendingPathComponent(name)
    }

    func remove() {
        try? FileManager.default.removeItem(at: directory)
    }

    static func spec(_ executable: URL, _ arguments: [String] = [], configure: (inout ProcessSpec) -> Void = { _ in })
        -> ProcessSpec {
        var spec = ProcessSpec(executable: executable, arguments: arguments, environment: environment,
                               workingDirectory: nil)
        configure(&spec)
        return spec
    }
}

/// Scripts shared by the runner tests.
enum FakeScripts {
    /// 2000 stdout lines and 2000 stderr lines of 101 bytes each, interleaved: ~200 KB per pipe, far past the
    /// 64 KB pipe buffer.
    static let bigInterleaved = """
        i=0
        while [ $i -lt 2000 ]; do
          printf '%0100d\\n' $i
          printf 'ERR %096d\\n' $i >&2
          i=$((i+1))
        done
        """

    /// Coloured tracing output and a carriage-return progress bar.
    static let ansi = #"""
        printf '\033[2m2026-10-01T22:50:58.796461Z\033[0m \033[32m INFO\033[0m \033[2mworktree_core::git\033[0m\033[2m:\033[0m Created worktree at /tmp/x\n' >&2
        printf '\033]8;;https://example.com\007link\033]8;;\007 done\n' >&2
        printf 'progress 10%%\rprogress 50%%\rprogress 100%%\r\n' >&2
        printf '[]'
        """#

    /// `feature exec` whose inner command failed: payload on stdout, exit 1.
    static let execFail = """
        printf '{"exit_code": 3, "stdout": "out\\\\n", "stderr": "err\\\\n"}\\n'
        echo "Error: Runtime command exited with status 3" >&2
        exit 1
        """

    static let stdinProbe = """
        if read line; then echo "read:$line"; else echo "stdin-eof"; fi
        """

    /// Writes its pid to $1, then becomes `sleep 30`.
    static let markerThenSleep = """
        echo $$ > "$1"
        exec /bin/sleep 30
        """
}

/// Collects lines from the runner's `@Sendable` callback.
final class LineCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var collected: [OutputLine] = []

    var lines: [OutputLine] { lock.withLock { collected } }
    func texts(_ channel: OutputLine.Channel) -> [String] { lines.filter { $0.channel == channel }.map(\.text) }
    func append(_ line: OutputLine) { lock.withLock { collected.append(line) } }
}

/// Polls `condition` every 10 ms; fails the test after `timeout`.
func eventually(within timeout: Duration = .seconds(5), _ condition: () -> Bool,
                sourceLocation: SourceLocation = #_sourceLocation) async throws {
    let deadline = ContinuousClock.now + timeout
    while !condition() {
        try #require(ContinuousClock.now < deadline, "condition not met within \(timeout)",
                     sourceLocation: sourceLocation)
        try await Task.sleep(for: .milliseconds(10))
    }
}

/// Whether a process with this pid exists (zombies excluded once reaped).
func isAlive(_ pid: pid_t) -> Bool {
    kill(pid, 0) == 0 || errno == EPERM
}

/// The pid a script wrote to `file`, once it has.
func recordedPID(in file: URL) -> pid_t? {
    guard let text = try? String(contentsOf: file, encoding: .utf8) else { return nil }
    return pid_t(text.trimmingCharacters(in: .whitespacesAndNewlines))
}
