import BranchBoxPreview
import SwiftUI

/// Copies text to the clipboard, confirms with a small "Copied" HUD and announces it to VoiceOver.
struct CopyButton: View {
    private let text: () -> String
    private let label: String
    private let showsTitle: Bool
    @State private var copies = 0
    @State private var showsHUD = false

    /// `label` is the button's title or help text ("Copy Path"); `showsTitle` shows it next to the icon.
    init(text: String, label: String = "Copy", showsTitle: Bool = false) {
        self.init(label: label, showsTitle: showsTitle) { text }
    }

    /// The text is computed when the button is pressed (e.g. a diagnostic report).
    init(label: String = "Copy", showsTitle: Bool = false, text: @escaping () -> String) {
        self.text = text
        self.label = label
        self.showsTitle = showsTitle
    }

    var body: some View {
        Button {
            Pasteboard.general.copy(text())
            AccessibilityNotification.Announcement("Copied").post()
            copies += 1
        } label: {
            if showsTitle {
                Label(label, systemImage: "doc.on.doc")
            } else {
                Image(systemName: "doc.on.doc")
            }
        }
        .help(label)
        .accessibilityLabel(label)
        .overlay(alignment: .top) {
            if showsHUD {
                Text("Copied")
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(.regularMaterial, in: Capsule())
                    .fixedSize()
                    .offset(y: -26)
                    .transition(.opacity)
                    .accessibilityHidden(true)
                    .allowsHitTesting(false)
            }
        }
        .task(id: copies) {
            guard copies > 0 else { return }
            withAnimation(.easeOut(duration: 0.15)) { showsHUD = true }
            do {
                try await Task.sleep(for: .seconds(1.2))
            } catch {
                return                                   // a newer copy restarted the HUD
            }
            withAnimation(.easeIn(duration: 0.3)) { showsHUD = false }
        }
    }
}

#Preview("Copy buttons") {
    VStack(alignment: .leading, spacing: 24) {
        CopyButton(text: PreviewSamples.features[0].branchName, label: "Copy Branch")
        CopyButton(text: PreviewSamples.features[0].worktreePath ?? "", label: "Copy Path", showsTitle: true)
    }
    .padding(40)
}
