import Foundation

/// The links a feature offers, derived once from its record (Kit owns URL normalization; views never
/// build URLs from raw strings).
public struct FeatureURLs: Sendable, Hashable {
    public let primary: URL?                         // feature_url; keeps scheme if present, else https:// (CLI parity)
    public let primaryHTTP: URL?                     // "Open with http://" alternative
    public let tunnel: URL?                          // https://<tunnel.hostname> when it differs
    public let ports: [PortLink]                     // http://localhost:<host>, subtitle "→ container :<runtime>"
    public let inContainerServiceURL: String?        // adapter.service_url — display/copy only, never a link

    public init(primary: URL?, primaryHTTP: URL?, tunnel: URL?, ports: [PortLink], inContainerServiceURL: String?) {
        self.primary = primary
        self.primaryHTTP = primaryHTTP
        self.tunnel = tunnel
        self.ports = ports
        self.inContainerServiceURL = inContainerServiceURL
    }

    /// Derives the links from the raw record fields (shared by `FeatureRecord` and start summaries).
    public init(featureURL: String?, tunnel: TunnelState?, runtime: RuntimeInfo?, adapter: AdapterInfo?) {
        let primary = FeatureURLs.link(featureURL, defaultScheme: "https")
        self.primary = primary
        self.primaryHTTP = primary.flatMap(FeatureURLs.httpAlternative)
        let tunnelURL = FeatureURLs.link(tunnel?.hostname, defaultScheme: "https")
        self.tunnel = FeatureURLs.sameHost(tunnelURL, primary) ? nil : tunnelURL
        self.ports = (runtime?.publishedPorts ?? []).compactMap { port in
            guard (1...65_535).contains(port.host), let url = URL(string: "http://localhost:\(port.host)") else {
                return nil
            }
            return PortLink(label: "localhost:\(port.host)", url: url, runtimePort: port.runtime)
        }
        self.inContainerServiceURL = FeatureURLs.trimmed(adapter?.serviceURL)
    }

    private static func trimmed(_ text: String?) -> String? {
        guard let text = text?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else { return nil }
        return text
    }

    /// core strips the scheme before storing `feature_url`, and the CLI prints it back as https://.
    private static func link(_ text: String?, defaultScheme: String) -> URL? {
        guard let text = trimmed(text) else { return nil }
        guard let url = URL(string: text.contains("://") ? text : "\(defaultScheme)://\(text)"),
              url.host?.isEmpty == false else { return nil }
        return url
    }

    private static func httpAlternative(_ url: URL) -> URL? {
        guard url.scheme?.lowercased() == "https",
              var components = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return nil }
        components.scheme = "http"
        return components.url
    }

    private static func sameHost(_ lhs: URL?, _ rhs: URL?) -> Bool {
        guard let left = lhs?.host?.lowercased(), let right = rhs?.host?.lowercased() else { return false }
        return left == right
    }
}

public struct PortLink: Sendable, Hashable {
    public let label: String; public let url: URL; public let runtimePort: Int
    public init(label: String, url: URL, runtimePort: Int) {
        self.label = label
        self.url = url
        self.runtimePort = runtimePort
    }
}
