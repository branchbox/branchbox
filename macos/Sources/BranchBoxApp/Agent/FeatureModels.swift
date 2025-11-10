import Foundation

struct FeatureViewData: Identifiable, Hashable {
    enum Source: String {
        case grpc
        case cliFallback
    }

    var id: String { workFeature }
    let workFeature: String
    let branchName: String
    let status: String
    let featureURL: String?
    let promptSeed: String?
    let startMode: String
    let updatedAt: Date?
    let tunnelStatus: String?
    let tunnelProvider: String?
    let adapterName: String?
    let adapterServiceURL: String?
    let adapterWarnings: [String]
    let moduleOutcomes: [ModuleOutcomeSummary]
    let source: Source

    var statusLabel: String {
        status.capitalized
    }

    var updatedAtLabel: String {
        guard let updatedAt else { return "—" }
        return Self.dateFormatter.string(from: updatedAt)
    }

    var moduleSummary: String {
        guard !moduleOutcomes.isEmpty else { return "—" }
        let collapsed = moduleOutcomes.reduce(into: [String: Int]()) { acc, outcome in
            let key = outcome.status.lowercased()
            acc[key, default: 0] += 1
        }
        let ok = collapsed["ok"] ?? 0
        let failed = collapsed["failed"] ?? 0
        if failed > 0 {
            return "\(ok) ok / \(failed) fail"
        }
        return "\(ok) ok"
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()
}

extension FeatureViewData {
    init(grpc feature: Branchbox_Agent_Feature) {
        self.workFeature = feature.workFeature
        self.branchName = feature.branchName
        self.status = feature.status
        self.featureURL = feature.featureURL.isEmpty ? nil : feature.featureURL
        self.promptSeed = feature.promptSeed.isEmpty ? nil : feature.promptSeed
        self.startMode = feature.startMode.isEmpty ? "full" : feature.startMode
        self.updatedAt = FeatureViewData.parse(dateString: feature.updatedAt)
        self.tunnelStatus = feature.tunnelStatus.isEmpty ? nil : feature.tunnelStatus
        self.tunnelProvider = feature.tunnelProvider.isEmpty ? nil : feature.tunnelProvider
        if let adapter = feature.adapter {
            self.adapterName = adapter.name.isEmpty ? nil : adapter.name
            self.adapterServiceURL = adapter.serviceURL.isEmpty ? nil : adapter.serviceURL
            self.adapterWarnings = adapter.warnings
        } else {
            self.adapterName = nil
            self.adapterServiceURL = nil
            self.adapterWarnings = []
        }
        self.moduleOutcomes = feature.moduleOutcomes.map(ModuleOutcomeSummary.init(grpc:))
        self.source = .grpc
    }

    init(cli record: CLICompat.FeatureRecord) {
        self.workFeature = record.work_feature
        self.branchName = record.branch_name
        self.status = record.status
        self.featureURL = record.feature_url
        self.promptSeed = record.prompt_seed
        self.startMode = record.start_mode ?? "full"
        self.updatedAt = FeatureViewData.parse(dateString: record.updated_at)
        self.tunnelStatus = record.tunnel_status
        self.tunnelProvider = record.tunnel_provider
        if let adapter = record.adapter {
            self.adapterName = adapter.name
            self.adapterServiceURL = adapter.service_url
            self.adapterWarnings = adapter.warnings ?? []
        } else {
            self.adapterName = nil
            self.adapterServiceURL = nil
            self.adapterWarnings = []
        }
        self.moduleOutcomes = record.module_outcomes?.map(ModuleOutcomeSummary.init(record:)) ?? []
        self.source = .cliFallback
    }

    private static func parse(dateString: String?) -> Date? {
        guard let dateString, !dateString.isEmpty else { return nil }
        return iso8601.date(from: dateString)
    }

    private static let iso8601: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()
}

struct FeatureStartIntent {
    let name: String
    let title: String?
    let minimal: Bool
    let promptSeed: String?
    let branchPrefix: String?
    let skipModules: [String]
    let reuseExisting: Bool
}

enum FeatureAction {
    case started(FeatureStartIntent)
    case teardown(String)
}

struct FeatureAlert: Identifiable {
    let id = UUID()
    let title: String
    let message: String
}

struct ModuleOutcomeSummary: Hashable {
    let name: String
    let status: String

    init(name: String, status: String) {
        self.name = name
        self.status = status
    }

    init(grpc outcome: Branchbox_Agent_ModuleOutcome) {
        self.init(name: outcome.module, status: outcome.status)
    }

    init(record: CLICompat.ModuleOutcomeRecord) {
        self.init(name: record.module, status: record.status)
    }
}
