import Foundation

/// The Start Feature sheet's editable state: one value per sheet presentation, discarded on cancel (MAC-18), so
/// nothing typed for one start leaks into the next.
///
/// The request carries only the resolved slug as `name` (never a title), an explicit runtime, and a prompt the
/// CLI will not truncate.
public struct StartDraft: Sendable, Hashable {           // one value per sheet presentation; discarded on cancel (MAC-18)
    public var project: ProjectRef?; public var input: String; public var preview: NamePreview?; public var base: String?
    public var runtime: RuntimeProvider; public var mode: StartFeatureRequest.Mode; public var prompt: String
    public var useDefaultPrompt: Bool; public var branchPrefix: String; public var skipModules: Set<String>
    public var reuse: StartFeatureRequest.Reuse; public var keepRuntimeOnFailure: Bool; public var verbose: Bool
    public static let promptLimit = 2000

    /// The prefix the project's config applies on its own. While `branchPrefix` still equals it, the request
    /// leaves `--branch-prefix` out so the CLI keeps reading the project's setting.
    private let configuredBranchPrefix: String

    /// A fresh draft for `project`, with the runtime and branch prefix the project's config defaults to.
    public init(project: ProjectRef?, config: ProjectConfig? = nil) {
        let config = config ?? .defaults
        self.project = project
        self.input = ""
        self.preview = nil
        self.base = nil
        self.runtime = config.runtimeProvider
        self.mode = .full
        self.prompt = ""
        self.useDefaultPrompt = false
        self.branchPrefix = config.branchPrefix
        self.skipModules = []
        self.reuse = .none
        self.keepRuntimeOnFailure = false
        self.verbose = false
        self.configuredBranchPrefix = config.branchPrefix
    }

    /// A draft prefilled from an earlier request ("Edit and Retry", a remediation prefill).
    public init(prefill request: StartFeatureRequest, config: ProjectConfig? = nil) {
        self.init(project: request.project, config: config)
        input = request.name
        base = request.base
        runtime = request.runtime
        mode = request.mode
        prompt = request.prompt ?? ""
        useDefaultPrompt = request.useDefaultPrompt
        if let prefix = request.branchPrefix { branchPrefix = prefix }
        skipModules = Set(request.skipModules)
        reuse = request.reuse
        keepRuntimeOnFailure = request.keepRuntimeOnFailure
        verbose = request.verbose
    }

