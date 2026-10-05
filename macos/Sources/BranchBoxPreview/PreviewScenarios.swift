import BranchBoxKit
import Foundation

/// A canned world for `PreviewBackend`: what the backend says it is, and what every project lists until a
/// start or teardown changes it.
public struct PreviewScenario: Sendable, Hashable {
    public var name: String
    /// The backend's identity, or the error that explains why there is no backend.
    public var identity: Result<BackendIdentity, BackendError>
    /// What `listFeatures` returns for each project; removed records are filtered out unless asked for.
    public var listing: FeatureListing
    /// Features whose worktree has uncommitted user changes: a teardown refuses (with the plan) unless its
    /// discard consent names exactly these files.
    public var dirtyFeatures: [String: [ChangedFile]]
    /// Features whose branch has this many commits not merged into main.
    public var unmergedBranches: [String: Int]

    public init(name: String, identity: Result<BackendIdentity, BackendError>, listing: FeatureListing,
                dirtyFeatures: [String: [ChangedFile]] = [:], unmergedBranches: [String: Int] = [:]) {
        self.name = name
        self.identity = identity
        self.listing = listing
        self.dirtyFeatures = dirtyFeatures
        self.unmergedBranches = unmergedBranches
    }

    /// The same world on another backend identity, e.g. to compare legacy and contract behaviour.
    public func with(identity: Result<BackendIdentity, BackendError>, name: String? = nil) -> PreviewScenario {
        var copy = self
        copy.identity = identity
        copy.name = name ?? self.name
        return copy
    }
}

extension PreviewScenario {
    /// CLI 0.13.4: no `version --json`, so no contract version and an empty capability set.
    public static let legacy0134 = PreviewScenario(
        name: "legacy0134",
        identity: .success(BackendIdentity(kind: .preview, version: SemVer(0, 13, 4), contractVersion: nil, capabilities: [])),
        listing: PreviewSamples.listing)

    /// A contract CLI with every capability.
    public static let contract = PreviewScenario(
        name: "contract",
        identity: .success(BackendIdentity(kind: .preview, version: SemVer(0, 14, 0), contractVersion: 1,
                                           capabilities: PreviewSamples.allCapabilities)),
        listing: PreviewSamples.listing)

    /// A contract CLI whose projects have no features and no strays yet.
    public static let emptyProject = PreviewScenario(
        name: "emptyProject", identity: contract.identity, listing: FeatureListing(features: []))

    /// No usable CLI: bootstrap is unavailable and every throwing call throws `.cliNotFound`.
    public static let cliMissing = PreviewScenario(
        name: "cliMissing", identity: .failure(.cliNotFound(searched: PreviewSamples.searchedPaths)),
        listing: FeatureListing(features: []))

    /// A contract CLI with a start that was interrupted (`setup.state == interrupted`) next to the sample features.
    public static let interruptedSetup = PreviewScenario(
        name: "interruptedSetup", identity: contract.identity,
        listing: FeatureListing(features: PreviewSamples.features + [PreviewSamples.interruptedFeature],
                                strays: [PreviewSamples.stray]))

    /// A contract CLI whose project has several unregistered worktrees, one locked and one prunable.
    public static let strays = PreviewScenario(
        name: "strays", identity: contract.identity,
        listing: FeatureListing(features: PreviewSamples.features, strays: PreviewSamples.strays))

    /// A contract CLI where tearing down `prine` meets uncommitted changes (refused with the plan until the user
    /// consents to discarding exactly those files) and `remotion`'s branch is 3 commits ahead of main.
    public static let dirtyWorktree = PreviewScenario(
        name: "dirtyWorktree", identity: contract.identity, listing: PreviewSamples.listing,
        dirtyFeatures: ["prine": PreviewSamples.dirtyFiles], unmergedBranches: ["remotion": 3])

