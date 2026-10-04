import Darwin
import Foundation

/// What a path names, following symbolic links.
public enum FileKind: Sendable, Hashable {
    case missing
    /// A symbolic link whose target does not exist.
    case brokenSymbolicLink
    case directory
    case file(executable: Bool)
}

/// Identifies one version of a file: `brew upgrade` (a new binary behind the same unresolved path) changes it.
public struct FileIdentity: Sendable, Hashable, Codable {
    public let inode: UInt64
    public let modificationSeconds: Int64
    public let modificationNanoseconds: Int64
    public let size: Int64

    public init(inode: UInt64, modificationSeconds: Int64, modificationNanoseconds: Int64, size: Int64) {
        self.inode = inode
        self.modificationSeconds = modificationSeconds
        self.modificationNanoseconds = modificationNanoseconds
        self.size = size
    }

    private enum CodingKeys: String, CodingKey {
        case inode, size
        case modificationSeconds = "mtime_sec"
        case modificationNanoseconds = "mtime_nsec"
    }
}

/// The file-system questions `CLILocator` and `CLIProbe` ask, so tests can answer them without real files.
public protocol FileSystemProbing: Sendable {
    func kind(at path: String) -> FileKind
    /// The identity of the file `path` resolves to, or nil if it does not exist.
    func identity(at path: String) -> FileIdentity?
}

/// The real file system.
public struct LocalFileSystem: FileSystemProbing {
    public init() {}

    public func kind(at path: String) -> FileKind {
        var info = stat()
        guard stat(path, &info) == 0 else {
            var link = stat()
            return lstat(path, &link) == 0 ? .brokenSymbolicLink : .missing
        }
        switch info.st_mode & S_IFMT {
        case S_IFDIR: return .directory
        case S_IFREG: return .file(executable: access(path, X_OK) == 0)
        default: return .file(executable: false)
        }
    }

    public func identity(at path: String) -> FileIdentity? {
        var info = stat()
        guard stat(path, &info) == 0 else { return nil }
        return FileIdentity(inode: UInt64(info.st_ino), modificationSeconds: Int64(info.st_mtimespec.tv_sec),
                            modificationNanoseconds: Int64(info.st_mtimespec.tv_nsec), size: Int64(info.st_size))
    }
}

/// `~/Library/Application Support/BranchBox`, where the environment and probe caches live (next to
/// `projects.json`, D-22).
public enum ApplicationSupport {
    public static var directory: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("BranchBox", isDirectory: true)
    }

    /// Writes `data` atomically to `file`, creating its directory first.
    static func write(_ data: Data, to file: URL) throws {
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: file, options: .atomic)
    }
}
