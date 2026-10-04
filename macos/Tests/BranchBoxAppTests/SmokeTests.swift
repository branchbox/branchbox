@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation
import Testing

// Smoke tests: the navigation contract, the screen stubs (DESIGN §4.11) and the composition root (SW-4).
// Constructing every stub with its labelled initializer makes a signature change a compile error here.

private let project = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/main"))
private let feature = FeatureRef(project: project, name: "oauth")
private let stray = StrayWorktree(path: "/tmp/bbx/spike", branch: "spike/x", head: "abc1234")

@MainActor @Suite struct AppSkeletonTests {
    @Test func sceneIDs() {
        #expect(SceneID.main == "main")
        #expect(SceneID.run == "run")
        #expect(SceneID.activity == "activity")
        #expect(SceneID.diagnostics == "diagnostics")
    }

    @Test func stubsTakeTheirContractInitializers() {
        _ = FeatureDetailView(feature: feature)
        _ = FeatureActionsMenu(feature: feature, style: .contextMenu)
        _ = FeatureActionsMenu(feature: feature, style: .menuBar)
        _ = FeatureActionsMenu(feature: feature, style: .toolbarOverflow)
        _ = RunCommandWindow(feature: nil)
        _ = StartFeatureSheet(project: project, prefill: StartFeatureRequest(project: project, name: "oauth", runtime: .sbx))
        _ = TeardownSheet(feature: feature, preselect: nil)
        _ = PruneSheet(project: project)
        _ = StrayWorktreeSheet(project: project, stray: stray)
        _ = ActivityInspector(target: .feature(feature))
        _ = ActivityWindow()
        _ = WelcomeView()
        _ = ProjectDetailView(project: project)
        _ = AddProjectSheet(initialFolder: nil)
        _ = InitProjectSheet(folder: project.root, mode: .repair)
        _ = SyncDevcontainersSheet(project: project)
        _ = ProjectSettingsSheet(project: project)
        _ = AppSettingsView()
        _ = DiagnosticsWindow()
    }

    private static let routes: [SheetRoute] = [
        .startFeature(project: nil, prefill: nil),
        .startFeature(project: project, prefill: nil),
        .teardown(feature, preselect: nil),
        .teardown(feature, preselect: .forceDelete),
        .prune(project),
        .stray(project, stray),
        .addProject(nil),
        .addProject(URL(fileURLWithPath: "/tmp/bbx/other")),
        .initProject(project.root, mode: .setUp),
        .initProject(project.root, mode: .repair),
        .projectSettings(project),
        .syncDevcontainers(project),
        .quickOpen,
    ]

    @Test func sheetRouteIDsAreDistinctAndStable() {
        let ids = Self.routes.map(\.id)
        #expect(Set(ids).count == ids.count)
        #expect(SheetRoute.prune(ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/main/"))).id == SheetRoute.prune(project).id)
    }

    @Test func everySheetIntentMapsToItsRoute() {
        let intents: [(WindowIntent, SheetRoute)] = [
            (.startFeature(project: project, prefill: nil), .startFeature(project: project, prefill: nil)),
            (.teardown(feature, preselect: .keep), .teardown(feature, preselect: .keep)),
            (.prune(project), .prune(project)),
            (.stray(project, stray), .stray(project, stray)),
            (.addProject(nil), .addProject(nil)),
            (.initProject(project.root, mode: .setUp), .initProject(project.root, mode: .setUp)),
            (.projectSettings(project), .projectSettings(project)),
            (.syncDevcontainers(project), .syncDevcontainers(project)),
            (.quickOpen, .quickOpen),
        ]
        for (intent, route) in intents {
            #expect(SheetRoute(intent) == route)
        }
        #expect(SheetRoute(.select(.welcome)) == nil)
        #expect(SheetRoute(.showActivity(operation: nil)) == nil)
        #expect(SheetRoute(.showDiagnostics) == nil)
    }

    @Test func theCLIBackendIsTheDefault() {
        // The test target doesn't link BranchBoxCLI directly; the type's name is enough.
        #expect(String(describing: type(of: BranchBoxApp.makeBootstrapper(environment: [:]))) == "CLIBackendBootstrapper")
        #expect(CompositionRoot.backend(for: [:]) == .cli)
        #expect(CompositionRoot.backend(for: ["BRANCHBOX_BACKEND": "cli"]) == .cli)
    }

    @Test func testsGetTheNoopNotifier() {
        // xctest has a bundle identifier but is not a `.app`: UNUserNotificationCenter would trap there.
        #expect(!CompositionRoot.makeNotifier().isAvailable)
        #expect(CompositionRoot.makeNotifier() is NoopNotifier)
    }

    #if DEBUG
    @Test func previewBackendIsChosenFromTheEnvironment() async {
        let preview = BranchBoxApp.makeBootstrapper(environment: ["BRANCHBOX_BACKEND": "preview"])
        #expect((preview as? PreviewBootstrapper)?.backend.scenario == .contract)

        #expect(CompositionRoot.backend(for: ["BRANCHBOX_BACKEND": "preview", "BRANCHBOX_PREVIEW_SCENARIO": "showcase"])
                == .preview(.showcase))
        #expect(CompositionRoot.backend(for: ["BRANCHBOX_BACKEND": "preview", "BRANCHBOX_PREVIEW_SCENARIO": "nope"])
                == .preview(.contract))

        let missing = BranchBoxApp.makeBootstrapper(environment: ["BRANCHBOX_BACKEND": "preview",
                                                                  "BRANCHBOX_PREVIEW_SCENARIO": "cliMissing"])
        guard case .unavailable(.cliNotFound(let searched), nil) = await missing.bootstrap(BackendSettings()) else {
            Issue.record("expected the cliMissing scenario")
            return
        }
        #expect(searched == PreviewSamples.searchedPaths)
    }
    #endif
}
