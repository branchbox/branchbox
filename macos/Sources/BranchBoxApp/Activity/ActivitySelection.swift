import BranchBoxStores
import Foundation

/// Which operation the Activity window shows. The window has no parameters (`Window(id: "activity")`), so the
/// popover, a result card's [Show Log] and the inspector hand it the operation through this per-process value
/// before opening the window. A UI convenience only: losing it just shows the newest operation.
enum ActivitySelection {
    static let key = "activity.selectedOperation"

    /// Where the selection is kept; tests point it at a throwaway suite. Set once, before any window exists.
    /// The same domain as `AppSettings.defaultsForCurrentProcess()` (the dev suite under `swift run`).
    nonisolated(unsafe) static var defaults: UserDefaults =
        AppBundle.isBundledApp() ? .standard : UserDefaults(suiteName: "dev.branchbox.app.dev") ?? .standard

    @MainActor static func select(_ id: UUID?) {
        defaults.set(id?.uuidString, forKey: key)
    }

    @MainActor static var selected: UUID? {
        defaults.string(forKey: key).flatMap(UUID.init(uuidString:))
    }
}
