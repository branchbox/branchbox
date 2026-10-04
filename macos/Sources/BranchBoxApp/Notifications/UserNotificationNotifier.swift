import AppKit
import BranchBoxStores
import Foundation
import os
import SwiftUI
import UserNotifications

/// Posts operation notifications through `UNUserNotificationCenter` (§11). Built by `CompositionRoot` only inside
/// a real `.app` bundle; everywhere else the app uses `NoopNotifier`, because the notification center traps there.
///
/// - Authorization is requested lazily, the first time a note is due (or from onboarding).
/// - While BranchBox is frontmost, banners are suppressed: the note is announced to VoiceOver and shown as a toast
///   in the main window instead.
/// - Every note has the actions Show (open the main window on the operation) and Open in Editor; a click on the
///   note itself is Show. Both hand the note's intent to `handler` on the main actor.
/// - Failures are soft: they are logged and the operation's result is unaffected.
final class UserNotificationNotifier: NSObject, Notifier, UNUserNotificationCenterDelegate, @unchecked Sendable {
    /// What the user chose on a delivered note.
    enum Response: Sendable, Hashable {
        case show(WindowIntent)
        case openInEditor(WindowIntent)
        /// The note arrived while BranchBox was frontmost: show it in the window instead of a banner.
        case presentInApp(title: String, body: String)
    }

