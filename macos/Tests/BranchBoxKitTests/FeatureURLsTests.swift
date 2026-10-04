import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

@Suite struct FeatureURLsTests {
    private func record(_ name: String, in fixture: String) throws -> FeatureRecord {
        let records = try CLIJSON.decode([FeatureRecord].self, from: Fixtures.data("cli-0.13.4/\(fixture)")).value
        return try #require(records.first { $0.workFeature == name })
    }

    private func make(featureURL: String? = nil, tunnelHost: String? = nil, ports: [PublishedPort] = [],
                      serviceURL: String? = nil) -> FeatureURLs {
        FeatureURLs(featureURL: featureURL,
                    tunnel: tunnelHost.map { TunnelState(hostname: $0, status: .active) },
                    runtime: RuntimeInfo(provider: .sbx, publishedPorts: ports),
                    adapter: serviceURL.map { AdapterInfo(serviceURL: $0) })
    }

    @Test func capturedContainerFeature() throws {
        let urls = try record("prine", in: "main_feature_list_all.json").urls
        #expect(urls.primary == URL(string: "https://dev-prine.localhost"))
        #expect(urls.primaryHTTP == URL(string: "http://dev-prine.localhost"))
        #expect(urls.tunnel == nil)                  // tunnel has no hostname (disabled)
        #expect(urls.ports.isEmpty)
        #expect(urls.inContainerServiceURL == "http://dev:3000")
    }

    @Test func syntheticSandboxFeatureWithTunnelAndPorts() throws {
        let urls = try record("sbx-demo", in: "synthetic_feature_list_new_statuses.json").urls
        #expect(urls.primary == URL(string: "https://dev-sbx-demo.localhost"))
        #expect(urls.tunnel == URL(string: "https://sbx-demo.example.dev"))
        #expect(urls.ports == [PortLink(label: "localhost:49152", url: URL(string: "http://localhost:49152")!, runtimePort: 3000)])
        #expect(urls.inContainerServiceURL == "http://app:3000")
    }

    @Test func featureWithoutURLs() throws {
        let urls = try record("retained", in: "synthetic_feature_list_new_statuses.json").urls
        #expect(urls == FeatureURLs(primary: nil, primaryHTTP: nil, tunnel: nil, ports: [], inContainerServiceURL: nil))
    }

    @Test func schemeLessURLGetsHTTPS() {
        let urls = make(featureURL: "dev-oauth.localhost:8443/app")
        #expect(urls.primary == URL(string: "https://dev-oauth.localhost:8443/app"))
        #expect(urls.primaryHTTP == URL(string: "http://dev-oauth.localhost:8443/app"))
    }

    @Test func existingSchemeIsKept() {
        let http = make(featureURL: "http://dev-oauth.localhost")
        #expect(http.primary == URL(string: "http://dev-oauth.localhost"))
        #expect(http.primaryHTTP == nil)             // already http: no alternative to offer

        let https = make(featureURL: "https://dev-oauth.localhost")
        #expect(https.primary == URL(string: "https://dev-oauth.localhost"))
        #expect(https.primaryHTTP == URL(string: "http://dev-oauth.localhost"))
    }

    @Test(arguments: [nil, "", "   ", "\n"] as [String?])
    func blankURLIsNoLink(_ featureURL: String?) {
        let urls = make(featureURL: featureURL, serviceURL: featureURL)
        #expect(urls.primary == nil)
        #expect(urls.primaryHTTP == nil)
        #expect(urls.inContainerServiceURL == nil)
    }

    @Test func surroundingWhitespaceIsTrimmed() {
        #expect(make(featureURL: "  dev-a.localhost\n").primary == URL(string: "https://dev-a.localhost"))
        #expect(make(serviceURL: " http://app:3000 ").inContainerServiceURL == "http://app:3000")
    }

    @Test func tunnelMatchingThePrimaryHostIsNotRepeated() {
        #expect(make(featureURL: "demo.example.dev", tunnelHost: "Demo.Example.dev").tunnel == nil)
        #expect(make(featureURL: "dev-demo.localhost", tunnelHost: "demo.example.dev").tunnel
                == URL(string: "https://demo.example.dev"))
        #expect(make(tunnelHost: "demo.example.dev").tunnel == URL(string: "https://demo.example.dev"))
        #expect(make(tunnelHost: "").tunnel == nil)
    }

    @Test func portsKeepOrderAndSkipInvalidHostPorts() {
        let urls = make(ports: [PublishedPort(host: 49153, runtime: 5432), PublishedPort(host: 0, runtime: 80),
                                PublishedPort(host: 70_000, runtime: 80), PublishedPort(host: 49152, runtime: 3000)])
        #expect(urls.ports.map(\.label) == ["localhost:49153", "localhost:49152"])
        #expect(urls.ports.map(\.runtimePort) == [5432, 3000])
        #expect(urls.ports.map(\.url.absoluteString) == ["http://localhost:49153", "http://localhost:49152"])
    }

    @Test func startSummaryUsesTheSameRules() throws {
        let alpha = try CLIJSON.decode(StartSummary.self, from: Fixtures.data("cli-0.13.4/sandbox_start_alpha.json")).value
        #expect(alpha.urls.primary == nil)           // minimal start: no feature_url
        #expect(alpha.urls.tunnel == nil)
        #expect(alpha.urls.inContainerServiceURL == "http://dev:3000")
    }
}
