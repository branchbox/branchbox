import AppKit
import BranchBoxCLI
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation

/// The one place that builds the app's object graph (D-5): settings, the backend bootstrapper, the notifier and
/// the `AppModel`. Nothing else constructs `CLIBackendBootstrapper` or `UserNotificationNotifier`.
///
/// DEBUG builds can run fully on `PreviewBackend` for screenshots and UI work:
/// `BRANCHBOX_BACKEND=preview BRANCHBOX_PREVIEW_SCENARIO=<name>` (default `contract`, names from
/// `PreviewScenario.all`). Preview runs keep their projects and logs in a temporary folder of their own and add
/// the sample project, so they never touch the dev build's project list.
@MainActor enum CompositionRoot {
    /// The process-wide model, built on first use by the `App` and shared with the `AppDelegate`.
    static let model: AppModel = makeModel(environment: ProcessInfo.processInfo.environment)

    /// What one launch runs on.
    enum Backend: Equatable {
        case cli
        case preview(PreviewScenario)
    }

    /// The backend `environment` asks for. Preview needs a DEBUG build; release builds always use the CLI.
    nonisolated static func backend(for environment: [String: String]) -> Backend {
        #if DEBUG
        if environment["BRANCHBOX_BACKEND"] == "preview" {
            let scenario = environment["BRANCHBOX_PREVIEW_SCENARIO"].flatMap(PreviewScenario.named) ?? .contract
            return .preview(scenario)
        }
        #endif
        return .cli
    }

    static func makeBootstrapper(environment: [String: String]) -> any BackendBootstrapping {
        switch backend(for: environment) {
        case .cli: CLIBackendBootstrapper()
        case .preview(let scenario): PreviewBootstrapper(scenario: scenario)
        }
    }

    /// `UserNotificationNotifier` only inside a real `.app` bundle (`UNUserNotificationCenter` traps elsewhere,
    /// xctest included); `swift run` and tests get `NoopNotifier`.
    static func makeNotifier(bundle: Bundle = .main) -> any Notifier {
        guard AppBundle.isBundledApp(bundle) else { return NoopNotifier() }
        return UserNotificationNotifier(handler: NotificationRouter.handle)
    }

    static func makeModel(environment: [String: String]) -> AppModel {
        let settings = AppSettings(defaults: AppSettings.defaultsForCurrentProcess())
        let bootstrapper = makeBootstrapper(environment: environment)
        let notifier = makeNotifier()
        let model: AppModel
        switch backend(for: environment) {
        case .cli:
            model = AppModel(settings: settings, bootstrapper: bootstrapper, notifier: notifier)
        case .preview(let scenario):
            model = AppModel(settings: settings, bootstrapper: bootstrapper, notifier: notifier,
                             configuration: .isolated(in: previewDirectory(for: scenario)))
        }
        // Completion notifications wait until the user is not looking: the app is inactive, or the main window
        // is closed or minimized (§11).
        model.actions.isAppActive = { [weak model] in
            (model?.isAppActive ?? false) && WindowOpener.shared.isMainWindowVisible
        }
        return model
    }

    /// Starts the model once, from `applicationDidFinishLaunching` (the main window may never appear, e.g. a
    /// login item or `open -g` whose window was closed last time). A preview run with no projects yet gets the
    /// sample project, so every scenario opens on something to look at.
    static func start(_ model: AppModel, environment: [String: String]) async {
        await model.start()
        guard case .preview = backend(for: environment), model.projects.projects.isEmpty,
              model.environment.identity != nil else { return }
        _ = await model.projects.add(folder: PreviewSamples.project.root)
    }

    /// A fresh folder per preview scenario under the temporary directory.
    nonisolated static func previewDirectory(for scenario: PreviewScenario) -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("BranchBox Preview", isDirectory: true)
            .appendingPathComponent(scenario.name, isDirectory: true)
    }

    /// "DEV" for anything but the released bundle, "PREVIEW · <scenario>" on the preview backend; nil for the
    /// released app. Shown in the main window's toolbar (§11).
    nonisolated static func buildBadge(environment: [String: String], bundle: Bundle = .main) -> String? {
        if case .preview(let scenario) = backend(for: environment) { return "PREVIEW · \(scenario.name)" }
        let released = AppBundle.isBundledApp(bundle) && bundle.bundleIdentifier == "dev.branchbox.app"
        return released ? nil : "DEV"
    }
}
