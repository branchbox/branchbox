import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

private let project = ProjectRef(root: URL(fileURLWithPath: "/r/main"))
private func feature(_ name: String) -> FeatureRef { FeatureRef(project: project, name: name) }

private func teardown(_ name: String, branch: String?, policy: BranchPolicy, discard: [String]? = nil,
                      forceRemoval: Bool = false, completeSpec: Bool = false) -> TeardownRequest {
    var request = TeardownRequest(feature: feature(name), recordedBranch: branch, branch: policy)
    request.discard = discard.map { DiscardConsent(userFiles: $0) }
    request.forceRemoval = forceRemoval
    request.completeSpec = completeSpec
    return request
}

private let branchFlags: Set<String> = ["--keep-branch", "--delete-branch"]

@Suite struct CLICommandTests {
    // MARK: - Teardown

    @Test func contractTeardownMapsEachPolicyToItsBranchFlag() {
        #expect(CLICommand.teardown(teardown("eta", branch: "feature/eta", policy: .keep), mode: .contract)
                == ["feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature", "--keep-branch"])
        #expect(CLICommand.teardown(teardown("eta", branch: "feature/eta", policy: .deleteIfMerged), mode: .contract)
                == ["feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature", "--delete-branch"])
        #expect(CLICommand.teardown(teardown("eta", branch: "feature/eta", policy: .forceDelete), mode: .contract)
                == ["feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature",
                    "--delete-branch", "--force-delete-branch"])
    }

    @Test func contractDiscardUsesDiscardChangesNeverForce() {
        let arguments = CLICommand.teardown(teardown("eta", branch: "feature/eta", policy: .deleteIfMerged,
                                                     discard: ["notes.txt"], completeSpec: true), mode: .contract)
        #expect(arguments == ["feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature",
                              "--delete-branch", "--discard-changes", "--complete-spec"])
        #expect(!arguments.contains("--force"))
    }

    @Test func contractForcedRemovalKeepsTheBranchForTheAppStep() {
        #expect(CLICommand.teardownFlags(teardown("eta", branch: "feature/eta", policy: .forceDelete, forceRemoval: true),
                                         mode: .contract) == ["--force", "--keep-branch"])
    }

    @Test func legacyTeardownAlwaysKeepsTheBranchAndForcesOnlyWithConsentOrForcedRemoval() {
        for policy in [BranchPolicy.keep, .deleteIfMerged, .forceDelete] {
            #expect(CLICommand.teardownFlags(teardown("eta", branch: "feature/eta", policy: policy), mode: .legacy)
                    == ["--keep-branch"])
            #expect(CLICommand.teardownFlags(teardown("eta", branch: "feature/eta", policy: policy, discard: []),
                                             mode: .legacy) == ["--keep-branch", "--force"])
            #expect(CLICommand.teardownFlags(teardown("eta", branch: "feature/eta", policy: policy, forceRemoval: true,
                                                      completeSpec: true), mode: .legacy)
                    == ["--keep-branch", "--force", "--complete-spec"])
        }
        #expect(CLICommand.teardown(teardown("eta", branch: "feature/eta", policy: .forceDelete), mode: .legacy)
                == ["feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature", "--keep-branch"])
        // A worktree holding only BranchBox's own files gets `--force`, so 0.13.x removes them with
        // `git worktree remove --force` instead of refusing or falling back to `remove_dir_all`.
        #expect(CLICommand.teardown(teardown("eta", branch: "feature/eta", policy: .keep), mode: .legacy,
                                    onlyGeneratedChanges: true)
                == ["feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature", "--keep-branch",
                    "--force"])
        #expect(CLICommand.teardownFlags(teardown("eta", branch: "feature/eta", policy: .keep), mode: .contract,
                                         onlyGeneratedChanges: true) == ["--keep-branch"])
    }

    /// Every combination: exactly one branch flag, `--json`, `--repo`, and no `--force` without consent or
    /// forced removal.
    @Test(arguments: [CLICommand.TeardownMode.contract, .legacy])
    func everyTeardownHasExactlyOneBranchFlagJSONAndRepo(mode: CLICommand.TeardownMode) {
        for policy in [BranchPolicy.keep, .deleteIfMerged, .forceDelete] {
            for discard in [nil, [String](), ["notes.txt"]] {
                for forceRemoval in [false, true] {
                    let request = teardown("eta", branch: "feature/eta", policy: policy, discard: discard,
                                           forceRemoval: forceRemoval)
                    let arguments = CLICommand.teardown(request, mode: mode)
                    #expect(arguments.filter(branchFlags.contains).count == 1, "\(arguments)")
                    #expect(arguments.filter { $0 == "--json" }.count == 1)
                    #expect(arguments.contains("--repo"))
                    if arguments.contains("--force") { #expect((discard != nil && mode == .legacy) || forceRemoval) }
                    if mode == .contract, discard != nil, !forceRemoval { #expect(!arguments.contains("--force")) }
                }
            }
        }
    }

    @Test func branchPrefixIsDerivedFromTheRecordedBranch() {
        #expect(CLICommand.branchPrefix(recordedBranch: "spike/zeta", name: "zeta") == "spike")
        #expect(CLICommand.branchPrefix(recordedBranch: "team/x/zeta", name: "zeta") == "team/x")
        #expect(CLICommand.branchPrefix(recordedBranch: "zeta", name: "zeta") == "")
        #expect(CLICommand.branchPrefix(recordedBranch: "other", name: "zeta") == nil)
        #expect(CLICommand.branchPrefix(recordedBranch: "", name: "zeta") == nil)
        #expect(CLICommand.branchPrefix(recordedBranch: nil, name: "zeta") == nil)

        #expect(CLICommand.teardown(teardown("zeta", branch: "spike/zeta", policy: .keep), mode: .legacy)
                == ["feature", "teardown", "zeta", "--repo", "/r/main", "--json", "--branch-prefix", "spike", "--keep-branch"])
        #expect(CLICommand.teardown(teardown("zeta", branch: "zeta", policy: .keep), mode: .legacy)
                == ["feature", "teardown", "zeta", "--repo", "/r/main", "--json", "--branch-prefix", "", "--keep-branch"])
        #expect(CLICommand.teardown(teardown("zeta", branch: nil, policy: .keep), mode: .legacy)
                == ["feature", "teardown", "zeta", "--repo", "/r/main", "--json", "--keep-branch"])
        #expect(CLICommand.teardown(teardown("zeta", branch: "spike/zeta", policy: .keep), mode: .contract, dryRun: true).last
                == "--dry-run")
    }

    // MARK: - Start

    @Test func startCarriesEveryOptionAndNeverTitleOrNoSummary() {
        var request = StartFeatureRequest(project: project, name: "oauth", runtime: .sbx)
        #expect(CLICommand.start(request) == ["feature", "start", "oauth", "--repo", "/r/main", "--json", "--runtime", "sbx"])

        request.base = "develop"
        request.branchPrefix = "spike"
        request.mode = .minimal
        request.useDefaultPrompt = true
        request.prompt = "Add OAuth"
        request.skipModules = ["tunnel", "compose"]
        request.reuse = .existingWorktree(.preserve)
        request.keepRuntimeOnFailure = true
        request.verbose = true
        let arguments = CLICommand.start(request)
        #expect(arguments == ["feature", "start", "oauth", "--repo", "/r/main", "--json", "--runtime", "sbx",
                              "--base", "develop", "--branch-prefix", "spike", "--minimal", "--default-prompt",
                              "--prompt=Add OAuth", "--skip-module", "tunnel", "--skip-module", "compose",
                              "--reuse", "--devcontainer-reuse", "preserve", "--keep-runtime-on-failure", "--telemetry"])
        #expect(!arguments.contains("--title") && !arguments.contains("--no-summary"))

        request.reuse = .retainedRuntime
        request.mode = .full                 // --default-prompt is minimal-only
        request.runtime = .localVM
        let retained = CLICommand.start(request)
        #expect(retained.contains("--reuse-runtime") && !retained.contains("--reuse"))
        #expect(!retained.contains("--minimal") && !retained.contains("--default-prompt"))

        // A prompt that starts with `-` (a pasted bullet list, a flag-like word) stays one argument.
        for prompt in ["- fix the login bug\n- add tests", "--reuse"] {
            request.prompt = prompt
            let arguments = CLICommand.start(request)
            #expect(arguments.contains("--prompt=\(prompt)"))
            #expect(!arguments.contains(prompt) && !arguments.contains("--prompt"))
        }
        #expect(retained[6...7] == ["--runtime", "local-vm"])
    }

    // MARK: - Exec

    @Test func execFlagsComeBeforeTheDoubleDash() throws {
        let request = ExecRequest(feature: feature("eta"), command: ["sh", "-c", "echo --json --repo x"])
        #expect(CLICommand.exec(request, worktree: "/r/eta")
                == ["feature", "exec", "--repo", "/r/main", "--json", "eta", "--", "sh", "-c", "echo --json --repo x"])
        let devcontainer = ExecRequest(feature: feature("eta"), command: ["ls", "-la"], target: .devcontainer)
        #expect(CLICommand.exec(devcontainer, worktree: "/r/eta")
                == ["devcontainer", "exec", "-w", "/r/eta", "--json", "--", "ls", "-la"])
        for arguments in [CLICommand.exec(request, worktree: "/r/eta"), CLICommand.exec(devcontainer, worktree: "/r/eta")] {
            let separator = try #require(arguments.firstIndex(of: "--"))
            #expect(arguments[..<separator].contains("--json"))
        }
    }

    // MARK: - Init

    @Test func initAlwaysHasYesAndReorganizesOnlyOnRequest() {
        let folder = URL(fileURLWithPath: "/r/new")
        #expect(CLICommand.initProject(InitRequest(folder: folder), json: false) == ["init", "-y"])
        #expect(CLICommand.initProject(InitRequest(folder: folder), json: true) == ["init", "-y", "--json"])

        let everything = InitRequest(folder: folder, stack: "rails", skipDevcontainer: true, skipEnv: true,
                                     codingAgents: false, reorganize: true, dryRun: true, mode: .update,
                                     onePassword: .configure(githubRef: "op://v/gh/token", signingKeyRef: "op://v/key",
                                                             verify: false))
        #expect(CLICommand.initProject(everything, json: true)
                == ["init", "-y", "--json", "-s", "rails", "--skip-devcontainer", "--skip-env", "--no-coding-agents",
                    "--reorganize", "--dry-run", "--update", "--op-github-ref", "op://v/gh/token",
                    "--op-signing-key-ref", "op://v/key", "--no-verify-op-refs"])
        // Legacy CLIs know none of the 1Password flags.
        #expect(CLICommand.initProject(everything, json: false)
                == ["init", "-y", "-s", "rails", "--skip-devcontainer", "--skip-env", "--no-coding-agents", "--reorganize",
                    "--dry-run", "--update"])
        #expect(CLICommand.initProject(InitRequest(folder: folder, mode: .validate, onePassword: .skip), json: true)
                == ["init", "-y", "--json", "--validate", "--skip-1password"])
        for request in [InitRequest(folder: folder), InitRequest(folder: folder, mode: .update)] {
            #expect(!CLICommand.initProject(request, json: true).contains("--reorganize"))
        }
    }

    // MARK: - Everything else

    @Test func readCommandsPassTheRepositoryExplicitly() {
        #expect(CLICommand.version == ["version", "--json"])
        #expect(CLICommand.legacyVersion == ["--version"])
        #expect(CLICommand.listFeatures(repo: "/r/main", includeRemoved: false) == ["feature", "list", "--json", "--repo", "/r/main"])
        #expect(CLICommand.listFeatures(repo: "/r/main", includeRemoved: true).last == "--all")
        #expect(CLICommand.detect(folder: "/r/x", json: true) == ["detect", "-p", "/r/x", "--json"])
        #expect(CLICommand.detect(folder: "/r/x", json: false) == ["detect", "-p", "/r/x"])
        #expect(CLICommand.configGet(repo: "/r/main") == ["config", "get", "--repo", "/r/main", "--json"])
        #expect(CLICommand.configApply(repo: "/r/main", dryRun: true)
                == ["config", "apply", "--repo", "/r/main", "--file", "-", "--json", "--dry-run"])
        #expect(CLICommand.nameValidate("-x") == ["name", "validate", "--", "-x"])
        #expect(CLICommand.nameGenerate("OAuth Integration") == ["name", "generate", "--", "OAuth Integration"])
        #expect(CLICommand.doctor(repo: nil) == ["doctor", "--json"])
        #expect(CLICommand.doctor(repo: "/r/main") == ["doctor", "--repo", "/r/main", "--json"])
        #expect(CLICommand.devcontainerDetect(worktree: "/r/eta") == ["devcontainer", "detect", "-p", "/r/eta", "--json"])
    }

    @Test func tokenIsNeverInArgv() {
        let set = TunnelCredentialsRequest(accountID: "acc", apiToken: SecretString("tok-123"))
        let arguments = CLICommand.tunnelCredentials(set, repo: "/r/main")
        #expect(arguments == ["tunnel", "credentials", "set", "--repo", "/r/main", "--account-id", "acc",
                              "--api-token-stdin", "--json"])
        #expect(!arguments.joined().contains("tok-123"))
        #expect(CLICommand.tunnelCredentials(TunnelCredentialsRequest(accountID: "acc", apiToken: nil, clear: true),
                                             repo: "/r/main")
                == ["tunnel", "credentials", "set", "--repo", "/r/main", "--account-id", "acc", "--clear", "--json"])
    }

    @Test func devcontainerSyncAndTunnelCommands() {
        #expect(CLICommand.devcontainer(.up(removeExisting: true, buildNoCache: true), worktree: "/r/eta")
                == ["devcontainer", "up", "/r/eta", "--json", "--remove-existing-container", "--build-no-cache"])
        #expect(CLICommand.devcontainer(.up(removeExisting: false, buildNoCache: false), worktree: "/r/eta")
                == ["devcontainer", "up", "/r/eta", "--json"])
        #expect(CLICommand.devcontainer(.down(removeVolumes: true), worktree: "/r/eta") == ["devcontainer", "down", "/r/eta", "--json", "-v"])
        #expect(CLICommand.devcontainer(.build(noCache: true), worktree: "/r/eta")
                == ["devcontainer", "build", "/r/eta", "--json", "--no-cache"])

        let sync = SyncRequest(project: project, strategy: .symlink, dryRun: true, features: ["eta", "zeta"])
        #expect(CLICommand.syncDevcontainers(sync, json: true)
                == ["devcontainer", "sync", "-p", "/r/main", "-s", "symlink", "-n", "--feature", "eta", "--feature", "zeta",
                    "--json"])
        #expect(CLICommand.syncDevcontainers(sync, json: false) == ["devcontainer", "sync", "-p", "/r/main", "-s", "symlink", "-n"])

        #expect(CLICommand.tunnelOpen(feature("eta")) == ["tunnel", "open", "eta", "--repo", "/r/main", "--json"])
        #expect(CLICommand.tunnelRemove(feature("eta"), force: true)
                == ["tunnel", "remove", "eta", "--repo", "/r/main", "--json", "--force"])
    }
}
