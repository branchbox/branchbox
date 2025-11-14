import Foundation

enum AppSection: Hashable, Identifiable, CaseIterable {
    case home
    case features
    case agent
    case settings

    var id: Self { self }
    var title: String {
        switch self {
        case .home: return "Home"
        case .features: return "Features"
        case .agent: return "Agent"
        case .settings: return "Settings"
        }
    }
    var icon: String {
        switch self {
        case .home: return "house"
        case .features: return "list.bullet"
        case .agent: return "waveform.path.ecg"
        case .settings: return "gearshape"
        }
    }
}

