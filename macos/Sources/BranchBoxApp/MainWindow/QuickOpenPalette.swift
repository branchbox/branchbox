import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// One entry of the Quick Open palette. Ids are stable across rebuilds (the same feature and action always get
/// the same id), so the highlighted row survives a refresh.
struct QuickOpenItem: Identifiable, Hashable, Sendable {
    enum Action: Hashable, Sendable {
        /// Posted to the window once the palette is closed.
        case intent(WindowIntent)
        case openInEditor(FeatureRef)
        case terminal(FeatureRef)
        case openURL(FeatureRef)
        case refreshAll
        case openWindow(String)
        case openSettings
    }

    enum Group: String, Sendable, CaseIterable {
        case features = "Features"
        case projects = "Projects"
        case commands = "Commands"
    }

    let id: String
    let title: String
    var subtitle: String?
    let systemImage: String
    let group: Group
    let action: Action
    /// Shown before anything is typed: a feature or project itself, and the global commands. Per-feature
    /// actions appear once the query narrows the list.
    var isPrimary = true
}

/// Builds and filters the palette's items: every feature × its actions, every project, and the global commands.
enum QuickOpenIndex {
    @MainActor static func items(model: AppModel) -> [QuickOpenItem] {
        let ready = model.environment.identity != nil
        var features: [QuickOpenItem] = []
        var projects: [QuickOpenItem] = []
        for project in model.projects.projects {
            let ref = project.ref
            let name = project.displayName
            projects.append(QuickOpenItem(id: "project:\(ref.path)", title: name, subtitle: ref.path, systemImage: "folder",
                                          group: .projects, action: .intent(.select(.project(path: ref.path)))))
            if ready, project.rootExists {
                projects.append(QuickOpenItem(id: "project:\(ref.path):start", title: "Start Feature in \(name)…",
                                              subtitle: nil, systemImage: "plus.circle", group: .projects,
                                              action: .intent(.startFeature(project: ref, prefill: nil)), isPrimary: false))
            }
            for record in project.features where record.status != .removed {
                features += featureItems(record, project: project, ready: ready)
            }
        }
        var commands: [QuickOpenItem] = []
        if ready, !model.projects.projects.isEmpty {
            commands.append(QuickOpenItem(id: "command:start", title: "Start Feature…", systemImage: "plus.circle",
                                          group: .commands, action: .intent(.startFeature(project: nil, prefill: nil))))
        }
        if ready {
            commands.append(QuickOpenItem(id: "command:addProject", title: "Add Project…", systemImage: "folder.badge.plus",
                                          group: .commands, action: .intent(.addProject(nil))))
            commands.append(QuickOpenItem(id: "command:refresh", title: "Refresh All Projects", systemImage: "arrow.clockwise",
                                          group: .commands, action: .refreshAll))
        }
        commands += [
            QuickOpenItem(id: "command:activity", title: "Show Activity", systemImage: "list.bullet.rectangle",
                          group: .commands, action: .openWindow(SceneID.activity)),
            QuickOpenItem(id: "command:diagnostics", title: "Show Diagnostics", systemImage: "stethoscope",
                          group: .commands, action: .openWindow(SceneID.diagnostics)),
            QuickOpenItem(id: "command:settings", title: "Settings…", systemImage: "gearshape", group: .commands,
                          action: .openSettings),
        ]
        return features + projects + commands
    }

    @MainActor private static func featureItems(_ record: FeatureRecord, project: ProjectStore, ready: Bool) -> [QuickOpenItem] {
        let feature = FeatureRef(project: project.ref, name: record.workFeature)
        let base = "feature:\(project.ref.path):\(record.workFeature)"
        let subtitle = "\(project.displayName) · \(record.status.label)"
        var items = [QuickOpenItem(id: base, title: record.workFeature, subtitle: subtitle, systemImage: "shippingbox",
                                   group: .features,
                                   action: .intent(.select(.feature(projectPath: project.ref.path, name: record.workFeature))))]
        let folderExists = project.folderExists(for: record)
        if folderExists {
            items.append(QuickOpenItem(id: base + ":editor", title: "Open \(record.workFeature) in Editor", subtitle: subtitle,
                                       systemImage: "chevron.left.forwardslash.chevron.right", group: .features,
                                       action: .openInEditor(feature), isPrimary: false))
            if record.runtime.provider != .sbx {
                items.append(QuickOpenItem(id: base + ":terminal", title: "Open \(record.workFeature) in Terminal",
                                           subtitle: subtitle, systemImage: "terminal", group: .features,
                                           action: .terminal(feature), isPrimary: false))
            }
        }
        if CommandContext.primaryURL(record) != nil {
            items.append(QuickOpenItem(id: base + ":url", title: "Open \(record.workFeature) URL", subtitle: subtitle,
                                       systemImage: "safari", group: .features, action: .openURL(feature),
                                       isPrimary: false))
        }
        if ready {
            items.append(QuickOpenItem(id: base + ":teardown", title: "Tear Down \(record.workFeature)…", subtitle: subtitle,
                                       systemImage: "trash", group: .features,
                                       action: .intent(.teardown(feature, preselect: nil)), isPrimary: false))
        }
        return items
    }

