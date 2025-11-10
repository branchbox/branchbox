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
    let source: Source

    var statusLabel: String {
        status.capitalized
    }

    var updatedAtLabel: String {
        guard let updatedAt else { return "—" }
        return Self.dateFormatter.string(from: updatedAt)
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
