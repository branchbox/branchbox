@testable import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// DESIGN §13.1 environment table: the sentinel parser, the login-shell capture and its fallbacks (with fake
// shells), ChildEnvironment.make, and EnvironmentProvider's provisional/awaited environments and PATH cache.

@Suite struct ChildEnvironmentTests {
    let process = [
        "HOME": "/Users/u", "USER": "u", "LOGNAME": "u", "TMPDIR": "/var/folders/xy/T/",
        "SSH_AUTH_SOCK": "/private/tmp/agent.sock", "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    ]

    func make(_ base: [String: String], cliPath: String? = nil,
              settings: BackendSettings = BackendSettings()) -> [String: String] {
        ChildEnvironment.make(base: base, processEnvironment: process, cliPath: cliPath, settings: settings,
                              home: "/Users/u")
    }

    @Test func dropsShellAndTerminalState() {
        let dropped = [
            "PWD", "OLDPWD", "SHLVL", "_", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "TERM_SESSION_ID", "TERMINFO",
            "ITERM_SESSION_ID", "ITERM_PROFILE", "COLORTERM", "CLICOLOR_FORCE", "PS1", "PS2", "PROMPT",
            "XPC_SERVICE_NAME", "XPC_FLAGS", "__CFBundleIdentifier",
        ]
        var base = Dictionary(uniqueKeysWithValues: dropped.map { ($0, "x") })
        base["TERM"] = "xterm-256color"
        base["LANG"] = "en_CA.UTF-8"
        base["NVM_DIR"] = "/Users/u/.nvm"

        let environment = make(base)

        #expect(dropped.allSatisfy { environment[$0] == nil })
        #expect(environment["TERM"] == "dumb")
        #expect(environment["LANG"] == "en_CA.UTF-8")
        #expect(environment["NVM_DIR"] == "/Users/u/.nvm")
    }

    @Test func identityVariablesComeFromTheProcessOnlyWhenMissing() {
        let environment = make(["HOME": "/Users/login-home", "PATH": "/usr/bin"])

        #expect(environment["HOME"] == "/Users/login-home")
        #expect(environment["USER"] == "u")
        #expect(environment["LOGNAME"] == "u")
        #expect(environment["TMPDIR"] == "/var/folders/xy/T/")
        #expect(environment["SSH_AUTH_SOCK"] == "/private/tmp/agent.sock")
    }

    @Test func pathPutsTheCLIFirstThenTheBaseThenToolDirectoriesWithoutRepeats() {
        let nvm = "/Users/u/.nvm/versions/node/v22/bin"
        let base = ["PATH": "\(nvm):/usr/local/bin:/opt/homebrew/bin/:/usr/bin::/Users/u/.cargo/bin"]

        let environment = make(base, cliPath: "/opt/homebrew/bin/branchbox")

        #expect(environment["PATH"]?.split(separator: ":").map(String.init) == [
            "/opt/homebrew/bin",
            "/Users/u/.nvm/versions/node/v22/bin", "/usr/local/bin", "/usr/bin", "/Users/u/.cargo/bin",
            "/opt/homebrew/sbin", "/Users/u/.local/bin", "/Users/u/.docker/bin",
            "/Applications/Docker.app/Contents/Resources/bin",
            "/bin", "/usr/sbin", "/sbin",
        ])
    }

    @Test func pathWithoutACLIOrABasePathStillHasEveryToolDirectory() {
        let environment = make([:])

        #expect(environment["PATH"] == [
            "/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/Users/u/.cargo/bin", "/Users/u/.local/bin",
            "/Users/u/.docker/bin", "/Applications/Docker.app/Contents/Resources/bin",
            "/usr/bin", "/bin", "/usr/sbin", "/sbin",
        ].joined(separator: ":"))
    }

    @Test func setsMachineFriendlyDefaults() {
        let environment = make(["PATH": "/usr/bin"])

        #expect(environment["NO_COLOR"] == "1")
        #expect(environment["CLICOLOR"] == "0")
        #expect(environment["TERM"] == "dumb")
        #expect(environment["GIT_TERMINAL_PROMPT"] == "0")
        #expect(environment["RUST_BACKTRACE"] == "0")
        #expect(environment["RUST_LOG"] == "info")
        #expect(environment["BRANCHBOX_DEFAULT_AGENT_CMD"] == nil)
        #expect(environment["BRANCHBOX_DEFAULT_AGENT_NAME"] == nil)
    }

    @Test func verboseAgentAndUserLogLevel() {
        let settings = BackendSettings(verboseLogs: true, agentCommand: "claude --dangerously-skip-permissions",
                                       agentName: "Claude")

        let verbose = make(["PATH": "/usr/bin"], settings: settings)
        let userLevel = make(["PATH": "/usr/bin", "RUST_LOG": "warn"], settings: settings)
        let emptyAgent = make([:], settings: BackendSettings(agentCommand: "", agentName: ""))

        #expect(verbose["RUST_LOG"] == "debug")
        #expect(verbose["BRANCHBOX_DEFAULT_AGENT_CMD"] == "claude --dangerously-skip-permissions")
        #expect(verbose["BRANCHBOX_DEFAULT_AGENT_NAME"] == "Claude")
        #expect(userLevel["RUST_LOG"] == "warn")
        #expect(emptyAgent["BRANCHBOX_DEFAULT_AGENT_CMD"] == nil)
        #expect(emptyAgent["BRANCHBOX_DEFAULT_AGENT_NAME"] == nil)
    }

    @Test func extraEnvironmentIsAppliedLast() {
        let extras = ["NO_COLOR": "0", "PATH": "/custom/bin", "RUST_LOG": "trace", "TERM": "xterm",
                      "CLOUDFLARE_API_TOKEN": "secret"]

        let environment = make(["PATH": "/usr/bin"], cliPath: "/opt/homebrew/bin/branchbox",
                               settings: BackendSettings(extraEnvironment: extras, verboseLogs: true))

        for (key, value) in extras { #expect(environment[key] == value) }
    }
}

