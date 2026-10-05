import Foundation

/// The machine-mode error document a 0.14+ CLI prints on stdout instead of a payload (§5.2):
/// `{"schema_version":1,"error":{"code","message","causes","details"}}`. Clients branch on `error.code`.
public struct ErrorEnvelope: Decodable, Sendable, Hashable {
    public struct Body: Decodable, Sendable, Hashable {
        public let code: String; public let message: String; public let causes: [String]; public let details: JSONValue?
        public init(code: String, message: String, causes: [String] = [], details: JSONValue? = nil) {
            self.code = code
            self.message = message
            self.causes = causes
            self.details = details
        }

        private enum CodingKeys: String, CodingKey { case code, message, causes, details }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            code = try c.decode(String.self, forKey: .code)
            message = c.lenient(String.self, forKey: .message) ?? ""
            causes = c.lossyArray(String.self, forKey: .causes)
            details = c.lenient(JSONValue.self, forKey: .details)
        }
    }
    public let schemaVersion: Int; public let error: Body
    public init(schemaVersion: Int, error: Body) { self.schemaVersion = schemaVersion; self.error = error }

    private enum CodingKeys: String, CodingKey { case schemaVersion = "schema_version", error }

    /// Both keys are required so a success payload (or a legacy `{"error": "..."}` string) never reads
    /// as an envelope.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        schemaVersion = try c.decode(Int.self, forKey: .schemaVersion)
        error = try c.decode(Body.self, forKey: .error)
    }
}
