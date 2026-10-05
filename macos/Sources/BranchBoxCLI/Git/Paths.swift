import Darwin
import Foundation

/// Path helpers for comparing what git prints with what the user picked. git prints symlink-resolved paths
/// (`/private/var/…` for `/var/…`), while the app keeps paths unresolved (DESIGN §4.1), so comparisons go through
/// `canonical` and results are mapped back with `unresolved`.
enum Paths {
    /// `.` and `..` removed and no trailing slash; symbolic links are left alone.
    static func standardized(_ path: String) -> String {
        URL(fileURLWithPath: path).standardizedFileURL.path
    }

    /// The symlink-resolved path (realpath(3)), or the standardized path when it does not exist.
    static func canonical(_ path: String) -> String {
        guard let resolved = realpath(path, nil) else { return standardized(path) }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    static func parent(_ path: String) -> String {
        (standardized(path) as NSString).deletingLastPathComponent
    }

    static func join(_ base: String, _ component: String) -> String {
        (base as NSString).appendingPathComponent(component)
    }

    /// The same location as `resolved`, spelled through the nearest ancestor of `requested` (itself included) that
    /// it lies under once symlinks are resolved. `/private/tmp/r/main` with `requested == /tmp/r/eta` becomes
    /// `/tmp/r/main`. Returns `resolved` unchanged when no ancestor matches.
    static func unresolved(_ resolved: String, relativeTo requested: String) -> String {
        let target = standardized(resolved)
        var ancestor = standardized(requested)
        while true {
            let real = canonical(ancestor)
            if target == real { return ancestor }
            let prefix = real == "/" ? "/" : real + "/"
            if target.hasPrefix(prefix) {
                return join(ancestor, String(target.dropFirst(prefix.count)))
            }
            guard ancestor != "/" else { return target }
            ancestor = parent(ancestor)
        }
    }

    /// Whether two paths name the same location once symlinks are resolved.
    static func same(_ lhs: String, _ rhs: String) -> Bool {
        canonical(lhs) == canonical(rhs)
    }
}
