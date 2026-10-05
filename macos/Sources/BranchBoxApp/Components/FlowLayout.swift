import BranchBoxPreview
import SwiftUI

/// Wraps chips and buttons onto as many lines as they need (ported from the 0.13.4 `FeatureDetailView`).
struct FlowLayout<Content: View>: View {
    var spacing: CGFloat = 8
    @ViewBuilder var content: Content

    var body: some View {
        FlowLayoutLayout(spacing: spacing) {
            content
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}

/// Places subviews left to right and starts a new line when the next one would overflow the proposed width.
struct FlowLayoutLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard !subviews.isEmpty else { return .zero }
        let maxWidth = proposal.width ?? .infinity
        var lineWidth: CGFloat = 0
        var lineHeight: CGFloat = 0
        var totalHeight: CGFloat = 0
        var measuredWidth: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if maxWidth.isFinite && lineWidth > 0 && lineWidth + spacing + size.width > maxWidth {
                totalHeight += lineHeight + spacing
                lineWidth = 0
                lineHeight = 0
            }
            if lineWidth > 0 {
                lineWidth += spacing
            }
            lineWidth += size.width
            lineHeight = max(lineHeight, size.height)
            measuredWidth = max(measuredWidth, lineWidth)
        }

        totalHeight += lineHeight
        let finalWidth = proposal.width ?? measuredWidth
        return CGSize(width: finalWidth, height: totalHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard !subviews.isEmpty else { return }
        var x = bounds.minX
        var y = bounds.minY
        var lineHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                x = bounds.minX
                y += lineHeight + spacing
                lineHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: size.width, height: size.height))
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
        }
    }
}

#Preview("Flow of chips") {
    FlowLayout(spacing: 6) {
        ForEach(PreviewSamples.features) { feature in
            Text(feature.workFeature)
                .font(.caption)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(.quaternary, in: Capsule())
        }
    }
    .frame(width: 320)
    .padding()
}
