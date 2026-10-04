import BranchBoxKit
import Foundation

/// The CLI's argv for every backend method (DESIGN §6.2, §6.5), without the executable. Pure; golden-tested.
///
/// - `--repo`/`-p` is always passed; cwd never selects the repository.
/// - Flags always come before `--`.
/// - `feature start` never gets `--title` or `--no-summary`; `init` always gets `-y`.
public enum CLICommand {
    // MARK: - Identity and projects

    public static let version = ["version", "--json"]
    public static let legacyVersion = ["--version"]

    public static func listFeatures(repo: String, includeRemoved: Bool) -> [String] {
        ["feature", "list", "--json", "--repo", repo] + (includeRemoved ? ["--all"] : [])
    }

    /// `detect -p F --json` with `detect-json`, else the text report.
    public static func detect(folder: String, json: Bool) -> [String] {
        ["detect", "-p", folder] + (json ? ["--json"] : [])
    }

    public static func configGet(repo: String) -> [String] {
        ["config", "get", "--repo", repo, "--json"]
    }

    /// The RFC 7386 merge patch travels on stdin (`--file -`).
    public static func configApply(repo: String, dryRun: Bool) -> [String] {
        ["config", "apply", "--repo", repo, "--file", "-", "--json"] + (dryRun ? ["--dry-run"] : [])
    }

    /// The token travels on stdin (`--api-token-stdin`), never in argv.
    public static func tunnelCredentials(_ request: TunnelCredentialsRequest, repo: String) -> [String] {
        var arguments = ["tunnel", "credentials", "set", "--repo", repo, "--account-id", request.accountID]
        if request.clear {
            arguments.append("--clear")
        } else if request.apiToken != nil {
            arguments.append("--api-token-stdin")
        }
        return arguments + ["--json"]
    }

    /// Run with cwd = the folder. `--json` and the 1Password flags only with `init-json`; `--reorganize` only on
    /// request (it moves the repository).
    public static func initProject(_ request: InitRequest, json: Bool) -> [String] {
        var arguments = ["init", "-y"]
        if json { arguments.append("--json") }
        if let stack = request.stack, !stack.isEmpty { arguments += ["-s", stack] }
        if request.skipDevcontainer { arguments.append("--skip-devcontainer") }
        if request.skipEnv { arguments.append("--skip-env") }
        if !request.codingAgents { arguments.append("--no-coding-agents") }
        if request.reorganize { arguments.append("--reorganize") }
        if request.dryRun { arguments.append("--dry-run") }
        switch request.mode {
        case .initialize: break
        case .update: arguments.append("--update")
        case .validate: arguments.append("--validate")
        }
        if json {
            switch request.onePassword {
            case .unchanged: break
            case .skip: arguments.append("--skip-1password")
            case .configure(let githubRef, let signingKeyRef, let verify):
                arguments += ["--op-github-ref", githubRef]
                if let signingKeyRef, !signingKeyRef.isEmpty { arguments += ["--op-signing-key-ref", signingKeyRef] }
                if !verify { arguments.append("--no-verify-op-refs") }
            }
        }
        return arguments
    }

    /// `--` keeps typed input that starts with `-` from reading as a flag.
    public static func nameValidate(_ input: String) -> [String] { ["name", "validate", "--", input] }
    public static func nameGenerate(_ input: String) -> [String] { ["name", "generate", "--", input] }

    // MARK: - Features

    public static func start(_ request: StartFeatureRequest) -> [String] {
        var arguments = ["feature", "start", request.name, "--repo", request.project.path, "--json",
                         "--runtime", request.runtime.raw]
        if let base = request.base, !base.isEmpty { arguments += ["--base", base] }
        if let prefix = request.branchPrefix { arguments += ["--branch-prefix", prefix] }
        if request.mode == .minimal {
            arguments.append("--minimal")
            if request.useDefaultPrompt { arguments.append("--default-prompt") }
        }
        // One argument: clap reads a separate value starting with `-` (a pasted bullet list) as a flag.
        if let prompt = request.prompt, !prompt.isEmpty { arguments.append("--prompt=\(prompt)") }
        for module in request.skipModules { arguments += ["--skip-module", module] }
        switch request.reuse {
        case .none: break
        case .existingWorktree(let policy): arguments += ["--reuse", "--devcontainer-reuse", policy.rawValue]
        case .retainedRuntime: arguments.append("--reuse-runtime")
        }
        if request.keepRuntimeOnFailure { arguments.append("--keep-runtime-on-failure") }
        if request.verbose { arguments.append("--telemetry") }
        return arguments
    }

    /// How the teardown argv is chosen (DESIGN §6.5 steps 5–6).
    public enum TeardownMode: Sendable, Hashable {
        /// The CLI owns the branch and the discard (`teardown-discard-changes`).
        case contract
        /// Always `--keep-branch`; `--force` only with consent, forced removal, or a worktree the preflight found
        /// holding nothing but BranchBox's own files; the app deletes the branch.
        case legacy
    }

