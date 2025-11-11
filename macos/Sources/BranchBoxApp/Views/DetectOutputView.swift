import SwiftUI
#if os(macOS)
import AppKit
#endif

struct DetectOutputView: View {
    let output: String
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Project detection").font(.headline)
                Spacer()
                Button("Close") { onClose() }
            }
            ScrollView {
                Text(output.isEmpty ? "No output" : output)
                    .font(.system(.body, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 220, maxHeight: 320)
            HStack {
                Spacer()
                Button {
                    #if os(macOS)
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(output, forType: .string)
                    #endif
                } label: {
                    Label("Copy output", systemImage: "doc.on.doc")
                }
            }
        }
        .padding()
        .frame(width: 520)
    }
}