    /// The trimmed prompt, as the CLI stores it.
    public var trimmedPrompt: String { prompt.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// The prompt's length as the CLI counts it (Unicode scalars of the trimmed text) against `promptLimit`.
    public var promptLength: Int { trimmedPrompt.unicodeScalars.count }

    /// The preview for the current input, or nil while the debounced `previewName` has not answered for it yet.
    public var currentPreview: NamePreview? {
        preview.flatMap { $0.input == input ? $0 : nil }
    }

    /// The slug the request will carry: the preview's when it answered for the current input, else the CLI's
    /// own resolution rule applied locally.
    public var resolvedName: String? {
        if let slug = currentPreview?.slug { return slug }
        return NameRules.resolve(input)
    }

    /// The branch the feature gets with the current prefix.
    public var branchName: String? {
        resolvedName.map { NameRules.branchName(prefix: branchPrefix, slug: $0) }
    }

    /// Everything that blocks Start: the draft's own problems plus collisions with the project's records and an
    /// existing folder (`folderExists` is the caller's disk check of the would-be worktree path).
    public func validationErrors(existing: [FeatureRecord], folderExists: Bool) -> [String] {
        var errors = intrinsicErrors()
        guard let slug = resolvedName else { return errors }
        let duplicate = existing.contains { $0.workFeature == slug && $0.status != .removed }
        if duplicate {
            errors.removeAll { $0 == currentPreview?.problem }    // the record says it better than the preview
            errors.append("A feature named “\(slug)” already exists in this project")
        }
        if folderExists, !duplicate, reuse == .none {
            errors.append("The folder for “\(slug)” already exists; choose Reuse to start in it, or pick another name")
        }
        return errors
    }

    /// Information that does not block Start: a removed feature with the same name, or a branch that already
    /// exists (`branches` are local branch names, e.g. from `listBranches`).
    public func notices(existing: [FeatureRecord], branches: [String] = []) -> [String] {
        guard let slug = resolvedName, let branch = branchName else { return [] }
        var notices: [String] = []
        if existing.contains(where: { $0.workFeature == slug && $0.status == .removed }) {
            notices.append("A removed feature named “\(slug)” is still in the registry; starting replaces its entry")
        }
        if branches.contains(branch) {
            notices.append("Branch \(branch) already exists")
        }
        return notices
    }

    /// The request to dispatch, or nil while the draft has a problem of its own. Collisions with existing records
    /// are reported by `validationErrors(existing:folderExists:)`, which the sheet checks first.
    public func makeRequest() -> StartFeatureRequest? {
        guard intrinsicErrors().isEmpty, let project, let name = resolvedName else { return nil }
        var request = StartFeatureRequest(project: project, name: name, runtime: runtime)
        let base = base?.trimmingCharacters(in: .whitespacesAndNewlines)
        request.base = base?.isEmpty == false ? base : nil
        request.branchPrefix = branchPrefix == configuredBranchPrefix ? nil : branchPrefix
        request.mode = mode
        request.prompt = trimmedPrompt.isEmpty ? nil : trimmedPrompt
        request.useDefaultPrompt = useDefaultPrompt
        request.skipModules = skipModules.sorted()
        request.reuse = reuse
        request.keepRuntimeOnFailure = keepRuntimeOnFailure
        request.verbose = verbose
        return request
    }

    /// Problems that need no knowledge of the project's other features.
    private func intrinsicErrors() -> [String] {
        var errors: [String] = []
        if project == nil { errors.append("Choose a project") }
        if let preview = currentPreview, preview.slug == nil {
            errors.append(preview.problem ?? NameRules.noNameProblem(input))
        } else if let preview = currentPreview, !preview.valid {
            // previewName resolved a slug and rejected it (reserved, taken…): Start stays blocked.
            errors.append(preview.problem ?? NameRules.invalidSlugProblem(preview.slug ?? input))
        } else if let slug = resolvedName {
            if !NameRules.isValidSlug(slug) {
                errors.append(NameRules.invalidSlugProblem(slug))
            } else if let problem = NameRules.unusableSlugProblem(slug) {
                errors.append(problem)
            }
        } else {
            errors.append(NameRules.noNameProblem(input))
        }
        if promptLength > Self.promptLimit {
            let length = promptLength.formatted(.number.locale(Locale(identifier: "en_US")))
            errors.append("The prompt is \(length) characters; the limit is 2,000")
        }
        if useDefaultPrompt {
            if mode != .minimal { errors.append("The default prompt is only available in Quick mode") }
            if !trimmedPrompt.isEmpty { errors.append("Clear the prompt to use the default prompt") }
        }
        if keepRuntimeOnFailure, runtime != .sbx {
            errors.append("Keeping the environment after a failed start is only available for Docker Sandboxes")
        }
        if reuse == .retainedRuntime, runtime != .sbx {
            errors.append("Reusing a kept environment is only available for Docker Sandboxes")
        }
        switch runtime {
        case .container, .sbx, .localVM: break
        case .inGuest: errors.append("The in-guest runtime is started by a supervisor, not from BranchBox for Mac")
        case .unknown(let raw):
            errors.append(raw.isEmpty ? "Choose a runtime" : "“\(raw)” isn't a runtime this app can start")
        }
        if let problem = NameRules.branchPrefixProblem(branchPrefix) { errors.append(problem) }
        let unknownModules = skipModules.subtracting(NameRules.skippableModules).sorted()
        if !unknownModules.isEmpty {
            errors.append("Unknown module to skip: \(unknownModules.joined(separator: ", "))")
        }
        return errors
    }
}
