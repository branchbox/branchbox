@testable import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

@Suite struct TextParsersTests {
    @Test func dirtyBannerListsItsBulletPaths() throws {
        #expect(TextParsers.dirtyBannerPaths(try Fixtures.string("cli-0.13.4/sandbox_teardown_alpha.json")) == [".devcontainer/"])
        #expect(TextParsers.dirtyBannerPaths("""
            ⚠️  Detected devcontainer/compose changes inside /r/eta:
                • .devcontainer/devcontainer.json
                • docker-compose.yml
                (BranchBox refuses to delete dirty module files without --force)
            • not indented, not a banner line
            """) == [".devcontainer/devcontainer.json", "docker-compose.yml"])
    }

    @Test func legacyDetectTextBecomesAReport() throws {
        let sandbox = TextParsers.detectReport(try Fixtures.string("cli-0.13.4/sandbox_detect.txt"), folder: "/tmp/bbx/demo",
                                               gitRepository: true, initialized: false, hasDevcontainer: false, hasEnv: false)
        #expect(sandbox.project == "/tmp/bbx/demo")
        #expect(sandbox.stack == "generic" && sandbox.adapter == "generic")
        #expect(sandbox.modules == ["tunnel", "specs"])
        #expect(sandbox.gitRepository && !sandbox.initialized)
        #expect(sandbox.rawText == (try Fixtures.string("cli-0.13.4/sandbox_detect.txt")))

        let main = TextParsers.detectReport(try Fixtures.string("cli-0.13.4/main_detect.txt"), folder: "/src/branchbox",
                                            gitRepository: true, initialized: true, hasDevcontainer: true, hasEnv: nil)
        #expect(main.project == "/src/branchbox")              // "Project: ." is the folder
        #expect(main.stack == "rust")
        #expect(main.modules == ["devcontainer", "compose", "tunnel", "specs"])

        let warned = TextParsers.detectReport("Stack: NodeJs\nAdapter: Rails\n\nWarnings:\n  - compose file missing\n",
                                              folder: "/r", gitRepository: false, initialized: false, hasDevcontainer: nil,
                                              hasEnv: nil)
        #expect(warned.stack == "nodejs" && warned.adapter == "rails")
        #expect(warned.warnings == ["compose file missing"])
    }

    @Test func legacySyncFailedRowIsFailedEvenWithExitZero() {
        let report = TextParsers.syncReport("""
            🔄 Syncing devcontainer configuration to 4 feature worktree(s)

              alpha ... ✓ synced 3 files (copy)
              beta ... ✗ failed: Validation error: Devcontainer directory not found
              gamma ... ⚠️  worktree not found at /r/gamma
              delta ... ✓ synced 0 files (symlink)
                ⚠️ failed to update registry: locked

            ✓ Successfully synced 2 feature worktree(s)

            ⚠️  1 error(s) occurred:
              - beta: Validation error: Devcontainer directory not found
            """, dryRun: false, strategy: "copy")
        #expect(report.rows == [
            SyncReport.Row(feature: "alpha", status: .synced),
            SyncReport.Row(feature: "beta", status: .failed, error: "Validation error: Devcontainer directory not found"),
            SyncReport.Row(feature: "gamma", worktreePath: "/r/gamma", status: .skipped, skipReason: "Worktree not found"),
            SyncReport.Row(feature: "delta", status: .synced),
        ])
        #expect(report.failedCount == 1)
        #expect(report.strategy == "copy" && !report.dryRun && report.rawText != nil)
    }

    @Test func errorsListAloneMarksAFeatureFailed() {
        let report = TextParsers.syncReport("""
              alpha ... ✓ synced 1 files (copy)

            ⚠️  2 error(s) occurred:
              - alpha: late failure
              - omega: never printed a row
            """, dryRun: false, strategy: nil)
        #expect(report.rows.map(\.status) == [.failed, .failed])
        #expect(report.failedCount == 2)
    }

    @Test func dryRunAndEmptySync() throws {
        let dryRun = TextParsers.syncReport("DRY RUN - no changes will be made\n\n  alpha ... would sync\n", dryRun: true,
                                            strategy: nil)
        #expect(dryRun.rows == [SyncReport.Row(feature: "alpha", status: .wouldSync)])
        let none = TextParsers.syncReport(try Fixtures.string("cli-0.13.4/sandbox_devcontainer_sync_noactive.stdout"),
                                          dryRun: false, strategy: nil)
        #expect(none.rows.isEmpty && none.failedCount == 0)
    }
}

@Suite struct TracingLineParserTests {
    @Test func tracingLinesAreSplitIntoTimestampLevelTargetAndMessage() throws {
        let lines = try Fixtures.string("cli-0.13.4/sandbox_start_theta_trace.stderr").split(separator: "\n")
            .map { TracingLineParser.parse(ANSI.strip(String($0)), source: .stderr) }
        let first = try #require(lines.first)
        #expect(first.level == .debug)
        #expect(first.target == "worktree_core::git")
        #expect(first.message.hasPrefix("Running: cd "))
        #expect(first.timestamp == RFC3339.parse("2026-10-01T23:09:42.889280Z"))
        #expect(lines.contains { $0.level == .warn && $0.message.hasPrefix("No .env found") })
        #expect(lines.allSatisfy { $0.level != .output })
    }

    @Test func otherLinesAreOutput() {
        for text in ["Error: boom", "", "⚠️  Prompt truncated", "2026-10-01T00:00:00Z INFO no-colon-target message",
                     "2026-10-01T00:00:00Z NOTICE worktree_core::git: x"] {
            let line = TracingLineParser.parse(text, source: .stdout)
            #expect(line.level == .output && line.target == nil && line.message == text && line.source == .stdout)
        }
        let spaced = TracingLineParser.parse("2026-10-01T00:00:00Z  INFO worktree_core::modules::specs:   indented", source: .stderr)
        #expect(spaced.target == "worktree_core::modules::specs" && spaced.message == "  indented")
    }

    @Test func phasesFollowTheTracingTargets() {
        func phase(_ target: String, _ message: String = "x", _ activity: PhaseMapper.Activity = .start) -> OperationPhase? {
            PhaseMapper.phase(for: LogLine(timestamp: nil, level: .info, source: .stderr, target: target, message: message),
                              during: activity)
        }
        #expect(phase("worktree_core::git", "Created worktree at /r/eta") == .creatingWorktree)
        #expect(phase("worktree_core::modules::compose") == .module("compose"))
        #expect(phase("worktree_core::runtime::sbx", "Preparing runtime") == .runtime("sbx"))
        #expect(phase("worktree_core::runtime", "Destroyed runtime abc") == .cleaningRuntime)
        #expect(phase("worktree_core::runtime::sbx", "anything", .teardown) == .cleaningRuntime)
        #expect(phase("worktree_core::adapters::generic") == .detectingAdapter)
        #expect(phase("worktree_core::workflows::feature", "Detected adapter: Generic") == .detectingAdapter)
        #expect(phase("worktree_core::tunnel") == .provisioningTunnel)
        #expect(phase("worktree_core::git", "Removed worktree at /r/eta", .teardown) == .removingWorktree)
        #expect(phase("worktree_core::git", "Deleted branch feature/eta", .teardown) == .deletingBranch)
        #expect(phase("worktree_core::workflows::feature", "Using service URL") == nil)
        #expect(PhaseMapper.phase(for: LogLine(timestamp: nil, level: .output, source: .stdout, target: nil, message: "x"),
                                  during: .start) == nil)
    }

    @Test func relayEmitsEachLineAndOnlyPhaseChanges() throws {
        let collector = ProgressCollector()
        let relay = ProgressRelay(collector.sink, activity: .start)
        relay.phase(.preparing)
        for text in ["2026-10-01T00:00:00Z  INFO worktree_core::git: Created worktree at /r/eta",
                     "2026-10-01T00:00:01Z  INFO worktree_core::modules::specs: one",
                     "2026-10-01T00:00:02Z  INFO worktree_core::modules::specs: two",
                     "plain text"] {
            relay.line(OutputLine(channel: .stderr, text: text))
        }
        relay.line(OutputLine(channel: .stdout, text: "stdout line"))
        relay.warning("careful")
        relay.note("app line")
        #expect(collector.phases == [.preparing, .creatingWorktree, .module("specs")])
        #expect(collector.logs.map(\.source) == [.stderr, .stderr, .stderr, .stderr, .stdout, .app])
        #expect(collector.warnings == ["careful"])

        let silent = ProgressRelay(nil, activity: .other)
        silent.line(OutputLine(channel: .stderr, text: "x"))
        #expect(!silent.isActive)
    }
}

@Suite struct RedactedCommandLineTests {
    @Test func argumentsAreShellQuoted() {
        let line = RedactedCommandLine().render(["/opt/homebrew/bin/branchbox", "feature", "exec", "--repo", "/r/my project",
                                                 "--json", "eta", "--", "sh", "-c", "echo 'hi'; ls $HOME", ""])
        #expect(line == #"/opt/homebrew/bin/branchbox feature exec --repo '/r/my project' --json eta -- sh -c 'echo '\''hi'\''; ls $HOME' ''"#)
    }

    @Test func promptValuesExtraEnvironmentValuesAndTokensAreRedacted() {
        let redaction = RedactedCommandLine(secrets: ["sk_live_abcdef", "x", "sbx"], tokens: ["tk"])
        let prompt = String(repeating: "p", count: 812)
        let line = redaction.render(["branchbox", "feature", "start", "eta", "--prompt", prompt, "--base", "sk_live_abcdef",
                                     "TOKEN=sk_live_abcdef", "tk", "x", "--runtime", "sbx", "box"])
        #expect(line == "branchbox feature start eta --prompt '<redacted 812 chars>' --base '<redacted>' 'TOKEN=<redacted>' '<redacted>' x --runtime sbx box")
        #expect(!line.contains("sk_live"))
    }

    @Test func anInlinePromptIsRedactedWhateverItStartsWith() {
        let redaction = RedactedCommandLine()
        for prompt in ["- fix the login bug\n- add tests", "--dangerously-skip-permissions", "plain"] {
            let line = redaction.render(["branchbox", "feature", "start", "eta", "--prompt=\(prompt)"])
            #expect(line == "branchbox feature start eta --prompt='<redacted \(prompt.count) chars>'")
        }
        // Only the redacted flags: other `--flag=value` arguments are quoted as usual.
        #expect(redaction.render(["git", "--format=%(refname)"]) == "git '--format=%(refname)'")
    }

    @Test func diagnosticsCutLongArguments() {
        let long = String(repeating: "a", count: 300)
        let line = RedactedCommandLine().render(["git", long], argumentLimit: 200)
        #expect(line == "git '" + String(repeating: "a", count: 200) + "…'")
    }
}
