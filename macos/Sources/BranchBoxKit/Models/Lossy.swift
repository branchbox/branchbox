import Foundation

/// One element of a lossily decoded array (R-3): a bad element keeps its error instead of failing
/// the whole array.
public struct Lossy<T: Decodable & Sendable>: Decodable, Sendable {
    public let value: T?
    public let error: String?

    public init(from decoder: any Decoder) throws {
        do {
            value = try T(from: decoder)
            error = nil
        } catch {
            value = nil
            self.error = Lossy.describe(error)
        }
    }

    /// "key 'work_feature' not found at [0]" rather than the multi-line `DecodingError` dump.
    private static func describe(_ error: any Error) -> String {
        guard let error = error as? DecodingError else { return String(describing: error) }
        func path(_ context: DecodingError.Context) -> String {
            let keys = context.codingPath.map { $0.intValue.map { "[\($0)]" } ?? $0.stringValue }
            return keys.isEmpty ? "the top level" : keys.joined(separator: ".")
        }
        switch error {
        case .keyNotFound(let key, let context):
            return "key '\(key.stringValue)' not found at \(path(context))"
        case .typeMismatch(let type, let context):
            return "expected \(type) at \(path(context)): \(context.debugDescription)"
        case .valueNotFound(let type, let context):
            return "missing \(type) at \(path(context))"
        case .dataCorrupted(let context):
            return "corrupted value at \(path(context)): \(context.debugDescription)"
        @unknown default:
            return String(describing: error)
        }
    }
}

// R-2 / R-3 helpers shared by every model's `init(from:)`.
extension KeyedDecodingContainer {
    /// R-2: an optional value. Absent, null or malformed all decode to nil.
    func lenient<T: Decodable>(_ type: T.Type, forKey key: Key) -> T? {
        try? decodeIfPresent(type, forKey: key)
    }

    /// R-3: an array whose bad elements are dropped. Absent, null or not an array decodes to [].
    func lossyArray<T: Decodable & Sendable>(_ type: T.Type, forKey key: Key) -> [T] {
        lossyArrayCountingDrops(type, forKey: key).values
    }

    /// `lossyArray` plus how many elements were dropped, for models where a silently missing element would make
    /// a document look safer than it is (the teardown plan's blockers and changes).
    func lossyArrayCountingDrops<T: Decodable & Sendable>(_ type: T.Type, forKey key: Key) -> (values: [T], dropped: Int) {
        let elements = lenient([Lossy<T>].self, forKey: key) ?? []
        let values = elements.compactMap(\.value)
        return (values, elements.count - values.count)
    }
}
