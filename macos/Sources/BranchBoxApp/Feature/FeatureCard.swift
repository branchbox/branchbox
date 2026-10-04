import AppKit
import BranchBoxKit
import BranchBoxStores
import Observation
import SwiftUI

// Building blocks shared by the feature detail's cards: the card container, the two-column grid, label/value
// rows, and the feedback line for host launches that failed.

/// A titled card: a header (icon, title marked as a heading for VoiceOver, optional trailing accessory) over its
/// content, on the control background with a hairline border.
struct FeatureCard<Content: View, Accessory: View>: View {
    let title: String
    let systemImage: String
    let content: Content
    let accessory: Accessory

    init(_ title: String, systemImage: String, @ViewBuilder accessory: () -> Accessory,
         @ViewBuilder content: () -> Content) {
        self.title = title
        self.systemImage = systemImage
        self.accessory = accessory()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Label {
                    Text(title).font(.headline)
                } icon: {
                    Image(systemName: systemImage).foregroundStyle(.secondary)
                }
                .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 8)
                accessory
            }
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color(nsColor: .separatorColor).opacity(0.6)))
        .accessibilityElement(children: .contain)
    }
}

extension FeatureCard where Accessory == EmptyView {
    init(_ title: String, systemImage: String, @ViewBuilder content: () -> Content) {
        self.init(title, systemImage: systemImage, accessory: { EmptyView() }, content: content)
    }
}

/// Cards in one column, or two balanced columns when wider than `twoColumnWidth` (each card goes to the shorter
/// column, in order).
struct CardGridLayout: Layout {
    var spacing: CGFloat = 16
    var twoColumnWidth: CGFloat = 760

    private func columns(for width: CGFloat?) -> Int {
        guard let width, width.isFinite else { return 1 }
        return width > twoColumnWidth ? 2 : 1
    }

    private func placements(width: CGFloat, subviews: Subviews) -> (frames: [CGRect], height: CGFloat) {
        let count = columns(for: width)
        let columnWidth = count == 1 ? width : (width - spacing) / 2
        var heights = Array(repeating: CGFloat(0), count: count)
        var frames: [CGRect] = []
        for subview in subviews {
            let column = heights.indices.min { heights[$0] < heights[$1] } ?? 0
            let size = subview.sizeThatFits(ProposedViewSize(width: columnWidth, height: nil))
            let y = heights[column] == 0 ? 0 : heights[column] + spacing
            frames.append(CGRect(x: CGFloat(column) * (columnWidth + spacing), y: y, width: columnWidth, height: size.height))
            heights[column] = y + size.height
        }
        return (frames, heights.max() ?? 0)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 600
        return CGSize(width: width, height: placements(width: width, subviews: subviews).height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let frames = placements(width: bounds.width, subviews: subviews).frames
        for (subview, frame) in zip(subviews, frames) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                          proposal: ProposedViewSize(width: frame.width, height: frame.height))
        }
    }
}

/// A grid of label/value rows: labels secondary and leading-aligned in their own column.
struct FactGrid<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 8) {
            content
        }
    }
}

/// One fact: a secondary label and a selectable value, with an optional trailing control (copy, reveal).
struct FactRow<Trailing: View>: View {
    let label: String
    let value: String
    var monospaced = false
    var valueStyle: HierarchicalShapeStyle = .primary
    @ViewBuilder var trailing: Trailing

    var body: some View {
        GridRow {
            Text(label)
                .foregroundStyle(.secondary)
                .gridColumnAlignment(.leading)
                .fixedSize()
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(value)
                    .font(monospaced ? .system(.body, design: .monospaced) : .body)
                    .foregroundStyle(valueStyle)
                    .textSelection(.enabled)
                    .lineLimit(2)
                    .truncationMode(.middle)
                    .help(value)
                Spacer(minLength: 4)
                trailing
            }
        }
        .accessibilityElement(children: .contain)
    }
}

extension FactRow where Trailing == EmptyView {
    init(label: String, value: String, monospaced: Bool = false, valueStyle: HierarchicalShapeStyle = .primary) {
        self.init(label: label, value: value, monospaced: monospaced, valueStyle: valueStyle) { EmptyView() }
    }
}

/// A small capsule with an icon and a word (status, "Missing", "Quick").
struct Tag: View {
    let title: String
    var systemImage: String?
    var tint: Color = .secondary

    var body: some View {
        Group {
            if let systemImage {
                Label(title, systemImage: systemImage)
            } else {
                Text(title)
            }
        }
        .font(.caption.weight(.medium))
        .foregroundStyle(tint)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(tint.opacity(0.14), in: Capsule())
        .fixedSize()
    }
}

// MARK: Host launches