    /// `dirtyWorktree` on CLI 0.13.4: the refusal comes from the app's preflight, and an unmerged branch survives
    /// "delete if merged" as a failed app-side `git branch -d`.
    public static let legacyDirtyWorktree = dirtyWorktree.with(identity: legacy0134.identity, name: "legacyDirtyWorktree")

    /// A contract CLI with a lived-in project for screenshots and visual review: features in every state and
    /// runtime, long names, ports and tunnels, an interrupted start, failed modules and unregistered worktrees.
    /// Additive (SW-4).
    public static let showcase = PreviewScenario(
        name: "showcase", identity: contract.identity,
        listing: FeatureListing(features: PreviewSamples.showcaseFeatures, strays: Array(PreviewSamples.strays.prefix(2))),
        dirtyFeatures: ["prine": PreviewSamples.dirtyFiles], unmergedBranches: ["checkout-redesign": 4])

    public static let all: [PreviewScenario] = [
        .legacy0134, .contract, .emptyProject, .cliMissing, .interruptedSetup, .strays, .dirtyWorktree, .legacyDirtyWorktree,
        .showcase,
    ]

    /// The built-in scenario called `name`, e.g. from `BRANCHBOX_PREVIEW_SCENARIO`.
    public static func named(_ name: String) -> PreviewScenario? {
        all.first { $0.name == name }
    }
}

/// Sample data behind the scenarios.
///
/// The feature records decode from verbatim copies of two scrubbed CLI 0.13.4 captures in
/// `Tests/BranchBoxTestSupport/Fixtures/cli-0.13.4/`; this target has no resources, so the JSON is embedded.
/// A Stores test checks that the copies still match the fixtures.
public enum PreviewSamples {
    /// The main worktree the sample features sit next to.
    public static let project = ProjectRef(root: URL(fileURLWithPath: "/Users/dev/projects/branchbox-suite/branchbox/main",
                                                     isDirectory: true))

    /// Every capability a contract CLI prints in `version --json` (§5.3).
    public static let allCapabilities: Set<Capability> = [
        .jsonErrorEnvelope, .registryLock, .writeAheadStart, .teardownPlan, .teardownDiscardChanges,
        .teardownUnmergedPreflight, .pruneJSON, .detectJSON, .devcontainerSyncJSON, .config, .tunnelCredentials,
        .doctor, .initJSON, .hostContainerTeardownVerified,
    ]

    /// Where the CLI locator looks when nothing overrides it.
    public static let searchedPaths = [
        "/opt/homebrew/bin/branchbox", "/usr/local/bin/branchbox", "/Users/dev/.cargo/bin/branchbox",
        "/Users/dev/.local/bin/branchbox",
    ]

    /// `feature list --all --json` (8 records: 2 active, 6 removed), then the synthetic degraded, failed_retained
    /// and orphaned records.
    public static let features: [FeatureRecord] = decode(featureListAllJSON) + decode(newStatusesJSON)

    /// A worktree in the project's container folder that the registry does not know.
    public static let stray = StrayWorktree(path: "/Users/dev/projects/branchbox-suite/branchbox/spike-search",
                                            branch: "spike/search", head: "4f2c9a1")

    public static let listing = FeatureListing(features: features, strays: [stray])

    /// Unregistered worktrees in the BranchBox layout: the sample stray, a locked one and a prunable one.
    public static let strays: [StrayWorktree] = [
        stray,
        StrayWorktree(path: "/Users/dev/projects/branchbox-suite/branchbox/hotfix-login", branch: "feature/hotfix-login",
                      head: "9b1d7e0", locked: true),
        StrayWorktree(path: "/Users/dev/projects/branchbox-suite/branchbox/old-spike", branch: "feature/old-spike",
                      head: "51c0a2f", prunable: true),
    ]

