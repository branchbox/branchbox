@testable import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

private func fixture(_ name: String) throws -> String { try Fixtures.string("cli-0.13.4/\(name)") }

@Suite struct CLIBackendTests {
    private var contract: BackendIdentity { Scripted.identity(contract: true, Scripted.everything) }

    // MARK: - In-band payloads

    @Test func execExitOneStillReturnsTheInnerExitCode() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "exec"], 1, stdout: try fixture("sandbox_exec_alpha_fail.json"),
                  stderr: ["Error: Runtime command exited with status 3"]),
        ])
        let environment = StaticEnvironment()
        let result = try await Scripted.backend(runner, environment: environment)
            .exec(ExecRequest(feature: Scripted.eta, command: ["sh", "-c", "exit 3"], timeout: .seconds(30)), progress: { _ in })
        #expect(result == ExecResult(exitCode: 3, stdout: "out\n", stderr: "err\n"))
        let spec = try #require(runner.specs.first)
        #expect(Array(spec.arguments.prefix(6)) == ["feature", "exec", "--repo", "/r/main", "--json", "eta"])
        #expect(spec.timeout == .seconds(30))
        #expect(spec.workingDirectory?.path == "/r/main")
        #expect(spec.standardInput == nil)
        #expect(environment.purposes == [.mutation])
    }

    @Test func devcontainerErrorOutcomeIsCommandFailedNamingDocker() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["branchbox", "devcontainer", "up"], 1,
                  stdout: try fixture("synthetic_devcontainer_up_docker_unavailable.json")),
        ])
        let error = await backendError {
            try await Scripted.backend(runner).devcontainer(.up(removeExisting: false, buildNoCache: false), for: Scripted.eta,
                                                            progress: { _ in })
        }
        guard case .commandFailed(let diagnostics)? = error else {
            Issue.record("expected commandFailed, got \(String(describing: error))")
            return
        }
        #expect(diagnostics.summary == "Docker is not available")
        #expect(diagnostics.exitCode == 1)
        let up = try #require(runner.specs.last)
        #expect(up.arguments == ["devcontainer", "up", "/r/eta", "--json"])
        #expect(up.workingDirectory?.path == "/r/eta")
    }

    @Test func devcontainerUpDecodesItsCamelCasePayload() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["branchbox", "devcontainer", "up"], stdout: try fixture("synthetic_devcontainer_up.json")),
        ])
        let result = try await Scripted.backend(runner).devcontainer(.up(removeExisting: true, buildNoCache: true),
                                                                    for: Scripted.eta, progress: { _ in })
        #expect(result.outcome == "created" && result.containerID == "3f2a9c1d7e4b" && result.remoteUser == "vscode")
    }

    @Test func devcontainerExecRunsInTheWorktreeAndReadsCamelCase() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["branchbox", "devcontainer", "exec"], 1, stdout: try fixture("synthetic_devcontainer_exec.json")),
        ])
        let result = try await Scripted.backend(runner)
            .exec(ExecRequest(feature: Scripted.eta, command: ["false"], target: .devcontainer), progress: { _ in })
        #expect(result.exitCode == 3 && result.outcome == "error" && result.stdout == "out\n")
        let exec = try #require(runner.specs.last)
        #expect(exec.arguments == ["devcontainer", "exec", "-w", "/r/eta", "--json", "--", "false"])
        #expect(exec.workingDirectory?.path == "/r/eta")
        #expect(exec.timeout == nil)
    }

    @Test func devcontainerDownAndBuildReportTheirPhases() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["branchbox", "devcontainer", "down"], stdout: try fixture("synthetic_devcontainer_down.json")),
            .exit(["branchbox", "devcontainer", "build"], stdout: try fixture("synthetic_devcontainer_build.json")),
        ])
        let backend = Scripted.backend(runner)
        let down = ProgressCollector()
        let removed = try await backend.devcontainer(.down(removeVolumes: true), for: Scripted.eta, progress: down.sink)
        #expect(down.phases == [.cleaningRuntime] && !removed.isError)
        let build = ProgressCollector()
        let built = try await backend.devcontainer(.build(noCache: false), for: Scripted.eta, progress: build.sink)
        #expect(build.phases == [.building] && built.imageName != nil)
        #expect(runner.specs.filter { $0.arguments.first == "devcontainer" }.map(\.arguments) == [
            ["devcontainer", "down", "/r/eta", "--json", "-v"], ["devcontainer", "build", "/r/eta", "--json"],
        ])
    }

    @Test func tunnelOpenAndRemoveDecodeTheirChanges() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "tunnel", "open"],
                  stdout: #"{"work_feature":"eta","state":{"provider":"cloudflared","hostname":"eta.example.com","status":"active"},"warnings":[]}"#),
            .exit(["branchbox", "tunnel", "remove"],
                  stdout: #"{"work_feature":"eta","previous_state":{"status":"active"},"updated_state":{"status":"disabled"},"warnings":["kept DNS"]}"#),
        ])
        let backend = Scripted.backend(runner)
        let progress = ProgressCollector()
        let opened = try await backend.openTunnel(Scripted.eta, progress: progress.sink)
        #expect(opened.state?.status == .active && opened.state?.hostname == "eta.example.com")
        #expect(progress.phases == [.provisioningTunnel])
        let removed = try await backend.removeTunnel(Scripted.eta, force: false, progress: { _ in })
        #expect(removed.previousState?.status == .active && removed.state?.status == .disabled && removed.warnings == ["kept DNS"])
        #expect(runner.specs.map(\.arguments) == [["tunnel", "open", "eta", "--repo", "/r/main", "--json"],
                                                  ["tunnel", "remove", "eta", "--repo", "/r/main", "--json"]])
    }

    @Test func deleteBranchRunsGitFromTheMainRoot() async throws {
        let runner = ScriptedProcessRunner([.exit(["git", "-C", "/r/main", "branch", "-D", "feature/eta"])])
        try await Scripted.backend(runner).deleteBranch("feature/eta", in: Scripted.project, force: true)
        #expect(runner.invocations == [["/usr/bin/git", "-C", "/r/main", "branch", "-D", "feature/eta"]])
    }

    @Test func legacySyncFailedLineWithExitZeroIsAFailedRow() async throws {
        let text = """
            🔄 Syncing devcontainer configuration to 2 feature worktree(s)

              alpha ... ✓ synced 3 files (copy)
              beta ... ✗ failed: IO error: Permission denied (os error 13)

            ✓ Successfully synced 1 feature worktree(s)

            ⚠️  1 error(s) occurred:
              - beta: IO error: Permission denied (os error 13)
            """
        let runner = ScriptedProcessRunner([
            ScriptedProcessRunner.Rule(["branchbox", "devcontainer", "sync"],
                                       lines: text.split(separator: "\n").map { OutputLine(channel: .stdout, text: String($0)) },
                                       outcome: .exit(0, stdout: text)),
        ])
        let progress = ProgressCollector()
        let report = try await Scripted.backend(runner).syncDevcontainers(SyncRequest(project: Scripted.project, strategy: .copy),
                                                                         progress: progress.sink)
        #expect(report.failedCount == 1)
        #expect(report.rows.first { $0.feature == "beta" }?.status == .failed)
        #expect(runner.specs.first?.arguments == ["devcontainer", "sync", "-p", "/r/main", "-s", "copy"])
        #expect(runner.specs.first?.streamStdout == true)
        #expect(progress.logs.contains { $0.source == .stdout && $0.message.contains("✗ failed") })

        // Selecting features needs the JSON surface.
        let error = await backendError {
            try await Scripted.backend(runner).syncDevcontainers(SyncRequest(project: Scripted.project, features: ["beta"]),
                                                                 progress: { _ in })
        }
        #expect(error == .unsupported(.devcontainerSyncJSON, minimumCLI: "0.14.0"))
    }

    @Test func legacySyncWithoutASourceIsDevcontainerSourceMissing() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "devcontainer", "sync"], 1, stdout: try fixture("sandbox_devcontainer_sync.stdout"),
                  stderr: try fixture("sandbox_devcontainer_sync.stderr").split(separator: "\n", omittingEmptySubsequences: false).map(String.init)),
        ])
        let error = await backendError {
            try await Scripted.backend(runner).syncDevcontainers(SyncRequest(project: Scripted.project), progress: { _ in })
        }
        #expect(error?.refusal?.cause == .devcontainerSourceMissing)
    }

    @Test func contractSyncReadsTheReportEvenWhenARowFailed() async throws {
        let payload = #"{"schema_version":1,"dry_run":false,"strategy":"copy","results":[{"work_feature":"eta","worktree_path":"/r/eta","status":"failed","files":[],"skip_reason":null,"error":"boom","registry_updated":false},{"work_feature":"zeta","status":"synced","files":["devcontainer.json"]}],"synced":1,"failed":1,"skipped":0}"#
        let runner = ScriptedProcessRunner([.exit(["branchbox", "devcontainer", "sync"], 1, stdout: payload)])
        let report = try await Scripted.backend(runner, identity: contract)
            .syncDevcontainers(SyncRequest(project: Scripted.project, features: ["eta", "zeta"]), progress: { _ in })
        #expect(report.rows == [SyncReport.Row(feature: "eta", worktreePath: "/r/eta", status: .failed, error: "boom"),
                                SyncReport.Row(feature: "zeta", status: .synced, files: ["devcontainer.json"])])
        #expect(runner.specs.first?.arguments.suffix(5) == ["--feature", "eta", "--feature", "zeta", "--json"])
    }

    // MARK: - Failures

    @Test func cancellationIsCancelledAfterTheGroupIsGone() async throws {
        let runner = ScriptedProcessRunner([ScriptedProcessRunner.Rule(["branchbox", "feature", "start"], outcome: .hang)])
        let request = StartFeatureRequest(project: Scripted.project, name: "eta", runtime: .container)
        let legacy = Task { try await Scripted.backend(runner).startFeature(request, progress: { _ in }) }
        try await eventually { !runner.launched.isEmpty }
        legacy.cancel()
        #expect(await backendError { try await legacy.value } == .cancelled(note: CLIBackend.partialWorktreeNote))

        // git worktree add can be stopped before a write-ahead CLI registers the half-done start.
        let writeAhead = Task { try await Scripted.backend(runner, identity: contract).startFeature(request, progress: { _ in }) }
        try await eventually { runner.launched.count == 2 }
        writeAhead.cancel()
        #expect(await backendError { try await writeAhead.value } == .cancelled(note: CLIBackend.partialWorktreeNote))

        // Already cancelled: nothing is launched.
        let early = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await Scripted.backend(runner).listFeatures(in: Scripted.project, includeRemoved: false)
        }
        if case .cancelled? = await backendError({ try await early.value }) {} else { Issue.record("expected .cancelled") }
        #expect(runner.launched.count == 2)
    }

    @Test func decodeErrorIsDecodeFailedWithTheCLIVersion() async throws {
        let runner = ScriptedProcessRunner([.exit(["branchbox", "feature", "start"], stdout: "{\"not\": \"a summary\"}")])
        let error = await backendError {
            try await Scripted.backend(runner).startFeature(StartFeatureRequest(project: Scripted.project, name: "eta",
                                                                                runtime: .container), progress: { _ in })
        }
        guard case .decodeFailed(let what, let detail, let diagnostics)? = error else {
            Issue.record("expected decodeFailed, got \(String(describing: error))")
            return
        }
        #expect(what == "start summary")
        #expect(detail.contains("work_feature"))
        #expect(diagnostics.cliVersion == "0.13.4")
        #expect(diagnostics.invocation?.hasPrefix("/opt/homebrew/bin/branchbox feature start eta") == true)
    }

    @Test func registryParseErrorIsRegistryCorrupted() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], 1,
                  stderr: ["Error: Configuration error: Failed to parse feature registry: EOF while parsing a list at line 1 column 14"]),
        ])
        let error = await backendError { try await Scripted.backend(runner).listFeatures(in: Scripted.project, includeRemoved: true) }
        guard case .registryCorrupted(let path, _)? = error else {
            Issue.record("expected registryCorrupted, got \(String(describing: error))")
            return
        }
        #expect(path == "/r/main/.branchbox/registry.json")
    }

    @Test func runnerErrorsMapToBackendErrors() async throws {
        let runner = ScriptedProcessRunner([
            ScriptedProcessRunner.Rule(["branchbox", "tunnel", "open"],
                                       outcome: .error(.timedOut(after: .seconds(180), partial: ProcessResult(
                                           termination: .signaled(15), stdout: Data(), stderrTail: ["waiting"], duration: .seconds(180))))),
            ScriptedProcessRunner.Rule(["branchbox", "tunnel", "remove"], outcome: .error(.workingDirectoryMissing("/r/main"))),
            ScriptedProcessRunner.Rule(["branchbox", "feature", "list"], outcome: .error(.stdoutTooLarge(limit: 64))),
        ])
        let backend = Scripted.backend(runner)
        guard case .timedOut(let operation, let after, let diagnostics)? = await backendError({
            try await backend.openTunnel(Scripted.eta, progress: { _ in })
        }) else {
            Issue.record("expected timedOut")
            return
        }
        #expect(operation == "tunnel open" && after == .seconds(180))
        #expect(diagnostics.logTail == ["waiting"] && diagnostics.signal == 15)
        #expect(runner.specs.first?.timeout == .seconds(180))
        #expect(await backendError { try await backend.removeTunnel(Scripted.eta, force: true, progress: { _ in }) }
                == .projectInvalid(.workingDirectoryMissing("/r/main")))
        if case .commandFailed? = await backendError({ try await backend.listFeatures(in: Scripted.project, includeRemoved: false) }) {
        } else {
            Issue.record("expected commandFailed for an oversized output")
        }
    }

    // MARK: - Reads

    @Test func listFeaturesDecodesRecordsAndFindsStrays() async throws {
        let list = try fixture("sandbox_feature_list.json")
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: "Warning: old registry format\n" + list),
            .exit(["git", "-C", "/tmp/bbx/demo", "worktree", "list"], stdout: """
                worktree /tmp/bbx/demo
                HEAD aaa
                branch refs/heads/main

                worktree /tmp/bbx/alpha
                HEAD bbb
                branch refs/heads/feature/alpha

                worktree /tmp/bbx/omega
                HEAD ccc
                branch refs/heads/feature/omega

                """),
        ])
        let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo"))
        let listing = try await Scripted.backend(runner).listFeatures(in: project, includeRemoved: false)
        #expect(listing.features.map(\.workFeature).contains("alpha"))
        #expect(listing.strays.map(\.path) == ["/tmp/bbx/omega"])
        #expect(listing.warnings == ["Warning: old registry format"])
        #expect(runner.specs.first?.arguments == ["feature", "list", "--json", "--repo", "/tmp/bbx/demo"])
        #expect(runner.specs.first?.timeout == .seconds(30))
    }

    @Test func listFeaturesFromAFeatureWorktreeAsksTheMainWorktree() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["git", "-C", "/r/eta", "rev-parse"], stdout: "/r/main/.git\n/r/eta\n"),
            .exit(["branchbox", "feature", "list", "--json", "--repo", "/r/main"], stdout: Scripted.etaRecord),
            .exit(["git", "-C", "/r/main", "worktree", "list"], stdout: "worktree /r/main\nbranch refs/heads/main\n"),
        ])
        let fileSystem = MutableFileSystem(["/r/eta/.git": .file(executable: false)])
        let listing = try await Scripted.backend(runner, fileSystem: fileSystem)
            .listFeatures(in: ProjectRef(root: URL(fileURLWithPath: "/r/eta")), includeRemoved: false)
        #expect(listing.features.map(\.workFeature) == ["eta"])
    }

    @Test func previewNameValidatesThenGenerates() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "name", "validate", "--", "oauth"], stdout: "✓ Valid feature name: oauth\n"),
            .exit(["branchbox", "name", "validate"], 1, stdout: "✗ Invalid feature name: OAuth Integration\n  Feature names must be DNS-safe\n"),
            .exit(["branchbox", "name", "generate", "--", "OAuth Integration"], stdout: "oauth\n"),
            .exit(["branchbox", "name", "generate", "--", "!!!"], stdout: "\n"),
            .exit(["branchbox", "name", "generate", "--", "-x"], stdout: "-x\n"),
        ])
        let backend = Scripted.backend(runner)

        let valid = try await backend.previewName("oauth", in: Scripted.project)
        #expect(valid == NamePreview(input: "oauth", slug: "oauth", valid: true, branchName: "feature/oauth", worktreePath: "/r/oauth"))
        let generated = try await backend.previewName("OAuth Integration", in: Scripted.project)
        #expect(generated.slug == "oauth" && generated.valid && generated.problem == nil)
        let invalid = try await backend.previewName("!!!", in: Scripted.project)
        #expect(!invalid.valid && invalid.slug == nil)
        #expect(invalid.problem == "Invalid feature name: OAuth Integration. Feature names must be DNS-safe")
        let dash = try await backend.previewName("-x", in: Scripted.project)
        #expect(!dash.valid && dash.problem == "Feature names cannot start with “-”")
        let empty = try await backend.previewName("  ", in: Scripted.project)
        #expect(!empty.valid && empty.problem == "Enter a feature name")
        #expect(runner.specs.allSatisfy { $0.timeout == .seconds(5) })
    }

    @Test func legacyDetectParsesTheTextReport() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "detect"], stdout: try fixture("sandbox_detect.txt")),
            .exit(["git", "-C", "/tmp/bbx/demo", "rev-parse"], stdout: "/tmp/bbx/demo/.git\n/tmp/bbx/demo\n"),
        ])
        let report = try await Scripted.backend(runner).detect(URL(fileURLWithPath: "/tmp/bbx/demo"))
        #expect(report.gitRepository && !report.initialized)
        #expect(report.stack == "generic" && report.modules == ["tunnel", "specs"] && report.rawText != nil)
        #expect(runner.specs.first?.arguments == ["detect", "-p", "/tmp/bbx/demo"])
    }

    @Test func contractDetectDecodesJSON() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "detect", "-p", "/abs", "--json"],
                  stdout: #"{"schema_version":1,"project":"/abs","git_repository":true,"initialized":true,"stack":"rust","adapter":"generic","modules":["devcontainer","specs"],"has_devcontainer":true,"has_env":false,"warnings":[]}"#),
        ])
        let report = try await Scripted.backend(runner, identity: contract).detect(URL(fileURLWithPath: "/abs"))
        #expect(report == DetectReport(project: "/abs", gitRepository: true, initialized: true, stack: "rust", adapter: "generic",
                                       modules: ["devcontainer", "specs"], hasDevcontainer: true, hasEnv: false))
    }

    // MARK: - Config and credentials

    @Test func legacyConfigIsReadFromTheFileReadOnly() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests/config-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let project = ProjectRef(root: directory)
        let backend = Scripted.backend(ScriptedProcessRunner())

        let missing = try await backend.readConfig(project)
        #expect(!missing.exists && !missing.editable && missing.effective == .defaults)
        #expect(missing.keys.first { $0.key == "feature.branch_prefix" }?.value == .string("feature"))

        try FileManager.default.createDirectory(at: directory.appendingPathComponent(".branchbox"), withIntermediateDirectories: true)
        try Data(#"{"feature": {"branch_prefix": "spike"}, "runtime": {"provider": "sbx"}}"#.utf8)
            .write(to: directory.appendingPathComponent(".branchbox/config.json"))
        let present = try await backend.readConfig(project)
        #expect(present.exists && present.effective.branchPrefix == "spike" && present.effective.runtimeProvider == .sbx)
        let prefix = try #require(present.keys.first { $0.key == "feature.branch_prefix" })
        #expect(prefix.value == .string("spike") && prefix.source == "file")
        #expect(present.keys.first { $0.key == "tunnel.enabled" }?.source == "default")

        try Data("{ not json".utf8).write(to: directory.appendingPathComponent(".branchbox/config.json"))
        if case .decodeFailed(let what, _, _)? = await backendError({ try await backend.readConfig(project) }) {
            #expect(what == "project config")
        } else {
            Issue.record("expected decodeFailed for an invalid config.json")
        }
        #expect(await backendError {
            try await backend.applyConfig(ConfigPatch(changes: []), to: project, dryRun: false)
        } == .unsupported(.config, minimumCLI: "0.14.0"))
    }

    @Test func contractConfigApplySendsAMergePatchOnStdin() async throws {
        let effective = #"{"feature":{"branch_prefix":"spike"}}"#
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "config", "apply"],
                  stdout: #"{"schema_version":1,"changed":[{"key":"feature.branch_prefix","old":"feature","new":"spike"}],"effective":\#(effective)}"#),
            .exit(["branchbox", "config", "get"],
                  stdout: #"{"schema_version":1,"path":"/r/main/.branchbox/config.json","exists":true,"effective":\#(effective),"file":{},"keys":[{"key":"feature.branch_prefix","type":"string","allowed":[],"default":"feature","value":"spike","source":"file","description":"Prefix"}]}"#),
        ])
        let backend = Scripted.backend(runner, identity: contract)
        let patch = ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix", value: .string("spike")),
                                          ConfigChange(key: "tunnel.providers.cloudflared.account_id", value: nil)])
        let result = try await backend.applyConfig(patch, to: Scripted.project, dryRun: true)
        #expect(result.changed == [.init(key: "feature.branch_prefix", old: .string("feature"), new: .string("spike"))])
        let spec = try #require(runner.specs.first)
        #expect(spec.arguments == ["config", "apply", "--repo", "/r/main", "--file", "-", "--json", "--dry-run"])
        let body = try JSONDecoder().decode(JSONValue.self, from: try #require(spec.standardInput))
        #expect(body == .object(["feature": .object(["branch_prefix": .string("spike")]),
                                 "tunnel": .object(["providers": .object(["cloudflared": .object(["account_id": .null])])])]))

        let document = try await backend.readConfig(Scripted.project)
        #expect(document.editable && document.effective.branchPrefix == "spike" && document.keys.count == 1)
    }

    @Test func tunnelTokenTravelsOnStdinOnly() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "tunnel", "credentials", "set"], 1, stderr: ["Error: rejected token tok-SECRET-1234"]),
        ])
        let backend = Scripted.backend(runner, identity: contract)
        let request = TunnelCredentialsRequest(accountID: "acct", apiToken: SecretString("tok-SECRET-1234"))
        let error = await backendError { try await backend.setTunnelCredentials(request, in: Scripted.project) }
        let spec = try #require(runner.specs.first)
        #expect(spec.standardInput == Data("tok-SECRET-1234".utf8))
        #expect(!spec.arguments.joined(separator: " ").contains("tok-SECRET"))
        #expect(error?.diagnostics?.invocation?.contains("tok-SECRET") == false)

        #expect(await backendError {
            try await Scripted.backend(runner).setTunnelCredentials(request, in: Scripted.project)
        } == .unsupported(.tunnelCredentials, minimumCLI: "0.14.0"))
    }

    // MARK: - Start

    @Test func startSurfacesThePreambleAndRedactsThePrompt() async throws {
        let stderr = try fixture("sandbox_start_gamma_longprompt.stderr").split(separator: "\n").map { ANSI.strip(String($0)) }
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "start"], stdout: try fixture("sandbox_start_gamma_longprompt.json"), stderr: stderr),
        ])
        let environment = StaticEnvironment()
        let progress = ProgressCollector()
        var request = StartFeatureRequest(project: Scripted.project, name: "gamma", runtime: .container)
        request.prompt = String(repeating: "x", count: 2100)
        request.verbose = true
        let backend = Scripted.backend(runner, environment: environment,
                                       settings: BackendSettings(extraEnvironment: ["RUST_LOG": "trace"]))
        let summary = try await backend.startFeature(request, progress: progress.sink)

        #expect(summary.workFeature == "gamma")
        #expect(summary.preambleWarning == "⚠️  Prompt truncated to 2000 characters before storage.")
        #expect(progress.warnings == ["⚠️  Prompt truncated to 2000 characters before storage."])
        #expect(progress.phases.first == .preparing)
        #expect(progress.phases.contains(.creatingWorktree))
        #expect(progress.logs.contains { $0.level == .info && $0.target == "worktree_core::git" })
        let spec = try #require(runner.specs.first)
        #expect(spec.workingDirectory?.path == "/r/main")
        #expect(spec.environment["RUST_LOG"] == "trace", "the user's own RUST_LOG wins over verbose")
        #expect(environment.purposes == [.mutation])

        let preview = try #require(backend.previewCommandLine(.start(request)))
        #expect(preview.contains("--prompt='<redacted 2100 chars>'"))
        #expect(!preview.contains("xxxx"))
    }

    @Test func verboseStartSetsDebugLogging() async throws {
        let runner = ScriptedProcessRunner([.exit(["branchbox", "feature", "start"], stdout: try fixture("sandbox_start_alpha.json"))])
        var request = StartFeatureRequest(project: Scripted.project, name: "alpha", runtime: .container)
        request.verbose = true
        _ = try await Scripted.backend(runner).startFeature(request, progress: { _ in })
        #expect(runner.specs.first?.environment["RUST_LOG"] == "debug")
        #expect(runner.specs.first?.arguments.last == "--telemetry")
    }

    // MARK: - Doctor, status, init

    @Test func legacyDoctorIsTheHostProbeAndContractDoctorIsMerged() async throws {
        let fileSystem = MutableFileSystem(["/usr/bin/git": .file(executable: true)])
        let runner = ScriptedProcessRunner([
            .exit(["git", "--version"], stdout: "git version 2.39.5 (Apple Git-154)\n"),
            .exit(["branchbox", "doctor"], 1,
                  stdout: #"{"schema_version":1,"checks":[{"id":"docker.daemon","title":"Docker daemon","required":true,"status":"error","detail":"timed out after 3s","remediation":"Start Docker Desktop"},{"id":"git","title":"Git","required":true,"status":"ok","version":"2.40"}],"summary":{"ok":1,"warn":0,"error":1}}"#),
        ])
        let legacy = await Scripted.backend(runner, fileSystem: fileSystem).doctor(nil)
        #expect(legacy.source == .app)
        #expect(legacy.checks.map(\.id) == HostToolProbe.checkOrder)
        #expect(legacy.checks.first { $0.id == "git" }?.version == "2.39.5")
        #expect(legacy.checks.first { $0.id == "docker.cli" }?.status == .error)

        let merged = await Scripted.backend(runner, identity: contract, fileSystem: fileSystem).doctor(Scripted.project)
        #expect(merged.source == .merged)
        #expect(merged.checks.first { $0.id == "git" }?.version == "2.40", "the CLI's check wins")
        #expect(merged.checks.first { $0.id == "docker.daemon" }?.status == .error)
        #expect(merged.checks.contains { $0.id == "gh" })
        #expect(runner.specs.contains { $0.arguments == ["doctor", "--repo", "/r/main", "--json"] })
    }

    @Test func devcontainerStatusAsksDockerAndTheCLI() async throws {
        let fileSystem = MutableFileSystem(["/r/eta": .directory, "/usr/local/bin/docker": .file(executable: true)])
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["docker", "ps"], stdout: #"{"ID":"abc123","State":"running","Names":"eta"}"# + "\n"),
            .exit(["branchbox", "devcontainer", "detect"], stdout: try fixture("synthetic_devcontainer_detect.json")),
        ])
        let environment = StaticEnvironment(["PATH": "/usr/local/bin:/usr/bin"])
        let status = try await Scripted.backend(runner, fileSystem: fileSystem, environment: environment)
            .devcontainerStatus(for: Scripted.eta)
        #expect(status.state == .running && status.containerID == "abc123")
        #expect(status.service?.serviceName == "app")
        #expect(runner.specs.first { $0.executable.lastPathComponent == "docker" }?.arguments
                == ["ps", "-a", "--filter", "label=devcontainer.local_folder=/r/eta", "--format", "{{json .}}"])

        let imageOnly = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["docker", "ps"], stdout: #"{"ID":"image123","State":"running"}"# + "\n"),
            .exit(["branchbox", "devcontainer", "detect"], stdout:
                #"{"service_name":null,"port":0,"service_url":"","container_type":"image","container_user":"root","configured_user":"root","workspace_folder":"/workspace"}"#),
        ])
        let imageStatus = try await Scripted.backend(imageOnly, fileSystem: fileSystem, environment: environment)
            .devcontainerStatus(for: Scripted.eta)
        #expect(imageStatus.state == .running && imageStatus.containerID == "image123")
        #expect(imageStatus.service?.serviceName == nil)
        #expect(imageStatus.service?.effectiveUser == "root" && imageStatus.service?.workspaceFolder == "/workspace")

        #expect(CLIBackend.containerState("") == (.notCreated, nil))
        #expect(CLIBackend.containerState(#"{"ID":"x","State":"exited"}"#) == (.stopped, "x"))

        // Without docker the state is unknown; a missing .devcontainer leaves no service.
        let noDocker = ScriptedProcessRunner([
            .exit(["branchbox", "feature", "list"], stdout: Scripted.etaRecord),
            .exit(["branchbox", "devcontainer", "detect"], 1, stdout: #"{"error": "No .devcontainer"}"#),
        ])
        let unknown = try await Scripted.backend(noDocker).devcontainerStatus(for: Scripted.eta)
        #expect(unknown == DevcontainerStatus(state: .unknown))
    }

    @Test func legacyInitStreamsItsLogAndReresolvesTheWorkspace() async throws {
        let text = "🚀 Initializing BranchBox\n✓ Created .branchbox/config.json\n"
        let runner = ScriptedProcessRunner([
            ScriptedProcessRunner.Rule(["branchbox", "init"], lines: [OutputLine(channel: .stdout, text: "🚀 Initializing BranchBox")],
                                       outcome: .exit(0, stdout: text)),
            .exit(["git", "-C", "/r/new", "rev-parse"], stdout: "/r/new/.git\n/r/new\n"),
        ])
        let fileSystem = MutableFileSystem(["/r/new": .directory])
        let progress = ProgressCollector()
        let report = try await Scripted.backend(runner, fileSystem: fileSystem)
            .initProject(InitRequest(folder: URL(fileURLWithPath: "/r/new"), tunnelsEnabled: false), progress: progress.sink)
        #expect(report.workspacePath == "/r/new")
        #expect(report.log == ["🚀 Initializing BranchBox", "✓ Created .branchbox/config.json"])
        #expect(report.warnings.first?.contains("needs branchbox 0.14") == true)
        let spec = try #require(runner.specs.first)
        #expect(spec.arguments == ["init", "-y"] && spec.streamStdout && spec.workingDirectory?.path == "/r/new")
        #expect(progress.logs.contains { $0.source == .stdout })
    }

    @Test func contractInitDecodesItsReportAndAppliesTheTunnelSetting() async throws {
        let runner = ScriptedProcessRunner([
            .exit(["branchbox", "init"], stdout: #"{"schema_version":1,"workspace_path":"/r/new/main","reorganized":true,"stack":"rust","adapter":"generic","modules":["specs"],"onepassword":{"status":"skipped"},"warnings":[],"next_steps":["cd main"]}"#),
            .exit(["git", "-C", "/r/new/main", "rev-parse"], stdout: "/r/new/main/.git\n/r/new/main\n"),
            .exit(["branchbox", "config", "apply"], stdout: #"{"schema_version":1,"changed":[],"effective":{}}"#),
        ])
        let fileSystem = MutableFileSystem(["/r/new/main": .directory])
        let report = try await Scripted.backend(runner, identity: contract, fileSystem: fileSystem)
            .initProject(InitRequest(folder: URL(fileURLWithPath: "/r/new"), reorganize: true, tunnelsEnabled: true),
                         progress: { _ in })
        #expect(report.workspacePath == "/r/new/main" && report.reorganized && report.onePasswordStatus == "skipped")
        #expect(report.nextSteps == ["cd main"] && report.warnings.isEmpty)
        let apply = try #require(runner.specs.first { $0.arguments.starts(with: ["config", "apply"]) })
        #expect(apply.arguments.contains("/r/new/main"))
        #expect(apply.standardInput == Data(#"{"tunnel":{"enabled":true}}"#.utf8))
    }

    // MARK: - Identity and Copy as Command

    @Test func identityWithoutAProbeIsTheBootstrappedOne() async throws {
        let backend = Scripted.backend(ScriptedProcessRunner(), identity: contract)
        #expect(try await backend.identity() == contract)
    }

    @Test func previewCommandLinesAreShellReadyAndRedacted() {
        let backend = Scripted.backend(ScriptedProcessRunner(),
                                       settings: BackendSettings(extraEnvironment: ["API_KEY": "sk_live_abcdef"]))
        var teardown = TeardownRequest(feature: Scripted.eta, recordedBranch: "feature/eta", branch: .deleteIfMerged)
        #expect(backend.previewCommandLine(.teardown(teardown))
                == "/opt/homebrew/bin/branchbox feature teardown eta --repo /r/main --json --branch-prefix feature --keep-branch && git -C /r/main branch -d feature/eta")
        teardown.branch = .keep
        #expect(backend.previewCommandLine(.teardown(teardown))?.contains("&&") == false)
        #expect(backend.previewCommandLine(.deleteBranch("feature/eta", Scripted.project, force: true))
                == "git -C /r/main branch -D feature/eta")
        #expect(backend.previewCommandLine(.removeStray(StrayWorktree(path: "/r/my stray", branch: nil, head: nil), Scripted.project,
                                                        discard: DiscardConsent(userFiles: [])))
                == "git -C /r/main worktree remove --force '/r/my stray'")
        #expect(backend.previewCommandLine(.exec(ExecRequest(feature: Scripted.eta, command: ["echo", "sk_live_abcdef"])))
                == "/opt/homebrew/bin/branchbox feature exec --repo /r/main --json eta -- echo '<redacted>'")
        #expect(backend.previewCommandLine(.tunnelCredentials(TunnelCredentialsRequest(accountID: "a", apiToken: SecretString("tok")),
                                                              Scripted.project))?.contains("tok ") == false)
        #expect(backend.previewCommandLine(.applyConfig(ConfigPatch(changes: [ConfigChange(key: "tunnel.enabled", value: .bool(false))]),
                                                        Scripted.project))
                == #"printf '%s' '{"tunnel":{"enabled":false}}' | /opt/homebrew/bin/branchbox config apply --repo /r/main --file - --json"#)
        #expect(backend.previewCommandLine(.initProject(InitRequest(folder: URL(fileURLWithPath: "/r/new"))))
                == "cd /r/new && /opt/homebrew/bin/branchbox init -y")
        #expect(backend.previewCommandLine(.prune(PruneSelection(project: Scripted.project, rows: []))) == nil)
        #expect(backend.previewCommandLine(.prune(PruneSelection(project: Scripted.project, rows: [teardown, teardown])))?
            .split(separator: "\n").count == 2)
        #expect(backend.previewCommandLine(.devcontainer(.down(removeVolumes: false), Scripted.eta))
                == "/opt/homebrew/bin/branchbox devcontainer down /r/eta --json")
        #expect(backend.previewCommandLine(.syncDevcontainers(SyncRequest(project: Scripted.project, dryRun: true)))
                == "/opt/homebrew/bin/branchbox devcontainer sync -p /r/main -n")
        #expect(backend.previewCommandLine(.tunnelOpen(Scripted.eta)) == "/opt/homebrew/bin/branchbox tunnel open eta --repo /r/main --json")
        #expect(backend.previewCommandLine(.tunnelRemove(Scripted.eta, force: true))?.hasSuffix("--json --force") == true)
    }
}
