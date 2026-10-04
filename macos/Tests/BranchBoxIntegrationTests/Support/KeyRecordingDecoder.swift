import BranchBoxKit
import Foundation

/// Decodes a model from a parsed `JSONValue` while recording every key the model's `init(from:)` reads, so
/// `LiveFixtureDecodeTests` can list the keys a CLI prints that the app ignores. Paths look like
/// `runtime.published_ports[].host`. A container whose `allKeys` is read (a dictionary, `JSONValue`) counts as
/// reading all of its keys. Dates decode as RFC 3339 strings, as `CLIJSON.decoder()` does.
final class KeyRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var read: Set<String> = []
    private var open: Set<String> = []

    func didRead(_ path: String) { _ = lock.withLock { read.insert(path) } }
    func didOpen(_ path: String) { _ = lock.withLock { open.insert(path) } }

    /// Key paths in `value` that were never read and are not under a fully read container.
    func unknownKeys(in value: JSONValue) -> [String] {
        let (read, open) = lock.withLock { (self.read, self.open) }
        var unknown: [String] = []
        func walk(_ value: JSONValue, _ path: String) {
            switch value {
            case .object(let object):
                guard !open.contains(path) else { return }
                for (key, child) in object {
                    let childPath = path.isEmpty ? key : "\(path).\(key)"
                    if read.contains(childPath) { walk(child, childPath) } else { unknown.append(childPath) }
                }
            case .array(let elements):
                for element in elements { walk(element, path + "[]") }
            default:
                return
            }
        }
        walk(value, "")
        return Array(Set(unknown)).sorted()
    }

    static func path(_ codingPath: [any CodingKey]) -> String {
        codingPath.reduce(into: "") { path, key in
            if key.intValue != nil {
                path += "[]"
            } else {
                path += path.isEmpty ? key.stringValue : ".\(key.stringValue)"
            }
        }
    }

    /// Decodes `type` from `value`, returning the keys it did not read (or the decoding error).
    static func unknownKeys<T: Decodable>(_ type: T.Type, in value: JSONValue) -> Result<[String], any Error> {
        let recorder = KeyRecorder()
        do {
            _ = try T(from: RecordingDecoder(value: value, codingPath: [], recorder: recorder))
            return .success(recorder.unknownKeys(in: value))
        } catch {
            return .failure(error)
        }
    }
}

private struct IndexKey: CodingKey {
    let intValue: Int?
    var stringValue: String { "\(intValue ?? 0)" }
    init(_ index: Int) { intValue = index }
    init?(stringValue: String) { nil }
    init?(intValue: Int) { self.intValue = intValue }
}

private struct RecordingDecoder: Decoder {
    let value: JSONValue
    let codingPath: [any CodingKey]
    let recorder: KeyRecorder
    var userInfo: [CodingUserInfoKey: Any] { [:] }

    func container<Key: CodingKey>(keyedBy type: Key.Type) throws -> KeyedDecodingContainer<Key> {
        guard case .object(let object) = value else {
            throw DecodingError.typeMismatch([String: JSONValue].self, .init(codingPath: codingPath, debugDescription: "Not an object"))
        }
        return KeyedDecodingContainer(Keyed<Key>(object: object, codingPath: codingPath, recorder: recorder))
    }

    func unkeyedContainer() throws -> any UnkeyedDecodingContainer {
        guard case .array(let elements) = value else {
            throw DecodingError.typeMismatch([JSONValue].self, .init(codingPath: codingPath, debugDescription: "Not an array"))
        }
        return Unkeyed(elements: elements, codingPath: codingPath, recorder: recorder)
    }

    func singleValueContainer() throws -> any SingleValueDecodingContainer {
        Single(value: value, codingPath: codingPath, recorder: recorder)
    }

    static func decode<T: Decodable>(_ type: T.Type, from value: JSONValue, codingPath: [any CodingKey],
                                     recorder: KeyRecorder) throws -> T {
        if T.self == Date.self {
            guard case .string(let text) = value, let date = RFC3339.parse(text) as? T else {
                throw DecodingError.dataCorrupted(.init(codingPath: codingPath, debugDescription: "Not an RFC 3339 date"))
            }
            return date
        }
        return try T(from: RecordingDecoder(value: value, codingPath: codingPath, recorder: recorder))
    }
}

private struct Keyed<Key: CodingKey>: KeyedDecodingContainerProtocol {
    let object: [String: JSONValue]
    let codingPath: [any CodingKey]
    let recorder: KeyRecorder

    var allKeys: [Key] {
        recorder.didOpen(KeyRecorder.path(codingPath))
        return object.keys.compactMap { Key(stringValue: $0) }
    }

    private func note(_ key: Key) { recorder.didRead(KeyRecorder.path(codingPath + [key])) }

    func contains(_ key: Key) -> Bool {
        note(key)
        return object[key.stringValue] != nil
    }

