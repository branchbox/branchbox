import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 cancellation (DESIGN §13.2): a start cancelled 2 s in returns within 6 s with `.cancelled`, and what it
// leaves behind is recoverable from the app.
//
// - A sleeping `post-checkout` hook holds `git worktree add` itself. The worktree exists but no CLI has
//   registered it yet (contract CLIs write their write-ahead entry right after `git worktree add` returns), so
//   in both modes the listing reports a stray and `removeStray` cleans it up.
// - Contract CLIs: a slow runtime step after the worktree exists (a fake `sbx` whose `create` sleeps, as core's own
//   `list_reports_a_killed_start_as_interrupted` test does) leaves the write-ahead entry, which `list` reports as
//   `setup.state == interrupted`; the Resume remediation (`start --reuse`) then finishes the start.

extension RealCLI {
    @Suite struct CancellationTests {
        /// Starts `request` in a task, cancels it after 2 s and returns the error and how long the cancel took.
        static func cancelAfterTwoSeconds(_ cli: LiveCLI, _ request: StartFeatureRequest) async throws -> (BackendError?, Double) {
            let backend = cli.backend
            let task = Task { try await backend.startFeature(request, progress: { _ in }) }
            try await Task.sleep(for: .seconds(2))
            let cancelled = ContinuousClock.now
            task.cancel()
            let result = await task.result
            let seconds = elapsed(since: cancelled)
            switch result {
            case .success(let summary):
                Issue.record("the start finished instead of being cancelled: \(summary.workFeature)")
                return (nil, seconds)
            case .failure(let error):
                return (BackendError.normalize(error), seconds)
            }
        }

        /// Odd durations, so `pgrep` can tell this suite's sleeps from anything else on the machine.
        static let hookSleep = "29.25"

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func cancelDuringGitWorktreeAddLeavesAStrayThatRemoveStrayCleans() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            try repo.installHook("post-checkout", script: "#!/bin/sh\nsleep \(Self.hookSleep)\n")

            let (error, seconds) = try await Self.cancelAfterTwoSeconds(cli, LiveCLI.minimalStart("slow", in: repo.project))
            repo.removeHook("post-checkout")
            guard case .cancelled? = error else {
                Issue.record("expected .cancelled, got \(String(describing: error))")
                return
            }
            #expect(seconds < 6, "the cancel took \(seconds) s")
            #expect(try await processes(mentioning: repo.container.path).isEmpty, "the CLI outlived the cancel")
            #expect(try await processes(mentioning: "sleep \(Self.hookSleep)").isEmpty, "the hook outlived the cancel")

            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: true)
            #expect(!listing.features.contains { $0.workFeature == "slow" && $0.setup?.state != .interrupted },
                    "a cancelled start is never listed as a finished feature")
            let stray = try #require(listing.strays.first { URL(fileURLWithPath: $0.path).lastPathComponent == "slow" },
                                     "no stray reported after the cancel (\(cli.mode)): \(listing.strays)")
            try await cli.backend.removeStray(stray, in: repo.project, discardChanges: false)
            let after = try await cli.backend.listFeatures(in: repo.project, includeRemoved: true)
            #expect(after.strays.isEmpty)
            #expect(!FileManager.default.fileExists(atPath: repo.worktree("slow").path))
            #expect(try await repo.worktrees().count == 1)
        }

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func contractCancelAfterTheWorktreeExistsIsInterruptedAndResumes() async throws {
            let probe = try await LiveCLI.make()
            guard probe.isContract, probe.supports(.writeAheadStart) else {
                print("CancellationTests: skipped the interrupted/resume case on a legacy CLI (no write-ahead start)")
                return
            }
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            try repo.write("{\n  \"name\": \"it\",\n  \"image\": \"alpine:3.19\"\n}\n", to: ".devcontainer/devcontainer.json")
            try await repo.commitAll(message: "Add devcontainer")
            let tools = repo.container.appendingPathComponent("tools", isDirectory: true)
            let sbx = try FakeSandbox.install(in: tools)
            FakeSandbox.setSlow(true, in: tools)
            let cli = try await LiveCLI.make(extraEnvironment: ["BRANCHBOX_SBX_PATH": sbx.path])

            let request = LiveCLI.minimalStart("paused", in: repo.project, runtime: .sbx)
            let (error, seconds) = try await Self.cancelAfterTwoSeconds(cli, request)
            guard case .cancelled? = error else {
                Issue.record("expected .cancelled, got \(String(describing: error))")
                return
            }
            #expect(seconds < 6, "the cancel took \(seconds) s")
            #expect(try await processes(mentioning: repo.container.path).isEmpty, "the CLI or fake sbx outlived the cancel")
            #expect(try await processes(mentioning: "sleep \(FakeSandbox.sleepSeconds)").isEmpty, "the fake sbx outlived the cancel")

            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let record = try #require(listing.features.first { $0.workFeature == "paused" }, "\(listing.features)")
            #expect(record.setup?.state == .interrupted, "\(String(describing: record.setup))")
            #expect(listing.strays.isEmpty, "a write-ahead entry is not a stray")
            #expect(Remediation.attention(for: record, folderExists: true) == .interrupted)

            let actions = Remediation.actions(for: record, project: repo.project, identity: cli.identity, folderExists: true)
            let resume = try #require(actions.lazy.compactMap { action -> StartFeatureRequest? in
                if case .resumeSetup(let request) = action { return request }
                return nil
            }.first, "no Resume remediation in \(actions)")
            FakeSandbox.setSlow(false, in: tools)
            let summary = try await cli.backend.startFeature(resume, progress: { _ in })
            #expect(summary.workFeature == "paused")
            let resumed = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let finished = try #require(resumed.features.first { $0.workFeature == "paused" })
            #expect(finished.setup == nil, "a finished start clears its write-ahead marker: \(String(describing: finished.setup))")
            #expect(Remediation.attention(for: finished, folderExists: true) == nil)

            let teardown = TeardownRequest(feature: FeatureRef(project: repo.project, name: "paused"),
                                           recordedBranch: finished.branchName, branch: .deleteIfMerged)
            #expect(try await cli.backend.teardownFeature(teardown, progress: { _ in }).worktreeGone)
        }
    }
}

