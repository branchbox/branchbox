@testable import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

/// A `ChangeContext` over in-memory trees.
private struct FakeContext: ChangeContext {
    var featureName = "eta"
    var worktree: [String: FakeItem] = [:]
    var main: [String: FakeItem] = [:]
    var baseline: [String: String]?
    var committed: [String: String] = [:]

    enum FakeItem {
        case file(String)
        case link(String)
        case directory
    }

    func item(_ path: String, in tree: ChangeTree) -> ChangeItem {
        if path.split(separator: "/").contains("..") { return .unsafe }
        switch (tree == .worktree ? worktree : main)[path] {
        case nil: return .missing
        case .file(let text)?: return .file(size: text.utf8.count)
        case .link(let target)?: return .symbolicLink(target: target)
        case .directory?: return .directory
        }
    }

    func contents(_ path: String, in tree: ChangeTree, limit: Int) -> Data? {
        guard case .file(let text)? = (tree == .worktree ? worktree : main)[path], text.utf8.count <= limit else {
            return nil
        }
        return Data(text.utf8)
    }

    func devcontainerBaseline() -> [String: String]? { baseline }
    func committedContents(_ path: String) -> Data? { committed[path].map { Data($0.utf8) } }
}

private func untracked(_ path: String) -> GitStatusEntry { GitStatusEntry(index: "?", worktree: "?", path: path) }
private func modified(_ path: String) -> GitStatusEntry { GitStatusEntry(index: " ", worktree: "M", path: path) }
private func deleted(_ path: String) -> GitStatusEntry { GitStatusEntry(index: " ", worktree: "D", path: path) }

private let mainEnv = "DATABASE_URL=postgres://localhost/app\nSECRET=abc\n"
private let featureBlock = """
    # Feature-specific configuration (managed by branchbox)
    WORK_FEATURE=eta
    APP_URL="https://dev-eta.example.com"
    COMPOSE_PROJECT_NAME=demo-eta

    DEVCONTAINER_NAME=demo-eta
    GIT_BRANCH=feature/eta

    """
private let managedSettings = """
    {"peacock.color": "#e67e22", "peacock.remoteColor": "#e67e22",
     "window.title": "${rootName} [eta] - ${activeEditorShort}",
     "workbench.colorCustomizations": {"statusBar.background": "#e67e22"}}
    """

@Suite struct WorktreeChangeClassifierTests {
    @Test func stableContentHashMatchesCoreByteForByte() {
        // Vectors printed by core's own `stable_content_hash` fold (core/src/modules/devcontainer.rs).
        #expect(WorktreeChangeClassifier.digest(Data()) == "cbf29ce484222325")
        #expect(WorktreeChangeClassifier.digest(Data("a".utf8)) == "af63dc4c8601ec8c")
        #expect(WorktreeChangeClassifier.digest(Data("foobar".utf8)) == "85944171f73967e8")
        #expect(WorktreeChangeClassifier.digest(Data("{\n  \"name\": \"demo\"\n}\n".utf8)) == "533cf7a759345aaf")
        #expect(WorktreeChangeClassifier.digest(Data(0...255)) == "4242dc5249c33625")
    }

