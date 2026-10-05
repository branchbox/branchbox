import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

enum PayloadKind: String, Sendable {
    case featureList, startSummary, teardownSummary, execResult, devcontainerResult, devcontainerService
}

struct PayloadFixture: Sendable, CustomTestStringConvertible {
    let name: String
    let kind: PayloadKind
    init(_ name: String, _ kind: PayloadKind) { self.name = name; self.kind = kind }
    var path: String { "cli-0.13.4/\(name)" }
    var testDescription: String { name }
}

private func decodeFixture<T: Decodable>(_ type: T.Type, _ name: String) throws -> (value: T, preamble: String?) {
    try CLIJSON.decode(type, from: Fixtures.data("cli-0.13.4/\(name)"))
}

private func decodeJSON<T: Decodable>(_ type: T.Type, _ text: String) throws -> T {
    try CLIJSON.decode(type, from: Data(text.utf8)).value
}

private func listing(_ name: String) throws -> FeatureListing {
    FeatureListing(decoding: try decodeFixture([Lossy<FeatureRecord>].self, name).value)
}

@Suite struct ModelDecodingTests {
    /// Every `.json` capture that holds a CLI payload, with the model it decodes as.
    static let payloads: [PayloadFixture] = [
        PayloadFixture("main_feature_list.json", .featureList),
        PayloadFixture("main_feature_list_all.json", .featureList),
        PayloadFixture("main_feature_list_repo.json", .featureList),
        PayloadFixture("sandbox_feature_list.json", .featureList),
        PayloadFixture("sandbox_feature_list_all_after.json", .featureList),
        PayloadFixture("synthetic_feature_list_new_statuses.json", .featureList),
        PayloadFixture("sandbox_start_alpha.json", .startSummary),
        PayloadFixture("sandbox_start_beta_full.json", .startSummary),
        PayloadFixture("sandbox_start_gamma_longprompt.json", .startSummary),
        PayloadFixture("sandbox_start_theta_trace.json", .startSummary),
        PayloadFixture("sandbox_start_zeta_prefix.json", .startSummary),
        PayloadFixture("sandbox_teardown_alpha_force.json", .teardownSummary),
        PayloadFixture("sandbox_exec_alpha.json", .execResult),
        PayloadFixture("sandbox_exec_alpha_fail.json", .execResult),
        PayloadFixture("synthetic_devcontainer_exec.json", .execResult),
        PayloadFixture("synthetic_devcontainer_up.json", .devcontainerResult),
        PayloadFixture("synthetic_devcontainer_up_image.json", .devcontainerResult),
        PayloadFixture("synthetic_devcontainer_up_docker_unavailable.json", .devcontainerResult),
        PayloadFixture("synthetic_devcontainer_down.json", .devcontainerResult),
        PayloadFixture("synthetic_devcontainer_down_compose.json", .devcontainerResult),
        PayloadFixture("synthetic_devcontainer_build.json", .devcontainerResult),
        PayloadFixture("synthetic_devcontainer_detect.json", .devcontainerService),
    ]

    /// `.json` captures that are deliberately not payloads.
    static let nonPayloadJSON: Set<String> = [
        "main_agent_status.json",       // empty stdout: `agent status` failed (no daemon)
        "sandbox_teardown_alpha.json",  // 0.13.4 printed the dirty-module banner instead of JSON
        "synthetic_agent_status.json",  // gRPC-era agent status; the app no longer reads it
    ]

    @Test(arguments: ModelDecodingTests.payloads)
    func decodesEveryPayloadFixture(_ fixture: PayloadFixture) throws {
        let data = try Fixtures.data(fixture.path)
        switch fixture.kind {
        case .featureList:
            let records = try CLIJSON.decode([Lossy<FeatureRecord>].self, from: data).value
            #expect(!records.isEmpty)
            #expect(records.allSatisfy { $0.value != nil }, "dropped: \(records.compactMap(\.error))")
        case .startSummary:
            let summary = try CLIJSON.decode(StartSummary.self, from: data).value
            #expect(!summary.workFeature.isEmpty)
            #expect(!summary.moduleOutcomes.isEmpty)
            #expect(summary.generatedAt != nil)
        case .teardownSummary:
            #expect(try !CLIJSON.decode(TeardownSummary.self, from: data).value.workFeature.isEmpty)
        case .execResult:
            _ = try CLIJSON.decode(ExecResult.self, from: data)
        case .devcontainerResult:
            #expect(try !CLIJSON.decode(DevcontainerResult.self, from: data).value.outcome.isEmpty)
        case .devcontainerService:
            #expect(try CLIJSON.decode(DevcontainerServiceInfo.self, from: data).value.serviceName != nil)
        }
    }

    @Test func everyJSONFixtureIsAccountedFor() throws {
        let json = Set(try Fixtures.names(in: "cli-0.13.4").filter { $0.hasSuffix(".json") })
        let known = Set(Self.payloads.map(\.name)).union(Self.nonPayloadJSON)
        #expect(json == known)
    }

    @Test func fixturesCarryNoLocalPaths() throws {
        let names = try Fixtures.names(in: "cli-0.13.4")
        #expect(names.count > 50)
        for name in names {
            let text = try Fixtures.string("cli-0.13.4/\(name)")
            #expect(!text.contains("/Users/rbarazi"), "\(name)")
            #expect(!text.contains("/private/tmp/claude"), "\(name)")
        }
    }

    // MARK: feature list

