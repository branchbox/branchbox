import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

private struct Payload: Decodable, Equatable {
    let name: String
}

@Suite struct CLIJSONTests {
    private func data(_ text: String) -> Data { Data(text.utf8) }

    @Test func cleanPayloadHasNoPreamble() throws {
        let decoded = try CLIJSON.decode(Payload.self, from: data(#"{"name":"alpha"}"#))
        #expect(decoded.value == Payload(name: "alpha"))
        #expect(decoded.preamble == nil)
    }

    @Test func leadingWhitespaceIsNotAPreamble() throws {
        let decoded = try CLIJSON.decode(Payload.self, from: data("\n\n  {\"name\":\"alpha\"}\n"))
        #expect(decoded.value.name == "alpha")
        #expect(decoded.preamble == nil)
    }

    /// BUG-11: 0.13.x prints "Prompt truncated" on stdout ahead of the `feature start --json` payload.
    @Test func capturedPromptTruncationPreambleIsSkippedAndReturned() throws {
        let fixture = try Fixtures.data("cli-0.13.4/sandbox_start_gamma_longprompt.json")
        #expect(throws: (any Error).self) { try CLIJSON.decoder().decode(StartSummary.self, from: fixture) }

        let decoded = try CLIJSON.decode(StartSummary.self, from: fixture)
        #expect(decoded.preamble == "⚠️  Prompt truncated to 2000 characters before storage.")
        #expect(decoded.value.workFeature == "gamma")
        #expect(decoded.value.promptSeed?.count == 2000)
    }

    @Test func multiLinePreambleIsReturnedWhole() throws {
        let text = "warning: one\nwarning: two\n{\n  \"name\": \"beta\"\n}\n"
        let decoded = try CLIJSON.decode(Payload.self, from: data(text))
        #expect(decoded.value.name == "beta")
        #expect(decoded.preamble == "warning: one\nwarning: two")
    }

    @Test func preambleLineThatLooksLikeJSONIsSkipped() throws {
        let text = "[warn] registry is old\n{\"name\":\"gamma\"}"
        let decoded = try CLIJSON.decode(Payload.self, from: data(text))
        #expect(decoded.value.name == "gamma")
        #expect(decoded.preamble == "[warn] registry is old")
    }

    @Test func arrayDocumentsAreFoundToo() throws {
        let decoded = try CLIJSON.decode([Payload].self, from: data("note\r\n[{\"name\":\"a\"},{\"name\":\"b\"}]"))
        #expect(decoded.value.map(\.name) == ["a", "b"])
        #expect(decoded.preamble == "note")
    }

    /// 0.13.4 prints the dirty-module banner on stdout for `feature teardown --json`; there is no JSON.
    @Test func textOnlyOutputThrowsTheStrictError() throws {
        let fixture = try Fixtures.data("cli-0.13.4/sandbox_teardown_alpha.json")
        #expect(throws: DecodingError.self) { try CLIJSON.decode(TeardownSummary.self, from: fixture) }
        #expect(throws: DecodingError.self) { try CLIJSON.decode(Payload.self, from: Data()) }
    }

    @Test func wrongShapeAfterPreambleStillThrows() {
        #expect(throws: DecodingError.self) {
            try CLIJSON.decode(Payload.self, from: self.data("noise\n{\"other\":1}"))
        }
    }

    @Test func decoderReadsDatePropertiesAsRFC3339() throws {
        struct Stamped: Decodable { let at: Date }
        let stamped = try CLIJSON.decoder().decode(Stamped.self, from: data(#"{"at":"2026-10-01T18:51:10-04:00"}"#))
        #expect(sameInstant(stamped.at, utc(2026, 10, 1, 22, 51, 10)))
        #expect(throws: DecodingError.self) {
            try CLIJSON.decoder().decode(Stamped.self, from: self.data(#"{"at":"yesterday"}"#))
        }
    }

    // MARK: Error envelope (§5.2)

    @Test func decodesTheErrorEnvelope() throws {
        let text = """
        {"schema_version":1,"error":{"code":"teardown_refused","message":"Refusing to tear down 'eta'; nothing was removed.",
         "causes":["2 uncommitted changes"],"details":{"changed_anything":false,"completed_steps":[]}}}
        """
        let envelope = try CLIJSON.decode(ErrorEnvelope.self, from: data(text)).value
        #expect(envelope.schemaVersion == 1)
        #expect(envelope.error.code == "teardown_refused")
        #expect(envelope.error.message.hasPrefix("Refusing to tear down 'eta'"))
        #expect(envelope.error.causes == ["2 uncommitted changes"])
        #expect(envelope.error.details == .object(["changed_anything": .bool(false), "completed_steps": .array([])]))
    }

    @Test func envelopeWithoutDetailsOrCauses() throws {
        let text = #"{"schema_version":1,"error":{"code":"internal","message":"boom","details":null}}"#
        let envelope = try CLIJSON.decode(ErrorEnvelope.self, from: data(text)).value
        #expect(envelope.error.causes.isEmpty)
        #expect(envelope.error.details == nil)
    }

    @Test(arguments: [
        "cli-0.13.4/sandbox_start_alpha.json",
        "cli-0.13.4/sandbox_exec_alpha_fail.json",
        "cli-0.13.4/synthetic_devcontainer_up_docker_unavailable.json",
    ])
    func successAndInBandPayloadsAreNotEnvelopes(_ path: String) throws {
        #expect(throws: DecodingError.self) { try CLIJSON.decode(ErrorEnvelope.self, from: Fixtures.data(path)) }
    }

    @Test func legacyStringErrorIsNotAnEnvelope() {
        #expect(throws: DecodingError.self) {
            try CLIJSON.decode(ErrorEnvelope.self, from: self.data(#"{"error": "No .devcontainer directory found"}"#))
        }
    }

    // MARK: Version discovery (§5.3)

    @Test func decodesVersionInfo() throws {
        let text = """
        {"version":"0.13.4","contract_version":1,"capabilities":["json-error-envelope","registry-lock","write-ahead-start",
        "teardown-plan","teardown-discard-changes","teardown-unmerged-preflight","prune-json","detect-json",
        "devcontainer-sync-json","config","tunnel-credentials","doctor","init-json","future-thing"]}
        """
        let info = try CLIJSON.decode(VersionInfo.self, from: data(text)).value
        #expect(info.version == "0.13.4")
        #expect(info.contractVersion == 1)
        let known: Set<Capability> = [
            .jsonErrorEnvelope, .registryLock, .writeAheadStart, .teardownPlan, .teardownDiscardChanges,
            .teardownUnmergedPreflight, .pruneJSON, .detectJSON, .devcontainerSyncJSON, .config, .tunnelCredentials,
            .doctor, .initJSON,
        ]
        #expect(info.capabilitySet == known.union([Capability(rawValue: "future-thing")]))
        #expect(SemVer(parsing: info.version) == SemVer(0, 13, 4))
    }

    @Test func capabilityCodesAsABareString() throws {
        let encoded = try JSONEncoder().encode([Capability.teardownPlan])
        #expect(String(decoding: encoded, as: UTF8.self) == #"["teardown-plan"]"#)
        #expect(try JSONDecoder().decode([Capability].self, from: encoded) == [.teardownPlan])
    }

    // MARK: `branchbox --version` (legacy discovery)

    @Test(arguments: [
        ("0.13.4", SemVer(0, 13, 4)),
        ("branchbox 0.13.4", SemVer(0, 13, 4)),
        ("branchbox 0.13.4\n", SemVer(0, 13, 4)),
        ("branchbox v0.14.0", SemVer(0, 14, 0)),
        ("0.14.0-dev+abc123", SemVer(0, 14, 0, prerelease: "dev")),
        ("branchbox 1.2.3-rc.1", SemVer(1, 2, 3, prerelease: "rc.1")),
        ("10.20.30+build.7", SemVer(10, 20, 30)),
    ])
    func parsesVersions(_ text: String, _ expected: SemVer) {
        #expect(SemVer(parsing: text) == expected)
    }

    @Test(arguments: ["", "branchbox", "0.13", "0.13.4.1", "a.b.c", "0.13.x", "0.14.0-", "-1.0.0", "0..4", "version"])
    func rejectsNonVersions(_ text: String) {
        #expect(SemVer(parsing: text) == nil)
    }

    @Test func ordersPrereleasesBeforeTheirRelease() throws {
        let ordered = [
            "0.13.3", "0.13.4", "0.14.0-alpha", "0.14.0-alpha.1", "0.14.0-alpha.beta", "0.14.0-beta.2",
            "0.14.0-beta.11", "0.14.0-dev", "0.14.0", "0.14.1", "1.0.0",
        ].map { SemVer(parsing: $0)! }
        #expect(ordered == ordered.sorted())
        for (lower, higher) in zip(ordered, ordered.dropFirst()) {
            #expect(lower < higher, "\(lower) < \(higher)")
            #expect(!(higher < lower))
        }
        #expect(SemVer(0, 14, 0, prerelease: "dev") >= BackendIdentity.minimumCLI)
        #expect(SemVer(0, 13, 3) < BackendIdentity.minimumCLI)
    }

    @Test func descriptionDropsBuildMetadata() {
        #expect(SemVer(parsing: "0.14.0-dev+abc123")?.description == "0.14.0-dev")
        #expect(SemVer(0, 13, 4).description == "0.13.4")
        #expect(SemVer(0, 13, 4, prerelease: "") == SemVer(0, 13, 4))
    }
}
