import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation
import Observation
import SwiftUI

/// What the Add Project sheet is doing: waiting for a folder, resolving one, or showing the outcome.
@MainActor @Observable final class AddProjectModel {
    enum Phase: Equatable {
        case choosing
        case resolving(URL)
        case finished(URL, AddProjectOutcome)
    }

    private(set) var phase: Phase = .choosing

    init(phase: Phase = .choosing) {
        self.phase = phase
    }

    /// Resolves `folder` through `projects.add`, which normalizes a feature worktree or container folder to the
    /// main worktree, and returns what happened.
    @discardableResult func add(_ folder: URL, to projects: ProjectsStore) async -> AddProjectOutcome {
        phase = .resolving(folder)
        let outcome = await projects.add(folder: folder)
        phase = .finished(folder, outcome)
        return outcome
    }

    func reset() { phase = .choosing }

    /// The intent that follows a finished add, if the sheet should hand over to another flow or the sidebar.
    static func followUp(for outcome: AddProjectOutcome) -> WindowIntent? {
        switch outcome {
        case .added(let ref, _), .alreadyPresent(let ref): .select(.project(path: ref.path))
        case .needsInit(let ref): .initProject(ref.root, mode: .setUp)
        case .refused: nil
        }
    }
}

/// Adds a project from a folder: choose it with an open panel or drop it, and the sheet explains the outcome —
/// added (with a note when a feature worktree or container folder was normalized), already in the sidebar, not
/// set up yet ([Set Up BranchBox…]), or refused with its cause.
struct AddProjectSheet: View {
    let initialFolder: URL?

    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var state: AddProjectModel
    @State private var isTargeted = false

    init(initialFolder: URL?) {
        self.initialFolder = initialFolder
        _state = State(initialValue: AddProjectModel())
    }

    /// Additive: a sheet in a given phase (render tests, previews).
    init(initialFolder: URL?, model: AddProjectModel) {
        self.initialFolder = initialFolder
        _state = State(initialValue: model)
    }

    var body: some View {
        ProjectSheetScaffold(title: "Add Project",
                             subtitle: "Choose a Git repository. A feature folder or its parent folder works too.",
                             systemImage: "folder.badge.plus") {
            content
                .padding(20)
        } footer: {
            footer
        }
        .frame(minWidth: 520, idealWidth: 520, minHeight: 360, idealHeight: 400)
        .task {
            if let initialFolder, state.phase == .choosing { await add(initialFolder) }
        }
    }

