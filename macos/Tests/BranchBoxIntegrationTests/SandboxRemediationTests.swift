import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// The sbx failed_retained remediation (VER-1 brief item 5), Docker-free through `FakeSandbox`: a start with
// "keep the sandbox on failure" whose dev container fails leaves a failed_retained feature; the remediation offers
// Retry (`start --reuse-runtime`) and Copy Inspect Command (`sbx exec <id> bash`, copied, never run); the retry then
// brings the feature up in the retained sandbox. Real Docker Sandboxes are not used.

extension RealCLI {
    @Suite struct SandboxRemediationTests {
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func failedRetainedSandboxOffersRetryAndCopyCommandAndRetrySucceeds() async throws {
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            try repo.write("{\n  \"name\": \"it\",\n  \"image\": \"alpine:3.19\"\n}\n", to: ".devcontainer/devcontainer.json")
            try await repo.commitAll(message: "Add devcontainer")
            let tools = repo.container.appendingPathComponent("tools", isDirectory: true)
            let sbx = try FakeSandbox.install(in: tools)
            FakeSandbox.setFailingUp(true, in: tools)
            let cli = try await LiveCLI.make(extraEnvironment: ["BRANCHBOX_SBX_PATH": sbx.path])

            var request = LiveCLI.minimalStart("kept", in: repo.project, runtime: .sbx)
            request.keepRuntimeOnFailure = true
            do {
                _ = try await cli.backend.startFeature(request, progress: { _ in })
                Issue.record("the start succeeded although devcontainer up failed (\(cli.mode))")
                return
            } catch let error as BackendError {
                print("SandboxRemediationTests (\(cli.mode)): the failing start reported \(error)")
            }

            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let record = try #require(listing.features.first { $0.workFeature == "kept" }, "\(listing.features)")
            #expect(record.status == .failedRetained)
            #expect(record.runtime.provider == .sbx)
            let runtimeID = try #require(record.runtime.runtimeID)
            #expect(Remediation.attention(for: record, folderExists: true) == .failedRetained)

            let actions = Remediation.actions(for: record, project: repo.project, identity: cli.identity, folderExists: true)
            let retry = try #require(actions.lazy.compactMap { action -> StartFeatureRequest? in
                if case .retryRetainedRuntime(let request) = action { return request }
                return nil
            }.first, "no Retry in \(actions)")
            #expect(retry.reuse == .retainedRuntime && retry.runtime == .sbx)
            let copied = actions.compactMap { action -> String? in
                if case .copyCommand(let command, _) = action { return command }
                return nil
            }
            #expect(copied == [HostLaunchPlan.sandboxShellCommand(runtimeID: runtimeID)], "\(actions)")
            #expect(copied.first?.contains("sbx exec") == true)

            FakeSandbox.setFailingUp(false, in: tools)
            let summary = try await cli.backend.startFeature(retry, progress: { _ in })
            #expect(summary.workFeature == "kept")
            let after = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            let healthy = try #require(after.features.first { $0.workFeature == "kept" })
            #expect(healthy.status == .active)
            #expect(healthy.runtime.runtimeID == runtimeID, "the retry reuses the retained sandbox")

            let teardown = TeardownRequest(feature: FeatureRef(project: repo.project, name: "kept"),
                                           recordedBranch: healthy.branchName, branch: .deleteIfMerged)
            #expect(try await cli.backend.teardownFeature(teardown, progress: { _ in }).worktreeGone)
        }
    }
}
