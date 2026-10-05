import BranchBoxPreview
import SwiftUI

/// The feature's colour (`color`, "#e67e22") as an 8 pt dot with a separator-coloured stroke. Decorative: the
/// row's text already names the feature, so VoiceOver skips it.
struct ColorSwatch: View {
    let hex: String?
    var size: CGFloat = 8

    var body: some View {
        Circle()
            .fill(Self.color(hex))
            .overlay(Circle().strokeBorder(Color(nsColor: .separatorColor), lineWidth: 0.5))
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }

    /// The parsed colour, or a neutral fill when there is none.
    static func color(_ hex: String?) -> Color {
        guard let rgb = HexColor.rgb(hex) else { return Color(nsColor: .quaternaryLabelColor) }
        return Color(.sRGB, red: rgb.red, green: rgb.green, blue: rgb.blue)
    }
}

/// "#rgb" and "#rrggbb" (the "#" optional) in sRGB components 0...1.
enum HexColor {
    static func rgb(_ hex: String?) -> (red: Double, green: Double, blue: Double)? {
        guard var digits = hex?.trimmingCharacters(in: .whitespaces) else { return nil }
        if digits.hasPrefix("#") { digits.removeFirst() }
        if digits.count == 3 { digits = digits.map { "\($0)\($0)" }.joined() }
        guard digits.count == 6, digits.allSatisfy(\.isHexDigit), let value = UInt32(digits, radix: 16) else { return nil }
        return (Double((value >> 16) & 0xFF) / 255, Double((value >> 8) & 0xFF) / 255, Double(value & 0xFF) / 255)
    }
}

#Preview("Swatches") {
    VStack(alignment: .leading, spacing: 8) {
        ForEach(PreviewSamples.features.prefix(5)) { feature in
            HStack {
                ColorSwatch(hex: feature.color)
                Text(feature.workFeature)
            }
        }
        HStack {
            ColorSwatch(hex: nil)
            Text("no colour")
        }
        ColorSwatch(hex: "#3498db", size: 16)
    }
    .padding()
}
