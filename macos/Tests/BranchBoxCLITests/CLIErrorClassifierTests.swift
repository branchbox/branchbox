@testable import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

/// stderr lines as the runner delivers them: split, ANSI-stripped.
private func stderrLines(_ fixture: String) throws -> [String] {
    try Fixtures.string("cli-0.13.4/\(fixture)").split(separator: "\n", omittingEmptySubsequences: false)
        .map { ANSI.strip(String($0)) }
}

private func output(exit code: Int32, stdout: String = "", stderr: [String] = [],
                    registryPath: String? = "/r/main/.branchbox/registry.json") -> CLIOutput {
    CLIOutput(result: ProcessResult(termination: .exited(code), stdout: Data(stdout.utf8), stderrTail: stderr,
                                    duration: .milliseconds(5)),
              operation: "feature teardown", invocation: "branchbox feature teardown alpha", cliVersion: "0.13.4",
              context: CLIErrorClassifier.Context(registryPath: registryPath))
}

private func envelope(_ code: String, message: String = "message", causes: [String] = [], details: String = "null") -> String {
    #"{"schema_version":1,"error":{"code":"\#(code)","message":"\#(message)","causes":\#(causes),"details":\#(details)}}"#
}

private let planJSON = """
    {"schema_version":1,"work_feature":"eta","registered":true,"status":"active",
     "worktree":{"path":"/r/eta","exists":true,"locked":false,"lock_reason":null},
     "changes":{"status_available":true,"truncated":false,
       "user":[{"path":"README.md","kind":"modified","area":"other"},{"path":"notes.txt","kind":"untracked","area":"other"}],
       "generated":[{"path":".devcontainer/.branchbox.env","rule":"reserved_name"}],"preserved":[]},
     "branch":{"name":"feature/eta","source":"registry","exists":true,"upstream":null,"reference":"HEAD","reference_name":"main",
       "merged":false,"merged_into_head":false,"ahead":3,"action":"delete"},
     "blockers":[{"kind":"uncommitted_changes","count":2,"message":"…","override":"--discard-changes"},
                 {"kind":"unmerged_branch","branch":"feature/eta","ahead":3,"message":"…","override":"--keep-branch"}],
     "warnings":[]}
    """

@Suite struct CLIErrorClassifierTests {
    // MARK: - Real 0.13.4 stderr

    @Test func alphaDirtyModuleRefusalNamesTheBannerFiles() throws {
        let error = output(exit: 1, stdout: try Fixtures.string("cli-0.13.4/sandbox_teardown_alpha.json"),
                           stderr: try stderrLines("sandbox_teardown_alpha.stderr")).failure()
        let refusal = try #require(error.refusal)
        #expect(refusal.cause == .moduleFilesDirty(files: [".devcontainer/"], userChanges: []))
        #expect(refusal.message == "Devcontainer/compose changes detected; rerun this command with --force to proceed.")
        #expect(refusal.diagnostics.exitCode == 1)
        #expect(refusal.diagnostics.cliVersion == "0.13.4")
    }

    @Test func betaDefaultIsAPartialUnmergedBranchFailure() throws {
        let error = output(exit: 1, stderr: try stderrLines("sandbox_teardown_beta_default.stderr")).failure()
        guard case .partial(let partial) = error else {
            Issue.record("expected .partial, got \(error)")
            return
        }
        #expect(partial.completed == ["Worktree removed"])
        #expect(partial.remaining.cause == .unmergedBranch(branch: "feature/beta", ahead: nil))
        // The INFO log lines are kept in the tail but never become the summary.
        #expect(partial.remaining.diagnostics.summary.hasPrefix("Branch 'feature/beta' could not be deleted without force"))
        #expect(partial.remaining.diagnostics.logTail.contains { $0.contains("INFO") })
    }

    @Test func notAGitRepositoryIsRecognizedWithItsPath() {
        let error = output(exit: 1, stderr: ["Error: Validation error: Not a git repository: /"]).failure()
        #expect(error.refusal?.cause == .notGitRepository("/"))
        #expect(error.refusal?.message == "Validation error: Not a git repository: /")
    }