    private func value(_ key: Key) throws -> JSONValue {
        note(key)
        guard let value = object[key.stringValue] else {
            throw DecodingError.keyNotFound(key, .init(codingPath: codingPath, debugDescription: "No value"))
        }
        return value
    }

    func decodeNil(forKey key: Key) throws -> Bool {
        if case .null = try value(key) { return true }
        return false
    }

    func decode<T: Decodable>(_ type: T.Type, forKey key: Key) throws -> T {
        try RecordingDecoder.decode(type, from: value(key), codingPath: codingPath + [key], recorder: recorder)
    }

    func decode(_ type: Bool.Type, forKey key: Key) throws -> Bool { try decode(Wrapped<Bool>.self, forKey: key).value }
    func decode(_ type: String.Type, forKey key: Key) throws -> String { try decode(Wrapped<String>.self, forKey: key).value }
    func decode(_ type: Double.Type, forKey key: Key) throws -> Double { try decode(Wrapped<Double>.self, forKey: key).value }
    func decode(_ type: Float.Type, forKey key: Key) throws -> Float { try decode(Wrapped<Float>.self, forKey: key).value }
    func decode(_ type: Int.Type, forKey key: Key) throws -> Int { try decode(Wrapped<Int>.self, forKey: key).value }
    func decode(_ type: Int8.Type, forKey key: Key) throws -> Int8 { try decode(Wrapped<Int8>.self, forKey: key).value }
    func decode(_ type: Int16.Type, forKey key: Key) throws -> Int16 { try decode(Wrapped<Int16>.self, forKey: key).value }
    func decode(_ type: Int32.Type, forKey key: Key) throws -> Int32 { try decode(Wrapped<Int32>.self, forKey: key).value }
    func decode(_ type: Int64.Type, forKey key: Key) throws -> Int64 { try decode(Wrapped<Int64>.self, forKey: key).value }
    func decode(_ type: UInt.Type, forKey key: Key) throws -> UInt { try decode(Wrapped<UInt>.self, forKey: key).value }
    func decode(_ type: UInt8.Type, forKey key: Key) throws -> UInt8 { try decode(Wrapped<UInt8>.self, forKey: key).value }
    func decode(_ type: UInt16.Type, forKey key: Key) throws -> UInt16 { try decode(Wrapped<UInt16>.self, forKey: key).value }
    func decode(_ type: UInt32.Type, forKey key: Key) throws -> UInt32 { try decode(Wrapped<UInt32>.self, forKey: key).value }
    func decode(_ type: UInt64.Type, forKey key: Key) throws -> UInt64 { try decode(Wrapped<UInt64>.self, forKey: key).value }

    func nestedContainer<NestedKey: CodingKey>(keyedBy type: NestedKey.Type,
                                               forKey key: Key) throws -> KeyedDecodingContainer<NestedKey> {
        try RecordingDecoder(value: value(key), codingPath: codingPath + [key], recorder: recorder).container(keyedBy: type)
    }

    func nestedUnkeyedContainer(forKey key: Key) throws -> any UnkeyedDecodingContainer {
        try RecordingDecoder(value: value(key), codingPath: codingPath + [key], recorder: recorder).unkeyedContainer()
    }

    func superDecoder() throws -> any Decoder {
        RecordingDecoder(value: .object(object), codingPath: codingPath, recorder: recorder)
    }

    func superDecoder(forKey key: Key) throws -> any Decoder {
        RecordingDecoder(value: try value(key), codingPath: codingPath + [key], recorder: recorder)
    }
}

private struct Unkeyed: UnkeyedDecodingContainer {
    let elements: [JSONValue]
    let codingPath: [any CodingKey]
    let recorder: KeyRecorder
    var currentIndex = 0
    var count: Int? { elements.count }
    var isAtEnd: Bool { currentIndex >= elements.count }

    init(elements: [JSONValue], codingPath: [any CodingKey], recorder: KeyRecorder) {
        self.elements = elements
        self.codingPath = codingPath
        self.recorder = recorder
    }

    private mutating func next() throws -> (JSONValue, [any CodingKey]) {
        guard !isAtEnd else {
            throw DecodingError.valueNotFound(JSONValue.self, .init(codingPath: codingPath, debugDescription: "At the end"))
        }
        defer { currentIndex += 1 }
        return (elements[currentIndex], codingPath + [IndexKey(currentIndex)])
    }

    mutating func decodeNil() throws -> Bool {
        guard !isAtEnd, case .null = elements[currentIndex] else { return false }
        currentIndex += 1
        return true
    }

    mutating func decode<T: Decodable>(_ type: T.Type) throws -> T {
        let (value, path) = try next()
        return try RecordingDecoder.decode(type, from: value, codingPath: path, recorder: recorder)
    }