@Suite(.timeLimit(.minutes(1)))
struct LoginShellEnvironmentTests {
    @Test func parsesBetweenSentinelsIgnoringRCNoise() throws {
        var output = Data("Last login: Thu Oct  1 on ttys001\n__BRANCHBOX_ENV_BEGIN__ in a banner\n\n".utf8)
        output.append(Data("__BRANCHBOX_ENV_BEGIN__\n".utf8))
        output.append(Data("PATH=/opt/homebrew/bin:/usr/bin\0MULTI=line1\nline2\0EQ=a=b\0EMPTY=\0=nameless\0".utf8))
        output.append(Data("\n__BRANCHBOX_ENV_END__\nlogout noise\n".utf8))

        let variables = try #require(LoginShellEnvironment.parse(output))

        #expect(variables == ["PATH": "/opt/homebrew/bin:/usr/bin", "MULTI": "line1\nline2", "EQ": "a=b", "EMPTY": ""])
    }

    @Test(arguments: [
        "noise\n__BRANCHBOX_ENV_BEGIN__\nPATH=/usr/bin\0",
        "noise\nPATH=/usr/bin\0\n__BRANCHBOX_ENV_END__\n",
        "__BRANCHBOX_ENV_BEGIN__\nHOME=/Users/u\0\n__BRANCHBOX_ENV_END__\n",
        "__BRANCHBOX_ENV_BEGIN__\nPATH=\0\n__BRANCHBOX_ENV_END__\n",
        "",
    ])
    func incompleteOutputIsRejected(_ output: String) {
        #expect(LoginShellEnvironment.parse(Data(output.utf8)) == nil)
    }