    @ViewBuilder private var content: some View {
        switch state.phase {
        case .choosing:
            dropZone
        case .resolving(let folder):
            VStack(spacing: 12) {
                ProgressView()
                Text("Checking \(folder.lastPathComponent)…").font(.headline)
                Text(folder.path)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .finished(let folder, let outcome):
            outcomeView(folder: folder, outcome: outcome)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
    }

    private var dropZone: some View {
        VStack(spacing: 12) {
            Image(systemName: "folder.badge.plus")
                .font(.system(size: 40, weight: .light))
                .foregroundStyle(isTargeted ? Color.accentColor : .secondary)
                .accessibilityHidden(true)
            Text("Drop a project folder here")
                .font(.headline)
            Text("or")
                .font(.callout)
                .foregroundStyle(.secondary)
            Button("Choose Folder…", action: choose)
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("addProject.choose")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(isTargeted ? Color.accentColor.opacity(0.08) : Color.clear, in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(isTargeted ? Color.accentColor : Color(nsColor: .separatorColor),
                              style: StrokeStyle(lineWidth: 1.5, dash: [6, 4]))
        )
        .dropDestination(for: URL.self) { urls, _ in
            guard let folder = urls.first else { return false }
            Task { await add(folder) }
            return true
        } isTargeted: { isTargeted = $0 }
    }

    @ViewBuilder private func outcomeView(folder: URL, outcome: AddProjectOutcome) -> some View {
        switch outcome {
        case .added(let ref, let note):
            // One card: a normalized folder explains itself in the message.
            ProjectNotice(style: .success, title: "Added \(Self.name(of: ref))",
                          message: note.map(Self.sentence) ?? ref.path)
        case .alreadyPresent(let ref):
            ProjectNotice(style: .info, title: "\(Self.name(of: ref)) is already in your sidebar", message: ref.path)
        case .needsInit(let ref):
            VStack(alignment: .leading, spacing: 12) {
                ProjectNotice(style: .info, title: "BranchBox isn't set up in \(Self.name(of: ref)) yet",
                             message: "Setting up adds a .branchbox folder and a dev container to the repository, so new features know how to run. You review everything before anything changes.")
                LabeledContent("Repository") {
                    Text(ref.path).lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                }
                .font(.callout)
            }
        case .refused(let error):
            let presentation = error.presentation()
            VStack(alignment: .leading, spacing: 12) {
                ProjectNotice(style: .error, title: Self.refusalTitle(error, folder: folder),
                              message: Self.refusalHint(error) ?? presentation.message)
                if !presentation.details.isEmpty {
                    Text(presentation.details.joined(separator: "\n"))
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
        }
    }

    @ViewBuilder private var footer: some View {
        switch state.phase {
        case .choosing:
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
        case .resolving:
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
                .disabled(true)
        case .finished(_, let outcome):
            switch outcome {
            case .added(let ref, _), .alreadyPresent(let ref):
                Button("Add Another…") {
                    state.reset()
                    choose()
                }
                Spacer()
                Button("Show Project") { finish(with: .select(.project(path: ref.path))) }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            case .needsInit(let ref):
                Button("Choose Another Folder…") {
                    state.reset()
                    choose()
                }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Set Up BranchBox…") { finish(with: .initProject(ref.root, mode: .setUp)) }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("addProject.setUp")
            case .refused:
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Choose Another Folder…") {
                    state.reset()
                    choose()
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            }
        }
    }

    // MARK: Actions

    private func choose() {
        guard let folder = ProjectActions.chooseFolder(title: "Add Project", prompt: "Add") else { return }
        Task { await add(folder) }
    }

    private func add(_ folder: URL) async {
        await state.add(folder, to: model.projects)
    }

    /// Hands over to the next flow or the sidebar: the router acts on the intent after this sheet is gone.
    private func finish(with intent: WindowIntent) {
        model.post(intent)
        dismiss()
    }

    // MARK: Text

    /// "branchbox" for `…/branchbox/main`, else the folder name.
    static func name(of ref: ProjectRef) -> String {
        ref.root.lastPathComponent == "main" ? ref.root.deletingLastPathComponent().lastPathComponent : ref.displayName
    }

    static func sentence(_ note: String) -> String {
        note.hasSuffix(".") ? note : note + "."
    }

    /// What to do instead, for refusals with an obvious next step.
    static func refusalHint(_ error: BackendError) -> String? {
        switch error {
        case .projectInvalid(.notGitRepository):
            "BranchBox works with Git repositories. Choose the folder that holds your project's .git folder, or any folder inside it."
        case .projectInvalid(.missing), .projectInvalid(.workingDirectoryMissing):
            "The folder may have been moved or renamed. Choose it again from its new location."
        default:
            nil
        }
    }

    static func refusalTitle(_ error: BackendError, folder: URL) -> String {
        switch error {
        case .projectInvalid(.notGitRepository): "\(folder.lastPathComponent) isn't a Git repository"
        case .projectInvalid(.missing), .projectInvalid(.workingDirectoryMissing): "\(folder.lastPathComponent) doesn't exist"
        case .cliNotFound, .cliTooOld, .cliUnusable: "The branchbox tool isn't ready"
        default: "Couldn't add \(folder.lastPathComponent)"
        }
    }
}

#Preview("Add project") {
    AddProjectSheet(initialFolder: nil)
        .environment(ProjectsPreviewModel.model())
}
