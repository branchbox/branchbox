import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Observation
import SwiftUI

/// Project Settings' state: the config form, the review of a dry run, and the save operations. The Cloudflare
/// token lives only here, between typing and `tunnel credentials set` (D-20); it is never stored by the app.
@MainActor @Observable final class ProjectSettingsModel {
    enum Phase: Equatable {
        case loading
        case failedToLoad(BackendError)
        case editing
        case reviewing([ConfigApplyResult.Change])
        case saving
    }

    /// How the save operations went, read from their records.
    enum SaveState: Equatable { case idle, running, succeeded, failed(BackendError, OperationRequestContext) }

    let project: ProjectRef
    var tab: ConfigForm.Tab = .features
    var form: ConfigForm?
    private(set) var document: ProjectConfigDocument?
    private(set) var phase: Phase
    /// The token being typed; cleared as soon as it is handed to the CLI.
    var apiToken = ""
    private(set) var keyErrors: [String: String] = [:]
    private(set) var problem: String?
    private(set) var saveRecords: [OperationRecord] = []
    /// The token waits here until the config change it depends on (the account ID) has been saved.
    private(set) var pendingCredentials: TunnelCredentialsRequest?
    private(set) var isReviewing = false

    init(project: ProjectRef, document: ProjectConfigDocument? = nil) {
        self.project = project
        if let document {
            self.document = document
            form = ConfigForm(document: document)
            phase = .editing
        } else {
            phase = .loading
        }
    }