    /// A feature whose `feature start` was killed midway: still `active`, with `setup.state == interrupted`.
    public static let interruptedFeature = FeatureRecord(
        workFeature: "oauth", branchName: "feature/oauth",
        worktreePath: "/Users/dev/projects/branchbox-suite/branchbox/oauth", featureURL: "dev-oauth.localhost",
        composeProjectName: "branchbox-oauth", status: .active,
        createdAt: RFC3339.parse("2026-10-01T22:50:29.222458Z"), updatedAt: RFC3339.parse("2026-10-01T22:50:29.222458Z"),
        color: "#9b59b6", startMode: "full",
        moduleOutcomes: [ModuleOutcome(module: "devcontainer", status: .success, durationMs: 4)],
        setup: SetupInfo(state: .interrupted, pid: 48211, startedAt: RFC3339.parse("2026-10-01T22:50:29.222458Z")))

    /// The `showcase` scenario's features (additive, SW-4): the two active sample records plus synthetic ones that
    /// cover each status, runtime and attention reason, with realistic long names, ports and tunnels.
    public static let showcaseFeatures: [FeatureRecord] = {
        let root = "/Users/dev/projects/branchbox-suite/branchbox"
        let now = Date.now
        func ago(_ minutes: Double) -> Date { now.addingTimeInterval(-minutes * 60) }
        let okModules = [
            ModuleOutcome(module: "devcontainer", status: .success, durationMs: 840),
            ModuleOutcome(module: "compose", status: .success, durationMs: 12_400),
            ModuleOutcome(module: "specs", status: .success, durationMs: 0),
            ModuleOutcome(module: "tunnel", status: .success, durationMs: 3_100),
        ]
        let active = features.filter { $0.status == .active }
        let synthetic = [
            FeatureRecord(
                workFeature: "checkout-redesign", branchName: "feature/checkout-redesign", worktreePath: "\(root)/checkout-redesign",
                baseBranch: "main", featureURL: "dev-checkout-redesign.localhost", composeProjectName: "branchbox-checkout-redesign",
                status: .active, createdAt: ago(60 * 26), updatedAt: ago(4),
                tunnel: TunnelState(provider: "cloudflared", hostname: "checkout-redesign.branchbox.dev", serviceURL: "http://app:3000",
                                    status: .active, lastUpdated: ago(5)),
                color: "#e67e22", lastCommit: "9f1c2ab7d", prNumber: 418, startMode: "full",
                promptSeed: "Rebuild the checkout flow with the new payment sheet", moduleOutcomes: okModules,
                adapter: AdapterInfo(name: "Rails", serviceURL: "http://app:3000"),
                runtime: RuntimeInfo(provider: .container, publishedPorts: [PublishedPort(host: 49_152, runtime: 3000),
                                                                            PublishedPort(host: 49_153, runtime: 5432)])),
            FeatureRecord(
                workFeature: "oauth-device-flow-for-cli-login-with-refresh-tokens",
                branchName: "feature/oauth-device-flow-for-cli-login-with-refresh-tokens",
                worktreePath: "\(root)/oauth-device-flow-for-cli-login-with-refresh-tokens", baseBranch: "release/0.14",
                featureURL: "dev-oauth-device-flow.localhost", status: .active, createdAt: ago(60 * 3), updatedAt: ago(20),
                color: "#9b59b6", startMode: "minimal",
                moduleOutcomes: [ModuleOutcome(module: "devcontainer", status: .success, durationMs: 610),
                                 ModuleOutcome(module: "compose", status: .failed, durationMs: 1_830,
                                               notes: ["port 5432 is already in use"])],
                runtime: RuntimeInfo(provider: .container, publishedPorts: [PublishedPort(host: 49_160, runtime: 8080)])),
            FeatureRecord(
                workFeature: "search-reindex", branchName: "feature/search-reindex", worktreePath: "\(root)/search-reindex",
                status: .active, createdAt: ago(12), updatedAt: ago(11), color: "#1abc9c", startMode: "full",
                moduleOutcomes: [ModuleOutcome(module: "devcontainer", status: .success, durationMs: 4)],
                setup: SetupInfo(state: .interrupted, pid: 48_211, startedAt: ago(12))),
            FeatureRecord(
                workFeature: "agent-sandbox", branchName: "feature/agent-sandbox", worktreePath: "\(root)/agent-sandbox",
                status: .degraded, createdAt: ago(60 * 50), updatedAt: ago(90), color: "#3498db", startMode: "full",
                runtime: RuntimeInfo(provider: .sbx, runtimeID: "branchbox-agent-sandbox")),
            FeatureRecord(
                workFeature: "vm-kernel-bump", branchName: "feature/vm-kernel-bump", worktreePath: "\(root)/vm-kernel-bump",
                status: .failedRetained, createdAt: ago(60 * 72), updatedAt: ago(60 * 70), color: "#e74c3c",
                startMode: "minimal", runtime: RuntimeInfo(provider: .localVM)),
        ]
        return synthetic + active
    }()

