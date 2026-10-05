import BranchBoxKit
import Foundation
import Testing

@Suite struct StartDraftTests {
    private func draft(_ input: String = "oauth", runtime: RuntimeProvider = .container,
                       configure: (inout StartDraft) -> Void = { _ in }) -> StartDraft {
        var draft = StartDraft(project: Sample.project)
        draft.input = input
        draft.runtime = runtime
        configure(&draft)
        return draft
    }

    @Test func promptLimitIsInclusive() {
        let atLimit = draft { $0.prompt = String(repeating: "a", count: StartDraft.promptLimit) }
        #expect(atLimit.validationErrors(existing: [], folderExists: false).isEmpty)
        #expect(atLimit.makeRequest()?.prompt?.count == 2000)

        let over = draft { $0.prompt = String(repeating: "a", count: 2001) }
        #expect(over.validationErrors(existing: [], folderExists: false) == ["The prompt is 2,001 characters; the limit is 2,000"])
        #expect(over.makeRequest() == nil)
    }

    /// The CLI trims the prompt and counts Unicode scalars, so surrounding whitespace never pushes it over.
    @Test func promptIsCountedAsTheCLICountsIt() {
        let padded = draft { $0.prompt = "  \n" + String(repeating: "é", count: 2000) + "\n  " }
        #expect(padded.promptLength == 2000)
        #expect(padded.makeRequest()?.prompt == String(repeating: "é", count: 2000))

        let combining = draft { $0.prompt = String(repeating: "e\u{301}", count: 1001) }   // 1001 graphemes, 2002 scalars
        #expect(combining.promptLength == 2002)
        #expect(combining.makeRequest() == nil)
    }

    @Test func requestCarriesTheResolvedSlugNeverTheTitle() throws {
        let request = try #require(draft("Fix Critical Bug in Authentication").makeRequest())
        #expect(request.name == "fix-critical-bug")
        #expect(request.project == Sample.project)
        #expect(request.runtime == .container)
        #expect(request.prompt == nil)
        #expect(request.branchPrefix == nil)             // the project's own prefix applies
        #expect(request.base == nil)
        #expect(request.reuse == .none)
    }

    @Test func previewForTheCurrentInputWins() throws {
        var title = draft("OAuth Thing")
        title.preview = NamePreview(input: "OAuth Thing", slug: "oauth-thing-2", valid: true)
        #expect(try #require(title.makeRequest()).name == "oauth-thing-2")
        // A preview for an older input is ignored until the debounce catches up.
        title.input = "Billing"
        #expect(title.currentPreview == nil)
        #expect(try #require(title.makeRequest()).name == "billing")
    }

    @Test func previewProblemsBlock() {
        var noName = draft("???")
        noName.preview = NamePreview(input: "???", slug: nil, valid: false, problem: "Not a name")
        #expect(noName.validationErrors(existing: [], folderExists: false) == ["Not a name"])
        #expect(noName.makeRequest() == nil)

        var taken = draft("oauth")
        taken.preview = NamePreview(input: "oauth", slug: "oauth", valid: false, problem: "A feature named oauth already exists")
        #expect(taken.validationErrors(existing: [], folderExists: false) == ["A feature named oauth already exists"])
        // An invalid preview blocks the request too, not only the sheet.
        #expect(taken.makeRequest() == nil)
    }

    @Test(arguments: ["--reuse", "-x", "--leading dashes"])
    func aNameStartingWithAHyphenIsRefused(input: String) {
        let draft = draft(input)
        #expect(draft.validationErrors(existing: [], folderExists: false) == ["A feature name can't start with “-”"])
        #expect(draft.makeRequest() == nil)
    }

    @Test(arguments: [
        ("", "Enter a feature name or title"),
        ("The and", "“The and” has no words BranchBox can use for a name (filler words such as “the”, “and” and “feature” are dropped)"),
        ("!!", "“!!” has no letters or digits to make a feature name from"),
        ("snake_case", "“snake_case” isn't a valid feature name: use only lowercase letters, digits and hyphens"),
    ])
    func unusableNames(input: String, error: String) {
        let draft = draft(input)
        #expect(draft.validationErrors(existing: [], folderExists: false) == [error])
        #expect(draft.makeRequest() == nil)
    }