    @Test func captureScriptRoundTripsTrickyValues() async throws {
        let environment = ["PATH": "/usr/bin:/bin", "MULTI": "a\nb", "EQ": "x=y", "SPACE": "with space"]
        let spec = ProcessSpec(executable: URL(fileURLWithPath: "/bin/sh"),
                               arguments: ["-c", LoginShellEnvironment.captureScript], environment: environment,
                               workingDirectory: nil)

        let result = try await ProcessRunner().run(spec) { _ in }
        let variables = try #require(LoginShellEnvironment.parse(result.stdout))

        for (key, value) in environment { #expect(variables[key] == value) }
    }

    @Test func capturesTheInteractiveLoginEnvironment() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let shell = try await fakeShell(in: cli)
        var process = FakeCLI.environment
        process["TERM"] = "xterm-256color"
        process["TERM_PROGRAM"] = "iTerm.app"

        let captured = try #require(await LoginShellEnvironment.capture(shell: shell.path, processEnvironment: process,
                                                                         runner: ProcessRunner()))

        #expect(captured.source == .interactiveLogin)
        #expect(captured.shell == shell.path)
        #expect(captured.variables["PATH"] == "/opt/fake-login/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        #expect(captured.variables["TERM"] == "dumb")
        #expect(captured.variables["TERM_PROGRAM"] == nil)
        #expect(try invocations(in: cli) == ["interactive"])
    }

    @Test func fallsBackToLoginWhenInteractiveHangs() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let shell = try await fakeShell(in: cli, behavior: "[ $interactive = yes ] && exec /bin/sleep 30")
        let started = ContinuousClock.now

        let captured = try #require(await LoginShellEnvironment.capture(
            shell: shell.path, processEnvironment: FakeCLI.environment, runner: ProcessRunner(),
            interactiveTimeout: .milliseconds(500)))

        #expect(captured.source == .login)
        #expect(captured.variables["PATH"]?.hasPrefix("/opt/fake-login/bin:") == true)
        #expect(ContinuousClock.now - started < .seconds(4))
        #expect(try invocations(in: cli) == ["interactive", "login"])
    }

    @Test func fallsBackToLoginWhenTheEndSentinelIsMissing() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let shell = try await fakeShell(in: cli, behavior: """
            if [ $interactive = yes ]; then printf '\\n__BRANCHBOX_ENV_BEGIN__\\n'; /usr/bin/env -0; exit 0; fi
            """)

        let captured = try #require(await LoginShellEnvironment.capture(
            shell: shell.path, processEnvironment: FakeCLI.environment, runner: ProcessRunner()))

        #expect(captured.source == .login)
    }

    @Test func returnsNilWhenBothAttemptsFail() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let shell = try await fakeShell(in: cli, behavior: "exit 1")

        let captured = await LoginShellEnvironment.capture(shell: shell.path, processEnvironment: FakeCLI.environment,
                                                           runner: ProcessRunner())

        #expect(captured == nil)
        #expect(try invocations(in: cli) == ["interactive", "login"])
    }

    @Test func jobControlIsTurnedOffForPOSIXShellsOnly() {
        #expect(LoginShellEnvironment.interactiveFlags(for: "/bin/zsh") == ["-l", "-i", "+m", "-c"])
        #expect(LoginShellEnvironment.interactiveFlags(for: "/opt/homebrew/bin/bash") == ["-l", "-i", "+m", "-c"])
        #expect(LoginShellEnvironment.interactiveFlags(for: "/opt/homebrew/bin/fish") == ["-l", "-i", "-c"])
    }

    @Test func userShellIsAnAbsoluteExecutable() {
        let shell = LoginShellEnvironment.userShell(processEnvironment: [:])

        #expect(shell.hasPrefix("/"))
        #expect(FileManager.default.isExecutableFile(atPath: shell))
    }
}

@Suite(.timeLimit(.minutes(1)))
struct EnvironmentProviderTests {
    @Test func readDoesNotWaitForTheCaptureAndMutationDoes() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        try seedCache(in: cli, path: "/cached/bin:/usr/bin", capturedAt: "2026-09-30T12:00:00Z")
        let provider = try await makeProvider(cli, behavior: "/bin/sleep 1.5")
        await provider.startCapture()
        let started = ContinuousClock.now

        let read = await provider.childEnvironment(for: .read, settings: BackendSettings())
        let provisional = await provider.summary()

        #expect(ContinuousClock.now - started < .milliseconds(700))
        #expect(read["PATH"]?.hasPrefix("/cached/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:") == true)
        #expect(provisional.isProvisional)
        #expect(provisional.source == .cachedPath)
        #expect(provisional.capturedAt == RFC3339.parse("2026-09-30T12:00:00Z"))
        #expect(provisional.captureDuration == nil)

        let mutation = await provider.childEnvironment(for: .mutation, settings: BackendSettings())
        let summary = await provider.summary()
        let readAfter = await provider.childEnvironment(for: .read, settings: BackendSettings())

