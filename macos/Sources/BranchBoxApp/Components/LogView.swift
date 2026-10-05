import AppKit
import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// An operation's live log: monospaced, selectable lines with level icons, optional timestamps, a warnings filter
/// and Find. It follows new lines until the user scrolls up, then offers [Jump to Latest].
///
/// Following is driven by `revision` (`LogBuffer.revision`, bumped with every batch), not the line count, which
/// stops changing once the 10,000-line ring is full. Line ids are `firstIndex` + position, so they stay stable
/// while the ring drops its oldest lines. A scroll-wheel or trackpad scroll up over the log pauses following at
/// once (macOS 14 has no scroll-geometry API, and the bottom marker's visibility alone cannot tell the user's
/// scroll from the view's own while lines stream in); scrolling back down to the bottom resumes it.
struct LogView: View {
    let lines: [LogLine]
    /// The full log on disk, for [Reveal Log File].
    var archiveURL: URL?
    /// Lines the buffer dropped before `lines[0]` (`LogBuffer.droppedLines`).
    var firstIndex: Int = 0
    /// Changes whenever lines arrive (`LogBuffer.revision`); nil uses `firstIndex + lines.count`.
    var revision: Int?

    @State private var filter = LogFilter()
    @State private var showsTimestamps = false
    @State private var followsTail = true
    /// Whether the bottom marker is on screen.
    @State private var atBottom = true
    /// When the view last scrolled itself; the bottom marker disappearing right after that is not the user.
    @State private var lastAutoScroll = Date.distantPast

    private static let bottomID = "log.bottom"

    private var tailMarker: Int { revision ?? (firstIndex + lines.count) }

    var body: some View {
        let visible = filter.apply(to: lines, firstIndex: firstIndex)
        VStack(spacing: 0) {
            toolbar(visibleCount: visible.count)
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 1) {
                        ForEach(visible) { entry in
                            LogRow(line: entry.line, showsTimestamp: showsTimestamps, highlight: filter.query)
                                .id(entry.index)
                        }
                        Color.clear
                            .frame(height: 1)
                            .id(Self.bottomID)
                            .onAppear {
                                atBottom = true
                                followsTail = true
                            }
                            .onDisappear {
                                atBottom = false
                                // A scroll-bar drag has no wheel event; a disappearance long after the last
                                // automatic scroll is the user's.
                                if Date.now.timeIntervalSince(lastAutoScroll) > 0.5 { followsTail = false }
                            }
                    }
                    .padding(8)
                    .textSelection(.enabled)
                }
                .background(ScrollWheelMonitor { direction in
                    switch direction {
                    case .up: followsTail = false
                    case .down: if atBottom { followsTail = true }
                    }
                })
                .overlay(alignment: .bottomTrailing) {
                    if !followsTail {
                        Button {
                            scrollToBottom(proxy)
                        } label: {
                            Label("Jump to Latest", systemImage: "arrow.down.to.line")
                        }
                        .buttonStyle(.borderedProminent)
                        .padding(10)
                    }
                }
                .overlay {
                    if visible.isEmpty {
                        Text(lines.isEmpty ? "No log messages" : "No matching lines")
                            .foregroundStyle(.secondary)
                    }
                }
                .onChange(of: tailMarker) {
                    if followsTail { scrollToBottom(proxy) }
                }
                .onAppear { scrollToBottom(proxy) }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Log")
    }

    /// Full labels when they fit (the Activity window, sheets); in a narrow pane such as the inspector, the toggles
    /// become icon buttons with help text so no label breaks mid-word.
    private func toolbar(visibleCount: Int) -> some View {
        ViewThatFits(in: .horizontal) {
            toolbarRow(visibleCount: visibleCount, compact: false)
            toolbarRow(visibleCount: visibleCount, compact: true)
        }
        .toggleStyle(.checkbox)
        .controlSize(.small)
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
    }

    private func toolbarRow(visibleCount: Int, compact: Bool) -> some View {
        HStack(spacing: compact ? 6 : 10) {
            TextField("Find", text: $filter.query)
                .textFieldStyle(.roundedBorder)
                .frame(minWidth: compact ? 60 : 140, idealWidth: compact ? 120 : 200, maxWidth: 200)
            if !filter.query.trimmingCharacters(in: .whitespaces).isEmpty {
                let matches = visibleCount == 1 ? "1 match" : "\(visibleCount) matches"
                Text(compact ? "\(visibleCount)" : matches)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .fixedSize()
                    .help(matches)
                    .accessibilityLabel(matches)
            }
            if compact {
                Toggle(isOn: $filter.warningsOnly) {
                    Label("Warnings only", systemImage: "exclamationmark.triangle")
                }
                .toggleStyle(.button)
                .labelStyle(.iconOnly)
                .help(filter.warningsOnly ? "Showing warnings and errors only" : "Show warnings and errors only")
                Toggle(isOn: $showsTimestamps) {
                    Label("Timestamps", systemImage: "clock")
                }
                .toggleStyle(.button)
                .labelStyle(.iconOnly)
                .help(showsTimestamps ? "Hide timestamps" : "Show timestamps")
            } else {
                Toggle("Warnings only", isOn: $filter.warningsOnly)
                    .fixedSize()
                Toggle("Timestamps", isOn: $showsTimestamps)
                    .fixedSize()
            }
            Spacer(minLength: compact ? 0 : 4)
            CopyButton(label: "Copy Log") {
                LogFilter.plainText(filter.apply(to: lines).map(\.line), timestamps: showsTimestamps)
            }
            if let archiveURL {
                Button {
                    NSWorkspace.shared.activateFileViewerSelecting([archiveURL])
                } label: {
                    Image(systemName: "doc.text.magnifyingglass")
                }
                .help("Reveal Log File")
                .accessibilityLabel("Reveal Log File")
            }
        }
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy) {
        lastAutoScroll = .now
        proxy.scrollTo(Self.bottomID, anchor: .bottom)
        followsTail = true
    }
}