    var isEditable: Bool { form?.editable ?? false }
    var hasToken: Bool { !apiToken.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var canSave: Bool { isEditable && ((form?.hasChanges ?? false) || hasToken) && (form?.problems.isEmpty ?? true) }

    /// A token is already saved for this project (the config names its file).
    var tokenSaved: Bool {
        guard case .string(let path)? = form?.original["tunnel.providers.cloudflared.api_token_path"] else { return false }
        return !path.isEmpty
    }

    func load(using model: AppModel) async {
        guard let store = model.projects.project(project) else {
            phase = .failedToLoad(.projectInvalid(.missing(project.path)))
            return
        }
        await store.reloadConfig()
        if let document = store.config {
            self.document = document
            form = ConfigForm(document: document)
            phase = .editing
        } else {
            phase = .failedToLoad(store.configError ?? .projectInvalid(.notInitialized(project.path)))
        }
    }

    /// Dry-runs the patch (`config apply --dry-run`) and shows what would change. Never runs on a read-only form.
    func review(using model: AppModel) async {
        guard let form, form.editable, canSave else { return }
        keyErrors = [:]
        problem = nil
        guard form.hasChanges else {
            phase = .reviewing([])
            return
        }
        isReviewing = true
        defer { isReviewing = false }
        do {
            let result = try await model.backend().applyConfig(form.patch, to: project, dryRun: true)
            phase = .reviewing(result.changed.isEmpty ? Self.changes(from: form) : result.changed)
        } catch {
            show(BackendError.normalize(error))
        }
    }

    /// Applies the patch, then hands the token to `tunnel credentials set` once the patch succeeded: a refused
    /// config change must not leave the token saved against an account ID that wasn't. The typed token is cleared
    /// once the credentials operation is admitted, and kept when it never is.
    func apply(using model: AppModel) {
        guard let form, form.editable else { return }
        problem = nil
        let credentials = credentialsRequest()
        if form.hasChanges {
            guard let record = dispatch(.applyConfig(form.patch, project), using: model) else { return }
            saveRecords = [record]
            phase = .saving
            if let credentials {
                pendingCredentials = credentials
                Task { await sendCredentialsAfter(record, using: model) }
            }
        } else if let credentials {
            guard let record = dispatch(.tunnelCredentials(credentials, project), using: model) else { return }
            apiToken = ""
            saveRecords = [record]
            phase = .saving
        }
    }

    private func sendCredentialsAfter(_ config: OperationRecord, using model: AppModel) async {
        while config.isCancellable { try? await Task.sleep(for: .milliseconds(50)) }
        guard let request = pendingCredentials else { return }
        pendingCredentials = nil
        switch config.state {
        case .succeeded, .succeededWithWarnings:
            if let record = dispatch(.tunnelCredentials(request, project), using: model) {
                apiToken = ""
                saveRecords.append(record)
            }
        default:
            break                                   // the config failure is shown; the token stays typed
        }
    }

    /// Removes the saved token (`tunnel credentials set --clear`).
    func clearToken(using model: AppModel) {
        guard isEditable else { return }
        let request = TunnelCredentialsRequest(accountID: accountID, apiToken: nil, clear: true)
        if let record = dispatch(.tunnelCredentials(request, project), using: model) {
            saveRecords = [record]
            phase = .saving
        }
    }

    /// The credentials request for the typed token, or nil without one.
    func credentialsRequest() -> TunnelCredentialsRequest? {
        guard hasToken else { return nil }
        return TunnelCredentialsRequest(accountID: accountID, apiToken: SecretString(apiToken.trimmingCharacters(in: .whitespacesAndNewlines)))
    }

    var accountID: String {
        if case .string(let id)? = form?.value("tunnel.providers.cloudflared.account_id") { return id }
        return ""
    }

    var saveState: SaveState {
        guard !saveRecords.isEmpty else { return .idle }
        if pendingCredentials != nil, saveRecords.allSatisfy({ !$0.isCancellable && $0.failure == nil }) { return .running }
        for record in saveRecords {
            switch record.state {
            case .queued, .running: return .running
            case .failed(let error): return .failed(error, record.context)
            case .cancelled(let note): return .failed(.cancelled(note: note), record.context)
            case .succeeded, .succeededWithWarnings, .partial: continue
            }
        }
        return .succeeded
    }

    /// After the save operations finished: success reloads the config; a failure goes back to editing with the
    /// error next to its key when the CLI named one.
    func saveFinished(using model: AppModel) async -> Bool {
        switch saveState {
        case .idle, .running:
            return false
        case .succeeded:
            saveRecords = []
            await model.projects.project(project)?.reloadConfig()
            return true
        case .failed(let error, _):
            saveRecords = []
            show(error)
            return false
        }
    }

    func backToEditing() {
        phase = .editing
    }

    private func show(_ error: BackendError) {
        phase = form == nil ? .failedToLoad(error) : .editing
        if case .refused(let refusal) = error, case .configInvalid(let key?, let detail) = refusal.cause {
            keyErrors[key] = detail.isEmpty ? refusal.message : detail
            tab = ConfigForm.Tab.of(key)
        } else {
            problem = error.presentation().message
        }
    }

    private func dispatch(_ request: OperationRequestContext, using model: AppModel) -> OperationRecord? {
        switch model.actions.dispatch(request) {
        case .started(let record), .queued(let record, _):
            return record
        case .rejected(let reason):
            problem = reason
        case .unavailable(let error):
            problem = error.presentation().message
        }
        return nil
    }

    /// The patch as review rows when the CLI reported no changes list.
    private static func changes(from form: ConfigForm) -> [ConfigApplyResult.Change] {
        form.patch.changes.map { ConfigApplyResult.Change(key: $0.key, old: form.original[$0.key], new: $0.value) }
    }
}

/// Edits the project's .branchbox/config.json through `config apply`: a form built from the CLI's key table in
/// five tabs, a dry-run review of the changes, then the apply. The Cloudflare API token goes to `tunnel
/// credentials set` on stdin. On a CLI without config support everything is read-only.
struct ProjectSettingsSheet: View {
    let project: ProjectRef

    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var sheet: ProjectSettingsModel
    @State private var confirmingClearToken = false
    @State private var showsEditorIntegration = false

    init(project: ProjectRef) {
        self.project = project
        _sheet = State(initialValue: ProjectSettingsModel(project: project))
    }

    /// Additive: a sheet with prepared state (render tests, previews).
    init(model: ProjectSettingsModel) {
        project = model.project
        _sheet = State(initialValue: model)
    }

    private var projectName: String { model.projects.project(project)?.displayName ?? project.displayName }

