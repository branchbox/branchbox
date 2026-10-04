import BranchBoxKit
import Foundation
import Testing

@Suite struct NameRulesTests {
    /// core `naming.rs` doc tests and unit tests, plus the edges of its character filter.
    @Test(arguments: [
        ("OpenAI Responses API Integration", "openai-responses-api"),
        ("OAuth Connected Apps Feature", "oauth-connected-apps"),
        ("Fix Critical Bug in Authentication", "fix-critical-bug"),
        ("Support for Multiple LLM Providers", "multiple-llm-providers"),
        ("OAuth Integration Feature", "oauth"),
        ("OAuth-Integration", "oauth-integration"),
        ("  Spaces   everywhere  ", "spaces-everywhere"),
        ("Café: add crème brûlée!", "caf-add-crme"),
        ("snake_case title", "snake_case-title"),
        ("tab\tseparated", "tabseparated"),
        ("The a an and", ""),
        ("!!!", ""),
        ("", ""),
    ])
    func slugFromTitle(title: String, slug: String) {
        #expect(NameRules.slug(fromTitle: title) == slug)
    }

    @Test(arguments: [
        ("oauth-integration", true), ("api-v2", true), ("bug-fix-123", true), ("-x", true),
        ("OAuth-Integration", false), ("oauth_integration", false), ("oauth integration", false),
        ("oauth.integration", false), ("", false), ("é", false),
    ])
    func validSlugs(name: String, valid: Bool) {
        #expect(NameRules.isValidSlug(name) == valid)
    }

    @Test(arguments: [
        ("oauth", "oauth"),                              // valid names are used exactly as typed
        ("the-feature", "the-feature"),                  // even when a title would drop the words
        ("OAuth Integration", "oauth"),
        ("Bad Name", "bad-name"),
    ])
    func resolve(input: String, slug: String) {
        #expect(NameRules.resolve(input) == slug)
    }

    @Test func resolveGivesNilWithoutAName() {
        #expect(NameRules.resolve("") == nil)
        #expect(NameRules.resolve("The and of") == nil)
    }

    @Test(arguments: [
        (String?.none, "feature/oauth"), ("feature", "feature/oauth"), ("", "oauth"), ("spike/", "spike/oauth"),
        ("spike//", "spike/oauth"), ("team/rida", "team/rida/oauth"), ("we ird$", "weird/oauth"),
    ])
    func branchName(prefix: String?, branch: String) {
        #expect(NameRules.branchName(prefix: prefix, slug: "oauth") == branch)
    }

    @Test func worktreeSitsNextToTheMainWorktree() {
        #expect(NameRules.worktreePath(for: "oauth", in: Sample.project) == "/tmp/bbx/oauth")
        #expect(NameRules.worktreePath(for: "oauth", in: ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx/main/"))) == "/tmp/bbx/oauth")
    }

    @Test func preview() {
        #expect(NameRules.preview("OAuth Integration", in: Sample.project, branchPrefix: "spike")
            == NamePreview(input: "OAuth Integration", slug: "oauth", valid: true, branchName: "spike/oauth",
                           worktreePath: "/tmp/bbx/oauth"))
        #expect(NameRules.preview("  ", in: Sample.project, branchPrefix: nil)
            == NamePreview(input: "  ", slug: nil, valid: false, problem: "Enter a feature name or title"))
        #expect(NameRules.preview("!!", in: Sample.project, branchPrefix: nil).problem
            == "“!!” has no letters or digits to make a feature name from")
        let underscore = NameRules.preview("snake_case", in: Sample.project, branchPrefix: nil)
        #expect(underscore.slug == "snake_case")
        #expect(!underscore.valid)
        #expect(underscore.problem?.contains("lowercase letters, digits and hyphens") == true)
    }

    @Test(arguments: [
        ("feature", nil), ("", nil), ("team/rida", nil), ("v1.2", nil), ("spike/", nil),
        ("has space", "A branch prefix can only use letters, digits, “.”, “_”, “-” and “/”, not spaces"),
        ("a~b", "A branch prefix can only use letters, digits, “.”, “_”, “-” and “/”, not “~”"),
        ("/abs", "A branch prefix can't start with “/”"),
        ("-dash", "A branch prefix can't start with “-”"),
        (".hidden", "A branch prefix can't start with “.”"),
        ("a..b", "A branch prefix can't contain “..”"),
        ("a//b", "A branch prefix can't contain “//”"),
        ("team/x.lock", "A branch prefix part can't start or end with “.” or end with “.lock”"),
        ("team/.x", "A branch prefix part can't start or end with “.” or end with “.lock”"),
    ] as [(String, String?)])
    func branchPrefixProblems(prefix: String, problem: String?) {
        #expect(NameRules.branchPrefixProblem(prefix) == problem)
    }
}