/// Runs host launches (editor, terminal, agent, links) and keeps the last failure, which the feature detail shows
/// inline. Menus can't present alerts, so they report here too.
@MainActor @Observable final class HostLaunchFeedback {
    struct Failure: Identifiable, Hashable {
        let id = UUID()
        let feature: FeatureRef?
        let message: String
    }

    static let shared = HostLaunchFeedback()

    private(set) var failure: Failure?

    func launch(_ plan: HostLaunchPlan, for feature: FeatureRef?) {
        Task {
            do {
                try await HostLauncher().launch(plan)
            } catch let error as HostLaunchError {
                report(error.message, for: feature)
            } catch {
                report(error.localizedDescription, for: feature)
            }
        }
    }

    func open(_ url: URL, for feature: FeatureRef?) {
        launch(HostLaunchPlan(kind: .openURL(url)), for: feature)
    }

    func reveal(_ path: String) {
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }

    func report(_ message: String, for feature: FeatureRef?) {
        failure = Failure(feature: feature, message: message)
        NSSound.beep()
    }

    func dismiss() {
        failure = nil
    }

    /// Runs a result card's recovery from a feature surface (detail, cards, Run Command): a retry re-dispatches,
    /// everything else is a host action (Terminal, Finder, Activity, Diagnostics) or a refresh. A failed launch or
    /// a rejected retry is reported inline like any other launch failure.
    func perform(_ recovery: RecoveryAction, for feature: FeatureRef?, model: AppModel, openWindow: OpenWindowAction) {
        Task {
            let actions = FlowActions(model: model, openWindow: { openWindow(id: $0) })
            if case .failure(let error)? = await actions.perform(recovery) {
                report(error.message, for: feature)
            }
        }
    }
}

// MARK: Confirmations

/// A destructive operation a menu asks the feature detail to confirm before dispatching (menus can't present).
struct FeatureConfirmation: Identifiable, Hashable {
    let id = UUID()
    let title: String
    let message: String
    let confirmLabel: String
    let request: OperationRequestContext
}

private struct FeatureConfirmationKey: EnvironmentKey {
    static let defaultValue: (@MainActor @Sendable (FeatureConfirmation) -> Void)? = nil
}

extension EnvironmentValues {
    /// Set by the feature detail: shows the confirmation, then dispatches its request.
    var requestFeatureConfirmation: (@MainActor @Sendable (FeatureConfirmation) -> Void)? {
        get { self[FeatureConfirmationKey.self] }
        set { self[FeatureConfirmationKey.self] = newValue }
    }
}

/// The standard texts for the confirmed environment and sharing operations.
enum FeatureConfirmations {
    static func rebuild(_ feature: FeatureRef) -> FeatureConfirmation {
        FeatureConfirmation(
            title: "Rebuild the dev container for \(feature.name)?",
            message: "The container is removed and its image rebuilt without cache. Files in the worktree are kept; "
                + "anything stored only inside the container is lost. This can take several minutes.",
            confirmLabel: "Rebuild",
            request: .devcontainer(.up(removeExisting: true, buildNoCache: true), feature))
    }

    static func stopDeletingVolumes(_ feature: FeatureRef) -> FeatureConfirmation {
        FeatureConfirmation(
            title: "Stop the dev container and delete its volumes?",
            message: "Databases and caches kept in Docker volumes for \(feature.name) are deleted permanently. "
                + "Files in the worktree are kept.",
            confirmLabel: "Stop and Delete Volumes",
            request: .devcontainer(.down(removeVolumes: true), feature))
    }

    static func stopSharing(_ feature: FeatureRef, hostname: String?) -> FeatureConfirmation {
        FeatureConfirmation(
            title: "Stop sharing \(feature.name)?",
            message: (hostname.map { "https://\($0) stops working for everyone using it. " } ?? "The tunnel stops working. ")
                + "You can share the feature again later; it may get a new address.",
            confirmLabel: "Stop Sharing",
            request: .tunnelRemove(feature, force: false))
    }
}

extension View {
    /// Presents `pending` as a confirmation dialog and dispatches its request when confirmed.
    func featureConfirmationDialog(_ pending: Binding<FeatureConfirmation?>, model: AppModel) -> some View {
        confirmationDialog(pending.wrappedValue?.title ?? "", isPresented: Binding(
            get: { pending.wrappedValue != nil }, set: { if !$0 { pending.wrappedValue = nil } }),
                           titleVisibility: .visible, presenting: pending.wrappedValue) { confirmation in
            Button(confirmation.confirmLabel, role: .destructive) { FeatureCommands.dispatch(confirmation.request, model: model) }
            Button("Cancel", role: .cancel) {}
        } message: { confirmation in
            Text(confirmation.message)
        }
    }
}
