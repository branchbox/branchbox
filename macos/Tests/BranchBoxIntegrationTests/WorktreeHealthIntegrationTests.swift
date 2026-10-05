import BranchBoxKit
import Foundation
import Testing

extension RealCLI {
    @Suite struct WorktreeHealthIntegrationTests {
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func aValidLinkedWorktreeOnAnUnbornOrphanBranchDoesNotNeedRepair() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            _ = try await cli.backend.startFeature(LiveCLI.minimalStart("orphan", in: repo.project), progress: { _ in })
            let worktree = repo.worktree("orphan")
            try await repo.git(["-C", worktree.path, "switch", "--orphan", "review-orphan"])
            try await repo.git(["-C", worktree.path, "status", "--porcelain"])
            #expect(try await repo.git(["worktree", "list", "--porcelain"]).contains("HEAD " + String(repeating: "0", count: 40)))
            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let record = try #require(listing.features.first { $0.workFeature == "orphan" })
            #expect(record.worktreeIssue == nil)
        }

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)), arguments: [false, true])
        func missingGitMetadataKeepsFilesAndStatusButAddsLocalAttentionEvidence(removeCommonPointer: Bool) async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            _ = try await cli.backend.startFeature(LiveCLI.minimalStart("broken", in: repo.project), progress: { _ in })
            let worktree = repo.worktree("broken")
            let note = worktree.appendingPathComponent("owner-notes.txt")
            try Data("keep my work\n".utf8).write(to: note)
            let pointerFile = worktree.appendingPathComponent(".git")
            let pointer = try String(contentsOf: pointerFile, encoding: .utf8)
            let gitdir = String(pointer.dropFirst("gitdir:".count)).trimmingCharacters(in: .whitespacesAndNewlines)
            let metadata = URL(fileURLWithPath: gitdir, relativeTo: worktree).standardizedFileURL
            if removeCommonPointer {
                try FileManager.default.removeItem(at: metadata.appendingPathComponent("commondir"))
            } else {
                try FileManager.default.moveItem(at: metadata, to: repo.container.appendingPathComponent("saved-git-metadata"))
            }
            let registry = repo.main.appendingPathComponent(".branchbox/registry.json")
            let before = try Data(contentsOf: registry)

            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let record = try #require(listing.features.first { $0.workFeature == "broken" })
            #expect(record.status == .active, "the public registry status remains unchanged")
            #expect(record.worktreeIssue?.contains("Git metadata") == true)
            #expect(Remediation.attention(for: record, folderExists: true) == .worktreeInvalid)
            #expect(try String(contentsOf: note, encoding: .utf8) == "keep my work\n")
            #expect(try String(contentsOf: pointerFile, encoding: .utf8) == pointer)
            #expect(try Data(contentsOf: registry) == before, "the health probe does not repair or rewrite the registry")
        }
    }
}