    /// `items` in display order: features, projects, then commands, each group keeping its ranking.
    static func grouped(_ items: [QuickOpenItem]) -> [QuickOpenItem] {
        QuickOpenItem.Group.allCases.flatMap { group in items.filter { $0.group == group } }
    }

    /// Items whose title or subtitle contains every word of `query`, best first: a title that starts with the
    /// query, then a title word that does, then any match; ties keep their order. An empty query keeps the
    /// primary items.
    static func filter(_ items: [QuickOpenItem], query: String) -> [QuickOpenItem] {
        let words = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !words.isEmpty else { return items.filter(\.isPrimary) }
        let needle = words.joined(separator: " ")
        let ranked: [(rank: Int, offset: Int, item: QuickOpenItem)] = items.enumerated().compactMap { offset, item in
            let title = item.title.lowercased()
            let haystack = title + " " + (item.subtitle ?? "").lowercased()
            guard words.allSatisfy({ haystack.contains($0) }) else { return nil }
            let rank: Int
            if title.hasPrefix(needle) {
                rank = 0
            } else if title.split(whereSeparator: { !$0.isLetter && !$0.isNumber }).contains(where: { $0.hasPrefix(words[0]) }) {
                rank = 1
            } else {
                rank = 2
            }
            return (rank, offset, item)
        }
        return ranked.sorted { ($0.rank, $0.offset) < ($1.rank, $1.offset) }.map(\.item)
    }
}

/// ⌘K: an overlay palette (never a sheet) over the main window. ↑↓ move, Return performs, Esc or a click
/// outside closes. The chosen item runs after the palette has closed: intents go to the window's router, which
/// presents their sheet only then.
struct QuickOpenPalette: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
    let router: PresentationRouter
    @State private var query = ""
    @State private var highlighted: QuickOpenItem.ID?
    @FocusState private var fieldFocused: Bool

    private var results: [QuickOpenItem] {
        QuickOpenIndex.grouped(QuickOpenIndex.filter(QuickOpenIndex.items(model: model), query: query))
    }

    var body: some View {
        ZStack(alignment: .top) {
            Color.black.opacity(0.12)
                .ignoresSafeArea()
                .onTapGesture { router.closeQuickOpen() }
                .accessibilityHidden(true)
            QuickOpenPanel(query: $query, results: results, highlighted: currentHighlight(in: results),
                           fieldFocused: $fieldFocused,
                           onMove: { move($0, in: results) },
                           onSubmit: performHighlighted,
                           onPerform: { perform($0) },
                           onClose: { router.closeQuickOpen() })
                .padding(.top, 72)
        }
        .onAppear { fieldFocused = true }
        .onChange(of: query) { highlighted = nil }
    }

    private func currentHighlight(in results: [QuickOpenItem]) -> QuickOpenItem.ID? {
        if let highlighted, results.contains(where: { $0.id == highlighted }) { return highlighted }
        return results.first?.id
    }

    private func move(_ delta: Int, in results: [QuickOpenItem]) {
        guard !results.isEmpty else { return }
        let current = results.firstIndex { $0.id == currentHighlight(in: results) } ?? 0
        highlighted = results[min(max(current + delta, 0), results.count - 1)].id
    }

    private func performHighlighted() {
        // The native field can retain its original submit handler while the panel's highlight changes.
        // Resolve the current state here instead of capturing the panel's highlighted value in that handler.
        let currentResults = results
        let current = currentHighlight(in: currentResults)
        perform(currentResults.first { $0.id == current })
    }

    private func perform(_ item: QuickOpenItem?) {
        guard let item else { return }
        switch item.action {
        case .intent(let intent):
            router.closeQuickOpen(then: intent)
            return
        case .openInEditor(let feature), .terminal(let feature), .openURL(let feature):
            router.closeQuickOpen()
            guard let record = model.projects.project(feature.project)?.feature(named: feature.name) else { return }
            switch item.action {
            case .openURL: FeatureCommands.openURL(record)
            case .terminal: FeatureCommands.launch(.terminal, record, project: feature.project, model: model)
            default: FeatureCommands.openInEditor(record, project: feature.project, model: model)
            }
        case .refreshAll:
            router.closeQuickOpen()
            model.projects.refreshAll(.manual)
        case .openWindow(let id):
            router.closeQuickOpen()
            openWindow(id: id)
        case .openSettings:
            router.closeQuickOpen()
            openSettings()
        }
    }
}

