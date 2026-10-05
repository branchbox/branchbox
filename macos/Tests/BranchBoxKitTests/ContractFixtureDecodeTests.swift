@testable import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 (DESIGN §13.2): every Rust golden fixture in `cli/tests/fixtures/contract/**` decodes into the Swift type
// the app reads it with. The folder is found from this file's path, so a payload change on the Rust side fails here
// too. A fixture this table does not know fails `everyFixtureHasADecoder`: add its mapping with the new payload.
//
// The contract payloads whose Kit models are not `Decodable` (detect, sync, config get, doctor, init; wave-1
// deviations) are read through BranchBoxCLI's own payload decoders, exactly as `CLIBackend` reads them.

enum ContractFixtures {
    /// `<repo>/cli/tests/fixtures/contract`, from `<repo>/macos/Tests/BranchBoxKitTests/<this file>`.
    static let root: URL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("cli/tests/fixtures/contract", isDirectory: true)

    /// Every `<area>/<name>.json`, sorted.
    static let all: [String] = {
        let files = FileManager.default
        guard let areas = try? files.contentsOfDirectory(atPath: root.path) else { return [] }
        return areas.sorted().flatMap { area -> [String] in
            let names = (try? files.contentsOfDirectory(atPath: root.appendingPathComponent(area).path)) ?? []
            return names.filter { $0.hasSuffix(".json") }.sorted().map { "\(area)/\($0)" }
        }
    }()

    static func data(_ path: String) throws -> Data {
        try Data(contentsOf: root.appendingPathComponent(path))
    }
}

/// `prune --json` (§5.6 / RS-2): the app tears down row by row, so only the embedded plans and summaries are models.
private struct PrunePlanPayload: Decodable {
    struct Candidate: Decodable { let workFeature: String; let plan: TeardownPlanDocument?
        private enum CodingKeys: String, CodingKey { case workFeature = "work_feature", plan }
    }
    let candidates: [Candidate]
}

private struct PruneExecutePayload: Decodable {
    struct Result: Decodable { let workFeature: String; let outcome: String; let summary: TeardownSummary?
        private enum CodingKeys: String, CodingKey { case workFeature = "work_feature", outcome, summary }
    }
    let results: [Result]
}

/// The staged-runtime compatibility probe is not part of the Mac app's backend, but its
/// Rust golden fixture still participates in the shared contract gate.
private struct ManagedRuntimeCapabilitiesPayload: Decodable {
    let schemaVersion: UInt32
    let managedWorkspaceContractV1: Bool
    let preloadedComposeSanitizationV1: Bool

    private enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case managedWorkspaceContractV1 = "managed_workspace_contract_v1"
        case preloadedComposeSanitizationV1 = "preloaded_compose_sanitization_v1"
    }
}

