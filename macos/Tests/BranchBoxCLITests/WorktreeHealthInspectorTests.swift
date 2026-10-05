@testable import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

@Suite struct WorktreeHealthInspectorTests {
    private let feature = FeatureRecord(workFeature: "eta", branchName: "feature/eta", worktreePath: "/r/eta")
    private let worktrees = [WorktreeEntry(path: "/r/main"), WorktreeEntry(path: "/r/eta", branch: "feature/eta")]

    @Test func anExistingFolderWithAMissingGitdirNeedsAttention() {
        let fs = MutableFileSystem(["/r/eta": .directory, "/r/eta/.git": .file(executable: false)])
        let issue = WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: worktrees, fileSystem: fs,
                                                   readFile: { _ in Data("gitdir: /old/main/.git/worktrees/eta\n".utf8) })
        #expect(issue == "The .git file points to Git metadata that cannot be found at /old/main/.git/worktrees/eta.")
    }

    @Test func relativeGitdirAndCommondirPointersResolveFromTheirContainingFolders() {
        let fs = MutableFileSystem(["/r/eta": .directory, "/r/eta/.git": .file(executable: false),
                                    "/r/main/.git/worktrees/eta": .directory,
                                    "/r/main/.git/worktrees/eta/commondir": .file(executable: false),
                                    "/r/main/.git": .directory])
        let read: (String) throws -> Data = { path in
            Data((path.hasSuffix("commondir") ? "../..\n" : "gitdir: ../main/.git/worktrees/eta\n").utf8)
        }
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: worktrees,
                                               fileSystem: fs, readFile: read) == nil)
        fs.set("/r/main/.git", nil)
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: worktrees,
                                               fileSystem: fs, readFile: read) == "Git's common metadata cannot be found at /r/main/.git.")
    }

    @Test func missingAndPrunableRegistrationsAreDistinguishedFromHealthyOnes() {
        let fs = MutableFileSystem(["/r/eta": .directory, "/r/eta/.git": .directory])
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: [], fileSystem: fs)
            == "Git no longer registers this folder as a worktree of /r/main.")
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main",
                                               worktrees: [WorktreeEntry(path: "/r/eta", prunable: true)], fileSystem: fs)
            == "Git marks this worktree's registration as invalid or prunable.")
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: worktrees, fileSystem: fs) == nil)
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: nil, fileSystem: fs) == nil,
                "a failed git worktree list must not mark every record broken")
    }

    @Test func missingGitPointerRemainsVisibleButAnUnbornHeadIsNotAnError() {
        let fs = MutableFileSystem(["/r/eta": .directory])
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: nil, fileSystem: fs)
            == "The worktree folder exists, but its .git metadata pointer is missing at /r/eta/.git.")
        fs.set("/r/eta/.git", .directory)
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main",
                                               worktrees: [WorktreeEntry(path: "/r/eta", head: String(repeating: "0", count: 40))],
                                               fileSystem: fs) == nil)
    }

    @Test func missingCommondirIsAnErrorOnlyForLinkedWorktreeMetadata() {
        let fs = MutableFileSystem(["/r/eta": .directory, "/r/eta/.git": .file(executable: false),
                                    "/r/metadata": .directory])
        let read: (String) throws -> Data = { _ in Data("gitdir: /r/metadata\n".utf8) }
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: worktrees,
                                               fileSystem: fs, readFile: read) == nil,
                "standalone separate Git directories do not need commondir")
        fs.set("/r/metadata/gitdir", .file(executable: false))
        #expect(WorktreeHealthInspector.issue(for: feature, in: "/r/main", worktrees: worktrees,
                                               fileSystem: fs, readFile: read)
            == "Git metadata for this linked worktree is missing its common-directory pointer at /r/metadata/commondir.")
    }

    @Test func removedAndInProgressFeaturesDoNotGetFalseBrokenGitWarnings() {
        let fs = MutableFileSystem(["/r/eta": .directory])
        for record in [FeatureRecord(workFeature: "eta", worktreePath: "/r/eta", status: .removed),
                       FeatureRecord(workFeature: "eta", worktreePath: "/r/eta", setup: SetupInfo(state: .inProgress))] {
            #expect(WorktreeHealthInspector.issue(for: record, in: "/r/main", worktrees: [], fileSystem: fs) == nil)
        }
    }
}