        #expect(mutation["PATH"]?.hasPrefix("/opt/fake-login/bin:/usr/bin:") == true)
        #expect(readAfter == mutation)
        #expect(!summary.isProvisional)
        #expect(summary.source == .interactiveLogin)
        #expect(summary.captureDuration != nil)
        #expect(summary.pathEntries.first == "/opt/fake-login/bin")
        #expect(summary.shell == cli.file("shell").path)
    }

    @Test func provisionalEnvironmentWithoutACacheIsTheProcessEnvironment() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let provider = try await makeProvider(cli, behavior: "/bin/sleep 1.5")

        let read = await provider.childEnvironment(for: .read, settings: BackendSettings())
        let summary = await provider.summary()

        #expect(read["PATH"]?.hasPrefix("/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:") == true)
        #expect(summary.isProvisional)
        #expect(summary.source == .processEnvironment)
        #expect(summary.capturedAt == nil)
    }

    @Test func persistsOnlyThePATH() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let provider = try await makeProvider(cli)

        _ = await provider.childEnvironment(for: .mutation, settings: BackendSettings())

        let data = try Data(contentsOf: cacheFile(in: cli))
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(Set(object.keys) == ["PATH", "captured_at"])
        #expect(object["PATH"] as? String == "/opt/fake-login/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        #expect(RFC3339.parse(object["captured_at"] as? String ?? "") != nil)
        #expect(!String(decoding: data, as: UTF8.self).contains("hunter2"))
    }

    @Test func failedCaptureFallsBackToTheProcessEnvironment() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let provider = try await makeProvider(cli, behavior: "exit 1")

        let mutation = await provider.childEnvironment(for: .mutation, settings: BackendSettings())
        let read = await provider.childEnvironment(for: .read, settings: BackendSettings())
        let summary = await provider.summary()

        #expect(mutation["PATH"]?.hasPrefix("/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:") == true)
        #expect(read == mutation)
        #expect(summary.source == .processEnvironment)
        #expect(!summary.isProvisional)
        #expect(!FileManager.default.fileExists(atPath: cacheFile(in: cli).path))
    }

    @Test func recaptureRunsTheShellAgainAndKeepsTheLastGoodEnvironment() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let provider = try await makeProvider(cli, behavior: """
            [ -f "$here/fail" ] && exit 1
            export PATH="$(cat "$here/login-path"):$PATH"
            """)
        try Data("/first/bin".utf8).write(to: cli.file("login-path"))

        let first = await provider.childEnvironment(for: .mutation, settings: BackendSettings())
        try Data("/second/bin".utf8).write(to: cli.file("login-path"))
        await provider.recapture()
        let second = await provider.childEnvironment(for: .read, settings: BackendSettings())
        try Data().write(to: cli.file("fail"))
        await provider.recapture()
        let third = await provider.childEnvironment(for: .read, settings: BackendSettings())

        #expect(first["PATH"]?.split(separator: ":").contains("/first/bin") == true)
        #expect(second["PATH"]?.split(separator: ":").contains("/second/bin") == true)
        #expect(second["PATH"]?.split(separator: ":").contains("/first/bin") == false)
        #expect(third == second)
        #expect(try invocations(in: cli) == ["interactive", "interactive", "interactive", "login"])
    }

    @Test func cliDirectoryAndSettingsShapeTheChildEnvironment() async throws {
        let cli = try FakeCLI()
        defer { cli.remove() }
        let provider = try await makeProvider(cli)
        await provider.setCLIPath("/opt/tools/bin/branchbox")
        let settings = BackendSettings(extraEnvironment: ["EXTRA": "1"], verboseLogs: true)

        let environment = await provider.childEnvironment(for: .mutation, settings: settings)
        let summary = await provider.summary()

        #expect(environment["PATH"]?.hasPrefix("/opt/tools/bin:/opt/fake-login/bin:") == true)
        #expect(environment["RUST_LOG"] == "debug")
        #expect(environment["EXTRA"] == "1")
        #expect(summary.pathEntries.prefix(2) == ["/opt/tools/bin", "/opt/fake-login/bin"])
    }

    private func makeProvider(_ cli: FakeCLI, behavior: String = "") async throws -> EnvironmentProvider {
        let shell = try await fakeShell(in: cli, behavior: behavior)
        let configuration = EnvironmentProvider.Configuration(shell: shell.path,
                                                              processEnvironment: FakeCLI.environment,
                                                              home: "/Users/tester",
                                                              cacheDirectory: cli.file("support"))
        return EnvironmentProvider(configuration: configuration)
    }

    private func cacheFile(in cli: FakeCLI) -> URL {
        cli.file("support").appendingPathComponent(EnvironmentProvider.cacheFileName)
    }

    private func seedCache(in cli: FakeCLI, path: String, capturedAt: String) throws {
        try FileManager.default.createDirectory(at: cli.file("support"), withIntermediateDirectories: true)
        let json = "{\"PATH\": \"\(path)\", \"captured_at\": \"\(capturedAt)\"}"
        try Data(json.utf8).write(to: cacheFile(in: cli))
    }
}

/// A stand-in for `zsh -l [-i] -c SCRIPT`: logs whether it ran interactive, prints rc-file noise around the
/// script, and exports a login PATH and a secret. `behavior` runs after `$interactive` and `$here` are set.
/// Warmed, so timeouts measure the shell rather than the first exec of a new file.
private func fakeShell(in cli: FakeCLI, behavior: String = "") async throws -> URL {
    try await cli.warmedScript("shell", """
        here="$(dirname "$0")"
        interactive=no
        for arg in "$@"; do [ "$arg" = "-i" ] && interactive=yes; done
        if [ $interactive = yes ]; then mode=interactive; else mode=login; fi
        echo $mode >> "$here/invocations"
        for script; do :; done
        echo "Last login: Thu Oct  1 09:00:00 on ttys001"
        echo "compinit: insecure directories" >&2
        \(behavior)
        export PATH="/opt/fake-login/bin:$PATH"
        export SECRET_TOKEN=hunter2
        /bin/sh -c "$script"
        echo "Saving session..."
        """)
}

private func invocations(in cli: FakeCLI) throws -> [String] {
    try String(contentsOf: cli.file("invocations"), encoding: .utf8).split(separator: "\n").map(String.init)
}
