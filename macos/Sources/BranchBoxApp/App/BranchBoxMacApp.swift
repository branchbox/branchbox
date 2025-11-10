#if canImport(SwiftUI)
import SwiftUI

@main
struct BranchBoxMacApp: App {
    @StateObject private var viewModel = FeatureListViewModel()

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
