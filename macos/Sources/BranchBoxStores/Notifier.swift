import Foundation

/// Posts user notifications for finished operations. The app uses `UserNotificationNotifier` (SW-4) when
/// `isAvailable`, and `NoopNotifier` otherwise.
public protocol Notifier: Sendable {
    var isAvailable: Bool { get }                                 // false unless bundle id != nil && bundleURL.pathExtension == "app"
    func requestAuthorizationIfNeeded() async -> Bool
    func post(_ note: UserNote) async
}

public struct UserNote: Sendable, Hashable {
    public let title: String
    public let body: String
    public let intent: WindowIntent?                              // performed when the user clicks the notification
    public let threadID: String
    public init(title: String, body: String, intent: WindowIntent? = nil, threadID: String) {
        self.title = title
        self.body = body
        self.intent = intent
        self.threadID = threadID
    }
}

/// Used under `swift run`, in tests and in previews, where `UNUserNotificationCenter` must not be touched.
public struct NoopNotifier: Notifier {
    public init() {}
    public var isAvailable: Bool { false }
    public func requestAuthorizationIfNeeded() async -> Bool { false }
    public func post(_ note: UserNote) async {}
}

/// Whether this process runs from a `.app` bundle. Notifications and `UserDefaults.standard` both require it.
public enum AppBundle {
    /// A bundle identifier alone is not enough: xctest has one, and `UNUserNotificationCenter` still traps
    /// there. Only a real `.app` bundle qualifies.
    public static func isBundledApp(_ bundle: Bundle = .main) -> Bool {
        bundle.bundleIdentifier != nil && bundle.bundleURL.pathExtension == "app"
    }
}
