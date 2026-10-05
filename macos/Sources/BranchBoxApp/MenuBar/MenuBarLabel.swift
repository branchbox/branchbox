import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// What the menu bar icon says (DESIGN §9 Menu bar): idle, working (a dot), attention (a badge plus a count) or
/// blocked (the CLI is unavailable), with one spoken label.
struct MenuBarStatus: Sendable, Hashable {
    enum State: Sendable, Hashable {
        case idle
        case working
        case attention(Int)
        case blocked
    }

    let state: State
    let accessibilityLabel: String

    /// Blocked wins over attention, attention over working. `attention` counts features and strays that need
    /// attention plus finished operations that failed and haven't been looked at.
    static func make(blocked: Bool, attention: Int, running: Int) -> MenuBarStatus {
        let state: State = if blocked {
            .blocked
        } else if attention > 0 {
            .attention(attention)
        } else if running > 0 {
            .working
        } else {
            .idle
        }
        var parts = ["BranchBox"]
        if blocked { parts.append("the BranchBox CLI is unavailable") }
        if attention > 0 { parts.append(attention == 1 ? "1 item needs attention" : "\(attention) items need attention") }
        if running > 0 { parts.append(running == 1 ? "1 operation running" : "\(running) operations running") }
        return MenuBarStatus(state: state, accessibilityLabel: parts.joined(separator: ", "))
    }

    @MainActor static func make(model: AppModel) -> MenuBarStatus {
        var blocked = false
        if case .unavailable = model.environment.backendState { blocked = true }
        let failed = model.operations.records.filter(\.needsAttention).count
        return make(blocked: blocked, attention: model.projects.attentionCount + failed,
                    running: model.operations.running.count)
    }
}

/// The menu bar icon. It is always alive while the extra is inserted, so it also lends its `openWindow` to
/// `WindowOpener` (Dock reopen, notification clicks).
struct MenuBarLabel: View {
    @Environment(AppModel.self) private var model
    @AppStorage(GeneralTab.menuBarShowsCountKey, store: AppSettings.defaultsForCurrentProcess()) private var showsCount = true

    var body: some View {
        let status = MenuBarStatus.make(model: model)
        MenuBarIcon(status: status, showsCount: showsCount)
            .registersWindowOpener()
    }
}

/// The icon for a status: the template image, plus the count when something needs attention.
struct MenuBarIcon: View {
    let status: MenuBarStatus
    /// Settings › General › Show the attention count (the badge in the image stays either way).
    var showsCount = true

    var body: some View {
        HStack(spacing: 2) {
            Image(nsImage: MenuBarImage.image(for: status.state))
            if showsCount, case .attention(let count) = status.state {
                Text("\(count)")
                    .monospacedDigit()
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(status.accessibilityLabel)
    }
}

/// Draws the `shippingbox` symbol with a dot (working) or an exclamation badge (attention, blocked) cut into its
/// corner, as a template image so the menu bar tints it for light, dark and highlighted states.
@MainActor enum MenuBarImage {
    static let size = NSSize(width: 20, height: 16)

    static func image(for state: MenuBarStatus.State) -> NSImage {
        let configuration = NSImage.SymbolConfiguration(pointSize: 14, weight: .regular)
        let box = NSImage(systemSymbolName: "shippingbox", accessibilityDescription: "BranchBox")?
            .withSymbolConfiguration(configuration)
        let badge: NSImage? = switch state {
        case .idle, .working: nil
        case .attention: NSImage(systemSymbolName: "exclamationmark.circle.fill", accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 8, weight: .bold))
        case .blocked: NSImage(systemSymbolName: "xmark.circle.fill", accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 8, weight: .bold))
        }
        let showsDot = state == .working
        let image = NSImage(size: size, flipped: false) { rect in
            if let box {
                let boxSize = box.size
                box.draw(in: NSRect(x: (rect.width - boxSize.width) / 2 - 1, y: (rect.height - boxSize.height) / 2,
                                    width: boxSize.width, height: boxSize.height))
            }
            let corner = NSRect(x: rect.maxX - 9, y: rect.minY, width: 9, height: 9)
            if showsDot || badge != nil {
                // Cut a ring out of the box so the mark stays legible on top of it.
                NSGraphicsContext.current?.compositingOperation = .clear
                NSBezierPath(ovalIn: corner).fill()
                NSGraphicsContext.current?.compositingOperation = .sourceOver
            }
            if showsDot {
                NSColor.black.setFill()
                NSBezierPath(ovalIn: corner.insetBy(dx: 1.5, dy: 1.5)).fill()
            } else if let badge {
                badge.draw(in: corner.insetBy(dx: 0.5, dy: 0.5))
            }
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = "BranchBox"
        return image
    }
}
