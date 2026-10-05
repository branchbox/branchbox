@testable import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// DESIGN §13.1 locator table: precedence, unresolved paths, rejected reasons (injected file system), and the
// probe against fake CLIs in temp dirs: version --json, the exit-2 fallback, 0.13.3 → tooOld, the cache.

/// An in-memory file system: unknown paths are missing.
struct FakeFileSystem: FileSystemProbing {
    var kinds: [String: FileKind] = [:]

    func kind(at path: String) -> FileKind { kinds[path] ?? .missing }
    func identity(at path: String) -> FileIdentity? { nil }

    static let executable = FileKind.file(executable: true)
}

@Suite struct CLILocatorTests {
    let home = "/Users/u"
    let bundle = URL(fileURLWithPath: "/Applications/BranchBox.app")
    let embedded = "/Applications/BranchBox.app/Contents/Helpers/branchbox"

    func locate(_ fileSystem: FakeFileSystem, environment: [String: String] = [:], settings: String? = nil,
                path: String? = "/Users/u/bin:/opt/homebrew/bin:/usr/bin",
                bundleURL: URL? = nil) -> CLILocator.Outcome {
        CLILocator(fileSystem: fileSystem).locate(processEnvironment: environment, settingsOverride: settings,
                                                  searchPath: path, home: home, bundleURL: bundleURL ?? bundle)
    }

