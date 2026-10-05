import Foundation

/// Read-only access to the captured CLI payloads bundled with this target (`Fixtures/`).
///
/// Paths are relative to the bundle's `Fixtures` folder, e.g. `"cli-0.13.4/main_feature_list_all.json"`.
public enum Fixtures {
    public struct Missing: Error, CustomStringConvertible {
        public let path: String
        public var description: String { "No fixture at Fixtures/\(path)" }
    }

    /// The bundled `Fixtures` folder.
    public static var root: URL {
        // `.copy("Fixtures")` keeps the folder as a single resource, so it is looked up by name.
        guard let url = Bundle.module.url(forResource: "Fixtures", withExtension: nil) else {
            preconditionFailure("BranchBoxTestSupport was built without its Fixtures resource folder")
        }
        return url
    }

    public static func url(_ path: String) -> URL {
        root.appendingPathComponent(path)
    }

    public static func data(_ path: String) throws -> Data {
        let url = url(path)
        guard FileManager.default.fileExists(atPath: url.path) else { throw Missing(path: path) }
        return try Data(contentsOf: url)
    }

    public static func string(_ path: String) throws -> String {
        String(decoding: try data(path), as: UTF8.self)
    }

    /// File names (not paths) directly inside `directory`, sorted, e.g. every capture in `"cli-0.13.4"`.
    public static func names(in directory: String) throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: url(directory).path).filter { !$0.hasPrefix(".") }.sorted()
    }
}
