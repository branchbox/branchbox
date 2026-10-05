import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Application lifecycle (§8.7):
/// - the model starts at launch, without activating the app (`open -g` leaves it in the background);
/// - closing the last window keeps the app running in the menu bar;
/// - the Dock icon reopens the main window;
/// - quitting with operations running asks first, and stops them before replying;
/// - activation changes reach the model (refresh when stale, re-check the CLI, notification decisions).
@MainActor final class AppDelegate: NSObject, NSApplicationDelegate {
    /// The composition root's model (the same instance the scenes use).
    var model: AppModel { CompositionRoot.model }

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Settings › General › Show Dock icon (off: a menu-bar-only app). The menu bar icon can't be hidden too.
        let defaults = AppSettings.defaultsForCurrentProcess()
        if defaults.object(forKey: GeneralTab.showDockIconKey) != nil,
           !defaults.bool(forKey: GeneralTab.showDockIconKey), model.settings.showMenuBarIcon {
            NSApp.setActivationPolicy(.accessory)
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let model = self.model
        let environment = ProcessInfo.processInfo.environment
        Task { await CompositionRoot.start(model, environment: environment) }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { WindowOpener.shared.open(SceneID.main) }
        return true
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        model.appDidBecomeActive()
    }

    func applicationDidResignActive(_ notification: Notification) {
        model.appDidResignActive()
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let model = self.model
        let running = model.operations.running.count
        if running == 0 {
            // Nothing the user started is running, but a refresh may be: stop every child process before exiting.
            Task {
                await model.prepareForTermination()
                sender.reply(toApplicationShouldTerminate: true)
            }
            return .terminateLater
        }
        Task { @MainActor in
            let alert = NSAlert()
            alert.alertStyle = .warning
            alert.messageText = QuitConfirmation.title(running: running)
            alert.informativeText = QuitConfirmation.message
            // "Keep Running" is the default (Return); the destructive choice has no key equivalent and needs a click
            // (§9: Return is the default non-destructive action). Activate first: Quit often comes from the menu
            // bar while BranchBox is in the background, and the alert must not open behind other apps.
            let keep = alert.addButton(withTitle: QuitConfirmation.keepLabel)
            keep.keyEquivalent = "\r"
            let quit = alert.addButton(withTitle: QuitConfirmation.quitLabel)
            quit.hasDestructiveAction = true
            quit.keyEquivalent = ""
            NSApp.activate()
            let response = alert.runModal()
            guard response == .alertSecondButtonReturn else {
                sender.reply(toApplicationShouldTerminate: false)
                return
            }
            await model.prepareForTermination()
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}

/// The text of the quit confirmation (D-18).
enum QuitConfirmation {
    static func title(running: Int) -> String {
        running == 1 ? "1 operation is running" : "\(running) operations are running"
    }

    static let message = "Quitting stops them and may leave partial state."
    static let quitLabel = "Cancel and Quit"
    static let keepLabel = "Keep Running"
}

/// Performs what the user chose on a notification (§11): Show opens the main window on the operation; Open in
/// Editor opens the operation's feature in the preferred editor. A note that arrived while BranchBox was frontmost
/// becomes an in-window toast.
@MainActor enum NotificationRouter {
    static func handle(_ response: UserNotificationNotifier.Response) {
        let model = CompositionRoot.model
        switch response {
        case .show(let intent):
            WindowOpener.shared.showMain(posting: intent, to: model)
        case .openInEditor(let intent):
            guard let feature = feature(for: intent, in: model),
                  let record = model.projects.project(feature.project)?.feature(named: feature.name) else {
                WindowOpener.shared.showMain(posting: intent, to: model)
                return
            }
            FeatureCommands.openInEditor(record, project: feature.project, model: model)
        case .presentInApp(let title, let body):
            ToastCenter.shared.show(title: title, body: body)
        }
    }

    /// The feature an intent is about: a selected feature, or the target of the operation it shows.
    static func feature(for intent: WindowIntent, in model: AppModel) -> FeatureRef? {
        switch intent {
        case .select(.feature(let projectPath, let name)):
            return FeatureRef(project: ProjectRef(root: URL(fileURLWithPath: projectPath, isDirectory: true)), name: name)
        case .showActivity(let operation?):
            if case .feature(let ref)? = model.operations.record(operation)?.target { return ref }
            return nil
        case .teardown(let ref, _):
            return ref
        default:
            return nil
        }
    }
}
