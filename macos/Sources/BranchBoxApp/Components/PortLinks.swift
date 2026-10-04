import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The runtime's published ports as `http://localhost:<host>` links, each with the container port it reaches
/// and a copy button (FeatureURLs rules).
struct PortLinks: View {
    let ports: [PortLink]

    init(ports: [PortLink]) {
        self.ports = ports
    }

    init(urls: FeatureURLs) {
        self.init(ports: urls.ports)
    }

    var body: some View {
        if ports.isEmpty {
            Text("No published ports")
                .foregroundStyle(.secondary)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(ports, id: \.self) { port in
                    HStack(spacing: 6) {
                        Link(destination: port.url) {
                            Label(port.label, systemImage: "network")
                        }
                        .help(port.url.absoluteString)
                        Text("→ container :\(String(port.runtimePort))")
                            .foregroundStyle(.secondary)
                        Spacer(minLength: 4)
                        CopyButton(text: port.url.absoluteString, label: "Copy \(port.label)")
                            .buttonStyle(.borderless)
                    }
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel("\(port.label), forwards to container port \(String(port.runtimePort))")
                }
            }
        }
    }
}

#Preview("Ports") {
    VStack(alignment: .leading, spacing: 16) {
        PortLinks(urls: (PreviewSamples.features.first { $0.workFeature == "sbx-demo" } ?? PreviewSamples.features[0]).urls)
        PortLinks(ports: [
            PortLink(label: "localhost:49152", url: URL(string: "http://localhost:49152")!, runtimePort: 3000),
            PortLink(label: "localhost:49153", url: URL(string: "http://localhost:49153")!, runtimePort: 5432),
        ])
        PortLinks(urls: PreviewSamples.features[0].urls)
    }
    .frame(width: 360)
    .padding()
}