    @Test func precedenceIsEnvironmentSettingsLoginPathWellKnownEmbedded() {
        var fileSystem = FakeFileSystem(kinds: [
            "/env/branchbox": FakeFileSystem.executable,
            "/settings/branchbox": FakeFileSystem.executable,
            "/Users/u/bin/branchbox": FakeFileSystem.executable,
            "/Users/u/.cargo/bin/branchbox": FakeFileSystem.executable,
            embedded: FakeFileSystem.executable,
        ])
        let environment = ["BRANCHBOX_CLI_PATH": "/env/branchbox"]

        #expect(locate(fileSystem, environment: environment, settings: "/settings/branchbox").resolution
                == CLIResolution(path: "/env/branchbox", source: .environmentOverride))
        #expect(locate(fileSystem, settings: "/settings/branchbox").resolution
                == CLIResolution(path: "/settings/branchbox", source: .settingsOverride))
        #expect(locate(fileSystem).resolution == CLIResolution(path: "/Users/u/bin/branchbox", source: .loginShellPath))
        #expect(locate(fileSystem, path: "/usr/bin").resolution
                == CLIResolution(path: "/Users/u/.cargo/bin/branchbox", source: .wellKnownPath))
        fileSystem.kinds["/Users/u/.cargo/bin/branchbox"] = nil
        #expect(locate(fileSystem, path: nil).resolution == CLIResolution(path: embedded, source: .embedded))
    }

    @Test func pathIsSearchedInOrderAndRelativeEntriesAreSkipped() {
        let fileSystem = FakeFileSystem(kinds: [
            "bin/branchbox": FakeFileSystem.executable,
            "/second/branchbox": FakeFileSystem.executable,
            "/third/branchbox": FakeFileSystem.executable,
        ])

        let outcome = locate(fileSystem, path: "bin:/first:/second/:/third")

        #expect(outcome.resolution == CLIResolution(path: "/second/branchbox", source: .loginShellPath))
        #expect(outcome.searched == ["/first/branchbox", "/second/branchbox"])
        #expect(outcome.rejected.isEmpty)
    }

    @Test func wellKnownDirectoriesFollowTheDocumentedOrder() {
        let fileSystem = FakeFileSystem(kinds: [
            "/usr/local/bin/branchbox": FakeFileSystem.executable,
            "/opt/homebrew/bin/branchbox": FakeFileSystem.executable,
            "/Users/u/.local/bin/branchbox": FakeFileSystem.executable,
        ])

        let outcome = locate(fileSystem, path: "/usr/bin:/bin")

        #expect(outcome.resolution == CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .wellKnownPath))
        #expect(CLILocator.wellKnownDirectories(home: home)
                == ["/opt/homebrew/bin", "/usr/local/bin", "/Users/u/.cargo/bin", "/Users/u/.local/bin"])
    }

    @Test func unusableCandidatesAreRecordedWithTheirReasons() {
        let fileSystem = FakeFileSystem(kinds: [
            "/env/branchbox": .directory,
            "/settings/branchbox": .file(executable: false),
            "/Users/u/bin/branchbox": .brokenSymbolicLink,
            "/opt/homebrew/bin/branchbox": FakeFileSystem.executable,
        ])

        let outcome = locate(fileSystem, environment: ["BRANCHBOX_CLI_PATH": "/env/branchbox"],
                             settings: "/settings/branchbox")

        let expectedRejections = [
            RejectedCandidate(path: "/env/branchbox", reason: "Is a directory (from BRANCHBOX_CLI_PATH)"),
            RejectedCandidate(path: "/settings/branchbox", reason: "Not executable (from Settings)"),
            RejectedCandidate(path: "/Users/u/bin/branchbox",
                              reason: "Symbolic link to a missing file (on the login-shell PATH)"),
        ]
        #expect(outcome.resolution == CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .loginShellPath,
                                                    rejected: expectedRejections))
        #expect(outcome.rejected == expectedRejections)
    }

    @Test func overridesThatDoNotWorkAreRejectedNotFatal() {
        let fileSystem = FakeFileSystem(kinds: ["/Users/u/tools/branchbox": FakeFileSystem.executable])

        let missing = locate(fileSystem, environment: ["BRANCHBOX_CLI_PATH": "/gone/branchbox"],
                             settings: "relative/branchbox", path: "/Users/u/tools")
        let tilde = locate(fileSystem, settings: "~/tools/branchbox", path: nil)

        #expect(missing.rejected == [
            RejectedCandidate(path: "/gone/branchbox", reason: "Does not exist (from BRANCHBOX_CLI_PATH)"),
            RejectedCandidate(path: "relative/branchbox", reason: "Not an absolute path (from Settings)"),
        ])
        #expect(missing.resolution?.path == "/Users/u/tools/branchbox")
        #expect(tilde.resolution == CLIResolution(path: "/Users/u/tools/branchbox", source: .settingsOverride))
    }

    @Test func nothingFoundListsEverySearchedPath() throws {
        let locator = CLILocator(fileSystem: FakeFileSystem())

        let outcome = locator.locate(processEnvironment: [:], settingsOverride: nil, searchPath: "/a:/opt/homebrew/bin",
                                     home: home, bundleURL: URL(fileURLWithPath: "/tmp/xctest"))

        let searched = ["/a/branchbox", "/opt/homebrew/bin/branchbox", "/usr/local/bin/branchbox",
                        "/Users/u/.cargo/bin/branchbox", "/Users/u/.local/bin/branchbox"]
        #expect(outcome.resolution == nil)
        #expect(outcome.searched == searched)
        #expect(throws: BackendError.cliNotFound(searched: searched)) {
            try locator.resolve(processEnvironment: [:], settingsOverride: nil, searchPath: "/a:/opt/homebrew/bin",
                                home: home, bundleURL: nil)
        }
    }

    @Test func pathsAreKeptUnresolved() throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let real = try cli.script("Cellar/branchbox/0.13.4/bin/branchbox", "echo branchbox 0.13.4")
        let bin = cli.file("bin")
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(atPath: bin.appendingPathComponent("branchbox").path,
                                                   withDestinationPath: "../Cellar/branchbox/0.13.4/bin/branchbox")

        let outcome = CLILocator().locate(processEnvironment: [:], settingsOverride: nil, searchPath: bin.path,
                                          home: home, bundleURL: nil)

        #expect(outcome.resolution?.path == bin.appendingPathComponent("branchbox").path)
        #expect(outcome.resolution?.path != real.path)
        #expect(outcome.resolution?.source == .loginShellPath)
    }
}

@Suite struct LocalFileSystemTests {
    @Test func kindsFollowSymbolicLinks() throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let executable = try cli.script("tool", "exit 0")
        let plain = cli.file("plain")
        try Data().write(to: plain)
        let fileManager = FileManager.default
        try fileManager.createSymbolicLink(atPath: cli.file("link").path, withDestinationPath: executable.path)
        try fileManager.createSymbolicLink(atPath: cli.file("dangling").path,
                                           withDestinationPath: cli.file("nope").path)
        let fileSystem = LocalFileSystem()

