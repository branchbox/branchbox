import AppKit
import BranchBoxStores
import SwiftUI

/// Opens SwiftUI windows from AppKit code (the Dock reopen, notification clicks) and from places without an
/// `openWindow` of their own (§8.7).
///
/// SwiftUI only hands out `OpenWindowAction` inside a view, so always-alive views (the menu bar label, and the
/// main window while it exists) register theirs here. Without any, the main window is found among
/// `NSApp.windows` by its identifier prefix and brought front.
@MainActor final class WindowOpener {
    static let shared = WindowOpener()

    private var openWindow: OpenWindowAction?

    /// Remembers `action` (the latest registration wins; every registrar's action opens the same scenes).
    func register(_ action: OpenWindowAction) {
        openWindow = action
    }

    /// Opens (or brings front) the scene `id`; for the main window, falls back to the AppKit window.
    func open(_ id: String) {
        if let openWindow {
            openWindow(id: id)
        } else if id == SceneID.main {
            mainWindow?.makeKeyAndOrderFront(nil)
        }
    }

    /// Opens the main window and activates the app: the path every menu bar item that needs a sheet takes.
    func showMain(posting intent: WindowIntent? = nil, to model: AppModel) {
        open(SceneID.main)
        NSApp.activate()
        if let intent { model.post(intent) }
    }

    /// The main window's AppKit window, if SwiftUI has it.
    var mainWindow: NSWindow? {
        NSApp.windows.first { Self.isMainWindow($0) }
    }

    /// The main window is on screen and not minimized (notifications wait while it is hidden, §11).
    var isMainWindowVisible: Bool {
        guard let window = mainWindow else { return false }
        return window.isVisible && !window.isMiniaturized
    }

    static func isMainWindow(_ window: NSWindow) -> Bool {
        window.identifier?.rawValue.hasPrefix(SceneID.main) == true
    }
}

/// Registers the enclosing view's `openWindow` with `WindowOpener`.
struct RegistersWindowOpener: ViewModifier {
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        content.onAppear { WindowOpener.shared.register(openWindow) }
    }
}

extension View {
    func registersWindowOpener() -> some View {
        modifier(RegistersWindowOpener())
    }
}
