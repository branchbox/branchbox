@testable import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

private func record(_ name: String, branch: String, path: String?, status: FeatureStatus = .active) -> FeatureRecord {
    FeatureRecord(workFeature: name, branchName: branch, worktreePath: path, status: status)
}

@Suite struct GitInspectorTests {
    // MARK: - resolveProject

    @Test func featureWorktreeResolvesToTheMainRootSpelledAsRequested() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        let git = sandbox.inspector()

        let resolution = try await git.resolveProject(at: URL(fileURLWithPath: eta))
        #expect(resolution.project.path == sandbox.main)
        #expect(!resolution.project.path.hasPrefix("/private/"), "git's resolved /private spelling leaked")
        #expect(resolution.normalization == .fromFeatureWorktree)
        #expect(resolution.requested.path == eta)
        #expect(!resolution.initialized)

        let main = try await git.resolveProject(at: URL(fileURLWithPath: sandbox.main))
        #expect(main.project == sandbox.mainRef)
        #expect(main.normalization == .none)

        try sandbox.write("src/app.rb", "", in: sandbox.main)
        let subdirectory = try await git.resolveProject(at: URL(fileURLWithPath: Paths.join(sandbox.main, "src")))
        #expect(subdirectory.project == sandbox.mainRef)
        #expect(subdirectory.normalization == .none)