        #expect(fileSystem.kind(at: executable.path) == .file(executable: true))
        #expect(fileSystem.kind(at: cli.file("link").path) == .file(executable: true))
        #expect(fileSystem.kind(at: plain.path) == .file(executable: false))
        #expect(fileSystem.kind(at: cli.directory.path) == .directory)
        #expect(fileSystem.kind(at: cli.file("dangling").path) == .brokenSymbolicLink)
        #expect(fileSystem.kind(at: cli.file("nope").path) == .missing)
    }

    @Test func identityChangesWithTheModificationTime() throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let tool = try cli.script("tool", "exit 0")
        let fileSystem = LocalFileSystem()
        let before = try #require(fileSystem.identity(at: tool.path))

        try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSinceNow: -3600)],
                                              ofItemAtPath: tool.path)

        let after = try #require(fileSystem.identity(at: tool.path))
        #expect(after.inode == before.inode)
        #expect(after.size == before.size)
        #expect(after != before)
        #expect(fileSystem.identity(at: cli.file("nope").path) == nil)
    }
}

@Suite(.timeLimit(.minutes(1)))
struct CLIProbeTests {
    static let contractCLI = """
        echo "$*" >> "$PROBE_LOG"
        case "$1" in
          version) echo '{"version":"0.14.0-dev+abc123","contract_version":1,"capabilities":["json-error-envelope","teardown-plan","something-new"]}' ;;
          --version) echo "branchbox 0.14.0-dev" ;;
          *) exit 64 ;;
        esac
        """

    /// 0.13.x: clap rejects the `version` subcommand with exit 2.
    static func legacyCLI(_ version: String) -> String {
        """
        echo "$*" >> "$PROBE_LOG"
        case "$1" in
          version) printf "error: unrecognized subcommand 'version'\\n\\nUsage: branchbox <COMMAND>\\n" >&2; exit 2 ;;
          --version) echo "branchbox \(version)" ;;
          *) exit 64 ;;
        esac
        """
    }

    @Test func contractCLIReportsItsCapabilities() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("branchbox", Self.contractCLI)
        let resolution = CLIResolution(path: script.path, source: .loginShellPath)

        let identity = try await makeProbe(cli).identity(for: resolution, environment: environment(cli))