    @Test func mainListWithRemovedDecodesEightDatedRecords() throws {
        let listing = try listing("main_feature_list_all.json")
        #expect(listing.features.count == 8)
        #expect(listing.droppedRecords == 0)
        #expect(listing.warnings.isEmpty)
        #expect(listing.features.allSatisfy { $0.createdAt != nil && $0.updatedAt != nil })
        let removed = listing.features.filter { $0.status == .removed }
        #expect(removed.count == 6)
        #expect(removed.allSatisfy { $0.removedAt != nil })
        #expect(listing.features.filter { $0.status == .active }.map(\.workFeature) == ["prine", "remotion"])
    }

    @Test func prineDecodesEveryField() throws {
        let prine = try #require(try listing("main_feature_list_all.json").features.first { $0.workFeature == "prine" })
        #expect(prine.status == .active)
        #expect(prine.branchName == "feature/prine")
        #expect(prine.branchPrefix == "feature")
        #expect(prine.urls.primary == URL(string: "https://dev-prine.localhost"))
        #expect(prine.worktreePath == "/Users/dev/projects/branchbox-suite/branchbox/prine")
        #expect(prine.baseBranch == nil)
        #expect(prine.composeProjectName == "branchbox-prine")
        #expect(prine.color == "#f39c12")
        #expect(prine.startMode == "full")
        #expect(sameInstant(prine.createdAt, utc(2026, 3, 17, 3, 37, 57, nanoseconds: 979_509_000)))
        #expect(prine.removedAt == nil)

        let tunnel = try #require(prine.tunnel)
        #expect(tunnel.status == .disabled)
        #expect(tunnel.provider == "cloudflared")
        #expect(tunnel.hostname == nil)
        #expect(tunnel.notes == "Tunnel provisioning disabled in project configuration")
        #expect(tunnel.lastUpdated != nil)

        #expect(prine.moduleOutcomes.map(\.module) == ["devcontainer", "compose", "specs", "tunnel"])
        #expect(prine.moduleOutcomes.filter { $0.status == .success }.count == 3)
        #expect(prine.moduleOutcomes.last?.status == .skipped)
        #expect(prine.moduleOutcomes.allSatisfy { $0.recordedAt != nil && !$0.forced })
        #expect(prine.moduleOutcomes[1].durationMs == 190)

        #expect(prine.adapter == AdapterInfo(name: "Generic", serviceURL: "http://dev:3000", warnings: []))
        #expect(prine.runtime == .containerDefault)
        #expect(prine.defaultAgent?.status == .disabled)
        #expect(prine.defaultAgent?.detail?.hasPrefix("Set BRANCHBOX_DEFAULT_AGENT_CMD") == true)
        #expect(prine.setup == nil)
    }