    mutating func decode(_ type: Bool.Type) throws -> Bool { try decode(Wrapped<Bool>.self).value }
    mutating func decode(_ type: String.Type) throws -> String { try decode(Wrapped<String>.self).value }
    mutating func decode(_ type: Double.Type) throws -> Double { try decode(Wrapped<Double>.self).value }
    mutating func decode(_ type: Float.Type) throws -> Float { try decode(Wrapped<Float>.self).value }
    mutating func decode(_ type: Int.Type) throws -> Int { try decode(Wrapped<Int>.self).value }
    mutating func decode(_ type: Int8.Type) throws -> Int8 { try decode(Wrapped<Int8>.self).value }
    mutating func decode(_ type: Int16.Type) throws -> Int16 { try decode(Wrapped<Int16>.self).value }
    mutating func decode(_ type: Int32.Type) throws -> Int32 { try decode(Wrapped<Int32>.self).value }
    mutating func decode(_ type: Int64.Type) throws -> Int64 { try decode(Wrapped<Int64>.self).value }
    mutating func decode(_ type: UInt.Type) throws -> UInt { try decode(Wrapped<UInt>.self).value }
    mutating func decode(_ type: UInt8.Type) throws -> UInt8 { try decode(Wrapped<UInt8>.self).value }
    mutating func decode(_ type: UInt16.Type) throws -> UInt16 { try decode(Wrapped<UInt16>.self).value }
    mutating func decode(_ type: UInt32.Type) throws -> UInt32 { try decode(Wrapped<UInt32>.self).value }
    mutating func decode(_ type: UInt64.Type) throws -> UInt64 { try decode(Wrapped<UInt64>.self).value }

    mutating func nestedContainer<NestedKey: CodingKey>(keyedBy type: NestedKey.Type) throws -> KeyedDecodingContainer<NestedKey> {
        let (value, path) = try next()
        return try RecordingDecoder(value: value, codingPath: path, recorder: recorder).container(keyedBy: type)
    }

    mutating func nestedUnkeyedContainer() throws -> any UnkeyedDecodingContainer {
        let (value, path) = try next()
        return try RecordingDecoder(value: value, codingPath: path, recorder: recorder).unkeyedContainer()
    }

    mutating func superDecoder() throws -> any Decoder {
        let (value, path) = try next()
        return RecordingDecoder(value: value, codingPath: path, recorder: recorder)
    }
}

/// A primitive read through a single-value container (where the JSON → Swift conversion happens).
private struct Wrapped<T: Decodable>: Decodable {
    let value: T
    init(from decoder: any Decoder) throws {
        value = try decoder.singleValueContainer().decode(T.self)
    }
}

private struct Single: SingleValueDecodingContainer {
    let value: JSONValue
    let codingPath: [any CodingKey]
    let recorder: KeyRecorder

    func decodeNil() -> Bool {
        if case .null = value { return true }
        return false
    }

    private func mismatch<T>(_ type: T.Type) -> DecodingError {
        .typeMismatch(type, .init(codingPath: codingPath, debugDescription: "Found \(value)"))
    }

    private func number<T>(_ type: T.Type, _ convert: (Double) -> T?) throws -> T {
        guard case .number(let number) = value, let converted = convert(number) else { throw mismatch(type) }
        return converted
    }

    private func integer<T: BinaryInteger>(_ type: T.Type) throws -> T {
        try number(type) { $0.rounded() == $0 ? T(exactly: $0) : nil }
    }

    func decode(_ type: Bool.Type) throws -> Bool {
        guard case .bool(let bool) = value else { throw mismatch(type) }
        return bool
    }

    func decode(_ type: String.Type) throws -> String {
        guard case .string(let string) = value else { throw mismatch(type) }
        return string
    }

    func decode(_ type: Double.Type) throws -> Double { try number(type) { $0 } }
    func decode(_ type: Float.Type) throws -> Float { try number(type) { Float($0) } }
    func decode(_ type: Int.Type) throws -> Int { try integer(type) }
    func decode(_ type: Int8.Type) throws -> Int8 { try integer(type) }
    func decode(_ type: Int16.Type) throws -> Int16 { try integer(type) }
    func decode(_ type: Int32.Type) throws -> Int32 { try integer(type) }
    func decode(_ type: Int64.Type) throws -> Int64 { try integer(type) }
    func decode(_ type: UInt.Type) throws -> UInt { try integer(type) }
    func decode(_ type: UInt8.Type) throws -> UInt8 { try integer(type) }
    func decode(_ type: UInt16.Type) throws -> UInt16 { try integer(type) }
    func decode(_ type: UInt32.Type) throws -> UInt32 { try integer(type) }
    func decode(_ type: UInt64.Type) throws -> UInt64 { try integer(type) }

    func decode<T: Decodable>(_ type: T.Type) throws -> T {
        try RecordingDecoder.decode(type, from: value, codingPath: codingPath, recorder: recorder)
    }
}
