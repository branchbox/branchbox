import SwiftUI

/// The key main window's router, published with `focusedSceneValue` so menu commands act on the window the user
/// is looking at (DESIGN §9 "Commands target the key main window via @FocusedValue").
struct MainWindowRouterKey: FocusedValueKey {
    typealias Value = PresentationRouter
}

extension FocusedValues {
    var mainWindowRouter: PresentationRouter? {
        get { self[MainWindowRouterKey.self] }
        set { self[MainWindowRouterKey.self] = newValue }
    }
}