    static let categoryID = "operation"
    static let showActionID = "show"
    static let openInEditorActionID = "openInEditor"

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "notifications")

    private let handler: @MainActor @Sendable (Response) -> Void
    /// Guards `intents` and `configured` (delegate callbacks arrive on arbitrary threads).
    private let lock = NSLock()
    /// The intent of each delivered note by request identifier; `WindowIntent` is not property-list encodable.
    private var intents: [String: WindowIntent] = [:]
    private var configured = false

    init(handler: @escaping @MainActor @Sendable (Response) -> Void) {
        self.handler = handler
        super.init()
        // Become the delegate at once (this is built while the app launches), so a click on a note left in
        // Notification Center by an earlier run still reaches `didReceive`.
        if isAvailable { configureIfNeeded(UNUserNotificationCenter.current()) }
    }

    var isAvailable: Bool { AppBundle.isBundledApp() }

    func requestAuthorizationIfNeeded() async -> Bool {
        guard isAvailable else { return false }
        let center = UNUserNotificationCenter.current()
        configureIfNeeded(center)
        switch await center.notificationSettings().authorizationStatus {
        case .authorized, .provisional, .ephemeral:
            return true
        case .denied:
            return false
        case .notDetermined:
            do {
                return try await center.requestAuthorization(options: [.alert, .sound])
            } catch {
                Self.logger.error("Notification authorization failed: \(error.localizedDescription, privacy: .public)")
                return false
            }
        @unknown default:
            return false
        }
    }

    func post(_ note: UserNote) async {
        guard isAvailable else { return }
        let center = UNUserNotificationCenter.current()
        configureIfNeeded(center)
        let content = UNMutableNotificationContent()
        content.title = note.title
        content.body = note.body
        content.threadIdentifier = note.threadID
        content.categoryIdentifier = Self.categoryID
        let identifier = UUID().uuidString
        if let intent = note.intent {
            remember(intent, for: identifier)
            content.userInfo = Self.userInfo(for: intent)
        }
        do {
            try await center.add(UNNotificationRequest(identifier: identifier, content: content, trigger: nil))
        } catch {
            Self.logger.error("Posting a notification failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    // MARK: UNUserNotificationCenterDelegate

    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        let title = notification.request.content.title
        let body = notification.request.content.body
        // A toast is only visible in the main window: with that window closed or minimized, show the banner even
        // while another BranchBox window (Settings, Activity) is frontmost.
        let frontmost = await MainActor.run { NSApp.isActive && WindowOpener.shared.isMainWindowVisible }
        guard frontmost else { return [.banner, .list, .sound] }
        let handler = self.handler
        await MainActor.run { handler(.presentInApp(title: title, body: body)) }
        return [.list]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let identifier = response.notification.request.identifier
        let action = response.actionIdentifier
        let userInfo = response.notification.request.content.userInfo
        guard let intent = takeIntent(for: identifier) ?? Self.intent(from: userInfo) else {
            // Nothing to route to (a note from an older build): still bring the main window forward.
            await MainActor.run {
                WindowOpener.shared.open(SceneID.main)
                NSApp.activate()
            }
            return
        }
        let handler = self.handler
        let choice: Response = action == Self.openInEditorActionID ? .openInEditor(intent) : .show(intent)
        await MainActor.run { handler(choice) }
    }

    // MARK: Private

    private func configureIfNeeded(_ center: UNUserNotificationCenter) {
        let first: Bool = lock.withLock {
            defer { configured = true }
            return !configured
        }
        guard first else { return }
        center.delegate = self
        let show = UNNotificationAction(identifier: Self.showActionID, title: "Show", options: [.foreground])
        let editor = UNNotificationAction(identifier: Self.openInEditorActionID, title: "Open in Editor", options: [.foreground])
        center.setNotificationCategories([UNNotificationCategory(identifier: Self.categoryID, actions: [show, editor],
                                                                 intentIdentifiers: [])])
    }

    /// The property-list form of the intents notes carry, so a click after a relaunch can still be routed.
    static func userInfo(for intent: WindowIntent) -> [String: String] {
        switch intent {
        case .showActivity(let operation?):
            return ["operation": operation.uuidString]
        case .select(let selection):
            guard let data = try? JSONEncoder().encode(selection),
                  let json = String(data: data, encoding: .utf8) else { return [:] }
            return ["selection": json]
        case .teardown(let feature, _):
            return userInfo(for: .select(.feature(projectPath: feature.project.path, name: feature.name)))
        default:
            return [:]
        }
    }

    static func intent(from userInfo: [AnyHashable: Any]) -> WindowIntent? {
        if let raw = userInfo["operation"] as? String, let id = UUID(uuidString: raw) {
            return .showActivity(operation: id)
        }
        if let json = userInfo["selection"] as? String,
           let selection = try? JSONDecoder().decode(SidebarSelection.self, from: Data(json.utf8)) {
            return .select(selection)
        }
        return nil
    }

    private func remember(_ intent: WindowIntent, for identifier: String) {
        lock.withLock {
            intents[identifier] = intent
            if intents.count > 200 { intents.removeAll() }        // notes nobody clicked; their intents are moot
        }
    }

    private func takeIntent(for identifier: String) -> WindowIntent? {
        lock.withLock { intents.removeValue(forKey: identifier) }
    }
}

/// A short message shown at the bottom of the main window: a notification that arrived while BranchBox was
/// frontmost. One at a time; a newer one replaces it.
@MainActor @Observable final class ToastCenter {
    static let shared = ToastCenter()

    struct Toast: Identifiable, Equatable {
        let id = UUID()
        let title: String
        let body: String
    }

    private(set) var current: Toast?

    func show(title: String, body: String) {
        let toast = Toast(title: title, body: body)
        current = toast
        AccessibilityNotification.Announcement("\(title). \(body)").post()
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(4))
            if self?.current == toast { self?.current = nil }
        }
    }

    func dismiss() {
        current = nil
    }
}

/// The toast's look: a material capsule with the title over the body.
struct ToastView: View {
    let toast: ToastCenter.Toast
    var onDismiss: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: "bell.fill")
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(toast.title).font(.callout.weight(.semibold))
                if !toast.body.isEmpty {
                    Text(toast.body)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            Button(action: onDismiss) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("Dismiss")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.separator))
        .shadow(color: .black.opacity(0.12), radius: 8, y: 2)
        .frame(maxWidth: 420)
        .accessibilityElement(children: .combine)
    }
}
