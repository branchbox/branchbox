@testable import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

@Suite struct HostToolProbeTests {
    @Test func probesEachToolOnTheChildPath() async throws {
        let tools = try FakeCLI()
        defer { tools.remove() }
        // Warmed: the first exec of a new script can take a few hundred milliseconds, which would race the probe
        // timeout.
        try await tools.warmedScript("git", #"echo "git version 2.39.5 (Apple Git-154)""#)
        try await tools.warmedScript("docker", """
            case "$1" in
              --version) echo "Docker version 27.3.1, build ce12230" ;;
              info) echo "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?" >&2; exit 1 ;;
              compose) echo "Docker Compose version v2.29.7-desktop.1" ;;
            esac
            """)
        try await tools.warmedScript("devcontainer", "echo 0.71.0")
        try await tools.warmedScript("sbx", #"echo "Error: not signed in. Sign in with: sbx login" >&2; exit 1"#)
        try await tools.warmedScript("op", "exec /bin/sleep 30")
        let environment = ["PATH": tools.directory.path, "HOME": NSHomeDirectory()]

        let started = ContinuousClock.now
        let checks = await HostToolProbe(runner: ProcessRunner(), environment: environment, timeout: .seconds(2)).checks()
        #expect(ContinuousClock.now - started < .seconds(10))
        let byID = Dictionary(uniqueKeysWithValues: checks.map { ($0.id, $0) })

        #expect(checks.map(\.id) == HostToolProbe.checkOrder)
        #expect(byID["git"]?.status == .ok && byID["git"]?.version == "2.39.5", "\(String(describing: byID["git"]))")
        #expect(byID["git"]?.path == tools.file("git").path)
        #expect(byID["docker.cli"]?.version == "27.3.1")
        #expect(byID["docker.daemon"]?.status == .error && byID["docker.daemon"]?.required == true)
        #expect(byID["docker.daemon"]?.detail?.hasPrefix("Cannot connect to the Docker daemon") == true)
        #expect(byID["docker.daemon"]?.remediation == "Start Docker Desktop")
        #expect(byID["docker.compose"]?.version == "2.29.7-desktop.1")
        #expect(byID["devcontainer.cli"]?.version == "0.71.0")
        #expect(byID["runtime.sbx"]?.status == .warn && byID["runtime.sbx"]?.remediation == "Sign in with: sbx login")
        #expect(byID["op"]?.status == .warn && byID["op"]?.detail == "timed out after 2 s")
        #expect(byID["gh"]?.status == .skipped)
    }

    @Test func missingToolsAreNamedAndDependentChecksSkipped() async {
        let checks = await HostToolProbe(runner: ProcessRunner(), environment: ["PATH": "/nonexistent"],
                                         fileSystem: FakeFileSystem()).checks()
        let byID = Dictionary(uniqueKeysWithValues: checks.map { ($0.id, $0) })
        #expect(byID["git"]?.status == .error && byID["git"]?.detail == "git was not found on PATH")
        #expect(byID["docker.cli"]?.status == .error)
        #expect(byID["docker.daemon"]?.status == .skipped && byID["docker.compose"]?.status == .skipped)
        #expect(byID["devcontainer.cli"]?.status == .warn)
        #expect(byID["runtime.sbx"]?.status == .skipped)
        #expect(byID["op"]?.status == .skipped && byID["gh"]?.status == .skipped)
    }

    @Test func sbxPathOverrideIsUsed() async throws {
        let tools = try FakeCLI()
        defer { tools.remove() }
        let sbx = try await tools.warmedScript("custom-sbx", "exit 0")
        let checks = await HostToolProbe(runner: ProcessRunner(),
                                         environment: ["PATH": "/nonexistent", "BRANCHBOX_SBX_PATH": sbx.path]).checks()
        #expect(checks.first { $0.id == "runtime.sbx" }?.status == .ok)
        #expect(checks.first { $0.id == "runtime.sbx" }?.path == sbx.path)
    }

    @Test func versionTokensAreFound() {
        #expect(HostToolProbe.version(in: "gh version 2.62.0 (2024-11-14)\nhttps://github.com/cli/cli") == "2.62.0")
        #expect(HostToolProbe.version(in: "2.30.0\n") == "2.30.0")
        #expect(HostToolProbe.version(in: "tool (no version)") == "tool (no version)")
        #expect(HostToolProbe.version(in: "") == nil)
    }
}

@Suite struct CLIBackendBootstrapperTests {
    /// A provider whose login shell fails at once, so the process environment (with `path`) is used.
    private func environment(_ cli: FakeCLI, path: String, extra: [String: String] = [:]) throws -> EnvironmentProvider {
        let shell = try cli.script("failing-shell", "exit 1")
        var processEnvironment = ["PATH": path, "HOME": NSHomeDirectory()]
        processEnvironment.merge(extra) { _, new in new }
        let configuration = EnvironmentProvider.Configuration(shell: shell.path, processEnvironment: processEnvironment,
                                                              home: NSHomeDirectory(), cacheDirectory: nil)
        return EnvironmentProvider(runner: ProcessRunner(), configuration: configuration)
    }

    @Test func contractCLIOnTheLoginPathIsReady() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let bin = cli.directory.appendingPathComponent("bin")
        try cli.script("bin/branchbox", """
            if [ "$1" = version ]; then
              echo '{"version":"0.14.0","contract_version":1,"capabilities":["json-error-envelope","registry-lock"]}'
              exit 0
            fi
            exit 3
            """)
        let runner = ProcessRunner()
        let bootstrapper = CLIBackendBootstrapper(runner: runner, environment: try environment(cli, path: bin.path + ":/usr/bin:/bin"),
                                                  probe: CLIProbe(runner: runner, cacheDirectory: nil), bundleURL: nil)
        let result = await bootstrapper.bootstrap(BackendSettings(verboseLogs: true))
        guard case .ready(let backend, let identity) = result else {
            Issue.record("expected .ready, got \(result)")
            return
        }
        #expect(identity.contractVersion == 1 && identity.version == SemVer(0, 14, 0))
        #expect(identity.supports(.registryLock) && !identity.isLegacy)
        #expect(identity.kind == .cli(CLIResolution(path: bin.appendingPathComponent("branchbox").path, source: .loginShellPath)))
        let cliBackend = try #require(backend as? CLIBackend)
        #expect(cliBackend.executable.path == bin.appendingPathComponent("branchbox").path)
        // identity() re-checks through the cached probe.
        #expect(try await backend.identity() == identity)

        let summary = try #require(await bootstrapper.environmentSummary())
        #expect(summary.pathEntries.first == bin.path, "the CLI's directory leads the child PATH")
        await bootstrapper.recaptureEnvironment()
        await bootstrapper.terminateAllProcesses()
        #expect(runner.liveRunCount == 0)
    }

    /// D-8: on a first launch (no remembered PATH) the login PATH is searched before the well-known folders, so a
    /// CLI only the login shell knows wins over `/opt/homebrew/bin/branchbox` (present or not on this machine).
    @Test func firstLaunchWaitsForTheLoginPathBeforeTheWellKnownFolders() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let bin = cli.directory.appendingPathComponent("login-bin")
        let fake = try cli.script("login-bin/branchbox", """
            [ "$1" = version ] && { echo '{"version":"0.14.0","contract_version":1,"capabilities":[]}'; exit 0; }
            exit 3
            """)
        let shell = try await cli.warmedScript("shell", """
            for script; do :; done
            export PATH="\(bin.path):$PATH"
            /bin/sh -c "$script"
            """)
        let runner = ProcessRunner()
        let configuration = EnvironmentProvider.Configuration(shell: shell.path,
                                                              processEnvironment: ["PATH": "/usr/bin:/bin", "HOME": NSHomeDirectory()],
                                                              home: NSHomeDirectory(), cacheDirectory: nil)
        let bootstrapper = CLIBackendBootstrapper(runner: runner,
                                                  environment: EnvironmentProvider(runner: runner, configuration: configuration),
                                                  probe: CLIProbe(runner: runner, cacheDirectory: nil), bundleURL: nil)
        let result = await bootstrapper.bootstrap(BackendSettings())
        guard case .ready(_, let identity) = result else {
            Issue.record("expected .ready, got \(result)")
            return
        }
        #expect(identity.kind == .cli(CLIResolution(path: fake.path, source: .loginShellPath)))
        #expect(await bootstrapper.environmentSummary()?.source == .interactiveLogin)
    }

