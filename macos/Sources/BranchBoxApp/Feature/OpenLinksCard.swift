import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// Where the feature can be opened: its URL, tunnel and published ports as links, then the dev container's
/// service and the adapter's in-container URL as copy-only text (`FeatureLinks`).
struct OpenLinksCard: View {
    let record: FeatureRecord
    let feature: FeatureRef
    /// From `devcontainerStatus` (container runtime); nil while loading or for other runtimes.
    let service: DevcontainerServiceInfo?

    var body: some View {
        let rows = FeatureLinks.rows(for: record, service: service)
        FeatureCard("Open", systemImage: "safari") {
            if rows.isEmpty {
                Text("No URLs yet. Ports appear here once the runtime publishes them.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(rows) { row in
                        linkRow(row)
                    }
                }
            }
        }
    }

    /// One compact row: icon, the address (a link only for host URLs), what it is, and copy.
    private func linkRow(_ row: FeatureLinkRow) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: Self.symbol(row.kind))
                .foregroundStyle(row.isLink ? Color.accentColor : .secondary)
                .frame(width: 18)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                if let url = row.url {
                    Button {
                        HostLaunchFeedback.shared.open(url, for: feature)
                    } label: {
                        Text(row.value)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    .buttonStyle(.link)
                    .help("Open \(row.value)")
                    .accessibilityHint("Opens in your browser")
                } else {
                    Text(row.value)
                        .font(.system(.body, design: .monospaced))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                        .help(row.value)
                }
                Text(Self.caption(row))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 4)
            if row.kind == .primary, let http = record.urls.primaryHTTP {
                // The http:// variant lives in a small menu next to Copy, not as a bare "http://" button.
                Menu {
                    Button("Open with http://") { HostLaunchFeedback.shared.open(http, for: feature) }
                    Button("Copy http:// Address") { Pasteboard.general.copy(http.absoluteString) }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Other ways to open this address")
                .accessibilityLabel("More ways to open \(row.value)")
            }
            CopyButton(text: row.value, label: "Copy \(row.title)")
                .buttonStyle(.borderless)
        }
        .accessibilityElement(children: .contain)
    }

    /// "Feature URL", "Tunnel · shared on the internet", "→ container :3000".
    static func caption(_ row: FeatureLinkRow) -> String {
        switch row.kind {
        case .port: row.subtitle ?? row.title
        default: [row.title, row.subtitle].compactMap { $0 }.joined(separator: " · ")
        }
    }

    static func symbol(_ kind: FeatureLinkRow.Kind) -> String {
        switch kind {
        case .primary: "globe"
        case .tunnel: "network"
        case .port: "point.3.connected.trianglepath.dotted"
        case .containerService: "shippingbox"
        case .inContainer: "link"
        }
    }
}

#Preview("Links") {
    let record = PreviewSamples.features.first { $0.workFeature == "sbx-demo" } ?? PreviewSamples.features[0]
    OpenLinksCard(record: record, feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature),
                  service: DevcontainerServiceInfo(serviceName: "app", port: 3000, serviceURL: "http://app:3000",
                                                   containerUser: "vscode"))
        .padding()
        .frame(width: 480)
}
