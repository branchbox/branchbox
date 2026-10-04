import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The menu bar extra's menu (`.menu` style; DESIGN §9 Menu bar, D-28). A native menu cannot present sheets or
/// alerts: anything that needs one opens the main window, activates the app and posts an intent there.
struct MenuBarContent: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    /// Features listed per project before "More in BranchBox…".
    static let featuresPerProject = 8
    /// Operations listed under Recent Activity.
    static let recentOperations = 3

    var body: some View {
        let ready = model.environment.identity != nil
        Text(MenuBarSummary.header(model: model))
        if case .unavailable(let error) = model.environment.backendState {
            Button {
                openWindow(id: SceneID.diagnostics)
                NSApp.activate()
            } label: {
                Label("\(error.presentation().title) — Open Diagnostics…", systemImage: "exclamationmark.triangle")
            }
        }
        Divider()

        let recent = Array(model.operations.records.prefix(Self.recentOperations))
        if !recent.isEmpty {
            Section("Recent Activity") {
                ForEach(recent) { record in
                    Button {
                        show(.showActivity(operation: record.id))
                    } label: {
                        Label(MenuBarSummary.operationLine(record), systemImage: record.state.symbol)
                    }
                }
            }
            Divider()
        }

        ForEach(model.projects.projects) { project in
            projectSection(project, ready: ready)
        }
        if !model.projects.projects.isEmpty { Divider() }

        Button("Start Feature…") { show(.startFeature(project: nil, prefill: nil)) }
            .keyboardShortcut("n")
            .disabled(!ready || model.projects.projects.isEmpty)
        Button("Open BranchBox") { WindowOpener.shared.showMain(to: model) }
        Button("Refresh") { model.projects.refreshAll(.manual) }
            .keyboardShortcut("r")
            .disabled(!ready || model.projects.projects.isEmpty)
        SettingsLink {
            Text("Settings…")
        }
        .keyboardShortcut(",")
        Divider()
        if let updated = MenuBarSummary.updatedLine(model: model) {
            Text(updated)
        }
        Button("Quit BranchBox") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }

    @ViewBuilder private func projectSection(_ project: ProjectStore, ready: Bool) -> some View {
        let features = project.features.filter { $0.status != .removed }
        let reasons = Dictionary(project.attention.map { ($0.featureOrPath, $0.reason) }, uniquingKeysWith: { first, _ in first })
        Section(project.displayName) {
            if !project.rootExists {
                Button("Folder missing — Locate…") {
                    show(.select(.project(path: project.ref.path)))
                    FeatureCommands.locate(project.ref, model: model)
                }
            } else if features.isEmpty {
                Text("No features")
            }
            ForEach(features.prefix(Self.featuresPerProject)) { record in
                Menu {
                    FeatureActionsMenu(feature: FeatureRef(project: project.ref, name: record.workFeature), style: .menuBar)
                } label: {
                    let attention = reasons[record.workFeature]
                    Label(MenuBarSummary.featureLine(record, attention: attention),
                          systemImage: attention?.symbol ?? record.status.symbol)
                }
            }
            if features.count > Self.featuresPerProject {
                Button("\(features.count - Self.featuresPerProject) More in BranchBox…") {
                    show(.select(.project(path: project.ref.path)))
                }
            }
            if ready, project.rootExists {
                Button("Start Feature in \(project.displayName)…") {
                    show(.startFeature(project: project.ref, prefill: nil))
                }
            }
        }
    }

    /// Opens the main window, activates the app and hands the window `intent`.
    private func show(_ intent: WindowIntent) {
        WindowOpener.shared.showMain(posting: intent, to: model)
    }
}

/// The menu's text, kept apart from the view for tests.
enum MenuBarSummary {
    /// "2 projects · 5 features · 1 needs attention" (or "· 1 running").
    @MainActor static func header(model: AppModel) -> String {
        let projects = model.projects.projects
        if projects.isEmpty { return model.hasStarted ? "No projects yet" : "BranchBox" }
        let features = projects.reduce(0) { $0 + $1.features.filter { $0.status != .removed }.count }
        return header(projects: projects.count, features: features, attention: model.projects.attentionCount,
                      running: model.operations.running.count)
    }

    static func header(projects: Int, features: Int, attention: Int, running: Int) -> String {
        var parts = [projects == 1 ? "1 project" : "\(projects) projects",
                     features == 1 ? "1 feature" : "\(features) features"]
        if attention > 0 { parts.append(attention == 1 ? "1 needs attention" : "\(attention) need attention") }
        if running > 0 { parts.append("\(running) running") }
        return parts.joined(separator: " · ")
    }

    /// "oauth — Active", "sbx-demo — Setup incomplete", or "oauth — Tearing down oauth" while an operation runs.
    static func featureLine(_ record: FeatureRecord, attention: AttentionReason?) -> String {
        "\(record.workFeature) — \(attention?.label ?? record.status.label)"
    }

    /// "Starting oauth — Running", "Tearing down prine — Failed".
    @MainActor static func operationLine(_ record: OperationRecord) -> String {
        "\(record.title) — \(record.state.label)"
    }

    /// "Updated 12 seconds ago", from the most recent successful refresh of any project.
    @MainActor static func updatedLine(model: AppModel, now: Date = .now) -> String? {
        guard let last = model.projects.projects.compactMap(\.lastLoadedAt).max() else { return nil }
        return updatedLine(last, now: now)
    }

    static func updatedLine(_ date: Date, now: Date) -> String {
        let seconds = Int(now.timeIntervalSince(date))
        if seconds < 5 { return "Updated just now" }
        if seconds < 60 { return "Updated \(seconds) s ago" }
        if seconds < 3600 { return "Updated \(seconds / 60) min ago" }
        return "Updated \(date.formatted(date: .omitted, time: .shortened))"
    }
}