@Suite struct ContractFixtureDecodeTests {
    /// What the app decodes a fixture as, by area and name; nil for a fixture with no mapping.
    static func decoder(for path: String) -> ((Data) throws -> Void)? {
        let name = URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent
        let area = URL(fileURLWithPath: path).deletingLastPathComponent().lastPathComponent
        if name.hasPrefix("envelope_") { return decodeEnvelope }
        if name.hasPrefix("version") { return { _ = try CLIJSON.decode(VersionInfo.self, from: $0) } }
        switch (area, name) {
        case ("core", "runtime_capabilities"):
            return { data in
                let payload = try CLIJSON.decode(ManagedRuntimeCapabilitiesPayload.self, from: data).value
                #expect(payload.schemaVersion == 1)
                #expect(payload.managedWorkspaceContractV1 && payload.preloadedComposeSanitizationV1)
            }
        case ("core", "exec_inband_failure"):
            return { data in
                let result = try CLIJSON.decode(ExecResult.self, from: data).value
                #expect(result.exitCode == 3 && result.stdout == "out\n" && result.stderr == "err\n")
            }
        case ("core", let list) where list.hasPrefix("list_"):
            return { data in
                let listing = FeatureListing(decoding: try CLIJSON.decode([Lossy<FeatureRecord>].self, from: data).value)
                #expect(listing.droppedRecords == 0, "\(listing.warnings)")
                #expect(!listing.features.isEmpty)
                if list == "list_interrupted" { #expect(listing.features.first?.setup?.state == .interrupted) }
                if list == "list_orphaned" { #expect(listing.features.first?.status == .orphaned) }
            }
        case ("core", let start) where start.hasPrefix("start_"):
            return { data in
                let summary = try CLIJSON.decode(StartSummary.self, from: data).value
                #expect(!summary.workFeature.isEmpty && !summary.branchName.isEmpty)
            }
        case ("teardown", let plan) where plan.hasPrefix("plan_"):
            return { data in try checkPlan(try CLIJSON.decode(TeardownPlanDocument.self, from: data).value) }
        case ("teardown", let summary) where summary.hasPrefix("summary_"):
            return { data in
                let summary = try CLIJSON.decode(TeardownSummary.self, from: data).value
                #expect(summary.worktreeRemoved)
            }
        case ("teardown", "refusal_envelope"):
            return { data in
                let envelope = try CLIJSON.decode(ErrorEnvelope.self, from: data).value
                #expect(envelope.error.code == "teardown_refused")
                // The CLI's own plan travels in `details.plan` and becomes the refusal's plan.
                let error = CLIErrorClassifier.classify(envelope: envelope, diagnostics: Diagnostics(summary: ""),
                                                        context: CLIErrorClassifier.Context())
                guard case .refused(let refusal) = error else {
                    Issue.record("a teardown_refused envelope classified as \(error)")
                    return
                }
                try checkPlan(try #require(refusal.plan, "details.plan did not decode"))
            }
        case ("teardown", "prune_dry_run"):
            return { data in
                let payload = try CLIJSON.decode(PrunePlanPayload.self, from: data).value
                #expect(!payload.candidates.isEmpty)
                for candidate in payload.candidates {
                    try checkPlan(try #require(candidate.plan, "\(candidate.workFeature) has no plan"))
                }
            }
        case ("teardown", "prune_execute"):
            return { data in
                let payload = try CLIJSON.decode(PruneExecutePayload.self, from: data).value
                #expect(!payload.results.isEmpty)
                for result in payload.results where result.outcome == "removed" {
                    #expect(result.summary?.worktreeRemoved == true, "\(result.workFeature)")
                }
            }
        case ("commands", "config_apply"):
            return { data in
                let result = try CLIJSON.decode(ConfigApplyResult.self, from: data).value
                #expect(!result.changed.isEmpty)
            }
        case ("commands", let config) where config.hasPrefix("config_get"):
            return { data in
                let document = try CLIJSON.decode(ConfigGetPayload.self, from: data).value.document
                #expect(!document.keys.isEmpty)
                #expect(!document.path.isEmpty)
            }
        case ("commands", let credentials) where credentials.hasPrefix("credentials_"):
            return { data in _ = try CLIJSON.decode(TunnelCredentialsResult.self, from: data) }
        case ("commands", let detect) where detect.hasPrefix("detect_"):
            return { data in
                let report = try CLIJSON.decode(DetectPayload.self, from: data).value.report
                #expect(report.project != nil)
            }
        case ("commands", let doctor) where doctor.hasPrefix("doctor_"):
            return { data in
                let checks = try CLIJSON.decode(DoctorPayload.self, from: data).value.checks
                #expect(!checks.isEmpty)
            }
        case ("commands", let initReport) where initReport.hasPrefix("init_"):
            return { data in
                let payload = try CLIJSON.decode(InitPayload.self, from: data).value
                #expect(!payload.workspacePath.isEmpty)
            }
        case ("commands", let sync) where sync.hasPrefix("sync_"):
            return { data in
                let report = try CLIJSON.decode(SyncPayload.self, from: data).value.report
                #expect(!report.rows.contains { $0.status == .unknown }, "\(report.rows)")
            }
        case ("commands", let tunnel) where tunnel.hasPrefix("tunnel_"):
            return { data in
                let change = try CLIJSON.decode(TunnelChange.self, from: data).value
                #expect(!change.workFeature.isEmpty)
            }
        default:
            return nil
        }
    }

    private static func decodeEnvelope(_ data: Data) throws {
        let envelope = try CLIJSON.decode(ErrorEnvelope.self, from: data).value
        #expect(envelope.schemaVersion == 1)
        #expect(!envelope.error.code.isEmpty && !envelope.error.message.isEmpty)
    }

    /// Nothing in a golden plan may be dropped by the lossy decoders: a dropped blocker or change reads as safer.
    private static func checkPlan(_ plan: TeardownPlanDocument) throws {
        #expect(plan.droppedBlockers == 0)
        #expect(plan.changes.droppedEntries == 0)
    }

    @Test func theFixtureFolderIsFound() {
        #expect(ContractFixtures.all.count >= 30, "found \(ContractFixtures.all.count) fixtures under \(ContractFixtures.root.path)")
        for area in ["core", "teardown", "commands"] {
            #expect(ContractFixtures.all.contains { $0.hasPrefix("\(area)/") }, "no \(area) fixtures")
        }
    }

    @Test func everyFixtureHasADecoder() {
        let unmapped = ContractFixtures.all.filter { Self.decoder(for: $0) == nil }
        #expect(unmapped.isEmpty, "add a decoder mapping for \(unmapped)")
    }

    @Test(arguments: ContractFixtures.all)
    func fixtureDecodes(_ path: String) throws {
        guard let decode = Self.decoder(for: path) else { return }
        try decode(try ContractFixtures.data(path))
    }
}
