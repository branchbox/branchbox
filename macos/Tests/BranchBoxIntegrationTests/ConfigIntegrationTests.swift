import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 config (DESIGN §13.2). Contract CLIs: `config get`/`config apply` round-trips a change and keeps keys the
// app does not know; an invalid enum value is `.configInvalid` naming the allowed values. Legacy CLIs: the config
// is read from `config.json` (read-only) and applying is `.unsupported`.

extension RealCLI {
    @Suite struct ConfigIntegrationTests {
        static let config = """
            {
              "version": "1",
              "feature": { "branch_prefix": "feature", "x_team_note": "keep me" },
              "x_custom_section": { "owner": "it" }
            }

            """

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func applyRoundTripsAndKeepsUnknownKeys() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make()
            defer { repo.remove() }
            try repo.write(Self.config, to: ".branchbox/config.json")
            let project = repo.project

            let before = try await cli.backend.readConfig(project)
            #expect(before.effective.branchPrefix == "feature")
            guard cli.supports(.config) else {
                #expect(!before.editable, "legacy config is read-only")
                await #expect(throws: BackendError.unsupported(.config, minimumCLI: "0.14.0")) {
                    _ = try await cli.backend.applyConfig(ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix",
                                                                                             value: .string("spike"))]),
                                                          to: project, dryRun: false)
                }
                return
            }
            #expect(before.editable)
            #expect(before.keys.contains { $0.key == "feature.branch_prefix" })

            let patch = ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix", value: .string("spike")),
                                              ConfigChange(key: "feature.teardown.delete_branch_by_default", value: .bool(false))])
            let dryRun = try await cli.backend.applyConfig(patch, to: project, dryRun: true)
            #expect(dryRun.effective.branchPrefix == "spike")
            #expect(try await cli.backend.readConfig(project).effective.branchPrefix == "feature", "a dry run changes nothing")

            let applied = try await cli.backend.applyConfig(patch, to: project, dryRun: false)
            #expect(Set(applied.changed.map(\.key)) == ["feature.branch_prefix", "feature.teardown.delete_branch_by_default"])
            let after = try await cli.backend.readConfig(project)
            #expect(after.effective.branchPrefix == "spike")
            #expect(!after.effective.deleteBranchByDefault)

            // The file keeps what the app does not model.
            let data = try Data(contentsOf: repo.main.appendingPathComponent(".branchbox/config.json"))
            let file = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
            #expect((file["x_custom_section"] as? [String: Any])?["owner"] as? String == "it", "\(file)")
            #expect((file["feature"] as? [String: Any])?["x_team_note"] as? String == "keep me", "\(file)")

            // A new start uses the saved prefix.
            let (_, record) = try await cli.start("prefixed", in: repo)
            #expect(record.branchName == "spike/prefixed")
        }

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func invalidEnumValueIsConfigInvalidNamingTheAllowedValues() async throws {
            let cli = try await LiveCLI.make()
            guard cli.supports(.config) else { return }
            let repo = try await TempRepo.make()
            defer { repo.remove() }
            try repo.write(Self.config, to: ".branchbox/config.json")
            let original = try Data(contentsOf: repo.main.appendingPathComponent(".branchbox/config.json"))

            let patch = ConfigPatch(changes: [ConfigChange(key: "runtime.provider", value: .string("teleporter"))])
            do {
                _ = try await cli.backend.applyConfig(patch, to: repo.project, dryRun: false)
                Issue.record("an invalid runtime.provider was accepted")
            } catch BackendError.refused(let refusal) {
                guard case .configInvalid(let key, let detail) = refusal.cause else {
                    Issue.record("expected configInvalid, got \(refusal.cause)")
                    return
                }
                #expect(key == "runtime.provider")
                for allowed in ["container", "sbx"] {
                    #expect(detail.contains(allowed), "the refusal does not name \(allowed): \(detail)")
                }
            }
            let unchanged = try Data(contentsOf: repo.main.appendingPathComponent(".branchbox/config.json"))
            #expect(unchanged == original, "a refused patch must not touch the file")
        }
    }
}