/// A stand-in for Docker's `sbx` CLI, enough for a Docker-free `feature start --runtime sbx --minimal`: `create`
/// records the sandbox name (and sleeps about 30 s while the `slow` marker exists), `ls` lists it, `exec …
/// devcontainer up` reports success (or fails while the `fail-up` marker exists), `rm` forgets it.
enum FakeSandbox {
    static let sleepSeconds = "28.75"

    static func install(in folder: URL) throws -> URL {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let script = """
            #!/bin/sh
            dir="$(cd "$(dirname "$0")" && pwd)"
            printf '%s\\n' "$*" >> "$dir/calls.log"
            case "$1" in
              ls) if [ -f "$dir/state" ]; then cat "$dir/state"; fi ;;
              create)
                if [ -f "$dir/slow" ]; then sleep \(sleepSeconds); fi
                previous=""
                for argument in "$@"; do
                  if [ "$previous" = "--name" ]; then printf '%s\\n' "$argument" > "$dir/state"; fi
                  previous="$argument"
                done ;;
              ports) case "$*" in *--json*) printf '%s\\n' '[]' ;; esac ;;
              exec)
                case "$*" in
                  *"devcontainer up"*)
                    if [ -f "$dir/fail-up" ]; then printf '%s\\n' 'simulated devcontainer up failure' >&2; exit 42; fi
                    printf '%s\\n' '{"outcome":"success","containerId":"fake-container-id"}' ;;
                  *) printf '%s\\n' "fake-sbx-command-ok" ;;
                esac ;;
              rm|stop) rm -f "$dir/state" ;;
              *) printf '%s\\n' "unexpected fake sbx command: $*" >&2; exit 2 ;;
            esac

            """
        let url = folder.appendingPathComponent("sbx")
        try Data(script.utf8).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        return url
    }

    static func setSlow(_ slow: Bool, in folder: URL) { set("slow", slow, in: folder) }

    /// While set, `exec … devcontainer up` fails (exit 42), as a broken dev container does.
    static func setFailingUp(_ failing: Bool, in folder: URL) { set("fail-up", failing, in: folder) }

    private static func set(_ name: String, _ on: Bool, in folder: URL) {
        let marker = folder.appendingPathComponent(name)
        if on {
            FileManager.default.createFile(atPath: marker.path, contents: Data())
        } else {
            try? FileManager.default.removeItem(at: marker)
        }
    }
}