        #expect(identity.kind == .cli(resolution))
        #expect(identity.version == SemVer(0, 14, 0, prerelease: "dev"))
        #expect(identity.contractVersion == 1)
        #expect(identity.capabilities
                == [.jsonErrorEnvelope, .teardownPlan, Capability(rawValue: "something-new")])
        #expect(!identity.isLegacy)
        #expect(try probeLog(cli) == ["version --json"])
    }

    @Test func legacyCLIFallsBackToTheVersionFlag() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let resolution = CLIResolution(path: try cli.script("branchbox", Self.legacyCLI("0.13.4")).path,
                                       source: .wellKnownPath)

        let identity = try await makeProbe(cli).identity(for: resolution, environment: environment(cli))

        #expect(identity.version == SemVer(0, 13, 4))
        #expect(identity.contractVersion == nil)
        #expect(identity.capabilities.isEmpty)
        #expect(identity.isLegacy)
        #expect(try probeLog(cli) == ["version --json", "--version"])
    }

    @Test func undecodableVersionOutputFallsBackToo() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("branchbox", """
            echo "$*" >> "$PROBE_LOG"
            case "$1" in
              version) echo 'version 0.15.0 (not json)' ;;
              --version) echo "branchbox 0.15.0" ;;
            esac
            """)

        let identity = try await makeProbe(cli).identity(for: CLIResolution(path: script.path, source: .embedded),
                                                         environment: environment(cli))

        #expect(identity.version == SemVer(0, 15, 0))
        #expect(identity.contractVersion == nil)
        #expect(try probeLog(cli) == ["version --json", "--version"])
    }

    @Test func olderThanTheMinimumIsTooOld() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("branchbox", Self.legacyCLI("0.13.3"))

        do {
            _ = try await makeProbe(cli).identity(for: CLIResolution(path: script.path, source: .settingsOverride),
                                                  environment: environment(cli))
            Issue.record("expected .cliTooOld")
        } catch let error as BackendError {
            #expect(error == .cliTooOld(found: SemVer(0, 13, 3), minimum: SemVer(0, 13, 4), path: script.path))
        }
    }

    @Test(arguments: [
        ("echo 'Error: registry is corrupt' >&2; exit 1",
         "`branchbox version --json` exited with status 1: Error: registry is corrupt"),
        ("[ \"$1\" = version ] && exit 2; echo 'not a version'", "`branchbox --version` printed \"not a version\""),
        ("[ \"$1\" = version ] && exit 2; exit 3", "`branchbox --version` exited with status 3"),
        ("kill -9 $$", "`branchbox version --json` was killed by signal 9"),
        // A tracing INFO line is never the reason.
        ("echo '2026-10-01T22:50:29.222458Z  INFO worktree_core::git: Created worktree' >&2; exit 1",
         "`branchbox version --json` exited with status 1"),
    ])
    func otherFailuresMakeTheCLIUnusable(_ body: String, _ reason: String) async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("branchbox", body)

        do {
            _ = try await makeProbe(cli).identity(for: CLIResolution(path: script.path, source: .loginShellPath),
                                                  environment: environment(cli))
            Issue.record("expected .cliUnusable")
        } catch let error as BackendError {
            #expect(error == .cliUnusable(path: script.path, reason: reason))
        }
    }

    @Test func aMissingOrHungCLIIsUnusable() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let hung = try cli.script("branchbox", "exec /bin/sleep 30")
        let probe = CLIProbe(runner: ProcessRunner(), cacheDirectory: nil, timeout: .milliseconds(300))

        let gone = BackendError.cliUnusable(path: cli.file("gone").path, reason: "The file no longer exists")
        await #expect(throws: gone) {
            _ = try await probe.identity(for: CLIResolution(path: cli.file("gone").path, source: .loginShellPath),
                                         environment: environment(cli))
        }
        do {
            _ = try await probe.identity(for: CLIResolution(path: hung.path, source: .loginShellPath),
                                         environment: environment(cli))
            Issue.record("expected .cliUnusable")
        } catch BackendError.cliUnusable(_, let reason) {
            #expect(reason.hasPrefix("`branchbox version --json` did not answer within"))
        }
    }

    @Test func resultsAreCachedByFileIdentityInMemoryAndOnDisk() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let script = try cli.script("branchbox", Self.contractCLI)
        let resolution = CLIResolution(path: script.path, source: .loginShellPath)
        let probe = makeProbe(cli)

        _ = try await probe.identity(for: resolution, environment: environment(cli))
        _ = try await probe.identity(for: resolution, environment: environment(cli))
        #expect(try probeLog(cli).count == 1)

        // A fresh probe (the next app launch) reads the persisted result.
        let relaunched = try await makeProbe(cli).identity(for: resolution, environment: environment(cli))
        #expect(relaunched.contractVersion == 1)
        #expect(try probeLog(cli).count == 1)

        // `brew upgrade` swaps the file: a new modification time means a new probe.
        try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSinceNow: -60)],
                                              ofItemAtPath: script.path)
        _ = try await probe.identity(for: resolution, environment: environment(cli))
        #expect(try probeLog(cli).count == 2)

        await probe.forgetCachedResults()
        _ = try await makeProbe(cli).identity(for: resolution, environment: environment(cli))
        #expect(try probeLog(cli).count == 3)
    }

    private func makeProbe(_ cli: FakeCLI) -> CLIProbe {
        CLIProbe(runner: ProcessRunner(), cacheDirectory: cli.file("support"))
    }

    private func environment(_ cli: FakeCLI) -> [String: String] {
        FakeCLI.environment.merging(["PROBE_LOG": cli.file("probe.log").path]) { $1 }
    }

    private func probeLog(_ cli: FakeCLI) throws -> [String] {
        guard FileManager.default.fileExists(atPath: cli.file("probe.log").path) else { return [] }
        return try String(contentsOf: cli.file("probe.log"), encoding: .utf8).split(separator: "\n").map(String.init)
    }
}
