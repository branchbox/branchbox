import AppKit

/// Plain-text copy to a pasteboard. `.general` is the system clipboard; tests use a private named pasteboard.
@MainActor struct Pasteboard {
    let pasteboard: NSPasteboard

    static let general = Pasteboard(pasteboard: .general)

    func copy(_ text: String) {
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }

    var string: String? { pasteboard.string(forType: .string) }
}
