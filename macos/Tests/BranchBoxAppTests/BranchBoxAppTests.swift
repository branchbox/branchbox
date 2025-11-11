import XCTest
@testable import BranchBoxApp

final class BranchBoxAppTests: XCTestCase {
    func testCLIRecordDecodes() throws {
        let payload = """
        [
          {
            "work_feature": "demo",
            "branch_name": "feature/demo",
            "status": "Active",
            "feature_url": "https://example.com/demo",
            "prompt_seed": null,
            "start_mode": "full",
            "updated_at": "2024-02-01T12:34:56Z",
            "tunnel_status": "none"
          }
        ]
        """.data(using: .utf8)!
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        let records = try decoder.decode([CLICompat.FeatureRecord].self, from: payload)
        XCTAssertEqual(records.first?.workFeature, "demo")
    }

    func testDevcontainerSummaryOutdated() {
        let feature = sampleFeature(devcontainerOutdated: true, lastSync: nil, syncStrategy: nil)
        XCTAssertEqual(feature.devcontainerStatusSummary, "Outdated")
        XCTAssertTrue(feature.devcontainerHasWarning)
    }

    func testDevcontainerSummaryPending() {
        let feature = sampleFeature(devcontainerOutdated: false, lastSync: nil, syncStrategy: nil)
        XCTAssertEqual(feature.devcontainerStatusSummary, "Pending")
        XCTAssertFalse(feature.devcontainerHasWarning)
    }

    func testDevcontainerSummarySyncedUsesRelativePrefix() {
        let feature = sampleFeature(devcontainerOutdated: false, lastSync: Date().addingTimeInterval(-60), syncStrategy: "copy")
        XCTAssertTrue(feature.devcontainerStatusSummary.hasPrefix("Synced"))
    }

    private func sampleFeature(devcontainerOutdated: Bool, lastSync: Date?, syncStrategy: String?) -> FeatureViewData {
        FeatureViewData(
            workFeature: "demo",
            branchName: "feature/demo",
            status: "active",
            featureURL: nil,
            promptSeed: nil,
            startMode: "full",
            updatedAt: Date(),
            tunnelStatus: nil,
            tunnelProvider: nil,
            adapterName: nil,
            adapterServiceURL: nil,
            adapterWarnings: [],
            moduleOutcomes: [],
            worktreePath: "/tmp/demo",
            devcontainerOutdated: devcontainerOutdated,
            lastSyncAt: lastSync,
            syncStrategy: syncStrategy,
            source: .grpc
        )
    }
}
