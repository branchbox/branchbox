import Foundation

/// The CLI's feature-naming rules, ported so the Start sheet can show a name before `previewName` answers and
/// so a request always carries the slug the CLI would pick (core `naming.rs`, `build_branch_name` and
/// `worktree_path` in `workflows/feature.rs`).
public enum NameRules {
    /// Words core drops when it turns a title into a slug (`FILLER_WORDS`).
    public static let fillerWords: Set<String> = [
        "the", "a", "an", "and", "or", "for", "with", "using", "feature", "integration", "implementation",
        "support", "system", "in", "of", "to",
    ]

    /// Most words a slug generated from a title keeps (`MAX_WORDS`).
    public static let maxWords = 3

    /// The modules `feature start --skip-module` accepts.
    public static let skippableModules: Set<String> = ["compose", "database", "tunnel", "specs"]

    /// `^[a-z0-9-]+$`: a name `feature start` uses exactly as typed (`validate_work_feature`).
    public static func isValidSlug(_ name: String) -> Bool {
        !name.isEmpty && name.unicodeScalars.allSatisfy { isLowercaseASCIILetterOrDigit($0) || $0 == "-" }
    }

    /// Core `generate_work_feature`: lowercase, drop every character but `a-z 0-9 space _ -`, drop filler words
    /// and join the first three remaining words with hyphens. "Fix Critical Bug in Authentication" →
    /// "fix-critical-bug". The result can be empty, and it can still be invalid (an underscore survives).
    public static func slug(fromTitle title: String) -> String {
        let kept = title.lowercased().unicodeScalars.filter { scalar in
            isLowercaseASCIILetterOrDigit(scalar) || scalar == " " || scalar == "_" || scalar == "-"
        }
        let words = String(String.UnicodeScalarView(kept)).split(separator: " ").map(String.init)
        return words.filter { !fillerWords.contains($0) }.prefix(maxWords).joined(separator: "-")
    }

    /// Core `resolve_work_feature`: a valid slug is used as typed; anything else is read as a title. nil when the
    /// input yields no name at all (the CLI refuses it with `invalid_feature_name`).
    public static func resolve(_ input: String) -> String? {
        if isValidSlug(input) { return input }
        let generated = slug(fromTitle: input)
        return generated.isEmpty ? nil : generated
    }

    /// Core `build_branch_name`: the prefix keeps only `A-Z a-z 0-9 . _ - /` and loses trailing slashes; an
    /// empty prefix gives the bare name. nil means the CLI default, "feature".
    public static func branchName(prefix: String?, slug: String) -> String {
        let sanitized = String(String.UnicodeScalarView((prefix ?? "feature").unicodeScalars.filter(isBranchPrefixScalar)))
        var trimmed = Substring(sanitized)
        while trimmed.hasSuffix("/") { trimmed = trimmed.dropLast() }
        return trimmed.isEmpty ? slug : "\(trimmed)/\(slug)"
    }

    /// Core `worktree_path`: features live next to the main worktree, in `<parent of root>/<slug>`.
    public static func worktreePath(for slug: String, in project: ProjectRef) -> String {
        project.root.deletingLastPathComponent().appendingPathComponent(slug).path
    }

    /// What `previewName` would answer, computed locally: the resolved slug, its branch and its folder, or the
    /// reason there is no usable name.
    public static func preview(_ input: String, in project: ProjectRef, branchPrefix: String?) -> NamePreview {
        guard let slug = resolve(input) else {
            return NamePreview(input: input, slug: nil, valid: false, problem: noNameProblem(input))
        }
        guard isValidSlug(slug) else {
            return NamePreview(input: input, slug: slug, valid: false, problem: invalidSlugProblem(slug))
        }
        return NamePreview(input: input, slug: slug, valid: true, branchName: branchName(prefix: branchPrefix, slug: slug),
                           worktreePath: worktreePath(for: slug, in: project))
    }

    /// Why `prefix` would not make a usable branch prefix, or nil when it would. Core silently drops characters
    /// outside `A-Z a-z 0-9 . _ - /`, so those are reported instead of being sent; the rest is the part of
    /// `git check-ref-format` a prefix can break. An empty prefix is fine: the branch is the bare name.
    public static func branchPrefixProblem(_ prefix: String) -> String? {
        if let bad = prefix.unicodeScalars.first(where: { !isBranchPrefixScalar($0) }) {
            let shown = bad.properties.isWhitespace ? "spaces" : "“\(Character(bad))”"
            return "A branch prefix can only use letters, digits, “.”, “_”, “-” and “/”, not \(shown)"
        }
        if prefix.hasPrefix("/") || prefix.hasPrefix("-") || prefix.hasPrefix(".") {
            return "A branch prefix can't start with “\(prefix.prefix(1))”"
        }
        if prefix.contains("..") { return "A branch prefix can't contain “..”" }
        if prefix.contains("//") { return "A branch prefix can't contain “//”" }
        let components = prefix.split(separator: "/")
        if components.contains(where: { $0.hasPrefix(".") || $0.hasSuffix(".") || $0.hasSuffix(".lock") }) {
            return "A branch prefix part can't start or end with “.” or end with “.lock”"
        }
        return nil
    }

    /// App-side rule on top of core's: a slug that starts with `-` would be read as a flag by `feature start` and
    /// `feature teardown` (and is not DNS-safe for feature URLs). nil when the slug is usable.
    public static func unusableSlugProblem(_ slug: String) -> String? {
        slug.hasPrefix("-") ? "A feature name can't start with “-”" : nil
    }

    static func noNameProblem(_ input: String) -> String {
        if input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "Enter a feature name or title" }
        let hasLettersOrDigits = input.lowercased().unicodeScalars.contains(where: isLowercaseASCIILetterOrDigit)
        return hasLettersOrDigits
            ? "“\(input)” has no words BranchBox can use for a name (filler words such as “the”, “and” and “feature” are dropped)"
            : "“\(input)” has no letters or digits to make a feature name from"
    }

    static func invalidSlugProblem(_ slug: String) -> String {
        "“\(slug)” isn't a valid feature name: use only lowercase letters, digits and hyphens"
    }

    private static func isLowercaseASCIILetterOrDigit(_ scalar: Unicode.Scalar) -> Bool {
        ("a"..."z").contains(scalar) || ("0"..."9").contains(scalar)
    }

    private static func isBranchPrefixScalar(_ scalar: Unicode.Scalar) -> Bool {
        scalar.isASCII && (scalar.properties.isAlphabetic || ("0"..."9").contains(scalar) || ".-_/".unicodeScalars.contains(scalar))
    }
}