    @Test func missingCLIIsUnavailableNamingEverywhereSearched() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let bootstrapper = CLIBackendBootstrapper(runner: ProcessRunner(),
                                                  environment: try environment(cli, path: "/nowhere/bin"),
                                                  locator: CLILocator(fileSystem: FakeFileSystem()),
                                                  probe: CLIProbe(runner: ProcessRunner(), cacheDirectory: nil),
                                                  bundleURL: nil)
        let result = await bootstrapper.bootstrap(BackendSettings(cliPathOverride: "/custom/branchbox"))
        guard case .unavailable(.cliNotFound(let searched), .none) = result else {
            Issue.record("expected cliNotFound, got \(result)")
            return
        }
        #expect(searched.first == "/custom/branchbox")
        #expect(searched.contains("/nowhere/bin/branchbox"))
        #expect(searched.contains("/opt/homebrew/bin/branchbox"))
    }

    @Test func tooOldCLIIsUnavailableWithItsResolution() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let old = try cli.script("old/branchbox", """
            [ "$1" = --version ] && { echo "branchbox 0.13.3"; exit 0; }
            echo "error: unrecognized subcommand 'version'" >&2; exit 2
            """)
        let runner = ProcessRunner()
        let bootstrapper = CLIBackendBootstrapper(runner: runner,
                                                  environment: try environment(cli, path: "/usr/bin",
                                                                               extra: ["BRANCHBOX_CLI_PATH": old.path]),
                                                  probe: CLIProbe(runner: runner, cacheDirectory: nil), bundleURL: nil)
        let result = await bootstrapper.bootstrap(BackendSettings())
        guard case .unavailable(let error, let resolution?) = result else {
            Issue.record("expected unavailable with a resolution, got \(result)")
            return
        }
        #expect(error == .cliTooOld(found: SemVer(0, 13, 3), minimum: SemVer(0, 13, 4), path: old.path))
        #expect(resolution == CLIResolution(path: old.path, source: .environmentOverride))
    }
}