    /// The user changes `dirtyWorktree` puts in `prine`'s worktree.
    public static let dirtyFiles: [ChangedFile] = [
        ChangedFile(path: "README.md", kind: "modified", area: "other"),
        ChangedFile(path: "notes.txt", kind: "untracked", area: "other"),
    ]

    /// Part of the `config get --json` key registry (§5.10), at its defaults.
    public static let configKeys: [ConfigKeyDescriptor] = [
        ConfigKeyDescriptor(key: "runtime.provider", type: "enum", allowed: ["container", "sbx", "local-vm", "in-guest"],
                            defaultValue: .string("container"), value: .string("container"),
                            description: "Runtime new features start in"),
        ConfigKeyDescriptor(key: "feature.branch_prefix", type: "string", defaultValue: .string("feature"),
                            value: .string("feature"), description: "Prefix of new feature branches"),
        ConfigKeyDescriptor(key: "feature.teardown.delete_branch_by_default", type: "bool", defaultValue: .bool(true),
                            value: .bool(true), description: "Delete a merged branch on teardown"),
        ConfigKeyDescriptor(key: "feature.teardown.force_delete_unmerged_by_default", type: "bool", defaultValue: .bool(false),
                            value: .bool(false), description: "Force-delete an unmerged branch on teardown"),
        ConfigKeyDescriptor(key: "tunnel.enabled", type: "bool", defaultValue: .bool(true), value: .bool(true),
                            description: "Provision a tunnel for new features"),
        ConfigKeyDescriptor(key: "editor.default_agent", type: "string", description: "Coding agent to launch"),
    ]

    /// Host checks a healthy Mac reports (§5.12 ids).
    public static let hostChecks: [DoctorCheck] = [
        DoctorCheck(id: "git", title: "Git", required: true, status: .ok, path: "/usr/bin/git", version: "2.50.1"),
        DoctorCheck(id: "docker.cli", title: "Docker CLI", required: true, status: .ok, path: "/usr/local/bin/docker",
                    version: "28.3.2"),
        DoctorCheck(id: "docker.daemon", title: "Docker daemon", required: true, status: .ok),
        DoctorCheck(id: "docker.compose", title: "Docker Compose", required: true, status: .ok, version: "2.38.2"),
        DoctorCheck(id: "devcontainer.cli", title: "Dev Container CLI", required: false, status: .warn,
                    detail: "Not installed", remediation: "npm install -g @devcontainers/cli"),
        DoctorCheck(id: "runtime.sbx", title: "Docker Sandboxes", required: false, status: .skipped,
                    detail: "sbx is not installed"),
        DoctorCheck(id: "gh", title: "GitHub CLI", required: false, status: .ok, path: "/opt/homebrew/bin/gh", version: "2.76.0"),
    ]

    /// The checks `doctor --repo` adds for a healthy project.
    public static let repoChecks: [DoctorCheck] = [
        DoctorCheck(id: "repo.git", title: "Git repository", required: true, status: .ok),
        DoctorCheck(id: "repo.initialized", title: "BranchBox set up", required: true, status: .ok),
        DoctorCheck(id: "repo.config", title: "Project config", required: true, status: .ok),
        DoctorCheck(id: "repo.registry", title: "Feature registry", required: true, status: .ok),
    ]