/// The palette's panel: search field and results, grouped. Separate from the overlay so it renders on its own.
struct QuickOpenPanel: View {
    @Binding var query: String
    let results: [QuickOpenItem]
    let highlighted: QuickOpenItem.ID?
    var fieldFocused: FocusState<Bool>.Binding
    var onMove: (Int) -> Void = { _ in }
    var onSubmit: () -> Void = {}
    var onPerform: (QuickOpenItem?) -> Void = { _ in }
    var onClose: () -> Void = {}

    static let maxVisible = 9

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                TextField("Search features, projects and commands", text: $query)
                    .textFieldStyle(.plain)
                    .font(.title3)
                    .focused(fieldFocused)
                    .onSubmit(onSubmit)
                    .onKeyPress(.upArrow) { onMove(-1); return .handled }
                    .onKeyPress(.downArrow) { onMove(1); return .handled }
                    .onKeyPress(.escape) { onClose(); return .handled }
                    .accessibilityIdentifier("quickOpen.field")
                Text("esc")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.tertiary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(.tertiary))
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            Divider()
            if results.isEmpty {
                Text(query.isEmpty ? "Nothing to show yet" : "No matches for “\(query)”")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 24)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(groups, id: \.group) { section in
                                Text(section.group.rawValue)
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(.secondary)
                                    .padding(.horizontal, 16)
                                    .padding(.top, 10)
                                    .padding(.bottom, 4)
                                    .accessibilityAddTraits(.isHeader)
                                ForEach(section.items) { item in
                                    QuickOpenRow(item: item, isHighlighted: item.id == highlighted)
                                        .id(item.id)
                                        .contentShape(Rectangle())
                                        .onTapGesture { onPerform(item) }
                                }
                            }
                        }
                        .padding(.bottom, 8)
                    }
                    .frame(height: listHeight)
                    .onChange(of: highlighted) { _, id in
                        if let id { proxy.scrollTo(id) }
                    }
                }
            }
        }
        .frame(width: 560)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.separator))
        .shadow(color: .black.opacity(0.2), radius: 20, y: 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Quick Open")
    }

    /// The list's height: its content up to `maxVisible` rows, so a short result list doesn't leave a gap.
    private var listHeight: CGFloat {
        let content = CGFloat(results.count) * 30 + CGFloat(groups.count) * 32 + 8
        return min(content, CGFloat(Self.maxVisible) * 30 + 3 * 32 + 8)
    }

    /// Results in palette order, grouped by kind; the ranking holds within each group.
    private var groups: [(group: QuickOpenItem.Group, items: [QuickOpenItem])] {
        QuickOpenItem.Group.allCases.compactMap { group in
            let items = results.filter { $0.group == group }
            return items.isEmpty ? nil : (group, items)
        }
    }
}

struct QuickOpenRow: View {
    let item: QuickOpenItem
    let isHighlighted: Bool

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: item.systemImage)
                .frame(width: 18)
                .foregroundStyle(isHighlighted ? Color.white : .secondary)
                .accessibilityHidden(true)
            Text(item.title)
                .lineLimit(1)
                .truncationMode(.middle)
                .foregroundStyle(isHighlighted ? Color.white : .primary)
            Spacer(minLength: 8)
            if let subtitle = item.subtitle {
                Text(subtitle)
                    .font(.caption)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .foregroundStyle(isHighlighted ? Color.white.opacity(0.85) : .secondary)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 30)
        .background(isHighlighted ? Color.accentColor : .clear, in: RoundedRectangle(cornerRadius: 6))
        .padding(.horizontal, 6)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(isHighlighted ? [.isButton, .isSelected] : .isButton)
    }
}