        #expect(try await git.mainRoot(of: ProjectRef(root: URL(fileURLWithPath: eta))) == sandbox.mainRef)
        #expect(try await git.mainRoot(of: sandbox.mainRef) == sandbox.mainRef)
    }

    @Test func parentContainerResolvesToItsMainFolder() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        try FileManager.default.createDirectory(atPath: Paths.join(sandbox.main, ".branchbox"), withIntermediateDirectories: true)

        let resolution = try await sandbox.inspector().resolveProject(at: sandbox.container)
        #expect(resolution.project == sandbox.mainRef)
        #expect(resolution.normalization == .fromParentContainer)
        #expect(resolution.initialized)
    }

    @Test func folderThatIsNotARepositoryIsRefusedNamingIt() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let plain = sandbox.path("plain")
        try FileManager.default.createDirectory(atPath: plain, withIntermediateDirectories: true)
        let git = sandbox.inspector()

        await #expect(throws: BackendError.projectInvalid(.notGitRepository(plain))) {
            _ = try await git.resolveProject(at: URL(fileURLWithPath: plain))
        }
        let missing = sandbox.path("missing")
        await #expect(throws: BackendError.projectInvalid(.missing(missing))) {
            _ = try await git.resolveProject(at: URL(fileURLWithPath: missing))
        }
    }

    // MARK: - Branches

    @Test func branchesListLocalRemoteAndCurrent() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        try await sandbox.addWorktree("eta", branch: "feature/eta")
        let remote = sandbox.path("remote.git")
        try await sandbox.run(["init", "-q", "--bare", remote], in: sandbox.container.path)
        try await sandbox.run(["remote", "add", "origin", remote], in: sandbox.main)
        try await sandbox.run(["push", "-q", "-u", "origin", "main"], in: sandbox.main)
        try await sandbox.run(["remote", "set-head", "origin", "main"], in: sandbox.main)
        let git = sandbox.inspector()

        let branches = try await git.branches(in: sandbox.mainRef)
        #expect(branches.current == "main")
        #expect(branches.local == ["feature/eta", "main"])
        #expect(branches.remote == ["origin/main"])
        #expect(try await git.branchExists("feature/eta", in: sandbox.main))
        #expect(try await !git.branchExists("feature/nope", in: sandbox.main))
    }

    // MARK: - Merge state (git branch -d semantics)

    @Test func branchAtMainsCommitIsMergedEvenWhileCheckedOutInAWorktree() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        try await sandbox.addWorktree("eta", branch: "feature/eta")
        // `git branch --merged` prints this branch as "+ feature/eta"; nothing here parses that list.
        let listing = try await sandbox.run(["branch", "--merged"], in: sandbox.main)
        #expect(listing.contains("+ feature/eta"))

        let state = try await sandbox.inspector().mergeState(of: "feature/eta", in: sandbox.main)
        #expect(state == BranchMergeState(branch: "feature/eta", exists: true, reference: "HEAD", referenceName: "main",
                                          merged: true, mergedIntoHead: true, ahead: 0))
    }

    @Test func branchWithACommitIsUnmergedAndAheadByOne() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        try sandbox.write("work.txt", "w\n", in: eta)
        try await sandbox.commitAll("work", in: eta)

        let state = try await sandbox.inspector().mergeState(of: "feature/eta", in: sandbox.main)
        #expect(!state.merged && !state.mergedIntoHead)
        #expect(state.ahead == 1)
        #expect(state.upstream == nil && state.reference == "HEAD")

        // Merging it into main makes it deletable.
        try await sandbox.run(["-c", "user.name=T", "-c", "user.email=t@example.com", "merge", "-q", "--ff-only",
                               "feature/eta"], in: sandbox.main)
        let merged = try await sandbox.inspector().mergeState(of: "feature/eta", in: sandbox.main)
        #expect(merged.merged && merged.ahead == 0)
    }

    @Test func upstreamPushedBranchIsMergedPerDashDButNotIntoHead() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let remote = sandbox.path("remote.git")
        try await sandbox.run(["init", "-q", "--bare", remote], in: sandbox.container.path)
        try await sandbox.run(["remote", "add", "origin", remote], in: sandbox.main)
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        try sandbox.write("work.txt", "w\n", in: eta)
        try await sandbox.commitAll("work", in: eta)
        try await sandbox.run(["push", "-q", "-u", "origin", "feature/eta"], in: eta)
        let git = sandbox.inspector()

        let state = try await git.mergeState(of: "feature/eta", in: sandbox.main)
        #expect(state.merged, "git branch -d checks the upstream, which has every commit")
        #expect(!state.mergedIntoHead)
        #expect(state.ahead == 0)
        #expect(state.upstream == "origin/feature/eta")
        #expect(state.referenceName == "origin/feature/eta")

        // A commit that is not pushed makes it unmerged again.
        try sandbox.write("more.txt", "m\n", in: eta)
        try await sandbox.commitAll("more", in: eta)
        let ahead = try await git.mergeState(of: "feature/eta", in: sandbox.main)
        #expect(!ahead.merged && ahead.ahead == 1)
    }

    @Test func missingBranchHasNothingToLose() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let state = try await sandbox.inspector().mergeState(of: "feature/gone", in: sandbox.main)
        #expect(!state.exists && state.merged && state.ahead == 0 && state.referenceName == "main")
    }

    @Test func deleteBranchRefusesUnmergedWithDashDAndForcesWithDashCapitalD() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        try sandbox.write("work.txt", "w\n", in: eta)
        try await sandbox.commitAll("work", in: eta)
        let git = sandbox.inspector()

        // Still checked out in its worktree: git's own message is the summary.
        let checkedOut = await backendError { try await git.deleteBranch("feature/eta", in: sandbox.main, force: true) }
        guard case .commandFailed(let diagnostics)? = checkedOut else {
            Issue.record("expected commandFailed, got \(String(describing: checkedOut))")
            return
        }
        #expect(diagnostics.summary.hasPrefix("error:"))
        #expect(diagnostics.summary.contains("feature/eta"))
        #expect(diagnostics.invocation?.contains("branch -D feature/eta") == true)

        try await sandbox.run(["worktree", "remove", "--force", eta], in: sandbox.main)
        let refusal = await backendError { try await git.deleteBranch("feature/eta", in: sandbox.main, force: false) }?.refusal
        #expect(refusal?.cause == .unmergedBranch(branch: "feature/eta", ahead: nil))
        #expect(refusal?.diagnostics.summary.contains("not fully merged") == true)
        try await git.deleteBranch("feature/eta", in: sandbox.main, force: true)
        #expect(try await !git.branchExists("feature/eta", in: sandbox.main))
    }

    // MARK: - Worktrees and strays

    @Test func worktreeListParsesLocksAndBranches() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        try await sandbox.run(["worktree", "lock", "--reason", "on a USB disk", eta], in: sandbox.main)

        let worktrees = try await sandbox.inspector().worktrees(in: sandbox.main)
        #expect(worktrees.count == 2)
        #expect(worktrees[0].branch == "main" && !worktrees[0].locked)
        #expect(Paths.same(worktrees[1].path, eta))
        #expect(worktrees[1].branch == "feature/eta")
        #expect(worktrees[1].locked && worktrees[1].lockReason == "on a USB disk")
    }

    /// A worktree whose `.git` link is gone, inside an enclosing repository that ignores everything: plain
    /// `git status` would report that repository's empty status, so the preflight would see nothing to lose.
    @Test func statusRefusesAWorktreeThatIsNotItsOwnWorkTree() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let eta = try await sandbox.addWorktree("eta", branch: "feature/eta")
        try sandbox.write("notes.txt", "unsaved work\n", in: eta)
        try FileManager.default.removeItem(atPath: (eta as NSString).appendingPathComponent(".git"))
        try await sandbox.run(["init", "-q", "-b", "main"], in: sandbox.container.path)
        try sandbox.write(".gitignore", "*\n", in: sandbox.container.path)

        do {
            _ = try await sandbox.inspector().status(of: eta)
            Issue.record("expected a refusal to read the enclosing repository's status")
        } catch let error as BackendError {
            let summary = error.diagnostics?.summary ?? ""
            #expect(summary.contains("is not its own git work tree"), "\(summary)")
        }
    }

    @Test func strayDetectionKeepsToBranchBoxsLayoutAndPrefixes() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let registered = try await sandbox.addWorktree("eta", branch: "feature/eta")
        let stray = try await sandbox.addWorktree("zeta", branch: "feature/zeta")
        let customPrefix = try await sandbox.addWorktree("kappa", branch: "spike/kappa")
        try await sandbox.addWorktree("lambda", branch: "experiment")                      // branch outside the layout
        try FileManager.default.createDirectory(atPath: sandbox.path("elsewhere"), withIntermediateDirectories: true)
        try await sandbox.run(["worktree", "add", "-q", "-b", "feature/mu", sandbox.path("elsewhere/mu")],
                              in: sandbox.main)                                            // folder outside the layout
        let worktrees = try await sandbox.inspector().worktrees(in: sandbox.main)
        let records = [record("eta", branch: "feature/eta", path: registered),
                       record("old", branch: "spike/old", path: sandbox.path("old"), status: .removed)]

        let strays = StrayDetector.strays(in: worktrees, mainRoot: sandbox.main, records: records, configPrefix: "feature")
            .sorted { ($0.branch ?? "") < ($1.branch ?? "") }
        try #require(strays.count == 2)
        #expect(strays.map(\.branch) == ["feature/zeta", "spike/kappa"])
        #expect(Paths.same(strays[0].path, stray))
        #expect(Paths.same(strays[1].path, customPrefix))

        // An empty prefix means branch == folder name.
        let unprefixed = [WorktreeEntry(path: "/r/main", branch: "main"), WorktreeEntry(path: "/r/nu", branch: "nu"),
                          WorktreeEntry(path: "/r/xi", branch: "feature/xi"), WorktreeEntry(path: "/r/bare", isBare: true)]
        let found = StrayDetector.strays(in: unprefixed, mainRoot: "/r/main", records: [], configPrefix: "",
                                         canonical: { $0 })
        #expect(found.map(\.path) == ["/r/nu"])
    }

    @Test func removeStrayRefusesADirtyOneUnlessDiscarding() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let zeta = try await sandbox.addWorktree("zeta", branch: "feature/zeta")
        try sandbox.write("notes.txt", "keep me\n", in: zeta)
        let backend = CLIBackend(executable: URL(fileURLWithPath: "/nonexistent/branchbox"),
                                 identity: BackendIdentity(kind: .preview, version: SemVer(0, 13, 4), contractVersion: nil,
                                                           capabilities: []),
                                 runner: ProcessRunner(), environment: StaticEnvironment(sandbox.environment),
                                 gitExecutable: GitSandbox.git)
        let stray = StrayWorktree(path: zeta, branch: "feature/zeta", head: nil)

        let refusal = await backendError {
            try await backend.removeStray(stray, in: sandbox.mainRef, discard: nil)
        }?.refusal
        #expect(refusal?.cause == .uncommittedChanges(files: [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]))
        #expect(refusal?.message.contains("notes.txt") == true)
        #expect(sandbox.exists(Paths.join(zeta, "notes.txt")))

        // A file appeared while the listed-path confirmation was open or its retry was queued.
        try sandbox.write("new-work.txt", "not confirmed\n", in: zeta)
        let changed = await backendError {
            try await backend.removeStray(stray, in: sandbox.mainRef, discard: DiscardConsent(userFiles: ["notes.txt"]))
        }?.refusal
        #expect(changed?.cause == .uncommittedChanges(files: [ChangedFile(path: "new-work.txt", kind: "untracked", area: "other")]))
        #expect(sandbox.exists(Paths.join(zeta, "notes.txt")) && sandbox.exists(Paths.join(zeta, "new-work.txt")))

        try await backend.removeStray(stray, in: sandbox.mainRef,
                                     discard: DiscardConsent(userFiles: ["notes.txt", "new-work.txt"]))
        #expect(!sandbox.exists(zeta))
        #expect(try await sandbox.inspector().worktrees(in: sandbox.main).count == 1)
    }

    @Test func removeStrayRefusesALockedWorktreeNamingTheLock() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let zeta = try await sandbox.addWorktree("zeta", branch: "feature/zeta")
        try await sandbox.run(["worktree", "lock", "--reason", "on a USB disk", zeta], in: sandbox.main)
        let git = sandbox.inspector()
        let error = await backendError { try await git.removeWorktree(zeta, in: sandbox.main, force: false) }
        #expect(error?.refusal?.cause == .worktreeLocked(reason: "on a USB disk"))
        #expect(sandbox.exists(zeta))
        try await sandbox.run(["worktree", "unlock", zeta], in: sandbox.main)
    }

    @Test func removeStrayCleansACleanOneAndOneWhoseFolderIsGone() async throws {
        let sandbox = try await GitSandbox()
        defer { sandbox.remove() }
        let clean = try await sandbox.addWorktree("clean", branch: "feature/clean")
        let gone = try await sandbox.addWorktree("gone", branch: "feature/gone")
        try FileManager.default.removeItem(atPath: gone)
        let backend = CLIBackend(executable: URL(fileURLWithPath: "/nonexistent/branchbox"),
                                 identity: BackendIdentity(kind: .preview, version: SemVer(0, 13, 4), contractVersion: nil,
                                                           capabilities: []),
                                 runner: ProcessRunner(), environment: StaticEnvironment(sandbox.environment),
                                 gitExecutable: GitSandbox.git)

        try await backend.removeStray(StrayWorktree(path: clean, branch: "feature/clean", head: nil), in: sandbox.mainRef,
                                      discard: nil)
        try await backend.removeStray(StrayWorktree(path: gone, branch: "feature/gone", head: nil, prunable: true),
                                      in: sandbox.mainRef, discard: nil)
        #expect(try await sandbox.inspector().worktrees(in: sandbox.main).count == 1)
        // Branches are left alone.
        #expect(try await sandbox.inspector().branchExists("feature/clean", in: sandbox.main))
    }

    // MARK: - Parsers

    @Test func porcelainParsersHandleEveryFieldShape() {
        let status = GitStatusParser.parse(Data("?? notes.txt\0 M README.md\0R  new.txt\0old.txt\0A  dir/a b.txt\0".utf8))
        #expect(status == [GitStatusEntry(index: "?", worktree: "?", path: "notes.txt"),
                           GitStatusEntry(index: " ", worktree: "M", path: "README.md"),
                           GitStatusEntry(index: "R", worktree: " ", path: "new.txt"),
                           GitStatusEntry(index: "A", worktree: " ", path: "dir/a b.txt")])
        #expect(status[2].kind == "staged")

        let list = WorktreeListParser.parse("""
            worktree /r/main
            HEAD abc
            branch refs/heads/main

            worktree /r/eta
            HEAD def
            detached
            locked
            prunable gitdir file points to non-existent location

            worktree /r/bare.git
            bare

            """)
        #expect(list == [WorktreeEntry(path: "/r/main", head: "abc", branch: "main"),
                         WorktreeEntry(path: "/r/eta", head: "def", isDetached: true, locked: true, prunable: true),
                         WorktreeEntry(path: "/r/bare.git", isBare: true)])
    }

    @Test func pathsAreMappedBackOntoTheRequestedSpelling() throws {
        let temporary = FileManager.default.temporaryDirectory.path          // /var/folders/…, a symlinked path
        let resolved = Paths.canonical(temporary)
        try #require(resolved != Paths.standardized(temporary), "TMPDIR is expected to go through /var → /private/var")
        #expect(Paths.unresolved(Paths.join(resolved, "x/main"), relativeTo: Paths.join(temporary, "x/eta"))
                == Paths.join(Paths.standardized(temporary), "x/main"))
        #expect(Paths.unresolved("/elsewhere/main", relativeTo: "/nonexistent/eta") == "/elsewhere/main")
    }
}