    /// `feature teardown <n> --repo R --json [--branch-prefix P] <flags> [--dry-run]`. Exactly one of
    /// `--keep-branch`/`--delete-branch` is always present.
    ///
    /// `onlyGeneratedChanges` (legacy mode): the re-plan just taken found no user changes, only BranchBox-generated
    /// or preserved files. 0.13.x then gets `--force`, so `git worktree remove --force` removes those files cleanly;
    /// without it 0.13.x refuses over its own `.devcontainer` files, or deletes the folder with `remove_dir_all`
    /// once `git worktree remove` fails on them.
    public static func teardown(_ request: TeardownRequest, mode: TeardownMode, dryRun: Bool = false,
                                onlyGeneratedChanges: Bool = false) -> [String] {
        var arguments = ["feature", "teardown", request.feature.name, "--repo", request.feature.project.path, "--json"]
        if let prefix = branchPrefix(recordedBranch: request.recordedBranch, name: request.feature.name) {
            arguments += ["--branch-prefix", prefix]
        }
        arguments += teardownFlags(request, mode: mode, onlyGeneratedChanges: onlyGeneratedChanges)
        if dryRun { arguments.append("--dry-run") }
        return arguments
    }

    public static func teardownFlags(_ request: TeardownRequest, mode: TeardownMode,
                                     onlyGeneratedChanges: Bool = false) -> [String] {
        var flags: [String] = []
        switch mode {
        case .contract:
            if request.forceRemoval {
                flags += ["--force", "--keep-branch"]
            } else {
                switch request.branch {
                case .keep: flags.append("--keep-branch")
                case .deleteIfMerged: flags.append("--delete-branch")
                case .forceDelete: flags += ["--delete-branch", "--force-delete-branch"]
                }
            }
            if request.discard != nil { flags.append("--discard-changes") }
        case .legacy:
            flags.append("--keep-branch")
            if request.discard != nil || request.forceRemoval || onlyGeneratedChanges { flags.append("--force") }
        }
        if request.completeSpec { flags.append("--complete-spec") }
        return flags
    }

    /// `--branch-prefix` derived from the recorded branch: `spike/zeta` → `spike`, a branch equal to the name → `""`;
    /// nil (omit the flag) when it cannot be derived.
    public static func branchPrefix(recordedBranch: String?, name: String) -> String? {
        guard let branch = recordedBranch, !branch.isEmpty else { return nil }
        if branch == name { return "" }
        let suffix = "/" + name
        guard branch.count > suffix.count, branch.hasSuffix(suffix) else { return nil }
        return String(branch.dropLast(suffix.count))
    }

    /// `feature exec --repo R --json <n> -- <cmd…>`, or `devcontainer exec -w <worktree> --json -- <cmd…>`.
    public static func exec(_ request: ExecRequest, worktree: String) -> [String] {
        switch request.target {
        case .featureRuntime:
            return ["feature", "exec", "--repo", request.feature.project.path, "--json", request.feature.name, "--"]
                + request.command
        case .devcontainer:
            return ["devcontainer", "exec", "-w", worktree, "--json", "--"] + request.command
        }
    }

    /// Run with cwd = the worktree.
    public static func devcontainer(_ action: DevcontainerAction, worktree: String) -> [String] {
        switch action {
        case .up(let removeExisting, let buildNoCache):
            return ["devcontainer", "up", worktree, "--json"]
                + (removeExisting ? ["--remove-existing-container"] : []) + (buildNoCache ? ["--build-no-cache"] : [])
        case .down(let removeVolumes):
            return ["devcontainer", "down", worktree, "--json"] + (removeVolumes ? ["-v"] : [])
        case .build(let noCache):
            return ["devcontainer", "build", worktree, "--json"] + (noCache ? ["--no-cache"] : [])
        }
    }

    public static func devcontainerDetect(worktree: String) -> [String] {
        ["devcontainer", "detect", "-p", worktree, "--json"]
    }

    /// `--json` and `--feature` need `devcontainer-sync-json`; legacy CLIs print text.
    public static func syncDevcontainers(_ request: SyncRequest, json: Bool) -> [String] {
        var arguments = ["devcontainer", "sync", "-p", request.project.path]
        if let strategy = request.strategy { arguments += ["-s", strategy.rawValue] }
        if request.dryRun { arguments.append("-n") }
        if json {
            for feature in request.features { arguments += ["--feature", feature] }
            arguments.append("--json")
        }
        return arguments
    }

    public static func tunnelOpen(_ feature: FeatureRef) -> [String] {
        ["tunnel", "open", feature.name, "--repo", feature.project.path, "--json"]
    }

    public static func tunnelRemove(_ feature: FeatureRef, force: Bool) -> [String] {
        ["tunnel", "remove", feature.name, "--repo", feature.project.path, "--json"] + (force ? ["--force"] : [])
    }

    public static func doctor(repo: String?) -> [String] {
        ["doctor"] + (repo.map { ["--repo", $0] } ?? []) + ["--json"]
    }
}
