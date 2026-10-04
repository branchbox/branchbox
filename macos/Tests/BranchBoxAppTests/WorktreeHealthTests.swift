@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxStores
import Testing

@MainActor @Suite struct WorktreeHealthTests {
    @Test func brokenGitNeedsAttentionAndBlocksGitOperationsWhileFilesCanStillBeOpened() {
        let record = FeatureRecord(workFeature: "eta", branchName: "feature/eta", worktreePath: "/r/eta",
                                   worktreeIssue: "The .git pointer refers to missing metadata.")
        let availability = FeatureActionAvailability(record: record, folderExists: true, backendReady: true,
                                                     preferences: LaunchPreferences())
        #expect(Remediation.attention(for: record, folderExists: true) == .worktreeInvalid)
        #expect(!availability.runCommand.isEnabled)
        #expect(!availability.teardown.isEnabled)
        #expect(!availability.devcontainer.isEnabled)
        #expect(availability.reveal.isEnabled)
        #expect(availability.editor(.vscode).isEnabled)
        #expect(availability.tunnel.isEnabled, "an existing public tunnel can still be removed")
        #expect(!availability.tunnelProvision.isEnabled)
        var commands = CommandContext()
        commands.feature = record
        commands.backendReady = true
        for command in [AppCommand.runCommand, .startDevContainer, .stopDevContainer, .tearDown, .shareViaTunnel] {
            #expect(!commands.state(command).isEnabled)
        }
        #expect(commands.state(.openInEditor).isEnabled)
        #expect(commands.state(.reveal).isEnabled)
        #expect(SyncSheetModel.applyBlockedReason(features: [record]) == "eta: The .git pointer refers to missing metadata.")
        let removed = FeatureRecord(workFeature: "gone", status: .removed, worktreeIssue: "Old metadata is gone.")
        #expect(SyncSheetModel.applyBlockedReason(features: [removed]) == nil)
        #expect(HealthCallout.isShown(for: record, folderExists: true))
        #expect(RemediationPresenter.explanation(for: record, folderExists: true)?.contains("files remain on disk") == true)
        #expect(AttentionReason.worktreeInvalid.label == "Git worktree broken")
        let actions = Remediation.actions(for: record, project: flowSampleProject, identity: nil, folderExists: true)
        #expect(actions == [.copyCommand("git -C /r/eta rev-parse --git-dir", label: "Copy Git Check Command")])
    }

    @Test func aReadyRecordedAgentPlanDoesNotClaimAnAgentWasStarted() {
        let plan = DefaultAgentPlan(status: .ready, label: "claude", command: "claude")
        let off = AgentPromptCard.summary(agent: "Claude", plan: plan, autoLaunchConfigured: false)
        #expect(off.contains("Automatic launch is off"))
        #expect(!off.contains("starts automatically"))
        #expect(!AgentPromptCard.badgeHelp(plan).contains("when the feature starts"))
        let on = AgentPromptCard.summary(agent: "Claude", plan: plan, autoLaunchConfigured: true)
        #expect(on.contains("after a successful start is on"))
        #expect(!on.contains("in the dev container"), "app auto-launch opens a host terminal")
    }

    @Test func historicalWaitingPlanDoesNotRecommendRepeatingACompletedSync() {
        let plan = DefaultAgentPlan(status: .waiting, detail: "Devcontainer skipped (minimal mode); run branchbox devcontainer sync first")
        let summary = AgentPromptCard.summary(agent: "Claude", plan: plan)
        #expect(summary.contains("recorded setup plan"))
        #expect(summary.contains("check Environment for its current state"))
        #expect(!summary.contains("run branchbox devcontainer sync first"))
    }

    @MainActor @Test func pruneNeverSelectsAKnownBrokenGitWorktreeEvenWithACleanPlan() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        let store = try await harness.start()
        var record = try #require(store.features.first { $0.workFeature == "prine" })
        record.worktreeIssue = "Git metadata is missing."
        await harness.backend.setListing(FeatureListing(features: [record]), for: flowSampleProject)
        await store.refresh(.manual)
        let flow = PruneFlow(model: harness.model, project: flowSampleProject)
        await flow.loadPlans()
        #expect(flow.selected.isEmpty)
        #expect(flow.unselectableReason("prine")?.contains("Git worktree needs repair") == true)
        flow.selectAll()
        #expect(flow.selected.isEmpty)
        #expect(!flow.canPrune)
    }
}