    @Test func r1ReservedNamesAreGeneratedWhateverTheirState() {
        let context = FakeContext()
        for path in [".devcontainer/.branchbox.env", ".devcontainer/.cloudflared.env", ".devcontainer/.devcontainer.json",
                     ".devcontainer/.branchbox-sbx-compose.yaml"] {
            #expect(WorktreeChangeClassifier.verdict(for: untracked(path), context: context) == .generated(rule: "reserved_name"))
            #expect(WorktreeChangeClassifier.verdict(for: deleted(path), context: context) == .generated(rule: "reserved_name"))
        }
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".devcontainer/branchbox.env"), context: context) == .user)
    }

    @Test func r2ThisFeaturesSpecIsPreservedIncludingAPromotedBacklogFile() {
        let context = FakeContext()
        for state in ["in-progress", "backlog", "completed"] {
            #expect(WorktreeChangeClassifier.verdict(for: untracked("docs/features/\(state)/eta.md"), context: context)
                    == .preserved(destination: "docs/features/backlog/eta.md"))
        }
        #expect(WorktreeChangeClassifier.verdict(for: deleted("docs/features/backlog/eta.md"), context: context)
                == .preserved(destination: "docs/features/backlog/eta.md"))
        #expect(WorktreeChangeClassifier.verdict(for: modified("docs/features/in-progress/eta.md"), context: context,
                                                 completeSpec: true)
                == .preserved(destination: "docs/features/completed/eta.md"))
        // Another feature's spec, or a file beside the spec, is the user's.
        #expect(WorktreeChangeClassifier.verdict(for: untracked("docs/features/in-progress/zeta.md"), context: context) == .user)
        #expect(WorktreeChangeClassifier.verdict(for: untracked("docs/features/in-progress/eta-notes.md"), context: context) == .user)
    }

    @Test func r3DevcontainerFileMatchingItsSyncBaselineIsGenerated() {
        let json = "{\n  \"name\": \"demo\"\n}\n"
        var context = FakeContext(worktree: [".devcontainer/devcontainer.json": .file(json)],
                                  baseline: ["devcontainer.json": "533cf7a759345aaf"])
        #expect(WorktreeChangeClassifier.verdict(for: modified(".devcontainer/devcontainer.json"), context: context)
                == .generated(rule: "devcontainer_baseline"))
        context.worktree[".devcontainer/devcontainer.json"] = .file(json + "// edited\n")
        #expect(WorktreeChangeClassifier.verdict(for: modified(".devcontainer/devcontainer.json"), context: context) == .user)
        context.baseline = nil
        context.worktree[".devcontainer/devcontainer.json"] = .file(json)
        #expect(WorktreeChangeClassifier.verdict(for: modified(".devcontainer/devcontainer.json"), context: context) == .user)
    }

    @Test func r4ByteIdenticalCopyOrSameSymlinkTargetIsDerivedFromMain() {
        var context = FakeContext(worktree: ["config/master.key": .file("k3y"), "bin/tool": .link("../vendor/tool"),
                                             "../outside": .file("x")],
                                  main: ["config/master.key": .file("k3y"), "bin/tool": .link("../vendor/tool"),
                                         "../outside": .file("x")])
        #expect(WorktreeChangeClassifier.verdict(for: untracked("config/master.key"), context: context)
                == .generated(rule: "derived_from_main"))
        #expect(WorktreeChangeClassifier.verdict(for: untracked("bin/tool"), context: context)
                == .generated(rule: "derived_from_main"))
        #expect(WorktreeChangeClassifier.verdict(for: untracked("../outside"), context: context) == .user)
        context.worktree["config/master.key"] = .file("k3Y")
        #expect(WorktreeChangeClassifier.verdict(for: untracked("config/master.key"), context: context) == .user)
        // A link and a file are never the same thing, and a deleted file is a user change.
        context.main["bin/tool"] = .file("../vendor/tool")
        #expect(WorktreeChangeClassifier.verdict(for: untracked("bin/tool"), context: context) == .user)
        #expect(WorktreeChangeClassifier.verdict(for: deleted("config/master.key"), context: context) == .user)
    }

    @Test func r5DevcontainerEnvLinkOrCopyOfTheWorktreeEnvIsGenerated() {
        var context = FakeContext(worktree: [".devcontainer/.env": .link("../.env")])
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".devcontainer/.env"), context: context)
                == .generated(rule: "devcontainer_env_link"))
        context.worktree[".devcontainer/.env"] = .link("/etc/passwd")
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".devcontainer/.env"), context: context) == .user)
        context.worktree = [".devcontainer/.env": .file("A=1\n"), ".env": .file("A=1\n")]
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".devcontainer/.env"), context: context)
                == .generated(rule: "devcontainer_env_link"))
        context.worktree[".devcontainer/.env"] = .file("A=2\n")
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".devcontainer/.env"), context: context) == .user)
    }

    @Test func r6EnvWithOnlyTheManagedFeatureBlockIsGenerated() {
        // core writes main's .env trimmed of trailing whitespace, then the block.
        var context = FakeContext(worktree: [".env": .file(mainEnv + featureBlock)], main: [".env": .file(mainEnv + "\n\n")])
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".env"), context: context)
                == .generated(rule: "env_feature_block"))
        // An extra key in the block, an edited base, a comment of the user's, or no block at all: the user's.
        context.worktree[".env"] = .file(mainEnv + featureBlock + "MY_TOKEN=secret\n")
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".env"), context: context) == .user)
        context.worktree[".env"] = .file(mainEnv.replacingOccurrences(of: "abc", with: "xyz") + featureBlock)
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".env"), context: context) == .user)
        context.worktree[".env"] = .file(mainEnv + featureBlock + "# my note\n")
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".env"), context: context) == .user)
        context.worktree[".env"] = .file(mainEnv)
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".env"), context: context) == .user)
        context.worktree[".env"] = .file(mainEnv + featureBlock)
        context.main = [:]
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".env"), context: context) == .user)
    }

    @Test func r7VSCodeSettingsWithOnlyManagedKeysAreGenerated() {
        var context = FakeContext(worktree: [".vscode/settings.json": .file(managedSettings)])
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".vscode/settings.json"), context: context)
                == .generated(rule: "vscode_managed_keys"))
        // Tracked: compared with the committed version, managed keys removed on both sides.
        context.committed[".vscode/settings.json"] = ##"{"editor.tabSize": 2, "peacock.color": "#000000"}"##
        context.worktree[".vscode/settings.json"] = .file(##"{"editor.tabSize": 2, "peacock.color": "#e67e22", "window.title": "t"}"##)
        #expect(WorktreeChangeClassifier.verdict(for: modified(".vscode/settings.json"), context: context)
                == .generated(rule: "vscode_managed_keys"))
        context.worktree[".vscode/settings.json"] = .file(##"{"editor.tabSize": 4, "peacock.color": "#e67e22"}"##)
        #expect(WorktreeChangeClassifier.verdict(for: modified(".vscode/settings.json"), context: context) == .user)
        // Unreadable JSON (comments) is never assumed generated.
        context.worktree[".vscode/settings.json"] = .file("// comment\n{}")
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".vscode/settings.json"), context: context) == .user)
    }

    @Test func r7VSCodeTasksWithOnlyTheFeatureURLTaskAreGenerated() {
        var context = FakeContext(worktree: [".vscode/tasks.json": .file(#"{"version":"2.0.0","tasks":[{"label":"Open Feature URL"}]}"#)])
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".vscode/tasks.json"), context: context)
                == .generated(rule: "vscode_managed_tasks"))
        context.worktree[".vscode/tasks.json"] = .file(#"{"tasks":[{"label":"Open Feature URL"},{"label":"Test"}]}"#)
        #expect(WorktreeChangeClassifier.verdict(for: untracked(".vscode/tasks.json"), context: context) == .user)
    }

    @Test func anythingElseIsAUserChangeWithKindAndArea() {
        let result = WorktreeChangeClassifier.classify([
            untracked("notes.txt"), modified("README.md"), deleted("src/app.rb"),
            GitStatusEntry(index: "A", worktree: " ", path: "docker-compose.yml"),
            GitStatusEntry(index: "U", worktree: "U", path: "docs/features/backlog/x.md"),
            GitStatusEntry(index: "M", worktree: " ", path: ".devcontainer/Dockerfile"),
            GitStatusEntry(index: " ", worktree: "T", path: ".env.local"),
        ], context: FakeContext())
        #expect(result.user == [
            ChangedFile(path: "notes.txt", kind: "untracked", area: "other"),
            ChangedFile(path: "README.md", kind: "modified", area: "other"),
            ChangedFile(path: "src/app.rb", kind: "deleted", area: "other"),
            ChangedFile(path: "docker-compose.yml", kind: "added", area: "compose"),
            ChangedFile(path: "docs/features/backlog/x.md", kind: "conflicted", area: "spec"),
            ChangedFile(path: ".devcontainer/Dockerfile", kind: "staged", area: "devcontainer"),
            ChangedFile(path: ".env.local", kind: "typechange", area: "env"),
        ])
        #expect(result.generated.isEmpty && result.preserved.isEmpty && !result.truncated)
    }

    @Test func entriesPastTheLimitAreUserChangesAndTruncate() {
        let entries = (0..<2003).map { untracked(".devcontainer/.branchbox.env\($0 == 0 ? "" : "-\($0)")") }
        var reserved = entries
        reserved[2001] = untracked(".devcontainer/.branchbox.env")
        let result = WorktreeChangeClassifier.classify(reserved, context: FakeContext())
        #expect(result.truncated)
        #expect(result.generated.count == 1)
        #expect(result.user.count == 2002)
        #expect(result.user.contains(ChangedFile(path: ".devcontainer/.branchbox.env", kind: "untracked", area: "devcontainer")))
    }

    // MARK: - On disk

    /// A worktree laid out like one 0.13.4 created, plus the acceptance criteria's user changes.
    @Test func realWorktreeSplitsGeneratedPreservedAndUserChanges() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        try sandbox.write(".vscode/settings.json", "{\n  \"editor.tabSize\": 2\n}\n", in: sandbox.main)
        try sandbox.write(".devcontainer/devcontainer.json", "{\n  \"name\": \"demo\"\n}\n", in: sandbox.main)
        try await sandbox.commitAll("tracked settings and devcontainer", in: sandbox.main)
        try sandbox.write(".env", mainEnv, in: sandbox.main)
        try sandbox.write("config/master.key", "k3y\n", in: sandbox.main)

        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        // What `feature start` writes.
        try sandbox.write(".devcontainer/.branchbox.env", "WORK_FEATURE=eta\n", in: eta)
        try sandbox.write("docs/features/in-progress/eta.md", "# eta\n", in: eta)
        try sandbox.write(".env", mainEnv + featureBlock, in: eta)
        try sandbox.symlink(".devcontainer/.env", to: "../.env", in: eta)
        try sandbox.write(".vscode/settings.json", managedSettings.replacingOccurrences(of: "{\"peacock.color\"",
                                                                                         with: "{\"editor.tabSize\": 2, \"peacock.color\""),
                          in: eta)
        try sandbox.write(".vscode/tasks.json", #"{"version": "2.0.0", "tasks": [{"label": "Open Feature URL"}]}"#, in: eta)
        try sandbox.write(".devcontainer/devcontainer.json", "{\n  \"name\": \"demo-eta\"\n}\n", in: eta)
        try sandbox.write(".branchbox/devcontainer-sync/eta.json",
                          #"{"devcontainer.json": "\#(WorktreeChangeClassifier.digest(Data("{\n  \"name\": \"demo-eta\"\n}\n".utf8)))"}"#,
                          in: sandbox.main)
        try sandbox.write("config/master.key", "k3y\n", in: eta)
        // The user's own work.
        try sandbox.write("notes.txt", "work\n", in: eta)
        try sandbox.write("README.md", "hello, edited\n", in: eta)

        let git = sandbox.inspector()
        let entries = try await git.status(of: eta)
        let committed = [".vscode/settings.json": try await git.contents(of: ".vscode/settings.json", in: eta) ?? Data()]
        let result = WorktreeChangeClassifier.classify(entries, context: FileSystemChangeContext(worktree: eta, main: sandbox.main,
                                                                                                committed: committed))
        #expect(Set(result.user.map(\.path)) == ["notes.txt", "README.md"])
        #expect(Set(result.generated.map { "\($0.path)=\($0.rule)" }) == [
            ".devcontainer/.branchbox.env=reserved_name", ".devcontainer/devcontainer.json=devcontainer_baseline",
            ".env=env_feature_block", ".devcontainer/.env=devcontainer_env_link", ".vscode/settings.json=vscode_managed_keys",
            ".vscode/tasks.json=vscode_managed_tasks", "config/master.key=derived_from_main",
        ])
        #expect(result.preserved == [PreservedFile(path: "docs/features/in-progress/eta.md",
                                                   destination: "docs/features/backlog/eta.md")])
    }

    /// The acceptance criteria's negative cases: an edited tracked `.vscode/settings.json` and an untracked `.env`
    /// with an extra key are user changes.
    @Test func editedTrackedVSCodeSettingsAndEnvWithAnExtraKeyAreUserChanges() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        try sandbox.write(".vscode/settings.json", "{\n  \"editor.tabSize\": 2\n}\n", in: sandbox.main)
        try await sandbox.commitAll("settings", in: sandbox.main)
        try sandbox.write(".env", mainEnv, in: sandbox.main)
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        try sandbox.write(".vscode/settings.json", ##"{"editor.tabSize": 8, "peacock.color": "#e67e22"}"##, in: eta)
        try sandbox.write(".env", mainEnv + featureBlock + "STRIPE_KEY=sk_test_123\n", in: eta)

        let git = sandbox.inspector()
        let entries = try await git.status(of: eta)
        let committed = [".vscode/settings.json": try await git.contents(of: ".vscode/settings.json", in: eta) ?? Data()]
        let result = WorktreeChangeClassifier.classify(entries, context: FileSystemChangeContext(worktree: eta, main: sandbox.main,
                                                                                                committed: committed))
        #expect(result.user == [ChangedFile(path: ".vscode/settings.json", kind: "modified", area: "vscode"),
                                ChangedFile(path: ".env", kind: "untracked", area: "env")])
        #expect(result.generated.isEmpty)
    }

    @Test func fileSystemContextNeverFollowsSymbolicLinks() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let outside = sandbox.path("outside")
        try sandbox.write("secret.txt", "s\n", in: outside)
        try sandbox.symlink("linked", to: outside, in: sandbox.main)
        try sandbox.symlink("file-link", to: Paths.join(outside, "secret.txt"), in: sandbox.main)
        let context = FileSystemChangeContext(worktree: sandbox.main, main: sandbox.main)

        #expect(context.item("linked/secret.txt", in: .worktree) == .unsafe)
        #expect(context.item("file-link", in: .worktree) == .symbolicLink(target: Paths.join(outside, "secret.txt")))
        #expect(context.contents("file-link", in: .worktree, limit: 100) == nil)
        #expect(context.item("../outside/secret.txt", in: .worktree) == .unsafe)
        #expect(context.item("README.md", in: .worktree) == .file(size: 6))
        #expect(context.contents("README.md", in: .worktree, limit: 3) == nil)
        #expect(context.item("missing/file", in: .main) == .missing)
        #expect(context.devcontainerBaseline() == nil)
    }
}