    @Test func collisions() {
        let active = Sample.record("oauth")
        let removed = Sample.record("oauth", status: .removed)
        let base = draft("oauth")

        #expect(base.validationErrors(existing: [active], folderExists: true) == ["A feature named “oauth” already exists in this project"])
        #expect(base.validationErrors(existing: [removed], folderExists: false).isEmpty)
        #expect(base.notices(existing: [removed])
            == ["A removed feature named “oauth” is still in the registry; starting replaces its entry"])
        #expect(base.notices(existing: [active]).isEmpty)

        #expect(base.validationErrors(existing: [], folderExists: true)
            == ["The folder for “oauth” already exists; choose Reuse to start in it, or pick another name"])
        let reusing = draft("oauth") { $0.reuse = .existingWorktree(.preserve) }
        #expect(reusing.validationErrors(existing: [], folderExists: true).isEmpty)
        let retained = draft("oauth", runtime: .sbx) { $0.reuse = .retainedRuntime }
        #expect(retained.validationErrors(existing: [], folderExists: true).isEmpty)
        let retainedVM = draft("oauth", runtime: .localVM) { $0.reuse = .retainedRuntime }
        #expect(retainedVM.validationErrors(existing: [], folderExists: true)
            == ["Reusing a kept environment is only available for Docker Sandboxes"])
        #expect(retainedVM.makeRequest() == nil)

        #expect(base.notices(existing: [], branches: ["main", "feature/oauth"]) == ["Branch feature/oauth already exists"])
        #expect(draft("") .notices(existing: [removed], branches: ["feature/oauth"]).isEmpty)
    }

    struct OptionCase: Sendable, CustomTestStringConvertible {
        let name: String
        let runtime: RuntimeProvider
        let mode: StartFeatureRequest.Mode
        let keepRuntime: Bool
        let defaultPrompt: Bool
        let prompt: String
        let errors: [String]
        var testDescription: String { name }
    }

    static let optionCases: [OptionCase] = [
        OptionCase(name: "keep sandbox on sbx", runtime: .sbx, mode: .full, keepRuntime: true, defaultPrompt: false,
                   prompt: "", errors: []),
        OptionCase(name: "keep runtime on container", runtime: .container, mode: .full, keepRuntime: true, defaultPrompt: false,
                   prompt: "", errors: ["Keeping the environment after a failed start is only available for Docker Sandboxes"]),
        OptionCase(name: "keep runtime on local-vm", runtime: .localVM, mode: .full, keepRuntime: true, defaultPrompt: false,
                   prompt: "", errors: ["Keeping the environment after a failed start is only available for Docker Sandboxes"]),
        OptionCase(name: "default prompt in quick mode", runtime: .container, mode: .minimal, keepRuntime: false,
                   defaultPrompt: true, prompt: "", errors: []),
        OptionCase(name: "default prompt in full mode", runtime: .container, mode: .full, keepRuntime: false,
                   defaultPrompt: true, prompt: "", errors: ["The default prompt is only available in Quick mode"]),
        OptionCase(name: "default prompt with a prompt", runtime: .container, mode: .minimal, keepRuntime: false,
                   defaultPrompt: true, prompt: "do it", errors: ["Clear the prompt to use the default prompt"]),
        OptionCase(name: "in-guest", runtime: .inGuest, mode: .full, keepRuntime: false, defaultPrompt: false, prompt: "",
                   errors: ["The in-guest runtime is started by a supervisor, not from BranchBox for Mac"]),
        OptionCase(name: "no runtime", runtime: .unknown(""), mode: .full, keepRuntime: false, defaultPrompt: false,
                   prompt: "", errors: ["Choose a runtime"]),
        OptionCase(name: "unknown runtime", runtime: .unknown("firecracker"), mode: .full, keepRuntime: false,
                   defaultPrompt: false, prompt: "", errors: ["“firecracker” isn't a runtime this app can start"]),
    ]

    @Test(arguments: StartDraftTests.optionCases)
    func optionValidation(_ testCase: OptionCase) {
        let draft = draft("oauth", runtime: testCase.runtime) {
            $0.mode = testCase.mode
            $0.keepRuntimeOnFailure = testCase.keepRuntime
            $0.useDefaultPrompt = testCase.defaultPrompt
            $0.prompt = testCase.prompt
        }
        #expect(draft.validationErrors(existing: [], folderExists: false) == testCase.errors)
        #expect((draft.makeRequest() == nil) == !testCase.errors.isEmpty)
        if let request = draft.makeRequest() {
            #expect(request.runtime == testCase.runtime)  // always explicit
            #expect(request.keepRuntimeOnFailure == testCase.keepRuntime)
            #expect(request.useDefaultPrompt == testCase.defaultPrompt)
            #expect(request.mode == testCase.mode)
        }
    }

    @Test func advancedOptionsReachTheRequest() throws {
        let draft = draft("oauth", runtime: .sbx) {
            $0.base = "  origin/main "
            $0.branchPrefix = "spike"
            $0.skipModules = ["tunnel", "compose"]
            $0.verbose = true
            $0.prompt = "  Build the OAuth flow  "
            $0.reuse = .existingWorktree(.inspect)
        }
        let request = try #require(draft.makeRequest())
        #expect(request.base == "origin/main")
        #expect(request.branchPrefix == "spike")
        #expect(request.skipModules == ["compose", "tunnel"])
        #expect(request.verbose)
        #expect(request.prompt == "Build the OAuth flow")
        #expect(request.reuse == .existingWorktree(.inspect))
        #expect(draft.branchName == "spike/oauth")

        let emptyPrefix = self.draft("oauth") { $0.branchPrefix = "" }
        #expect(try #require(emptyPrefix.makeRequest()).branchPrefix == "")
        #expect(emptyPrefix.branchName == "oauth")
        #expect(self.draft("oauth") { $0.base = "   " }.makeRequest()?.base == nil)
    }

    @Test func badAdvancedOptionsBlock() {
        let draft = draft("oauth") {
            $0.branchPrefix = "my prefix"
            $0.skipModules = ["tunnel", "warp-drive"]
        }
        #expect(draft.validationErrors(existing: [], folderExists: false) == [
            "A branch prefix can only use letters, digits, “.”, “_”, “-” and “/”, not spaces",
            "Unknown module to skip: warp-drive",
        ])
        #expect(draft.makeRequest() == nil)
    }

    @Test func projectIsRequired() {
        var draft = StartDraft(project: nil)
        draft.input = "oauth"
        #expect(draft.validationErrors(existing: [], folderExists: false) == ["Choose a project"])
        #expect(draft.makeRequest() == nil)
    }

    /// Each presentation starts from the project's defaults; nothing typed before survives (MAC-18).
    @Test func eachPresentationStartsFresh() {
        let config = ProjectConfig(runtimeProvider: .sbx, branchPrefix: "spike")
        var first = StartDraft(project: Sample.project, config: config)
        first.input = "oauth"
        first.prompt = "secret plan"
        first.keepRuntimeOnFailure = true
        first.skipModules = ["tunnel"]

        let second = StartDraft(project: Sample.project, config: config)
        #expect(second != first)
        #expect(second.input.isEmpty)
        #expect(second.prompt.isEmpty)
        #expect(second.preview == nil)
        #expect(second.skipModules.isEmpty)
        #expect(second.keepRuntimeOnFailure == false)
        #expect(second.runtime == .sbx)
        #expect(second.branchPrefix == "spike")
        #expect(second.mode == .full)
        #expect(second.reuse == .none)
        #expect(StartDraft(project: Sample.project).runtime == .container)
        #expect(StartDraft(project: Sample.project).branchPrefix == "feature")
    }

    @Test func prefillRoundTrips() throws {
        var original = StartFeatureRequest(project: Sample.project, name: "oauth", runtime: .sbx)
        original.base = "main"
        original.branchPrefix = "spike"
        original.mode = .minimal
        original.prompt = "Retry it"
        original.skipModules = ["specs"]
        original.reuse = .retainedRuntime
        original.verbose = true
        let draft = StartDraft(prefill: original)
        #expect(draft.input == "oauth")
        #expect(try #require(draft.makeRequest()) == original)

        // A prefill without an explicit prefix keeps the project's.
        let plain = StartFeatureRequest(project: Sample.project, name: "oauth", runtime: .container)
        #expect(StartDraft(prefill: plain, config: ProjectConfig(branchPrefix: "team")).makeRequest() == plain)
    }
}
