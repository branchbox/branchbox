import AppKit
import BranchBoxKit
import BranchBoxPreview
import SwiftUI
import Testing
@testable import BranchBoxApp

/// Renders the shared component kit for visual review. Run with BRANCHBOX_RENDER_DIR=<dir>.
@MainActor
@Suite(.enabled(if: SnapshotRenderer.isEnabled))
struct ComponentGalleryRenderTests {
    @Test func badgesPortsAndModules() throws {
        let features = PreviewSamples.features
        let withPorts = features.first { !$0.urls.ports.isEmpty } ?? features[0]
        try SnapshotRenderer.render("components-badges", size: CGSize(width: 760, height: 520)) {
            VStack(alignment: .leading, spacing: 14) {
                Text("Status").font(.headline)
                HStack {
                    ForEach([FeatureStatus.active, .degraded, .failedRetained, .orphaned, .removed], id: \.self) {
                        StatusBadge(status: $0)
                    }
                }
                Text("Runtime").font(.headline)
                HStack {
                    ForEach([RuntimeProvider.container, .sbx, .localVM, .inGuest], id: \.self) { RuntimeBadge(provider: $0) }
                }
                Text("Ports").font(.headline)
                PortLinks(urls: withPorts.urls)
                Text("Modules").font(.headline)
                ModuleChecklist(outcomes: features[0].moduleOutcomes)
                HStack {
                    ColorSwatch(hex: features[0].color, size: 12)
                    AttentionBadge(count: 3)
                    CopyButton(text: "feature/prine", label: "Copy branch", showsTitle: true)
                }
            }
            .padding(20)
        }
    }

    @Test func errorBanners() throws {
        let refusal = BackendError.refused(Refusal(
            cause: .uncommittedChanges(files: PreviewSamples.dirtyFiles),
            message: "Worktree 'prine' has 3 uncommitted changes",
            diagnostics: Diagnostics(summary: "teardown refused")
        ))
        try SnapshotRenderer.render("components-errors", size: CGSize(width: 760, height: 360)) {
            VStack(alignment: .leading, spacing: 16) {
                ErrorBanner(error: refusal, context: nil, onRetry: {}, onDetails: {})
                ErrorBanner(error: .cliNotFound(searched: PreviewSamples.searchedPaths), context: nil)
            }
            .padding(20)
        }
    }

    @Test func operationRows() throws {
        try SnapshotRenderer.render("components-operations", size: CGSize(width: 620, height: 420)) {
            VStack(alignment: .leading, spacing: 8) {
                OperationRow(summary: OperationPreviewData.running)
                OperationRow(summary: OperationPreviewData.queued)
                OperationRow(summary: OperationPreviewData.pruning)
                ForEach(Array(OperationPreviewData.finished.enumerated()), id: \.offset) { OperationRow(summary: $0.element) }
            }
            .padding(20)
        }
    }

    @Test func emptyStates() throws {
        try SnapshotRenderer.render("components-empty-cli-missing", size: CGSize(width: 620, height: 420)) {
            CLINotFoundView(searched: PreviewSamples.searchedPaths, onLocate: {}, onRedetect: {})
        }
        try SnapshotRenderer.render("components-empty-no-features", size: CGSize(width: 620, height: 360)) {
            NoFeaturesView(projectName: "branchbox", onStart: {})
        }
    }
}