    private static func decode(_ json: String) -> [FeatureRecord] {
        let records = (try? CLIJSON.decode([Lossy<FeatureRecord>].self, from: Data(json.utf8)).value) ?? []
        return records.compactMap(\.value)
    }

    // Verbatim copy of cli-0.13.4/main_feature_list_all.json.
    private static let featureListAllJSON = #"""
[
  {
    "work_feature": "prine",
    "branch_name": "feature/prine",
    "worktree_path": "/Users/dev/projects/branchbox-suite/branchbox/prine",
    "base_branch": null,
    "feature_url": "dev-prine.localhost",
    "compose_project_name": "branchbox-prine",
    "env_path": "/Users/dev/projects/branchbox-suite/branchbox/prine/.env",
    "status": "active",
    "created_at": "2026-03-17T03:37:57.979509Z",
    "updated_at": "2026-03-17T03:37:57.979509Z",
    "removed_at": null,
    "tunnel": {
      "provider": "cloudflared",
      "status": "disabled",
      "notes": "Tunnel provisioning disabled in project configuration",
      "last_updated": "2026-03-17T03:37:57.975531Z"
    },
    "color": "#f39c12",
    "last_commit": "d3f308c112340957ce1fc42ec5383ce1b7294074",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 9,
        "forced": false,
        "recorded_at": "2026-03-17T03:37:57.979509Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 190,
        "forced": false,
        "recorded_at": "2026-03-17T03:37:57.979509Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-03-17T03:37:57.979509Z"
      },
      {
        "module": "tunnel",
        "status": "skipped",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-03-17T03:37:57.979509Z"
      }
    ],
    "last_summary_rendered_at": "2026-03-17T03:37:57.979509Z",
    "adapter": {
      "name": "Generic",
      "service_url": "http://dev:3000",
      "warnings": []
    },
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "remotion",
    "branch_name": "feature/remotion",
    "worktree_path": "/Users/dev/projects/branchbox-suite/branchbox/remotion",
    "base_branch": null,
    "feature_url": "dev-remotion.localhost",
    "compose_project_name": "branchbox-remotion",
    "env_path": "/Users/dev/projects/branchbox-suite/branchbox/remotion/.env",
    "status": "active",
    "created_at": "2026-02-17T15:49:46.470702Z",
    "updated_at": "2026-02-17T15:49:46.470702Z",
    "removed_at": null,
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel provisioning disabled in project configuration",
      "last_updated": "2026-02-17T15:49:46.466211Z"
    },
    "color": "#c0392b",
    "last_commit": "dfcb8c9fedf3b1969e407c578655e8d3122c04f5",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 1,
        "forced": false,
        "recorded_at": "2026-02-17T15:49:46.470702Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 118,
        "forced": false,
        "recorded_at": "2026-02-17T15:49:46.470702Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-02-17T15:49:46.470702Z"
      },
      {
        "module": "tunnel",
        "status": "skipped",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-02-17T15:49:46.470702Z"
      }
    ],
    "last_summary_rendered_at": "2026-02-17T15:49:46.470702Z",
    "adapter": {
      "name": "Generic",
      "service_url": "http://dev:3000",
      "warnings": []
    },
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "coding-agents",
    "branch_name": "feature/coding-agents",
    "worktree_path": "/Users/dev/projects/branchbox-suite/branchbox/coding-agents",
    "base_branch": null,
    "feature_url": "dev-coding-agents.localhost",
    "compose_project_name": "branchbox-coding-agents",
    "env_path": "/Users/dev/projects/branchbox-suite/branchbox/coding-agents/.env",
    "status": "removed",
    "created_at": "2026-01-03T16:36:45.605945Z",
    "updated_at": "2026-01-07T00:37:51.474180Z",
    "removed_at": "2026-01-07T00:37:51.474180Z",
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel removed via CLI",
      "last_updated": "2026-01-07T00:37:51.474180Z",
      "removed_at": "2026-01-07T00:37:51.474180Z"
    },
    "color": "#2ecc71",
    "last_commit": "1aa9035a5ac3bd546ee903e9e6cd8a66bca1507f",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 2,
        "forced": false,
        "recorded_at": "2026-01-03T16:36:45.605945Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 220,
        "forced": false,
        "recorded_at": "2026-01-03T16:36:45.605945Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-01-03T16:36:45.605945Z"
      },
      {
        "module": "tunnel",
        "status": "skipped",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-01-03T16:36:45.605945Z"
      }
    ],
    "last_summary_rendered_at": "2026-01-03T16:36:45.605945Z",
    "adapter": {
      "name": "Generic",
      "service_url": "http://dev:3000",
      "warnings": []
    },
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "workspace-mount",
    "branch_name": "feature/workspace-mount",
    "worktree_path": "/Users/dev/projects/branchbox-suite/branchbox/workspace-mount",
    "base_branch": null,
    "feature_url": "dev-workspace-mount.localhost",
    "compose_project_name": "branchbox-workspace-mount",
    "env_path": "/Users/dev/projects/branchbox-suite/branchbox/workspace-mount/.env",
    "status": "removed",
    "created_at": "2026-01-02T01:33:18.729351Z",
    "updated_at": "2026-01-03T16:35:42.284748Z",
    "removed_at": "2026-01-03T16:35:42.284748Z",
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel removed via CLI",
      "last_updated": "2026-01-03T16:35:42.284748Z",
      "removed_at": "2026-01-03T16:35:42.284748Z"
    },
    "color": "#16a085",
    "last_commit": "2c76d851d42736e9a11c7cf7030e92a2bf09f3bb",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 1,
        "forced": false,
        "recorded_at": "2026-01-02T01:33:18.729351Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 237,
        "forced": false,
        "recorded_at": "2026-01-02T01:33:18.729351Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-01-02T01:33:18.729351Z"
      },
      {
        "module": "tunnel",
        "status": "skipped",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2026-01-02T01:33:18.729351Z"
      }
    ],
    "last_summary_rendered_at": "2026-01-02T01:33:18.729351Z",
    "adapter": {
      "name": "Generic",
      "service_url": "http://dev:3000",
      "warnings": []
    },
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "milestone2",
    "branch_name": "feature/milestone2",
    "worktree_path": "/Users/dev/projects/branchbox-suite/branchbox/milestone2",
    "base_branch": null,
    "feature_url": "dev-milestone2.localhost",
    "compose_project_name": "branchbox-milestone2",
    "env_path": "/Users/dev/projects/branchbox-suite/branchbox/milestone2/.env",
    "status": "removed",
    "created_at": "2025-11-10T16:05:34.457985Z",
    "updated_at": "2025-11-14T14:41:10.315190Z",
    "removed_at": "2025-11-14T14:41:10.315190Z",
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel removed via CLI",
      "last_updated": "2025-11-14T14:41:10.315190Z",
      "removed_at": "2025-11-14T14:41:10.315190Z"
    },
    "color": "#e67e22",
    "last_commit": "14233691e3bb17e6a3ec4c7d881d5474bde8bc58",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 1,
        "forced": false,
        "recorded_at": "2025-11-10T16:05:34.457985Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 171,
        "forced": false,
        "recorded_at": "2025-11-10T16:05:34.457985Z"
      },
      {
        "module": "tunnel",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2025-11-10T16:05:34.457985Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2025-11-10T16:05:34.457985Z"
      }
    ],
    "last_summary_rendered_at": "2025-11-10T16:05:34.457985Z",
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "cli-e2e-rust-smoke",
    "branch_name": "feature/cli-e2e-rust-smoke",
    "worktree_path": "/workspaces/cli-e2e-rust-smoke",
    "base_branch": null,
    "feature_url": "dev-cli-e2e-rust-smoke.localhost",
    "compose_project_name": "workspaces-cli-e2e-rust-smoke",
    "env_path": "/workspaces/cli-e2e-rust-smoke/.env",
    "status": "removed",
    "created_at": "2025-11-10T04:29:00.768535795Z",
    "updated_at": "2025-11-10T04:29:02.145183129Z",
    "removed_at": "2025-11-10T04:29:02.145183129Z",
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel removed via CLI",
      "last_updated": "2025-11-10T04:29:02.145183129Z",
      "removed_at": "2025-11-10T04:29:02.145183129Z"
    },
    "color": "#c0392b",
    "last_commit": "2aab883b3a1c15cc96da7cd9597a2e983757ac2d",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 4,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:00.768535795Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 74,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:00.768535795Z"
      },
      {
        "module": "tunnel",
        "status": "success",
        "duration_ms": 5,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:00.768535795Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 3,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:00.768535795Z"
      }
    ],
    "last_summary_rendered_at": "2025-11-10T04:29:00.768535795Z",
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "cli-e2e-rust-smoke-tunnel-fallback",
    "branch_name": "feature/cli-e2e-rust-smoke-tunnel-fallback",
    "worktree_path": "/workspaces/cli-e2e-rust-smoke-tunnel-fallback",
    "base_branch": null,
    "feature_url": "dev-cli-e2e-rust-smoke-tunnel-fallback.localhost",
    "compose_project_name": "workspaces-cli-e2e-rust-smoke-tunnel-fallback",
    "env_path": "/workspaces/cli-e2e-rust-smoke-tunnel-fallback/.env",
    "status": "removed",
    "created_at": "2025-11-10T04:29:01.712202421Z",
    "updated_at": "2025-11-10T04:29:01.966235379Z",
    "removed_at": "2025-11-10T04:29:01.966235379Z",
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel removed via CLI",
      "last_updated": "2025-11-10T04:29:01.966235379Z",
      "removed_at": "2025-11-10T04:29:01.966235379Z"
    },
    "color": "#c0392b",
    "last_commit": "2aab883b3a1c15cc96da7cd9597a2e983757ac2d",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 2,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.712202421Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 42,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.712202421Z"
      },
      {
        "module": "tunnel",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.712202421Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.712202421Z"
      }
    ],
    "last_summary_rendered_at": "2025-11-10T04:29:01.712202421Z",
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  },
  {
    "work_feature": "cli-e2e-rust-smoke-tunnel",
    "branch_name": "feature/cli-e2e-rust-smoke-tunnel",
    "worktree_path": "/workspaces/cli-e2e-rust-smoke-tunnel",
    "base_branch": null,
    "feature_url": "dev-cli-e2e-rust-smoke-tunnel.localhost",
    "compose_project_name": "workspaces-cli-e2e-rust-smoke-tunnel",
    "env_path": "/workspaces/cli-e2e-rust-smoke-tunnel/.env",
    "status": "removed",
    "created_at": "2025-11-10T04:29:01.150559004Z",
    "updated_at": "2025-11-10T04:29:01.428869795Z",
    "removed_at": "2025-11-10T04:29:01.428869795Z",
    "tunnel": {
      "provider": "manual",
      "status": "disabled",
      "notes": "Tunnel removed via CLI",
      "last_updated": "2025-11-10T04:29:01.428869795Z",
      "removed_at": "2025-11-10T04:29:01.428869795Z"
    },
    "color": "#27ae60",
    "last_commit": "2aab883b3a1c15cc96da7cd9597a2e983757ac2d",
    "devcontainer_outdated": false,
    "start_mode": "full",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "success",
        "duration_ms": 3,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.150559004Z"
      },
      {
        "module": "compose",
        "status": "success",
        "duration_ms": 71,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.150559004Z"
      },
      {
        "module": "tunnel",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.150559004Z"
      },
      {
        "module": "specs",
        "status": "success",
        "duration_ms": 0,
        "forced": false,
        "recorded_at": "2025-11-10T04:29:01.150559004Z"
      }
    ],
    "last_summary_rendered_at": "2025-11-10T04:29:01.150559004Z",
    "runtime": {
      "provider": "container"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "Set BRANCHBOX_DEFAULT_AGENT_CMD to auto-launch your preferred agent",
      "followup": null
    }
  }
]
"""#

    // Verbatim copy of cli-0.13.4/synthetic_feature_list_new_statuses.json.
    private static let newStatusesJSON = #"""
