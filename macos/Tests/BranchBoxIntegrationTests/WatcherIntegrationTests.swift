import BranchBoxCLI
import BranchBoxKit
import BranchBoxStores
import Foundation
import Testing

// VER-1 watcher (DESIGN §13.2): a feature started from Terminal (the CLI run directly, not through the app)
// reaches `ProjectStore.features` within 2 s, through the registry watcher rather than the 1-minute timer.

extension RealCLI {
    @Suite struct WatcherIntegrationTests {
        @MainActor
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func aCLIWriteRefreshesTheProjectWithinTwoSeconds() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            try await cli.start("before", in: repo)   // so `.branchbox/` exists when the watcher starts
            let app = LiveAppModel()
            let store = try await app.start(with: repo)
            #expect(store.features.map(\.workFeature) == ["before"])
            #expect(app.model.settings.watchProjectFiles)

            let terminal = try await cli.run(["feature", "start", "outside", "--repo", repo.main.path, "--json",
                                              "--runtime", "container", "--minimal", "--skip-module", "tunnel"],
                                             in: repo.main)
            #expect(terminal.termination == .exited(0), "\(terminal.stderrTail.suffix(5))")
            let written = ContinuousClock.now
            try await waitUntil(timeout: .seconds(2), "the watcher to refresh the project") {
                store.features.contains { $0.workFeature == "outside" }
            }
            print("WatcherIntegrationTests (\(cli.mode)): refreshed \(String(format: "%.2f", elapsed(since: written))) s after the CLI wrote")
            await app.tearDown()
        }
    }
}
