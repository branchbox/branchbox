import BranchBoxKit
import Foundation
import os

/// One project in `projects.json`. `root` is the standardized main-worktree path (projects are keyed by path,
/// never by URL string).
struct ProjectEntry: Codable, Hashable, Sendable {
    var root: String
    var displayName: String
    var addedAt: Date
    var lastOpenedAt: Date?
    var pinned: Bool
    var collapsed: Bool

    init(root: String, displayName: String, addedAt: Date, lastOpenedAt: Date? = nil, pinned: Bool = false,
         collapsed: Bool = false) {
        self.root = root
        self.displayName = displayName
        self.addedAt = addedAt
        self.lastOpenedAt = lastOpenedAt
        self.pinned = pinned
        self.collapsed = collapsed
    }

    var ref: ProjectRef { ProjectRef(root: URL(fileURLWithPath: root, isDirectory: true)) }

    private enum CodingKeys: String, CodingKey { case root, displayName, addedAt, lastOpenedAt, pinned, collapsed }

    /// Only `root` is required; a bad date reads as nil (or "now" for `addedAt`), so one odd entry never hides
    /// the rest. Dates are RFC 3339 strings.
    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let root = try container.decode(String.self, forKey: .root)
        guard root.hasPrefix("/") else {
            throw DecodingError.dataCorruptedError(forKey: .root, in: container, debugDescription: "not an absolute path")
        }
        self.root = URL(fileURLWithPath: root, isDirectory: true).standardizedFileURL.path
        let name = try? container.decodeIfPresent(String.self, forKey: .displayName)
        displayName = name.flatMap { $0.isEmpty ? nil : $0 } ?? ProjectEntry.defaultDisplayName(for: self.root)
        addedAt = (try? container.decodeIfPresent(String.self, forKey: .addedAt)).flatMap(RFC3339.parse) ?? Date()
        lastOpenedAt = (try? container.decodeIfPresent(String.self, forKey: .lastOpenedAt)).flatMap(RFC3339.parse)
        pinned = (try? container.decodeIfPresent(Bool.self, forKey: .pinned)) ?? false
        collapsed = (try? container.decodeIfPresent(Bool.self, forKey: .collapsed)) ?? false
    }

    func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(root, forKey: .root)
        try container.encode(displayName, forKey: .displayName)
        try container.encode(RFC3339.format(addedAt), forKey: .addedAt)
        try container.encode(lastOpenedAt.map(RFC3339.format), forKey: .lastOpenedAt)
        try container.encode(pinned, forKey: .pinned)
        try container.encode(collapsed, forKey: .collapsed)
    }

    /// A BranchBox main worktree usually sits at `<container>/main`, so "main" says nothing: use the container's
    /// name then (`…/branchbox/main` → "branchbox").
    static func defaultDisplayName(for root: String) -> String {
        let url = URL(fileURLWithPath: root)
        let name = url.lastPathComponent
        guard name == "main" else { return name }
        let parent = url.deletingLastPathComponent().lastPathComponent
        return parent.isEmpty || parent == "/" ? name : parent
    }
}

/// Reads and writes `projects.json`: `{"version":1,"projects":[{"root","displayName","addedAt","lastOpenedAt",
/// "pinned","collapsed"}]}` (D-22). Writes are atomic. An unreadable file is set aside as
/// `projects.json.unreadable` before the first write replaces it, so a bad edit never silently loses the list.
struct ProjectsRepository: Sendable {
    let directory: URL
    var fileURL: URL { directory.appendingPathComponent("projects.json") }

    static let currentVersion = 1
    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "projects")

    init(directory: URL) {
        self.directory = directory
    }

    private struct Document: Codable {
        var version: Int
        var projects: [ProjectEntry]

        private enum CodingKeys: String, CodingKey { case version, projects }

        init(version: Int, projects: [ProjectEntry]) {
            self.version = version
            self.projects = projects
        }

        init(from decoder: any Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            version = (try? container.decodeIfPresent(Int.self, forKey: .version)) ?? 1
            let entries = try container.decodeIfPresent([Lossy<ProjectEntry>].self, forKey: .projects) ?? []
            projects = entries.compactMap(\.value)
        }
    }

    /// The saved projects, deduplicated by path (first wins). A missing file is an empty list.
    func load() -> [ProjectEntry] {
        let url = fileURL
        guard let data = try? Data(contentsOf: url) else { return [] }
        do {
            let document = try JSONDecoder().decode(Document.self, from: data)
            var seen = Set<String>()
            return document.projects.filter { seen.insert($0.root).inserted }
        } catch {
            Self.logger.error("projects.json is unreadable (\(error, privacy: .public)); keeping a copy and starting empty")
            let aside = directory.appendingPathComponent("projects.json.unreadable")
            try? FileManager.default.removeItem(at: aside)
            try? FileManager.default.copyItem(at: url, to: aside)
            return []
        }
    }

    func save(_ entries: [ProjectEntry]) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(Document(version: Self.currentVersion, projects: entries))
        try data.write(to: fileURL, options: .atomic)
    }
}