    @Test func unknownFailureIsCommandFailedSummarizedByItsErrorLine() throws {
        let exec = output(exit: 1, stderr: try stderrLines("sandbox_exec_alpha_fail.stderr")).failure()
        #expect(exec == .commandFailed(Diagnostics(summary: "Runtime command exited with status 3", exitCode: 1,
                                                   logTail: try stderrLines("sandbox_exec_alpha_fail.stderr"),
                                                   invocation: "branchbox feature teardown alpha", cliVersion: "0.13.4")))

        let agent = output(exit: 1, stderr: try stderrLines("main_agent_status.stderr")).failure()
        #expect(agent.diagnostics?.summary == "failed to connect to BranchBox agent at /Users/dev/.branchbox/agent/branchbox-agent.sock")
        #expect(agent.diagnostics?.causes == ["No such file or directory (os error 2)"])
    }

    @Test func summaryIsNeverTheInfoLog() throws {
        // A run that died without an anyhow block: only tracing lines on stderr.
        let error = output(exit: 1, stderr: try stderrLines("sandbox_start_alpha.stderr")).failure()
        #expect(error.diagnostics?.summary == "feature teardown exited with status 1")
        let signaled = CLIOutput(result: ProcessResult(termination: .signaled(9), stdout: Data(), stderrTail: ["INFO x"],
                                                       duration: .zero),
                                 operation: "feature start", invocation: "", cliVersion: nil,
                                 context: CLIErrorClassifier.Context()).failure()
        #expect(signaled.diagnostics?.summary == "feature start was terminated by signal 9")
        #expect(signaled.diagnostics?.signal == 9)
    }

    @Test func aClapUsageErrorIsSummarizedByItsErrorLine() {
        // `feature start --prompt '- fix bug'` on 0.13.4: clap prints `error: …` and Usage, no anyhow block.
        let usage = output(exit: 2, stderr: ["error: unexpected argument '- ' found", "",
                                             "Usage: branchbox feature start [OPTIONS] [NAME]", "",
                                             "For more information, try '--help'."]).failure()
        #expect(usage.diagnostics?.summary == "unexpected argument '- ' found")
        #expect(usage.diagnostics?.exitCode == 2)
        // An anyhow `Error:` line still wins.
        let both = output(exit: 1, stderr: ["error: noise", "Error: the real cause"]).failure()
        #expect(both.diagnostics?.summary == "the real cause")
    }

    @Test func causedByListsAreParsedIncludingNumberedCauses() throws {
        let sync = output(exit: 1, stdout: try Fixtures.string("cli-0.13.4/sandbox_devcontainer_sync.stdout"),
                          stderr: try stderrLines("sandbox_devcontainer_sync.stderr")).failure()
        #expect(sync.refusal?.cause == .devcontainerSourceMissing)
        #expect(sync.diagnostics?.summary == "Failed to initialize devcontainer module")
        #expect(sync.diagnostics?.causes == ["Validation error: Devcontainer directory not found: /tmp/bbx/demo/.devcontainer"])

        let block = CLIErrorClassifier.anyhowBlock(in: ["INFO noise", "Error: outer", "", "Caused by:", "    0: middle",
                                                        "    1: inner", "trailing text"])
        #expect(block?.summary == "outer")
        #expect(block?.causes == ["middle", "inner"])
    }

    @Test func knownMessageTableCoversEveryRow() throws {
        func cause(_ line: String) -> RefusalCause? { output(exit: 1, stderr: [line]).failure().refusal?.cause }
        #expect(cause("Error: Worktree already exists at: /r/eta") == .worktreeExists(path: "/r/eta"))
        #expect(cause("Error: Worktree not found: nope") == .worktreeNotFound("nope"))
        #expect(cause("Error: Invalid feature name: Bad Name") == .invalidName("Bad Name"))
        #expect(cause("Error: Branch already exists: feature/eta") == .branchExists("feature/eta"))
        #expect(try cause(stderrLines("sandbox_prune_noyes.stderr").joined()) == .confirmationRequired)
        #expect(cause("Error: Validation error: sbx preflight failed: Sign in with: sbx login")
                == .runtimePrerequisite(provider: "sbx", detail: "Validation error: sbx preflight failed: Sign in with: sbx login"))
        #expect(cause("Error: Validation error: local-vm preflight failed: branchbox-local-vm: local-vm requires a Linux host")
                == .runtimePrerequisite(provider: "local-vm",
                                        detail: "Validation error: local-vm preflight failed: branchbox-local-vm: local-vm requires a Linux host"))
    }

    @Test func registryParseErrorIsRegistryCorrupted() {
        let text = output(exit: 1, stderr: ["Error: Configuration error: Failed to parse feature registry: EOF while parsing a list at line 1 column 14"]).failure()
        guard case .registryCorrupted(let path, let diagnostics) = text else {
            Issue.record("expected .registryCorrupted, got \(text)")
            return
        }
        #expect(path == "/r/main/.branchbox/registry.json")
        #expect(diagnostics.summary.contains("Failed to parse feature registry"))

        // The contract CLI codes the same failure config_invalid.
        let coded = output(exit: 1, stdout: envelope("config_invalid",
                                                     message: "Configuration error: Failed to parse feature registry: EOF"),
                           stderr: ["Error: Configuration error: Failed to parse feature registry: EOF"]).failure()
        if case .registryCorrupted = coded {} else { Issue.record("expected .registryCorrupted, got \(coded)") }

        let named = output(exit: 1, stderr: ["Error: JSON error: EOF while parsing an object in /r/main/.branchbox/registry.json"],
                           registryPath: nil).failure()
        if case .registryCorrupted(let path, _) = named { #expect(path == "registry.json") } else {
            Issue.record("expected .registryCorrupted, got \(named)")
        }
    }

    // MARK: - Envelopes (§6.4)

    @Test func envelopeBeatsTheTextOnStderr() {
        let error = output(exit: 1, stdout: envelope("worktree_exists", message: "Worktree already exists at: /r/eta",
                                                     details: #"{"path":"/r/eta"}"#),
                           stderr: ["Error: Validation error: Not a git repository: /"]).failure()
        #expect(error.refusal?.cause == .worktreeExists(path: "/r/eta"))
        #expect(error.refusal?.message == "Worktree already exists at: /r/eta")
    }

    @Test func envelopeCodesMapToRefusalCauses() {
        func cause(_ code: String, _ details: String = "null", message: String = "m") -> RefusalCause? {
            output(exit: 1, stdout: envelope(code, message: message, details: details)).failure().refusal?.cause
        }
        #expect(cause("worktree_not_found", #"{"name":"nope","path":"/r/nope"}"#) == .worktreeNotFound("nope"))
        #expect(cause("feature_not_found", #"{"name":"nope","registry":"/r/main/.branchbox/registry.json"}"#) == .featureNotFound("nope"))
        #expect(cause("invalid_feature_name", #"{"name":"Bad"}"#) == .invalidName("Bad"))
        #expect(cause("branch_exists", #"{"branch":"feature/eta"}"#) == .branchExists("feature/eta"))
        #expect(cause("not_a_git_repository", #"{"path":"/nonexistent"}"#) == .notGitRepository("/nonexistent"))
        #expect(cause("registry_locked", #"{"path":"/r/main/.branchbox","waited_secs":30}"#) == .registryLocked(path: "/r/main/.branchbox"))
        #expect(cause("confirmation_required", #"{"count":3}"#) == .confirmationRequired)
        #expect(cause("config_invalid", #"{"key":"runtime.provider"}"#, message: "bad value")
                == .configInvalid(key: "runtime.provider", detail: "bad value"))
        #expect(cause("config_unknown_key", #"{"key":"nope"}"#, message: "unknown") == .configInvalid(key: "nope", detail: "unknown"))
        #expect(cause("devcontainer_source_missing", #"{"path":"/r/main/.devcontainer"}"#) == .devcontainerSourceMissing)
        #expect(cause("validation_failed", message: "Validation error: --default-prompt requires --minimal")
                == .other(code: "validation_failed"))
        #expect(cause("validation_failed", message: "Sign in with: sbx login")
                == .runtimePrerequisite(provider: "sbx", detail: "Sign in with: sbx login"))
        for code in ["git_failed", "io_error", "command_failed", "internal", "internal_panic", "agent_unreachable", "brand_new"] {
            let error = output(exit: 1, stdout: envelope(code, message: "boom", causes: ["cause"])).failure()
            #expect(error == .commandFailed(Diagnostics(summary: "boom", causes: ["cause"], exitCode: 1,
                                                        invocation: "branchbox feature teardown alpha", cliVersion: "0.13.4")))
        }
    }

    @Test func teardownRefusedTakesItsCauseFromThePlansFirstBlocker() throws {
        let error = output(exit: 1, stdout: envelope("teardown_refused", message: "Refusing to tear down 'eta'",
                                                     details: #"{"plan":\#(planJSON),"changed_anything":false,"completed_steps":[]}"#))
            .failure()
        let refusal = try #require(error.refusal)
        #expect(refusal.cause == .uncommittedChanges(files: [ChangedFile(path: "README.md", kind: "modified", area: "other"),
                                                             ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]))
        #expect(refusal.plan?.workFeature == "eta")
        #expect(refusal.plan?.source == .cli)

        // Blocker kinds in turn.
        let plan = try CLIJSON.decoder().decode(TeardownPlanDocument.self, from: Data(planJSON.utf8))
        func first(_ blocker: TeardownPlanDocument.Blocker) -> RefusalCause {
            CLIErrorClassifier.teardownCause(plan: TeardownPlanDocument(workFeature: "eta", registered: true,
                                                                        worktree: .init(path: "/r/eta", exists: true, locked: true,
                                                                                        lockReason: "usb"),
                                                                        changes: plan.changes, branch: plan.branch,
                                                                        blockers: [blocker]))
        }
        #expect(first(.init(kind: "unmerged_branch", message: "m", branch: "feature/eta", ahead: 3))
                == .unmergedBranch(branch: "feature/eta", ahead: 3))
        #expect(first(.init(kind: "worktree_locked", message: "m")) == .worktreeLocked(reason: "usb"))
        #expect(first(.init(kind: "status_unavailable", message: "m", cause: "fatal: bad index")) == .statusUnavailable(cause: "fatal: bad index"))
        #expect(first(.init(kind: "worktree_removal_failed", message: "busy")) == .worktreeRemovalFailed(cause: "busy"))
        #expect(first(.init(kind: "new_kind", message: "m")) == .other(code: "new_kind"))
    }

    @Test func legacyTeardownRefusedEnvelopeWithoutAPlanIsModuleFilesDirty() {
        let details = #"{"plan":null,"changed_anything":false,"completed_steps":[],"worktree":"/r/beta","files":[".devcontainer/"]}"#
        let error = output(exit: 1, stdout: envelope("teardown_refused", details: details)).failure()
        #expect(error.refusal?.cause == .moduleFilesDirty(files: [".devcontainer/"], userChanges: []))
        #expect(error.refusal?.plan == nil)
    }

    @Test func refusalThatChangedSomethingIsAPartialFailure() {
        let details = #"{"plan":\#(planJSON),"changed_anything":true,"completed_steps":["Runtime stopped","Modules torn down"]}"#
        let error = output(exit: 1, stdout: envelope("teardown_refused", details: details)).failure()
        guard case .partial(let partial) = error else {
            Issue.record("expected .partial, got \(error)")
            return
        }
        #expect(partial.completed == ["Runtime stopped", "Modules torn down"])
        if case .uncommittedChanges = partial.remaining.cause {} else { Issue.record("unexpected cause") }
    }

    // MARK: - CLIOutput decoding

    @Test func inBandPayloadIsReadOnAnyExitButNeverAgainstAnEnvelope() throws {
        let failing = output(exit: 1, stdout: try Fixtures.string("cli-0.13.4/sandbox_exec_alpha_fail.json"),
                             stderr: try stderrLines("sandbox_exec_alpha_fail.stderr"))
        #expect(try failing.decodeInBand(ExecResult.self, what: "exec result") == ExecResult(exitCode: 3, stdout: "out\n", stderr: "err\n"))

        let enveloped = output(exit: 1, stdout: envelope("worktree_not_found", details: #"{"name":"nope"}"#))
        #expect(throws: BackendError.self) { _ = try enveloped.decodeInBand(ExecResult.self, what: "exec result") }

        let notAPayload = output(exit: 0, stdout: #"{"hello":"world"}"#)
        do {
            _ = try notAPayload.decodeInBand(ExecResult.self, what: "exec result")
            Issue.record("decoded a document that is not an exec result")
        } catch BackendError.decodeFailed(let what, _, let diagnostics) {
            #expect(what == "exec result")
            #expect(diagnostics.cliVersion == "0.13.4")
        }
    }

    @Test func successDecodesWithPreambleAndFailsTypedOtherwise() async throws {
        let start = output(exit: 0, stdout: try Fixtures.string("cli-0.13.4/sandbox_start_gamma_longprompt.json"))
        let (summary, preamble) = try start.decode(StartSummary.self, what: "start summary")
        #expect(summary.workFeature == "gamma")
        #expect(preamble == "⚠️  Prompt truncated to 2000 characters before storage.")

        let garbage = output(exit: 0, stdout: "not json")
        guard case .decodeFailed(let what, let detail, let diagnostics)? = await backendError({
            try garbage.decode(StartSummary.self, what: "start summary")
        }) else {
            Issue.record("expected .decodeFailed")
            return
        }
        #expect(what == "start summary")
        #expect(detail == "the output is not valid JSON")
        #expect(diagnostics.summary.contains("branchbox 0.13.4"))
        #expect(diagnostics.cliVersion == "0.13.4")
    }

    @Test func dockerDownPayloadSummarizesWithItsMessage() throws {
        let docker = output(exit: 1, stdout: try Fixtures.string("cli-0.13.4/synthetic_devcontainer_up_docker_unavailable.json"))
        // As an exec payload it is not one; the message still names the cause.
        #expect(throws: BackendError.commandFailed(Diagnostics(summary: "Docker is not available", exitCode: 1,
                                                               invocation: "branchbox feature teardown alpha",
                                                               cliVersion: "0.13.4"))) {
            _ = try docker.decodeInBand(ExecResult.self, what: "exec result")
        }
        let legacy = output(exit: 1, stdout: #"{"error": "No .devcontainer directory found"}"#)
        #expect(legacy.failure().diagnostics?.summary == "No .devcontainer directory found")
    }
}
