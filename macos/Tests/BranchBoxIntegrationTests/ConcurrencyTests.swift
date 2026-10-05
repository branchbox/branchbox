import BranchBoxCLI
import BranchBoxKit
import BranchBoxStores
import Foundation
import Testing

// VER-1 concurrency (DESIGN §13.2, contract CLIs only): with `registry-lock` the OperationStore runs registry
// writers concurrently instead of FIFO; two parallel starts plus a devcontainer sync must keep every registry entry.

extension RealCLI {
    @Suite struct ConcurrencyTests {
        @MainActor
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(3)))
        func parallelStartsAndASyncKeepEveryEntry() async throws {
            let probe = try await LiveCLI.make()
            guard probe.supports(.registryLock) else {
                print("ConcurrencyTests: skipped on a CLI without registry-lock (\(probe.mode)); the stores run its writers FIFO")
                return
            }
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            try await probe.start("seed", in: repo)   // an existing entry the concurrent writers must keep
            let app = LiveAppModel()
            let store = try await app.start(with: repo)
            #expect(app.model.environment.supports(.registryLock))

            let left = try operation(of: app.model.actions.dispatch(.start(LiveCLI.minimalStart("left", in: repo.project))))
            let right = try operation(of: app.model.actions.dispatch(.start(LiveCLI.minimalStart("right", in: repo.project))))
            let sync = try operation(of: app.model.actions.dispatch(.syncDevcontainers(SyncRequest(project: repo.project,
                                                                                                 dryRun: true))))
            let records = [left, right, sync]
            try await waitUntil(timeout: .seconds(90), "the three operations to finish") { records.allSatisfy { !$0.isCancellable } }
            for record in records {
                switch record.state {
                case .succeeded, .succeededWithWarnings: break
                default: Issue.record("\(record.title) ended \(record.state)")
                }
            }

            let listing = try await probe.backend.listFeatures(in: repo.project, includeRemoved: false)
            #expect(Set(listing.features.map(\.workFeature)) == ["seed", "left", "right"], "\(listing.features.map(\.workFeature))")
            #expect(listing.strays.isEmpty)
            try await waitUntil("the store to show every entry") {
                Set(store.features.map(\.workFeature)) == ["seed", "left", "right"]
            }
            await app.tearDown()
        }
    }
}
