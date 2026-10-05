import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// SW-1 smoke (DESIGN §13.2) against a real CLI, gated by BRANCHBOX_IT=1. The CLI comes from BRANCHBOX_IT_CLI or the
// locator; the child environment starts from launchd's PATH, as a Finder-launched app's does. Docker-free:
// `--minimal --skip-module tunnel`. Runs in a TempRepo, which the test removes, leaving no worktrees or branches.
//
//   BRANCHBOX_IT=1 BRANCHBOX_IT_CLI=/opt/homebrew/bin/branchbox swift test --filter CLISmokeTests

extension RealCLI {
    @Suite struct CLISmokeTests {
        /// The production bootstrapper, except: the login shell is `/usr/bin/false` (the capture fails at once, so the
        /// launchd environment is the base), and nothing is cached in Application Support (see `LiveCLI`).
        static func bootstrapper() -> CLIBackendBootstrapper {
            LiveCLI.bootstrapper(runner: ProcessRunner())
        }

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(3)))
        func startExecTeardownRoundTrip() async throws {
            let bootstrapper = Self.bootstrapper()
            let repo = try await TempRepo.make()
            defer { repo.remove() }

            // identity
            let bootstrap = await bootstrapper.bootstrap(BackendSettings())
            guard case .ready(let backend, let identity) = bootstrap else {
                Issue.record("the CLI is unavailable: \(bootstrap)")
                return
            }
            #expect(try await backend.identity() == identity)
            #expect(identity.version >= BackendIdentity.minimumCLI)
            print("CLISmokeTests: branchbox \(identity.version) in \(identity.isLegacy ? "legacy" : "contract") mode, capabilities: \(identity.capabilities.map(\.rawValue).sorted())")

            // empty list, resolved from the main worktree
            let resolution = try await backend.resolveProject(at: repo.main)
            #expect(resolution.project == repo.project && resolution.normalization == .none)
            let project = resolution.project
            #expect(try await backend.listFeatures(in: project, includeRemoved: true).features.isEmpty)

            // start --minimal --skip-module tunnel
            var start = StartFeatureRequest(project: project, name: "smoke", runtime: .container)
            start.mode = .minimal
            start.skipModules = ["tunnel"]
            let startEvents = ProgressLog()
            let summary = try await backend.startFeature(start, progress: startEvents.record)
            #expect(summary.workFeature == "smoke")
            #expect(startEvents.phases.contains(.creatingWorktree))
            let worktree = try #require(summary.worktreePath)
            #expect(FileManager.default.fileExists(atPath: worktree))

            // list shows it; a feature worktree resolves back to main
            let listing = try await backend.listFeatures(in: project, includeRemoved: false)
            let record = try #require(listing.features.first { $0.workFeature == "smoke" })
            #expect(record.status == .active)
            #expect(listing.strays.isEmpty)
            let fromFeature = try await backend.resolveProject(at: URL(fileURLWithPath: worktree))
            #expect(fromFeature.project == project && fromFeature.normalization == .fromFeatureWorktree)

            // exec ok, and a failing command is data
            let feature = FeatureRef(project: project, name: "smoke")
            let hi = try await backend.exec(ExecRequest(feature: feature, command: ["echo", "hi"], timeout: .seconds(60)),
                                            progress: { _ in })
            #expect(hi.exitCode == 0 && hi.stdout == "hi\n")
            let three = try await backend.exec(ExecRequest(feature: feature, command: ["sh", "-c", "exit 3"], timeout: .seconds(60)),
                                               progress: { _ in })
            #expect(three.exitCode == 3)

            try await Self.tearDownCleanly(backend, project: project, record: record, worktree: worktree)

            // list --all shows it removed; nothing is left in git
            let after = try await backend.listFeatures(in: project, includeRemoved: true)
            #expect(after.features.first { $0.workFeature == "smoke" }?.status == .removed)
            #expect(try await repo.worktrees().count == 1)
            #expect(try await repo.branches() == ["main"])
        }

        /// An initialized project ignores BranchBox's env files, so a fresh feature's only untracked files are its
        /// `.vscode/settings.json` and spec. 0.13.4's plain `git worktree remove` fails on those and it deletes the
        /// folder with `remove_dir_all`; the app's generated-only `--force` removes them cleanly instead.
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(3)))
        func initializedProjectTeardownNeedsNoManualRemoval() async throws {
            let bootstrapper = Self.bootstrapper()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            guard case .ready(let backend, _) = await bootstrapper.bootstrap(BackendSettings()) else {
                Issue.record("the CLI is unavailable")
                return
            }
            var start = StartFeatureRequest(project: repo.project, name: "inited", runtime: .container)
            start.mode = .minimal
            start.skipModules = ["tunnel"]
            let summary = try await backend.startFeature(start, progress: { _ in })
            let worktree = try #require(summary.worktreePath)
            let listing = try await backend.listFeatures(in: repo.project, includeRemoved: false)
            let record = try #require(listing.features.first { $0.workFeature == "inited" })

            try await Self.tearDownCleanly(backend, project: repo.project, record: record, worktree: worktree)
            #expect(try await repo.worktrees().count == 1)
            #expect(try await repo.branches() == ["main"])
        }

        /// Tear down a fresh feature with "Delete if merged" in one attempt: the plan finds no user changes, no refusal
        /// round trip is needed, the worktree and merged branch go, and 0.13.x's manual-removal warning never appears.
        static func tearDownCleanly(_ backend: any BranchBoxBackend, project: ProjectRef, record: FeatureRecord,
                                    worktree: String) async throws {
            let feature = FeatureRef(project: project, name: record.workFeature)
            let teardown = TeardownRequest(feature: feature, recordedBranch: record.branchName, branch: .deleteIfMerged)
            let plan = try await backend.planTeardown(teardown)
            #expect(plan.changes.user.isEmpty, "a fresh feature has no user changes: \(plan.changes.user)")
            #expect(plan.blockers.isEmpty, "\(plan.blockers)")
            print("CLISmokeTests: \(record.workFeature) plan (\(plan.source)): generated \(plan.changes.generated.map(\.path)), preserved \(plan.changes.preserved.map(\.path))")
            let events = ProgressLog()
            let outcome = try await backend.teardownFeature(teardown, progress: events.record)
            #expect(outcome.worktreeGone)
            #expect(!FileManager.default.fileExists(atPath: worktree))
            #expect(!outcome.summary.warnings.contains { $0.contains("removed manually after git removal failed") },
                    "\(outcome.summary.warnings)")
            #expect(!events.warnings.contains { $0.contains("by hand") }, "\(events.warnings)")
            switch outcome.branch {
            case .deleted(let branch, _): #expect(branch == record.branchName)
            default: Issue.record("the merged branch was not deleted: \(outcome.branch)")
            }
            // The preserved spec is moved to main's backlog (0.13.x and contract CLIs alike), not lost.
            for file in plan.changes.preserved where file.path.hasPrefix("docs/features/in-progress/") {
                let destination = project.root.appendingPathComponent(file.destination).path
                #expect(FileManager.default.fileExists(atPath: destination), "\(file.path) was not moved to \(destination)")
            }
        }
    }
}
