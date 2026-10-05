import BranchBoxKit
import Foundation

/// Parsers for the human-readable output of legacy (0.13.x) CLIs, used where those CLIs have no `--json`.
public enum TextParsers {
    // MARK: - Dirty-module banner

    /// The `    • path` lines of the 0.13.x teardown refusal banner, printed on stdout (on stderr by machine-mode
    /// CLIs):
    ///
    ///     ⚠️  Detected devcontainer/compose changes inside /r/alpha:
    ///         • .devcontainer/
    ///         (BranchBox refuses to delete dirty module files without --force)
    public static func dirtyBannerPaths(_ text: String) -> [String] {
        text.split(whereSeparator: \.isNewline).compactMap { line in
            let trimmed = line.drop { $0 == " " || $0 == "\t" }
            guard trimmed.count < line.count, trimmed.hasPrefix("• ") else { return nil }
            let path = trimmed.dropFirst(2).trimmingCharacters(in: .whitespaces)
            return path.isEmpty ? nil : path
        }
    }

    // MARK: - detect

    /// `branchbox detect -p <folder>` text:
    ///
    ///     📦 BranchBox Configuration
    ///
    ///     Project: /r/demo
    ///     Stack: Rust
    ///     Adapter: Generic
    ///
    ///     Enabled modules: 2
    ///       ✓ tunnel
    ///       ✓ specs
    ///
    ///     Warnings:
    ///       - …
    ///
    /// The stack is the `Stack` enum's Debug name, lowercased to match `detect --json` (`NodeJs` → `nodejs`). The
    /// facts the text lacks (repository, initialized, devcontainer, `.env`) come from the caller.
    public static func detectReport(_ text: String, folder: String, gitRepository: Bool, initialized: Bool,
                                    hasDevcontainer: Bool?, hasEnv: Bool?) -> DetectReport {
        var project: String?
        var stack: String?
        var adapter: String?
        var modules: [String] = []
        var warnings: [String] = []
        var inWarnings = false
        for raw in text.split(whereSeparator: \.isNewline) {
            let line = String(raw)
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if let value = field("Project:", in: line) {
                project = value == "." ? folder : value
            } else if let value = field("Stack:", in: line) {
                stack = value.lowercased().filter { $0.isLetter || $0.isNumber }
            } else if let value = field("Adapter:", in: line) {
                adapter = value.lowercased()
            } else if trimmed.hasPrefix("✓ ") {
                modules.append(String(trimmed.dropFirst(2)).trimmingCharacters(in: .whitespaces))
            } else if trimmed == "Warnings:" {
                inWarnings = true
            } else if inWarnings, trimmed.hasPrefix("- ") {
                warnings.append(String(trimmed.dropFirst(2)))
            }
        }
        return DetectReport(project: project ?? folder, gitRepository: gitRepository, initialized: initialized,
                            stack: stack, adapter: adapter, modules: modules, hasDevcontainer: hasDevcontainer,
                            hasEnv: hasEnv, warnings: warnings, rawText: text)
    }

    private static func field(_ name: String, in line: String) -> String? {
        guard line.hasPrefix(name) else { return nil }
        let value = line.dropFirst(name.count).trimmingCharacters(in: .whitespaces)
        return value.isEmpty ? nil : value
    }

    // MARK: - devcontainer sync

    /// `branchbox devcontainer sync` text (0.13.x), one row per active feature:
    ///
    ///     🔄 Syncing devcontainer configuration to 2 feature worktree(s)
    ///
    ///       alpha ... ✓ synced 3 files (copy)
    ///       beta ... ✗ failed: <error>
    ///       gamma ... ⚠️  worktree not found at /r/gamma
    ///       delta ... would sync
    ///
    ///     ✓ Successfully synced 1 feature worktree(s)
    ///
    ///     ⚠️  1 error(s) occurred:
    ///       - beta: <error>
    ///
    /// 0.13.x exits 0 even when a row failed (DRIFT-09); a failed row, or a feature named in the
    /// "N error(s) occurred" list, is `.failed` regardless of the exit status.
    public static func syncReport(_ text: String, dryRun: Bool, strategy: String?) -> SyncReport {
        var rows: [SyncReport.Row] = []
        var errorsListed = false
        for raw in text.split(whereSeparator: \.isNewline) {
            let line = String(raw)
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("⚠️"), trimmed.contains("error(s) occurred") {
                errorsListed = true
                continue
            }
            if errorsListed, trimmed.hasPrefix("- "), let colon = trimmed.range(of: ": ") {
                let feature = String(trimmed[trimmed.index(trimmed.startIndex, offsetBy: 2)..<colon.lowerBound])
                let error = String(trimmed[colon.upperBound...])
                if let index = rows.firstIndex(where: { $0.feature == feature }) {
                    if rows[index].status != .failed {
                        rows[index] = SyncReport.Row(feature: feature, status: .failed, error: error)
                    }
                } else {
                    rows.append(SyncReport.Row(feature: feature, status: .failed, error: error))
                }
                continue
            }
            guard line.hasPrefix("  "), let separator = trimmed.range(of: " ... ") else { continue }
            let feature = String(trimmed[..<separator.lowerBound])
            let outcome = String(trimmed[separator.upperBound...])
            rows.append(row(feature: feature, outcome: outcome))
        }
        return SyncReport(dryRun: dryRun, strategy: strategy, rows: rows, rawText: text)
    }

    private static func row(feature: String, outcome: String) -> SyncReport.Row {
        if outcome.hasPrefix("✓ synced") {
            return SyncReport.Row(feature: feature, status: .synced)
        }
        if outcome.hasPrefix("would sync") {
            return SyncReport.Row(feature: feature, status: .wouldSync)
        }
        if outcome.hasPrefix("✗ failed") {
            let error = outcome.range(of: "failed: ").map { String(outcome[$0.upperBound...]) }
            return SyncReport.Row(feature: feature, status: .failed, error: error ?? outcome)
        }
        if outcome.hasPrefix("⚠️"), let range = outcome.range(of: "worktree not found at ") {
            return SyncReport.Row(feature: feature, worktreePath: String(outcome[range.upperBound...]), status: .skipped,
                                  skipReason: "Worktree not found")
        }
        return SyncReport.Row(feature: feature, status: .unknown, skipReason: outcome)
    }
}