    @Test func syntheticStatusesDecode() throws {
        let features = try listing("synthetic_feature_list_new_statuses.json").features
        #expect(features.map(\.status) == [.degraded, .failedRetained, .orphaned])
        #expect(features.map(\.runtime.provider) == [.sbx, .localVM, .inGuest])

        let sbx = features[0]
        #expect(sbx.runtime.publishedPorts == [PublishedPort(host: 49152, runtime: 3000)])
        #expect(sbx.runtime.runtimeID == "branchbox-sbx-demo")
        #expect(sbx.runtime.containerUser == "vscode")
        #expect(sbx.urls.ports.first?.runtimePort == 3000)
        #expect(sbx.prNumber == 42)
        #expect(sbx.devcontainerOutdated)
        #expect(sbx.syncStrategy == "copy")
        #expect(sbx.promptSeed == "do the thing")
        #expect(sameInstant(sbx.createdAt, utc(2026, 9, 30, 10, 0, 0, nanoseconds: 123_456_789)))
        #expect(sameInstant(sbx.updatedAt, utc(2026, 9, 30, 10, 5, 0, nanoseconds: 500_000_000)))
        #expect(sameInstant(sbx.lastSyncAt, utc(2026, 9, 30, 10, 3, 0, nanoseconds: 250_000_000)))
        #expect(sbx.tunnel?.status == .active)
        #expect(sbx.tunnel?.hostname == "sbx-demo.example.dev")
        #expect(sbx.moduleOutcomes == [ModuleOutcome(module: "devcontainer", status: .failed, durationMs: 1200,
                                                     notes: ["devcontainer up failed"], forced: false,
                                                     recordedAt: sbx.updatedAt)])
        #expect(sbx.defaultAgent == DefaultAgentPlan(status: .waiting, label: "claude", command: "claude",
                                                     detail: "Devcontainer module not detected; launch deferred",
                                                     followup: "..."))
        #expect(features[1].startMode == "minimal")
        #expect(features[1].tunnel == nil)
        #expect(features[1].featureURL == nil)
    }

    @Test func unknownValuesRoundTrip() throws {
        let record = try decodeJSON(FeatureRecord.self, """
        {"work_feature":"x","status":"paused","runtime":{"provider":"firecracker"},
         "tunnel":{"status":"rebooting"},"module_outcomes":[{"module":"m","status":"partial"}],
         "default_agent":{"status":"thinking"},"setup":{"state":"resuming"}}
        """)
        #expect(record.status == .unknown("paused"))
        #expect(record.runtime.provider == .unknown("firecracker"))
        #expect(record.tunnel?.status == .unknown("rebooting"))
        #expect(record.moduleOutcomes.first?.status == .unknown("partial"))
        #expect(record.defaultAgent?.status == .unknown("thinking"))
        #expect(record.setup?.state == .unknown("resuming"))

        let encoder = JSONEncoder()
        #expect(String(decoding: try encoder.encode(record.status), as: UTF8.self) == #""paused""#)
        #expect(try JSONDecoder().decode(FeatureStatus.self, from: encoder.encode(record.status)) == .unknown("paused"))
        #expect(try JSONDecoder().decode(RuntimeProvider.self, from: encoder.encode(record.runtime.provider))
                == .unknown("firecracker"))
        let runtime = try JSONDecoder().decode(RuntimeInfo.self, from: encoder.encode(record.runtime))
        #expect(runtime.provider == .unknown("firecracker"))
    }

    @Test func knownRawValuesMapBothWays() {
        let statuses: [(String, FeatureStatus)] = [("active", .active), ("degraded", .degraded),
                                                   ("failed_retained", .failedRetained), ("orphaned", .orphaned),
                                                   ("removed", .removed)]
        for (raw, value) in statuses {
            #expect(FeatureStatus(raw: raw) == value)
            #expect(value.raw == raw)
        }
        let providers: [(String, RuntimeProvider)] = [("container", .container), ("sbx", .sbx), ("local-vm", .localVM),
                                                      ("in-guest", .inGuest)]
        for (raw, value) in providers {
            #expect(RuntimeProvider(raw: raw) == value)
            #expect(value.raw == raw)
        }
        #expect(SetupState(raw: "in_progress") == .inProgress)
        #expect(SetupState.inProgress.raw == "in_progress")
        #expect(TunnelStatus(raw: "manual") == .manual)
        #expect(AgentPlanStatus(raw: "blocked") == .blocked)
        // Raw spellings the CLI never prints stay unknown rather than being guessed at.
        #expect(FeatureStatus(raw: "failedRetained") == .unknown("failedRetained"))
        #expect(FeatureStatus(raw: "Active") == .unknown("Active"))
    }

    @Test func okModuleStatusIsSuccess() throws {
        let outcome = try decodeJSON(ModuleOutcome.self, #"{"module":"compose","status":"ok"}"#)
        #expect(outcome.status == .success)
        #expect(outcome.status.raw == "success")
    }

    @Test func recordWithoutWorkFeatureIsDroppedAndCounted() throws {
        let records = try decodeJSON([Lossy<FeatureRecord>].self, """
        [{"work_feature":"one","status":"active"},
         {"branch_name":"feature/ghost","status":"active"},
         {"work_feature":"two","status":"removed"},
         "not even an object"]
        """)
        let listing = FeatureListing(decoding: records, warnings: ["existing"])
        #expect(listing.features.map(\.workFeature) == ["one", "two"])
        #expect(listing.droppedRecords == 2)
        #expect(listing.warnings.count == 3)
        #expect(listing.warnings.first == "existing")
        #expect(listing.warnings[1].contains("work_feature"))
    }

    @Test func malformedOptionalFieldsDecodeToNil() throws {
        let record = try decodeJSON(FeatureRecord.self, """
        {"work_feature":"x","tunnel":"nope","adapter":5,"runtime":[],"default_agent":"x","pr_number":"42",
         "created_at":"yesterday","updated_at":1700000000,"color":7,"devcontainer_outdated":"yes",
         "module_outcomes":[{"module":"compose","status":"success"},{"status":"failed"},{"module":"specs","notes":"x"}],
         "setup":{"pid":1},"brand_new_key":{"anything":[1,2,3]}}
        """)
        #expect(record.tunnel == nil)
        #expect(record.adapter == nil)
        #expect(record.runtime == .containerDefault)
        #expect(record.defaultAgent == nil)
        #expect(record.prNumber == nil)
        #expect(record.createdAt == nil)
        #expect(record.updatedAt == nil)
        #expect(record.color == nil)
        #expect(record.devcontainerOutdated == false)
        #expect(record.moduleOutcomes.map(\.module) == ["compose", "specs"])
        #expect(record.moduleOutcomes.last?.notes == [])
        #expect(record.setup == nil)                  // a setup marker needs its state
    }

    @Test func minimalRecordTakesDefaults() throws {
        let record = try decodeJSON(FeatureRecord.self, #"{"work_feature":"bare"}"#)
        #expect(record == FeatureRecord(workFeature: "bare", status: .unknown("")))
        #expect(record.branchName == "")
        #expect(record.runtime.provider == .container)
        #expect(record.branchPrefix == nil)
    }

    @Test func writeAheadSetupDecodes() throws {
        let records = try decodeJSON([FeatureRecord].self, """
        [{"work_feature":"a","status":"active","setup":{"state":"in_progress","pid":48211,"started_at":"2026-10-01T22:50:29.222458Z"}},
         {"work_feature":"b","status":"active","setup":{"state":"interrupted","pid":null,"started_at":null}}]
        """)
        let setup = try #require(records[0].setup)
        #expect(setup.state == .inProgress)
        #expect(setup.pid == 48211)
        #expect(sameInstant(setup.startedAt, utc(2026, 10, 1, 22, 50, 29, nanoseconds: 222_458_000)))
        #expect(records[1].setup == SetupInfo(state: .interrupted))
        #expect(records[0].status == .active)
    }

    @Test(arguments: [
        ("feature/prine", "prine", "feature" as String?),
        ("spike/zeta", "zeta", "spike"),
        ("team/a/zeta", "zeta", "team/a"),
        ("zeta", "zeta", ""),
        ("/zeta", "zeta", nil),
        ("feature/other", "zeta", nil),
        ("feature/xzeta", "zeta", nil),
        ("", "zeta", nil),
    ])
    func branchPrefix(_ branch: String, _ name: String, _ expected: String?) {
        #expect(FeatureRecord(workFeature: name, branchName: branch).branchPrefix == expected)
    }

    @Test func subObjectsEncodeBackToTheCLIShape() throws {
        let prine = try #require(try listing("main_feature_list_all.json").features.first)
        let encoder = JSONEncoder()
        let decoder = CLIJSON.decoder()
        #expect(try decoder.decode(TunnelState.self, from: encoder.encode(prine.tunnel)) == prine.tunnel)
        #expect(try decoder.decode([ModuleOutcome].self, from: encoder.encode(prine.moduleOutcomes)) == prine.moduleOutcomes)
        #expect(try decoder.decode(RuntimeInfo.self, from: encoder.encode(prine.runtime)) == prine.runtime)
        #expect(try decoder.decode(AdapterInfo.self, from: encoder.encode(prine.adapter)) == prine.adapter)
        // Dates the CLI wrote (microseconds) survive the encode/decode round trip exactly.
        let setup = SetupInfo(state: .inProgress, pid: 7, startedAt: RFC3339.parse("2026-10-01T22:50:29.222458Z"))
        #expect(try decoder.decode(SetupInfo.self, from: encoder.encode(setup)) == setup)

        let json = try #require(try JSONSerialization.jsonObject(with: encoder.encode(prine.tunnel)) as? [String: Any])
        #expect(json["last_updated"] as? String == "2026-03-17T03:37:57.975531Z")
        #expect(json["status"] as? String == "disabled")
    }

    // MARK: start / teardown / exec

    @Test func startSummaryDecodes() throws {
        let decoded = try decodeFixture(StartSummary.self, "sandbox_start_alpha.json")
        #expect(decoded.preamble == nil)
        let alpha = decoded.value
        #expect(alpha.workFeature == "alpha")
        #expect(alpha.branchName == "feature/alpha")
        #expect(alpha.worktreePath == "/tmp/bbx/alpha")
        #expect(alpha.mode == "minimal")
        #expect(alpha.featureURL == nil)
        #expect(alpha.promptSeed == nil)
        #expect(alpha.runtime?.provider == .container)
        #expect(alpha.moduleOutcomes.allSatisfy { $0.status == .skipped })
        #expect(alpha.skippedModules.first == SkippedModule(module: "tunnel", reason: "Skipped via --skip-module"))
        #expect(alpha.skippedModules.count == 4)
        #expect(alpha.warnings.count == 2)
        #expect(alpha.tunnel?.provider == "manual")
        #expect(alpha.tunnel?.status == .disabled)
        #expect(alpha.promptBridgeEnabled == false)
        #expect(sameInstant(alpha.generatedAt, utc(2026, 10, 1, 22, 50, 29, nanoseconds: 320_402_000)))
        #expect(alpha.defaultAgent?.status == .disabled)
        #expect(alpha.preambleWarning == nil)

        #expect(try decodeFixture(StartSummary.self, "sandbox_start_zeta_prefix.json").value.branchName == "spike/zeta")
        #expect(try decodeFixture(StartSummary.self, "sandbox_start_beta_full.json").value.mode == "full")
    }

    @Test func longPromptStartDecodesWithPreamble() throws {
        let decoded = try decodeFixture(StartSummary.self, "sandbox_start_gamma_longprompt.json")
        let preamble = try #require(decoded.preamble)
        #expect(preamble.contains("Prompt truncated to 2000 characters"))
        #expect(decoded.value.workFeature == "gamma")
    }

    @Test func teardownSummaryDecodes() throws {
        let summary = try decodeFixture(TeardownSummary.self, "sandbox_teardown_alpha_force.json").value
        #expect(summary.workFeature == "alpha")
        #expect(summary.branchName == "feature/alpha")
        #expect(summary.worktreeRemoved)
        #expect(summary.branchDeleted)
        #expect(summary.adapterCleanupWarnings.isEmpty)
        #expect(summary.moduleReports == [ModuleReport(name: "specs", teardownOk: true)])
        #expect(summary.runtimeTeardown == RuntimeTeardownReport(provider: "container", verified: true, residueFree: true))
        #expect(summary.warnings == ["Tunnel descriptor missing; skipping provider teardown"])
        // 0.14 additions are absent on 0.13.4.
        #expect(summary.branchAction == nil)
        #expect(summary.discardedChanges.isEmpty)
        #expect(summary.registryUpdated == nil)
    }

    @Test func teardownSummaryAdditiveFields() throws {
        let summary = try decodeJSON(TeardownSummary.self, """
        {"work_feature":"eta","branch_name":"feature/eta","worktree_removed":true,"branch_deleted":false,
         "adapter_cleanup_warnings":[],"module_reports":[],"warnings":[],
         "runtime_teardown":{"provider":"sbx","runtime_id":"branchbox-eta","verified":false,"residue_free":false,
                             "residue":[{"kind":"volume","identifiers":["eta-data"]}]},
         "branch_action":"delete","branch_delete_error":"error: cannot delete branch 'feature/eta' used by worktree",
         "discarded_changes":[{"path":"notes.txt","kind":"untracked","area":"other"}],
         "preserved":[{"path":"docs/features/in-progress/eta.md","destination":"docs/features/backlog/eta.md"}],
         "registry_updated":true}
        """)
        #expect(summary.branchAction == "delete")
        #expect(summary.branchDeleteError?.hasPrefix("error: cannot delete branch") == true)
        #expect(summary.discardedChanges == [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")])
        #expect(summary.preserved == [PreservedFile(path: "docs/features/in-progress/eta.md",
                                                    destination: "docs/features/backlog/eta.md")])
        #expect(summary.registryUpdated == true)
        #expect(summary.runtimeTeardown?.runtimeID == "branchbox-eta")
        #expect(summary.runtimeTeardown?.residue == [ResidueItem(kind: "volume", identifiers: ["eta-data"])])
    }

    @Test func execResultsDecode() throws {
        #expect(try decodeFixture(ExecResult.self, "sandbox_exec_alpha.json").value
                == ExecResult(exitCode: 0, stdout: "hi\n", stderr: ""))
        // The CLI exits 1 here, but the payload is data: the inner command's exit code is 3.
        #expect(try decodeFixture(ExecResult.self, "sandbox_exec_alpha_fail.json").value
                == ExecResult(exitCode: 3, stdout: "out\n", stderr: "err\n"))
        #expect(try decodeFixture(ExecResult.self, "synthetic_devcontainer_exec.json").value
                == ExecResult(exitCode: 3, stdout: "out\n", stderr: "err\n", outcome: "error"))
        #expect(throws: DecodingError.self) { try decodeJSON(ExecResult.self, #"{"stdout":"x"}"#) }
    }

    // MARK: devcontainer (camelCase)

    @Test func devcontainerResultsDecode() throws {
        let up = try decodeFixture(DevcontainerResult.self, "synthetic_devcontainer_up.json").value
        #expect(up == DevcontainerResult(outcome: "created", containerID: "3f2a9c1d7e4b", remoteUser: "vscode",
                                         remoteWorkspaceFolder: "/workspaces/alpha", composeProjectName: "sandbox-alpha"))
        #expect(!up.isError)

        let image = try decodeFixture(DevcontainerResult.self, "synthetic_devcontainer_up_image.json").value
        #expect(image.outcome == "existing")
        #expect(image.remoteUser == nil)
        #expect(image.composeProjectName == nil)

        let down = try decodeFixture(DevcontainerResult.self, "synthetic_devcontainer_down.json").value
        #expect(down.removedContainers == ["3f2a9c1d7e4b"])
        #expect(try decodeFixture(DevcontainerResult.self, "synthetic_devcontainer_down_compose.json").value
                == DevcontainerResult(outcome: "stopped"))
        #expect(try decodeFixture(DevcontainerResult.self, "synthetic_devcontainer_build.json").value.imageName
                == "vsc-alpha-5d41402abc4b")

        let service = try decodeFixture(DevcontainerServiceInfo.self, "synthetic_devcontainer_detect.json").value
        #expect(service == DevcontainerServiceInfo(serviceName: "app", port: 3000, serviceURL: "http://app:3000",
                                                   containerUser: "vscode"))
        #expect(up.isRecognized)
        #expect(service.isRecognized)
        let imageConfig = try decodeJSON(DevcontainerServiceInfo.self,
            #"{"service_name":null,"port":0,"service_url":"","container_user":"vscode","container_type":"image","configured_user":"root","workspace_folder":"/workspace"}"#)
        #expect(imageConfig.isRecognized && imageConfig.hasEffectiveConfiguration)
        #expect(imageConfig.effectiveUser == "root" && imageConfig.workspaceFolder == "/workspace")
        let defaultUser = try decodeJSON(DevcontainerServiceInfo.self,
            #"{"container_type":"dockerfile","container_user":"vscode","configured_user":null,"workspace_folder":"/workspace"}"#)
        #expect(defaultUser.isRecognized && defaultUser.effectiveUser == nil)
        #expect(service.effectiveUser == "vscode", "Legacy detection retains its estimate")
    }

    @Test func unrecognizedDevcontainerPayloadsAreFlagged() throws {
        // Every field is lenient, so a legacy {"error": ...} or an error envelope decodes; isRecognized says so.
        let legacyError = #"{"error":"No .devcontainer directory found"}"#
        let envelope = #"{"schema_version":1,"error":{"code":"unsupported","message":"x","causes":[],"details":null}}"#
        for json in [legacyError, envelope] {
            #expect(!(try decodeJSON(DevcontainerResult.self, json)).isRecognized, "\(json)")
            #expect(!(try decodeJSON(DevcontainerServiceInfo.self, json)).isRecognized, "\(json)")
        }
    }

    @Test func dockerUnavailableOutcomeDecodes() throws {
        let result = try decodeFixture(DevcontainerResult.self, "synthetic_devcontainer_up_docker_unavailable.json").value
        #expect(result.outcome == "error")
        #expect(result.isError)
        #expect(result.message == "Docker is not available")
        #expect(result.containerID == nil)
        // `configure`/`detect` report failure as {"error": "..."}.
        #expect(try decodeJSON(DevcontainerResult.self, #"{"error":"No .devcontainer directory found"}"#).message
                == "No .devcontainer directory found")
    }

    // MARK: 0.14 contract payloads

    @Test func teardownPlanDecodesTheSpecExample() throws {
        let plan = try decodeJSON(TeardownPlanDocument.self, """
        {"schema_version":1,"work_feature":"eta","registered":true,"status":"active",
         "worktree":{"path":"/r/eta","exists":true,"locked":false,"lock_reason":null},
         "changes":{"status_available":true,"truncated":false,
           "user":[{"path":"README.md","kind":"modified","area":"other"},{"path":"notes.txt","kind":"untracked","area":"other"}],
           "generated":[{"path":".devcontainer/.branchbox.env","rule":"reserved_name"},{"path":".vscode/settings.json","rule":"vscode_managed_keys"}],
           "preserved":[{"path":"docs/features/in-progress/eta.md","destination":"docs/features/backlog/eta.md"}]},
         "branch":{"name":"feature/eta","source":"registry","exists":true,"upstream":null,"reference":"HEAD","reference_name":"main",
           "merged":false,"merged_into_head":false,"ahead":3,"action":"delete"},
         "defaults":{"delete_branch_by_default":true,"force_delete_unmerged_by_default":false},
         "runtime":{"provider":"container","runtime_id":null},"tunnel":{"status":"disabled"},
         "blockers":[{"kind":"uncommitted_changes","count":2,"message":"…","override":"--discard-changes"},
                     {"kind":"unmerged_branch","branch":"feature/eta","ahead":3,"message":"…","override":"--keep-branch | --force-delete-branch"}],
         "warnings":[]}
        """)
        #expect(plan.source == .cli)
        #expect(plan.workFeature == "eta")
        #expect(plan.registered)
        #expect(plan.status == .active)
        #expect(plan.worktree == .init(path: "/r/eta", exists: true))
        #expect(plan.changes.statusAvailable)
        #expect(plan.changes.user.map(\.path) == ["README.md", "notes.txt"])
        #expect(plan.changes.generated.map(\.rule) == ["reserved_name", "vscode_managed_keys"])
        #expect(plan.changes.preserved.first?.destination == "docs/features/backlog/eta.md")
        #expect(plan.branch == .init(name: "feature/eta", source: "registry", exists: true, reference: "HEAD",
                                     referenceName: "main", merged: false, mergedIntoHead: false, ahead: 3, action: "delete"))
        #expect(plan.defaults == .init(deleteBranchByDefault: true, forceDeleteUnmergedByDefault: false))
        #expect(plan.runtime == .init(provider: "container"))
        #expect(plan.tunnel?.status == .disabled)
        #expect(plan.blockers.map(\.kind) == ["uncommitted_changes", "unmerged_branch"])
        #expect(plan.blockers[0].count == 2)
        #expect(plan.blockers[0].override == "--discard-changes")
        #expect(plan.blockers[1].branch == "feature/eta")
        #expect(plan.blockers[1].ahead == 3)
        #expect(plan.warnings.isEmpty)
        #expect(plan.droppedBlockers == 0)
        #expect(plan.changes.droppedEntries == 0)
    }

    @Test func teardownPlanWithUnreadableEntriesIsNotClean() throws {
        let plan = try decodeJSON(TeardownPlanDocument.self, """
        {"work_feature":"eta","worktree":{"path":"/r/eta","exists":true},
         "changes":{"status_available":true,"truncated":false,"user":[{"kind":"modified"}],
                    "generated":[{"rule":"reserved_name"}],"preserved":[]},
         "blockers":[{"message":"a blocker without a kind"},{"kind":"unmerged_branch","message":"…"}]}
        """)
        // A user change without a path is dropped, but it is still a change: the set is not known to be clean.
        #expect(plan.changes.user.isEmpty)
        #expect(plan.changes.droppedEntries == 2)
        #expect(!plan.changes.statusAvailable)
        #expect(plan.blockers.map(\.kind) == ["unmerged_branch"])
        #expect(plan.droppedBlockers == 1)

        // Dropped generated or preserved entries are counted but do not hide user changes.
        let generatedOnly = try decodeJSON(TeardownPlanDocument.Changes.self, """
        {"status_available":true,"user":[],"generated":[{"rule":"reserved_name"}],"preserved":[{}]}
        """)
        #expect(generatedOnly.statusAvailable)
        #expect(generatedOnly.droppedEntries == 2)
    }

    @Test func teardownPlanWithoutChangesIsNotClean() throws {
        let plan = try decodeJSON(TeardownPlanDocument.self,
                                  #"{"work_feature":"eta","worktree":{"path":"/r/eta"},"changes":"broken"}"#)
        #expect(plan.changes == .unavailable)
        #expect(!plan.changes.statusAvailable)
        #expect(plan.worktree.exists == false)
        #expect(throws: DecodingError.self) { try decodeJSON(TeardownPlanDocument.self, #"{"work_feature":"eta"}"#) }
    }

    @Test func tunnelChangesDecode() throws {
        let opened = try decodeJSON(TunnelChange.self, """
        {"work_feature":"eta","state":{"provider":"cloudflared","hostname":"eta.example.dev","status":"active"},"warnings":["w"]}
        """)
        #expect(opened.state?.status == .active)
        #expect(opened.previousState == nil)
        #expect(opened.warnings == ["w"])

        let removed = try decodeJSON(TunnelChange.self, """
        {"work_feature":"eta","previous_state":{"status":"active"},"updated_state":{"status":"disabled"},"warnings":[]}
        """)
        #expect(removed.previousState?.status == .active)
        #expect(removed.state?.status == .disabled)
    }

    @Test func tunnelCredentialsResultDecodes() throws {
        let result = try decodeJSON(TunnelCredentialsResult.self, """
        {"schema_version":1,"credentials_path":"/r/.branchbox/secure/cloudflared.env","account_id":"abc","token_present":true}
        """)
        #expect(result == TunnelCredentialsResult(credentialsPath: "/r/.branchbox/secure/cloudflared.env",
                                                  accountID: "abc", tokenPresent: true))
    }

    @Test func projectConfigAppliesCoreDefaults() throws {
        #expect(try decodeJSON(ProjectConfig.self, "{}") == .defaults)
        #expect(ProjectConfig.defaults.branchPrefix == "feature")
        #expect(ProjectConfig.defaults.deleteBranchByDefault)
        #expect(ProjectConfig.defaults.tunnelDefaultProvider == "cloudflared")

        let config = try decodeJSON(ProjectConfig.self, """
        {"version":"1","runtime":{"provider":"sbx","sbx":{"run_services":["db"]}},
         "feature":{"branch_prefix":"spike","teardown":{"delete_branch_by_default":false}},
         "tunnel":{"enabled":false,"default_provider":null,
                   "providers":{"cloudflared":{"account_id":"acc","manual_instructions":false,"api_token_path":"/s/cf.env"}}},
         "editor":{"default_agent":"claude","auto_launch_agent_terminal":true,"hide_secondary_sidebar":true}}
        """)
        #expect(config.runtimeProvider == .sbx)
        #expect(config.sbxRunServices == ["db"])
        #expect(config.branchPrefix == "spike")
        #expect(!config.deleteBranchByDefault)
        #expect(!config.forceDeleteUnmergedByDefault)
        #expect(config.promptForceDeleteUnmerged)
        #expect(!config.tunnelEnabled)
        #expect(config.tunnelDefaultProvider == "cloudflared")  // null reads as core's effective default
        #expect(config.cloudflared == CloudflaredConfig(accountID: "acc", manualInstructions: false, apiTokenPath: "/s/cf.env"))
        #expect(config.editorDefaultAgent == "claude")
        #expect(config.autoLaunchAgentTerminal)
    }

    @Test func projectConfigTunnelProviderFollowsCore() throws {
        // Core's `ensure_defaults()` turns a null or absent provider into "cloudflared"; an explicit value is kept.
        for json in [#"{"tunnel":{"default_provider":null}}"#, #"{"tunnel":{}}"#, #"{"tunnel":{"default_provider":7}}"#] {
            #expect(try decodeJSON(ProjectConfig.self, json).tunnelDefaultProvider == "cloudflared", "\(json)")
        }
        #expect(try decodeJSON(ProjectConfig.self, #"{"tunnel":{"default_provider":"manual"}}"#)
            .tunnelDefaultProvider == "manual")
    }

    @Test func configKeysAndApplyResultDecode() throws {
        let key = try decodeJSON(ConfigKeyDescriptor.self, """
        {"key":"runtime.provider","type":"enum","allowed":["container","sbx","local-vm","in-guest"],
         "default":"container","value":"sbx","source":"file","description":"Runtime"}
        """)
        #expect(key == ConfigKeyDescriptor(key: "runtime.provider", type: "enum",
                                           allowed: ["container", "sbx", "local-vm", "in-guest"],
                                           defaultValue: .string("container"), value: .string("sbx"),
                                           source: "file", description: "Runtime"))

        let applied = try decodeJSON(ConfigApplyResult.self, """
        {"schema_version":1,"changed":[{"key":"runtime.provider","old":"container","new":"sbx"},
                                       {"key":"feature.branch_prefix","old":"spike","new":null}],
         "effective":{"runtime":{"provider":"sbx"}}}
        """)
        #expect(applied.changed == [.init(key: "runtime.provider", old: .string("container"), new: .string("sbx")),
                                    .init(key: "feature.branch_prefix", old: .string("spike"), new: nil)])
        #expect(applied.effective.runtimeProvider == .sbx)
        #expect(applied.effective.branchPrefix == "feature")
    }
}

