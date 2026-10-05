import AppKit
import SwiftUI

/// Renders SwiftUI views to PNG files without ever showing a window, so screens can be reviewed
/// visually from `swift test`. Rendering only happens when BRANCHBOX_RENDER_DIR is set.
///
/// Limitation: vibrancy-backed regions (the NavigationSplitView sidebar column, source lists)
/// render blank offscreen. Render their rows in a plain stack instead of a List.
@MainActor
enum SnapshotRenderer {
    enum Appearance: String, CaseIterable, Sendable { case light, dark }

    enum RenderError: Error { case noBitmap(String), encodeFailed(String) }

    nonisolated static var outputDirectory: URL? {
        ProcessInfo.processInfo.environment["BRANCHBOX_RENDER_DIR"].map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    nonisolated static var isEnabled: Bool { outputDirectory != nil }

    /// Writes `<name>-light.png` and `<name>-dark.png` (points are rendered at the screen's scale).
    static func render<V: View>(
        _ name: String,
        size: CGSize,
        appearances: [Appearance] = Appearance.allCases,
        @ViewBuilder _ view: () -> V
    ) throws {
        guard let directory = outputDirectory else { return }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        _ = NSApplication.shared
        let content = view()
        for appearance in appearances {
            let root = content
                .frame(width: size.width, height: size.height, alignment: .topLeading)
                .background(Color(nsColor: .windowBackgroundColor))
            let hosting = NSHostingView(rootView: root)
            let window = NSWindow(
                contentRect: CGRect(origin: .zero, size: size),
                styleMask: [.titled],
                backing: .buffered,
                defer: false
            )
            window.isReleasedWhenClosed = false
            window.appearance = NSAppearance(named: appearance == .dark ? .darkAqua : .aqua)
            window.contentView = hosting
            hosting.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.35))
            guard let bitmap = hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds) else {
                throw RenderError.noBitmap(name)
            }
            hosting.cacheDisplay(in: hosting.bounds, to: bitmap)
            guard let png = bitmap.representation(using: .png, properties: [:]) else {
                throw RenderError.encodeFailed(name)
            }
            try png.write(to: directory.appendingPathComponent("\(name)-\(appearance.rawValue).png"))
            window.close()
        }
    }
}