/// Reports scroll-wheel and trackpad scrolls over the view it backs, without taking any events itself.
private struct ScrollWheelMonitor: NSViewRepresentable {
    enum Direction { case up, down }

    let onScroll: (Direction) -> Void

    func makeNSView(context: Context) -> MonitorView {
        let view = MonitorView()
        view.onScroll = onScroll
        return view
    }

    func updateNSView(_ view: MonitorView, context: Context) {
        view.onScroll = onScroll
    }

    static func dismantleNSView(_ view: MonitorView, coordinator: ()) {
        view.stopMonitoring()
    }

    final class MonitorView: NSView {
        var onScroll: ((Direction) -> Void)?
        private var monitor: Any?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            stopMonitoring()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                MainActor.assumeIsolated { self?.handle(event) }
                return event
            }
        }

        /// Never the target of a click or a scroll: the log's scroll view underneath gets them.
        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        func stopMonitoring() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
        }

        /// A positive `scrollingDeltaY` scrolls toward the top of the content (natural scrolling is already applied).
        private func handle(_ event: NSEvent) {
            guard event.window === window, event.scrollingDeltaY != 0,
                  bounds.contains(convert(event.locationInWindow, from: nil)) else { return }
            onScroll?(event.scrollingDeltaY > 0 ? .up : .down)
        }
    }
}

/// One line: level icon, optional timestamp, a short dimmed source (the tracing target's last part) and the message
/// with Find matches highlighted. The full target (`worktree_core::modules::compose`) is in the line's help.
private struct LogRow: View {
    let line: LogLine
    let showsTimestamp: Bool
    let highlight: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: line.level.symbol)
                .foregroundStyle(line.level.tint.color)
                .frame(width: 12)
                .accessibilityLabel(line.level.rawValue)
            if showsTimestamp, let timestamp = line.timestamp {
                Text(LogFilter.timestamp(timestamp))
                    .foregroundStyle(.secondary)
            }
            Text(message)
                .foregroundStyle(line.level == .error ? Color.red : Color.primary)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(.system(.caption, design: .monospaced))
        .help(line.target ?? "")
    }

    /// "compose" for `worktree_core::modules::compose`: the crate and module path mean nothing to a user.
    static func shortSource(_ target: String?) -> String? {
        guard let target, !target.isEmpty else { return nil }
        return target.components(separatedBy: "::").last.flatMap { $0.isEmpty ? nil : $0 }
    }

    private var message: AttributedString {
        var text = AttributedString(Self.shortSource(line.target).map { "\($0) · " } ?? "")
        text.foregroundColor = Color(nsColor: .tertiaryLabelColor)
        var body = AttributedString(line.message)
        let needle = highlight.trimmingCharacters(in: .whitespaces)
        if !needle.isEmpty {
            var searchStart = body.startIndex
            while searchStart < body.endIndex,
                  let range = body[searchStart...].range(of: needle, options: [.caseInsensitive, .diacriticInsensitive]) {
                body[range].backgroundColor = .yellow.opacity(0.45)
                searchStart = range.upperBound
            }
        }
        return text + body
    }
}

private enum LogViewPreview {
    static let lines: [LogLine] = {
        let start = Date.now.addingTimeInterval(-40)
        let messages: [(LogLevel, String?, String)] = [
            (.info, "worktree_core::workflows::feature", "Creating worktree for \(PreviewSamples.features[0].workFeature)"),
            (.debug, "worktree_core::git", "git worktree add -b feature/prine ../prine HEAD"),
            (.info, "worktree_core::modules::compose", "Starting compose project branchbox-prine"),
            (.warn, "worktree_core::modules::tunnel", "Tunnel provisioning disabled in project configuration"),
            (.output, nil, "✓ devcontainer ready"),
            (.error, "worktree_core::modules::database", "database: connection refused (port 5432)"),
            (.info, "worktree_core::modules::specs", "Spec remains in in-progress (use --complete-spec to move to completed)"),
        ]
        return (0..<60).map { index in
            let (level, target, message) = messages[index % messages.count]
            return LogLine(timestamp: start.addingTimeInterval(Double(index) * 0.6), level: level, source: .stderr,
                           target: target, message: message)
        }
    }()
}

#Preview("Log") {
    LogView(lines: LogViewPreview.lines, archiveURL: URL(fileURLWithPath: "/tmp/branchbox-preview.log"))
        .frame(width: 640, height: 360)
}

#Preview("Empty log") {
    LogView(lines: [])
        .frame(width: 640, height: 200)
}
