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
}
