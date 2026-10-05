import BranchBoxKit
import Foundation
import Testing

// VER-1 live fixtures (DESIGN §13.2): decode what a real CLI printed, captured by
// `scripts/macos-capture-fixtures.sh <cli> <dir>`, with the app's models. Runs only when BRANCHBOX_LIVE_FIXTURES
// names that folder. Every captured stdout must decode as its manifest `kind`; keys the models never read are
// printed as GitHub `::warning::` annotations (drift to look at, not a failure).
//
//   scripts/macos-capture-fixtures.sh "$(command -v branchbox)" /tmp/live
//   BRANCHBOX_LIVE_FIXTURES=/tmp/live swift test --package-path macos --filter LiveFixtureDecodeTests

private let liveFixtures = ProcessInfo.processInfo.environment["BRANCHBOX_LIVE_FIXTURES"].flatMap { $0.isEmpty ? nil : $0 }

@Suite struct LiveFixtureDecodeTests {
    struct Manifest: Decodable {
        struct Entry: Decodable {
            let name: String
            let kind: String
            let exit: Int
            let stdout: String?
            let skipped: Bool?
        }
        let cliVersion: String?
        let entries: [Entry]

        private enum CodingKeys: String, CodingKey { case cliVersion = "cli_version", entries }
    }

    /// Decodes `data` as the model for `kind` with the production decoder, then reports the keys it never read.
    /// Returns nil for a kind this suite does not know.
    static func check(kind: String, data: Data) throws -> [String]? {
        func run<T: Decodable>(_ type: T.Type) throws -> [String] {
            _ = try CLIJSON.decode(type, from: data)
            let json = try CLIJSON.decode(JSONValue.self, from: data).value
            return try KeyRecorder.unknownKeys(type, in: json).get()
        }
        switch kind {
        case "version": return try run(VersionInfo.self)
        case "feature_list":
            let records = try CLIJSON.decode([Lossy<FeatureRecord>].self, from: data).value
            let listing = FeatureListing(decoding: records)
            #expect(listing.droppedRecords == 0, "\(listing.warnings)")
            return try run([Lossy<FeatureRecord>].self)
        case "start_summary": return try run(StartSummary.self)
        case "exec_result": return try run(ExecResult.self)
        case "teardown_plan": return try run(TeardownPlanDocument.self)
        case "teardown_summary": return try run(TeardownSummary.self)
        case "error_envelope": return try run(ErrorEnvelope.self)
        default: return nil
        }
    }

    @Test(.enabled(if: liveFixtures != nil))
    func everyCapturedPayloadDecodes() throws {
        let folder = URL(fileURLWithPath: try #require(liveFixtures), isDirectory: true)
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: folder.appendingPathComponent("manifest.json")))
        let version = manifest.cliVersion ?? "unknown CLI"
        var decoded = 0
        for entry in manifest.entries {
            if entry.skipped == true {
                print("LiveFixtureDecodeTests: \(entry.name) skipped: \(version) does not support it (exit \(entry.exit))")
                continue
            }
            guard let file = entry.stdout else {
                // 0.13.x prints no envelope; its failures are text on stderr only.
                #expect(entry.kind == "error_envelope" && entry.exit != 0, "\(entry.name) has no stdout")
                continue
            }
            let data = try Data(contentsOf: folder.appendingPathComponent(file))
            do {
                guard let unknown = try Self.check(kind: entry.kind, data: data) else {
                    Issue.record("\(entry.name): unknown manifest kind \(entry.kind)")
                    continue
                }
                decoded += 1
                for key in unknown {
                    print("::warning title=Unread CLI key::\(version) \(entry.name) (\(entry.kind)): the app does not read \(key)")
                }
            } catch {
                Issue.record("\(entry.name) (\(entry.kind)) from \(version) does not decode: \(error)")
            }
        }
        #expect(decoded > 0, "no payload in \(folder.path) was decoded")
    }

    /// The key recorder itself: unread keys are reported with their paths, and fully read containers are not.
    @Test func keyRecorderReportsUnreadKeys() throws {
        let json = """
            {"exit_code": 0, "stdout": "hi\\n", "stderr": "", "extra": {"nested": 1}, "list": [{"a": 1}]}
            """
        let unknown = try #require(try Self.check(kind: "exec_result", data: Data(json.utf8)))
        #expect(unknown == ["extra", "list"])
        let envelope = """
            {"schema_version": 1, "error": {"code": "x", "message": "m", "causes": [], "details": {"any": "thing"}, "new_field": true}}
            """
        let envelopeUnknown = try #require(try Self.check(kind: "error_envelope", data: Data(envelope.utf8)))
        #expect(envelopeUnknown == ["error.new_field"], "details is a free-form object the app reads whole")
    }
}