[
  {
    "work_feature": "sbx-demo",
    "branch_name": "feature/sbx-demo",
    "worktree_path": "/tmp/x/sbx-demo",
    "base_branch": "main",
    "feature_url": "dev-sbx-demo.localhost",
    "compose_project_name": "x-sbx-demo",
    "env_path": "/tmp/x/sbx-demo/.env",
    "status": "degraded",
    "created_at": "2026-09-30T10:00:00.123456789Z",
    "updated_at": "2026-09-30T10:05:00.5Z",
    "removed_at": null,
    "tunnel": {
      "provider": "cloudflared",
      "hostname": "sbx-demo.example.dev",
      "service_url": "http://app:3000",
      "status": "active",
      "last_updated": "2026-09-30T10:04:00.1Z"
    },
    "color": "#3498db",
    "pr_number": 42,
    "last_commit": "abc123",
    "devcontainer_outdated": true,
    "last_sync_at": "2026-09-30T10:03:00.25Z",
    "sync_strategy": "copy",
    "start_mode": "full",
    "prompt_seed": "do the thing",
    "module_outcomes": [
      {
        "module": "devcontainer",
        "status": "failed",
        "duration_ms": 1200,
        "notes": [
          "devcontainer up failed"
        ],
        "forced": false,
        "recorded_at": "2026-09-30T10:05:00.5Z"
      }
    ],
    "adapter": {
      "name": "Rails",
      "service_url": "http://app:3000",
      "warnings": [
        "w1"
      ]
    },
    "runtime": {
      "provider": "sbx",
      "runtime_id": "branchbox-sbx-demo",
      "published_ports": [
        {
          "host": 49152,
          "runtime": 3000
        }
      ],
      "container_id": "deadbeef",
      "workspace_folder": "/workspaces/sbx-demo",
      "container_user": "vscode"
    },
    "default_agent": {
      "status": "waiting",
      "label": "claude",
      "command": "claude",
      "detail": "Devcontainer module not detected; launch deferred",
      "followup": "..."
    }
  },
  {
    "work_feature": "retained",
    "branch_name": "feature/retained",
    "worktree_path": "/tmp/x/retained",
    "base_branch": null,
    "feature_url": null,
    "compose_project_name": null,
    "env_path": null,
    "status": "failed_retained",
    "created_at": "2026-09-30T10:00:00Z",
    "updated_at": "2026-09-30T10:00:00Z",
    "removed_at": null,
    "devcontainer_outdated": false,
    "start_mode": "minimal",
    "runtime": {
      "provider": "local-vm"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "x",
      "followup": null
    }
  },
  {
    "work_feature": "orphan",
    "branch_name": "feature/orphan",
    "worktree_path": "/tmp/x/orphan",
    "base_branch": null,
    "feature_url": null,
    "compose_project_name": null,
    "env_path": null,
    "status": "orphaned",
    "created_at": "2026-09-30T10:00:00Z",
    "updated_at": "2026-09-30T10:00:00Z",
    "removed_at": null,
    "devcontainer_outdated": false,
    "start_mode": "full",
    "runtime": {
      "provider": "in-guest"
    },
    "default_agent": {
      "status": "disabled",
      "label": null,
      "command": null,
      "detail": "x",
      "followup": null
    }
  }
]
"""#
}
