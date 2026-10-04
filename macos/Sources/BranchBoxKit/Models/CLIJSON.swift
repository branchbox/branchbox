import Foundation

/// Decoding entry point for every CLI payload (R-7).
public enum CLIJSON {
    /// A plain `JSONDecoder`: models declare explicit snake_case or camelCase `CodingKeys` (R-1) and
    /// read dates as strings (R-5). A `Date` decoded directly goes through `RFC3339.parse` too.
    public static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let text = try container.decode(String.self)
            guard let date = RFC3339.parse(text) else {
                throw DecodingError.dataCorruptedError(in: container, debugDescription: "Not an RFC 3339 date: \(text)")
            }
            return date
        }
        return decoder
    }

    /// Decodes `data` strictly first. If that fails, decodes again from the first line that starts with
    /// `{` or `[` and returns the text it skipped as `preamble` (BUG-11: CLI 0.13.x prints warnings such
    /// as "Prompt truncated" on stdout ahead of the JSON). Throws the strict decode's error when no
    /// later line decodes either.
    public static func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> (value: T, preamble: String?) {
        let decoder = decoder()
        do {
            return (try decoder.decode(type, from: data), nil)
        } catch let strictError {
            for start in documentStarts(in: data) {
                guard let value = try? decoder.decode(type, from: Data(data[start...])) else { continue }
                let skipped = String(decoding: data[data.startIndex..<start], as: UTF8.self)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                return (value, skipped.isEmpty ? nil : skipped)
            }
            throw strictError
        }
    }

    /// Offsets of every line after the first that begins (in column 0) with `{` or `[`. Pretty-printed
    /// payloads indent nested values, so only a top-level document can start in column 0.
    private static func documentStarts(in data: Data) -> [Data.Index] {
        var starts: [Data.Index] = []
        var previous: UInt8?
        for index in data.indices {
            let byte = data[index]
            if previous == UInt8(ascii: "\n"), byte == UInt8(ascii: "{") || byte == UInt8(ascii: "[") {
                starts.append(index)
            }
            previous = byte
        }
        return starts
    }
}
