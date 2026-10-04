import BranchBoxKit
import BranchBoxTestSupport
import Foundation

/// Builders for the Planning tests: plans, records and errors with only the facts a test is about.
enum Sample {
    static let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/main"))
    static let feature = FeatureRef(project: project, name: "eta")
    static let operation = UUID(uuidString: "6F3C1D2E-0000-4000-8000-000000000001")!

    typealias Plan = TeardownPlanDocument

    static func changed(_ path: String, _ kind: String = "modified") -> ChangedFile {
        ChangedFile(path: path, kind: kind, area: "other")
    }

    static func branch(_ name: String = "feature/eta", exists: Bool = true, merged: Bool = true, ahead: Int = 0) -> Plan.Branch {
        Plan.Branch(name: name, source: "registry", exists: exists, reference: "HEAD", referenceName: "main",
                    merged: merged, mergedIntoHead: merged, ahead: ahead, action: "keep")
    }

    static func plan(name: String = "eta", exists: Bool = true, locked: Bool = false, lockReason: String? = nil,
                     statusAvailable: Bool = true, truncated: Bool = false, user: [ChangedFile] = [],
                     branch: Plan.Branch? = branch(), defaults: Plan.Defaults? = nil,
                     blockers: [Plan.Blocker] = []) -> Plan {
        Plan(workFeature: name, registered: true, status: .active,
             worktree: Plan.Worktree(path: "/tmp/bbx/\(name)", exists: exists, locked: locked, lockReason: lockReason),
             changes: Plan.Changes(statusAvailable: statusAvailable, truncated: truncated, user: user),
             branch: branch, defaults: defaults, blockers: blockers)
    }

    /// A §5.5 plan as the CLI prints it.
    static func decodePlan(_ json: String) throws -> Plan {
        try CLIJSON.decode(Plan.self, from: Data(json.utf8)).value
    }

    /// The plan printed in DESIGN §5.5: two user changes and an unmerged branch three commits ahead.
    static let designPlanJSON = #"""
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
    """#

    static func record(_ name: String = "eta", status: FeatureStatus = .active, provider: RuntimeProvider = .container,
                       runtimeID: String? = nil, setup: SetupState? = nil, modules: [ModuleOutcome] = [],
                       outdated: Bool = false, startMode: String? = "full", prompt: String? = nil,
                       worktreePath: String? = nil, branch: String? = nil, workspaceFolder: String? = nil,
                       containerUser: String? = nil) -> FeatureRecord {
        FeatureRecord(workFeature: name, branchName: branch ?? "feature/\(name)",
                      worktreePath: worktreePath ?? "/tmp/bbx/\(name)", status: status, devcontainerOutdated: outdated,
                      startMode: startMode, promptSeed: prompt, moduleOutcomes: modules,
                      runtime: RuntimeInfo(provider: provider, runtimeID: runtimeID, workspaceFolder: workspaceFolder,
                                           containerUser: containerUser),
                      setup: setup.map { SetupInfo(state: $0, pid: 4242) })
    }

    static func refused(_ cause: RefusalCause, plan: Plan? = nil) -> BackendError {
        .refused(Refusal(cause: cause, message: "Refusing to tear down 'eta'", diagnostics: Diagnostics(summary: "refused"),
                         plan: plan))
    }

    static func teardown(_ branch: BranchPolicy = .deleteIfMerged) -> TeardownRequest {
        TeardownRequest(feature: feature, recordedBranch: "feature/eta", branch: branch)
    }

    static let identity = BackendIdentity(kind: .preview, version: SemVer(0, 14, 0), contractVersion: 1, capabilities: [])

    struct MissingRecord: Error { let name: String }

    /// A record from the scrubbed 0.13.4 captures.
    static func fixtureRecord(_ name: String, in fixture: String) throws -> FeatureRecord {
        let records = try CLIJSON.decode([FeatureRecord].self, from: Fixtures.data("cli-0.13.4/\(fixture)")).value
        guard let record = records.first(where: { $0.workFeature == name }) else { throw MissingRecord(name: name) }
        return record
    }
}
