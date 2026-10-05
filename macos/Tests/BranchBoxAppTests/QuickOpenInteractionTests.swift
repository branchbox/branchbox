import AppKit
@testable import BranchBoxApp
import BranchBoxStores
import Observation
import SwiftUI
import Testing

@MainActor @Suite(.serialized)
struct QuickOpenInteractionTests {
    /// An offscreen native field retains its submit action across SwiftUI updates. Its parent must resolve
    /// the current highlight when called, so Return performs the row that is visibly selected.
    @Test func returnUsesTheHighlightAfterTheSearchFieldWasCreated() throws {
        _ = NSApplication.shared
        let state = QuickOpenTestState()
        let hosting = NSHostingView(rootView: QuickOpenTestHost(state: state))
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 500),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = hosting
        defer { window.close() }
        hosting.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        let field = try #require(textField(in: hosting))
        state.highlighted = state.items[1].id
        hosting.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        let action = try #require(field.action)
        #expect(field.sendAction(action, to: field.target))
        #expect(state.performed == state.items[1])
    }

    private func textField(in view: NSView) -> NSTextField? {
        if let field = view as? NSTextField, field.isEditable { return field }
        for subview in view.subviews {
            if let field = textField(in: subview) { return field }
        }
        return nil
    }
}

@MainActor @Observable private final class QuickOpenTestState {
    var query = ""
    var highlighted = "first"
    var performed: QuickOpenItem?
    let items: [QuickOpenItem] = [
        QuickOpenItem(id: "first", title: "first", systemImage: "shippingbox", group: .features,
                      action: .intent(.select(.feature(projectPath: "/tmp/project", name: "first")))),
        QuickOpenItem(id: "second", title: "second", systemImage: "shippingbox", group: .features,
                      action: .intent(.select(.feature(projectPath: "/tmp/project", name: "second"))))
    ]
}

private struct QuickOpenTestHost: View {
    @Bindable var state: QuickOpenTestState
    @FocusState private var focused: Bool

    var body: some View {
        QuickOpenPanel(query: $state.query, results: state.items, highlighted: state.highlighted,
                       fieldFocused: $focused,
                       onSubmit: { state.performed = state.items.first { $0.id == state.highlighted } },
                       onPerform: { state.performed = $0 })
    }
}