// MARK: - Backend contract values

@Suite struct BackendContractTests {
    @Test func normalizePassesBackendErrorsThrough() {
        let error = BackendError.cliNotFound(searched: ["/opt/homebrew/bin/branchbox"])
        #expect(BackendError.normalize(error) == error)
    }

    @Test func normalizeMapsCancellation() {
        #expect(BackendError.normalize(CancellationError()) == .cancelled(note: nil))
        let partial = ProcessResult(termination: .signaled(2), stdout: Data(), stderrTail: [], duration: .seconds(1))
        #expect(BackendError.normalize(ProcessRunError.cancelled(partial: partial)) == .cancelled(note: nil))
    }

    @Test func normalizeWrapsAnythingElseAsCommandFailed() {
        struct Boom: Error, CustomStringConvertible { var description: String { "boom: disk full" } }
        #expect(BackendError.normalize(Boom()) == .commandFailed(Diagnostics(summary: "boom: disk full")))
    }

    @Test func normalizeKeepsProcessFailuresTyped() {
        let partial = ProcessResult(termination: .signaled(15), stdout: Data(), stderrTail: ["slow"], duration: .seconds(9))
        guard case .timedOut(_, let after, let diagnostics) = BackendError.normalize(
            ProcessRunError.timedOut(after: .seconds(9), partial: partial)) else {
            Issue.record("expected .timedOut")
            return
        }
        #expect(after == .seconds(9))
        #expect(diagnostics.logTail == ["slow"])
        #expect(BackendError.normalize(ProcessRunError.launchFailed(executable: "/x", reason: "denied"))
                == .launchFailed(executable: "/x", reason: "denied"))
        #expect(BackendError.normalize(ProcessRunError.workingDirectoryMissing("/gone"))
                == .projectInvalid(.workingDirectoryMissing("/gone")))
    }

    @Test func recoveryIDsAreStableAndDistinct() {
        let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo"))
        let feature = FeatureRef(project: project, name: "eta")
        let retry = { (label: String) in
            RecoveryAction.retry(.tunnelOpen(feature), label: label, destructive: false, confirmation: nil)
        }
        #expect(retry("Retry").id == retry("Retry").id)
        #expect(retry("Retry").id == RecoveryAction.retry(.tunnelOpen(feature), label: "Retry", destructive: true,
                                                          confirmation: "Lose it").id)
        let operation = UUID()
        let all: [RecoveryAction] = [
            retry("Retry"), retry("Discard 2 changes"),
            .runInTerminal(command: ["sbx", "login"], workingDirectory: nil, label: "Sign in"),
            .revealInFinder(path: "/tmp/bbx/eta"), .copyCommand("branchbox doctor", label: "Copy"),
            .openDoctor, .locateCLI, .refresh(project), .showLog(operation: operation),
        ]
        #expect(Set(all.map(\.id)).count == all.count)
        #expect(RecoveryAction.showLog(operation: operation).id == "showLog:\(operation.uuidString)")
        #expect(RecoveryAction.openDoctor.id == "openDoctor")
    }

    @Test func secretStringNeverPrintsItsValue() {
        let secret = SecretString("cf-token-123")
        #expect(secret.value == "cf-token-123")
        #expect("\(secret)" == "••••")
        #expect(String(reflecting: secret) == "••••")
        var dumped = ""
        dump(TunnelCredentialsRequest(accountID: "acc", apiToken: secret), to: &dumped)
        #expect(!dumped.contains("cf-token-123"))
    }

    @Test func jsonValueRoundTrips() throws {
        let value = try JSONDecoder().decode(JSONValue.self, from: Data(#"{"a":[1,true,null,"x",{"b":2.5}],"c":false}"#.utf8))
        #expect(value == .object(["a": .array([.number(1), .bool(true), .null, .string("x"), .object(["b": .number(2.5)])]),
                                  "c": .bool(false)]))
        #expect(try JSONDecoder().decode(JSONValue.self, from: JSONEncoder().encode(value)) == value)
    }

    @Test func requestInitializersUseSafeDefaults() {
        let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo"))
        let start = StartFeatureRequest(project: project, name: "oauth", runtime: .container)
        #expect(start.mode == .full)
        #expect(start.reuse == .none)
        #expect(start.base == nil && start.branchPrefix == nil && start.prompt == nil)
        #expect(!start.useDefaultPrompt && !start.keepRuntimeOnFailure && !start.verbose)
        #expect(start.skipModules.isEmpty)

        let teardown = TeardownRequest(feature: FeatureRef(project: project, name: "oauth"), recordedBranch: "feature/oauth",
                                       branch: .keep)
        #expect(teardown.discard == nil)
        #expect(!teardown.forceRemoval)
        #expect(!teardown.completeSpec)

        let exec = ExecRequest(feature: teardown.feature, command: ["ls"])
        #expect(exec.target == .featureRuntime)
        #expect(exec.timeout == nil)

        let spec = ProcessSpec(executable: URL(fileURLWithPath: "/opt/homebrew/bin/branchbox"), arguments: [],
                               environment: [:], workingDirectory: nil)
        #expect(spec.standardInput == nil && spec.timeout == nil)
        #expect(spec.interruptGrace == .seconds(5) && spec.terminateGrace == .seconds(3) && spec.drainGrace == .seconds(2))
        #expect(spec.stdoutLimit == 64 * 1024 * 1024)
        #expect(spec.stderrTailLines == 400)
        #expect(!spec.streamStdout)
    }

    @Test func requestsRoundTripThroughCodable() throws {
        let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo"))
        var start = StartFeatureRequest(project: project, name: "oauth", runtime: .sbx)
        start.reuse = .existingWorktree(.preserve)
        start.skipModules = ["tunnel"]
        #expect(try JSONDecoder().decode(StartFeatureRequest.self, from: JSONEncoder().encode(start)) == start)
    }

    @Test func projectRefIsStandardizedButNotResolved() throws {
        let ref = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo/../demo/./"))
        #expect(ref.path == "/tmp/bbx/demo")
        #expect(ref.displayName == "demo")
        #expect(ref == ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo")))
        // /tmp is a symlink to /private/tmp on macOS; the ref keeps the path the user chose.
        #expect(ref != ProjectRef(root: URL(fileURLWithPath: "/private/tmp/bbx/demo")))
        #expect(try JSONDecoder().decode(ProjectRef.self, from: JSONEncoder().encode(ref)) == ref)
    }

    @Test func projectRefIgnoresTheDirectoryHint() {
        let folder = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo", isDirectory: true))
        let file = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/demo", isDirectory: false))
        #expect(folder.root.absoluteString.hasSuffix("/"))
        #expect(folder == file)
        #expect(Set([folder, file]).count == 1)
        #expect(FeatureRef(project: folder, name: "eta") == FeatureRef(project: file, name: "eta"))
    }

    @Test func identityReportsCapabilities() {
        let resolution = CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .wellKnownPath)
        let legacy = BackendIdentity(kind: .cli(resolution), version: SemVer(0, 13, 4), contractVersion: nil, capabilities: [])
        #expect(legacy.isLegacy)
        #expect(!legacy.supports(.teardownPlan))
        let contract = BackendIdentity(kind: .preview, version: SemVer(0, 14, 0), contractVersion: 1,
                                       capabilities: [.teardownPlan, .config])
        #expect(!contract.isLegacy)
        #expect(contract.supports(.config))
        #expect(!contract.supports(.doctor))
    }

    @Test func diagnosticsAndLogLinesStayBounded() {
        let diagnostics = Diagnostics(summary: "x", logTail: (1...80).map(String.init))
        #expect(diagnostics.logTail.count == 50)
        #expect(diagnostics.logTail.first == "31")

        let long = String(repeating: "é", count: 10_000)        // 2 bytes each: 20 000 bytes
        let line = LogLine(timestamp: nil, level: .info, source: .stderr, target: nil, message: long)
        #expect(line.message.utf8.count == LogLine.maxMessageBytes)
        #expect(line.message.allSatisfy { $0 == "é" })
        #expect(LogLine(timestamp: nil, level: .warn, source: .app, target: nil, message: "short").message == "short")
    }
}
