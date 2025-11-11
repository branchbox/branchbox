#if canImport(SwiftUI)
import SwiftUI
#if os(macOS)
import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Bring the app to the foreground when launched via `swift run`.
        NSApp.activate(ignoringOtherApps: true)
    }
}
#endif

@main
struct BranchBoxMacApp: App {
    @StateObject private var viewModel = FeatureListViewModel()
    #if os(macOS)
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    #endif

    var body: some Scene {
        WindowGroup {
            FeatureListView()
                .environmentObject(viewModel)
        }
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Refresh Features") {
                    viewModel.refresh()
                }
                .keyboardShortcut("r", modifiers: [.command])
            }
        }
    }
}
#else
@main
struct BranchBoxMacApp {
    static func main() {
        fatalError("BranchBoxApp requires macOS 13+ and SwiftUI")
    }
}
#endif