    var body: some View {
        ProjectSheetScaffold(title: "\(projectName) Settings",
                             subtitle: "Saved in .branchbox/config.json. If that file is committed to Git, your changes apply to everyone.",
                             systemImage: "gearshape") {
            content
        } footer: {
            footer
        }
        .frame(minWidth: 620, idealWidth: 620, minHeight: 520, idealHeight: 600)
        .task {
            if sheet.form == nil { await sheet.load(using: model) }
        }
        .onChange(of: sheet.keyErrors) { _, errors in
            // A refused VS Code key opens its disclosure so the error is visible.
            if errors.keys.contains(where: ConfigForm.editorIntegrationKeys.contains) { showsEditorIntegration = true }
        }
        .onChange(of: sheet.saveState) { _, state in
            guard state != .running, state != .idle else { return }
            Task { if await sheet.saveFinished(using: model) { dismiss() } }
        }
        .confirmationDialog("Remove the saved Cloudflare token?", isPresented: $confirmingClearToken, titleVisibility: .visible) {
            Button("Remove Token", role: .destructive) { sheet.clearToken(using: model) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("New tunnels will need manual setup until you add a token again.")
        }
    }

    // MARK: Content

    @ViewBuilder private var content: some View {
        switch sheet.phase {
        case .loading:
            ProgressView("Reading settings…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failedToLoad(let error):
            VStack {
                ErrorBanner(error: error, onRetry: { Task { await sheet.load(using: model) } })
                Spacer()
            }
            .padding(20)
        case .editing:
            editor
        case .reviewing(let changes):
            review(changes)
        case .saving:
            VStack(spacing: 12) {
                ProgressView()
                Text("Saving…").font(.headline)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder private var editor: some View {
        if let form = sheet.form {
            VStack(spacing: 0) {
                Picker("Section", selection: $sheet.tab) {
                    ForEach(ConfigForm.Tab.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(.horizontal, 20)
                .padding(.top, 14)
                if !form.editable {
                    ProjectNotice(style: .info, title: "Requires BranchBox CLI with config support",
                                  message: "This version of branchbox can't change settings from the app. Update it, or edit the file yourself.") {
                        Button("Open config.json") { ProjectActions.open(sheet.document?.path ?? "") }
                            .disabled(sheet.document?.exists != true)
                        CopyButton(text: CLITooOldView.upgradeCommand, label: "Copy Update Command", showsTitle: true)
                    }
                    .padding(.horizontal, 20)
                    .padding(.top, 14)
                }
                Form {
                    if let problem = sheet.problem {
                        Section { ProjectNotice(style: .error, title: "Couldn't save", message: problem) }
                    }
                    let fields = form.fields(in: sheet.tab)
                    if fields.isEmpty, sheet.tab != .sharing {
                        Section { Text("Nothing to set here with this version of branchbox.").foregroundStyle(.secondary) }
                    } else if !fields.isEmpty {
                        let main = fields.filter { !$0.isEditorIntegration }
                        let integration = fields.filter(\.isEditorIntegration)
                        if !main.isEmpty {
                            Section {
                                ForEach(main) { field in
                                    ConfigFieldRow(field: field, form: formBinding,
                                                   error: sheet.keyErrors[field.key] ?? form.problem(for: field.key))
                                }
                            } footer: {
                                tabFooter
                            }
                        }
                        if !integration.isEmpty {
                            Section {
                                DisclosureGroup(isExpanded: $showsEditorIntegration) {
                                    ForEach(integration) { field in
                                        ConfigFieldRow(field: field, form: formBinding,
                                                       error: sheet.keyErrors[field.key] ?? form.problem(for: field.key))
                                    }
                                } label: {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text("VS Code integration")
                                        Text("How VS Code behaves when it opens a feature's dev container.")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                                .accessibilityIdentifier("projectSettings.editorIntegration")
                            }
                        }
                    }
                    if sheet.tab == .sharing { tokenSection }
                }
                .formStyle(.grouped)
            }
        }
    }

    @ViewBuilder private var tabFooter: some View {
        switch sheet.tab {
        case .features:
            Text("New features start on “prefix/name”. Leave the prefix empty to use the default (“feature”); branches always have a prefix.")
        case .teardown:
            Text("Defaults for the Tear Down sheet; you can still choose per feature.")
        case .runtime:
            Text("Container runs features in Docker on this Mac. Docker Sandbox runs each one in its own micro-VM.")
        case .sharing, .codingAgent:
            EmptyView()
        }
    }

    @ViewBuilder private var tokenSection: some View {
        let supported = model.environment.supports(.tunnelCredentials)
        Section {
            LabeledContent("API token") {
                SecureField("API token", text: $sheet.apiToken,
                            prompt: Text(sheet.tokenSaved ? "Saved; type to replace" : "Paste a Cloudflare API token"))
                    .textFieldStyle(.roundedBorder)
                    .labelsHidden()
                    .frame(width: 240)
            }
                .disabled(!sheet.isEditable || !supported)
                .accessibilityIdentifier("projectSettings.apiToken")
            if sheet.tokenSaved {
                LabeledContent("Saved token") {
                    HStack {
                        Label("Saved", systemImage: "checkmark.seal.fill").foregroundStyle(.green)
                        Button("Remove…") { confirmingClearToken = true }
                            .disabled(!sheet.isEditable || !supported)
                    }
                }
            }
            if sheet.hasToken, sheet.accountID.isEmpty {
                Label("Enter the Cloudflare account ID above so the token can be saved.", systemImage: "exclamationmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
        } header: {
            Text("Cloudflare API token")
        } footer: {
            Text(supported
                 ? "Stored in .branchbox/secure/cloudflared.env, readable only by you. BranchBox for Mac never keeps a copy."
                 : "Requires BranchBox CLI with tunnel credentials support.")
        }
    }

    private var formBinding: Binding<ConfigForm> {
        Binding(get: { sheet.form ?? ConfigForm(document: ProjectConfigDocument(path: "", exists: false, effective: .defaults, editable: false)) },
                set: { sheet.form = $0 })
    }

    private func review(_ changes: [ConfigApplyResult.Change]) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Review changes").font(.headline)
            Text("Nothing is saved until you apply.")
                .font(.callout)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(changes.enumerated()), id: \.offset) { index, change in
                    if index > 0 { Divider() }
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        // The config key is developer detail: it stays in the help tag, like the rest of the settings.
                        Text(ConfigForm.label(for: change.key))
                            .help(change.key)
                        Spacer(minLength: 12)
                        Text(ConfigForm.display(change.old))
                            .foregroundStyle(.secondary)
                            .strikethrough()
                        Image(systemName: "arrow.right").foregroundStyle(.secondary).accessibilityLabel("changes to")
                        Text(ConfigForm.display(change.new)).fontWeight(.medium)
                    }
                    .padding(.vertical, 8)
                    .padding(.horizontal, 12)
                    .accessibilityElement(children: .combine)
                }
                if sheet.hasToken {
                    if !changes.isEmpty { Divider() }
                    HStack {
                        Text("Cloudflare API token")
                        Spacer()
                        Text(sheet.tokenSaved ? "Replaced" : "Saved").fontWeight(.medium)
                    }
                    .padding(.vertical, 8)
                    .padding(.horizontal, 12)
                }
            }
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor)))
            Spacer()
        }
        .padding(20)
    }

    // MARK: Footer

    @ViewBuilder private var footer: some View {
        switch sheet.phase {
        case .reviewing:
            Button("Back") { sheet.backToEditing() }
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Apply") { sheet.apply(using: model) }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("projectSettings.apply")
        case .saving:
            Spacer()
            Button("Cancel", role: .cancel) {}
                .disabled(true)
        case .loading, .failedToLoad, .editing:
            if sheet.isEditable {
                Button("Revert") { sheet.form?.revert(); sheet.apiToken = "" }
                    .disabled(!(sheet.form?.hasChanges ?? false) && !sheet.hasToken)
                Spacer()
                if sheet.isReviewing { ProgressView().controlSize(.small) }
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save…") { Task { await sheet.review(using: model) } }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!sheet.canSave || sheet.isReviewing || (sheet.hasToken && sheet.accountID.isEmpty))
                    .accessibilityIdentifier("projectSettings.save")
            } else {
                Spacer()
                Button("Done") { dismiss() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}

#Preview("Project settings") {
    ProjectSettingsSheet(project: PreviewSamples.project)
        .environment(ProjectsPreviewModel.model())
}
