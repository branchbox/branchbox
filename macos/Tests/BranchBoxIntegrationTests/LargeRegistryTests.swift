import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 large registry and runner stress (DESIGN §13.2, VER-1 brief item 3): 40 real features push
// `feature list --json` past 64 KiB (more than one pipe buffer); the backend still reads it within 10 s, 20
// concurrent lists all succeed, and cancelling and `terminateAll` leave no `branchbox` process behind.

extension RealCLI {
    @Suite struct LargeRegistryTests {
        static let count = 40

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(5)))
        func fortyFeaturesListQuicklyAndSurviveConcurrentReaders() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            for index in 0..<Self.count {
                _ = try await cli.backend.startFeature(LiveCLI.minimalStart(String(format: "load-%02d", index), in: repo.project),
                                                       progress: { _ in })
            }

            // The raw document is larger than 64 KiB.
            let raw = try await cli.run(["feature", "list", "--json", "--repo", repo.main.path], in: repo.main)
            #expect(raw.termination == .exited(0))
            #expect(raw.stdout.count > 64 * 1024, "list --json printed only \(raw.stdout.count) bytes")

            // The backend reads all 40 within 10 s.
            let started = ContinuousClock.now
            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let seconds = elapsed(since: started)
            #expect(listing.features.count == Self.count)
            #expect(listing.droppedRecords == 0)
            #expect(listing.strays.isEmpty)
            #expect(seconds < 10, "listing \(Self.count) features took \(seconds) s")
            print("LargeRegistryTests (\(cli.mode)): \(raw.stdout.count) bytes, listed in \(String(format: "%.2f", seconds)) s")

            // 20 concurrent readers through one runner, as the stores' refreshes and the menu bar would.
            let backend = cli.backend
            let project = repo.project
            let counts = try await withThrowingTaskGroup(of: Int.self) { group in
                for _ in 0..<20 {
                    group.addTask { try await backend.listFeatures(in: project, includeRemoved: true).features.count }
                }
                return try await group.reduce(into: [Int]()) { $0.append($1) }
            }
            #expect(counts == Array(repeating: Self.count, count: 20))

            // Cancelling readers, then terminateAll, leaves no branchbox process for this repository.
            let readers = (0..<10).map { _ in
                Task { try await backend.listFeatures(in: project, includeRemoved: true) }
            }
            try await Task.sleep(for: .milliseconds(30))
            for reader in readers { reader.cancel() }
            for reader in readers { _ = await reader.result }
            await cli.runner.terminateAll()
            #expect(cli.runner.liveRunCount == 0)
            #expect(try await processes(mentioning: repo.container.path).isEmpty)
        }
    }
}
