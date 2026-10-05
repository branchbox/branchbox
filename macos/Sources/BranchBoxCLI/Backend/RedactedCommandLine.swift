import Foundation

/// Shell-escaped command lines with secrets removed, for "Copy as Command" and `Diagnostics.invocation`
/// (DESIGN §6.7).
///
/// - The value of `--prompt` (`--prompt V` or `--prompt=V`) becomes `'<redacted N chars>'`.
/// - An extra-environment value of four or more characters becomes `<redacted>` wherever it appears. Shorter
///   values (`DEBUG=1`, `RUNTIME=sbx`) are left alone, so the copied command keeps arguments such as `1`.
/// - A token is redacted wherever it appears, whatever its length. Tokens never reach argv in the first place (they
///   travel on stdin); redacting them here is a second line.
public struct RedactedCommandLine: Sendable {
    /// Flags whose value is always redacted.
    public static let redactedValueFlags: Set<String> = ["--prompt"]
    /// `Diagnostics.invocation` cuts arguments longer than this.
    public static let diagnosticsArgumentLimit = 200
    /// Extra-environment values shorter than this are not redacted.
    public static let minimumSecretLength = 4

    /// Everything redacted: tokens of any length, extra-environment values of `minimumSecretLength` or more.
    public let secrets: [String]

    public init(secrets: [String] = [], tokens: [String] = []) {
        let values = secrets.filter { $0.count >= Self.minimumSecretLength } + tokens.filter { !$0.isEmpty }
        // Longest first, so a secret that contains another is replaced whole.
        self.secrets = Array(Set(values)).sorted { $0.count > $1.count }
    }

    /// `argv` (executable first) rendered for a shell; `argumentLimit` cuts each argument after redaction.
    public func render(_ argv: [String], argumentLimit: Int? = nil) -> String {
        var rendered: [String] = []
        var redactNext = false
        for argument in argv {
            if redactNext {
                rendered.append(Self.quote("<redacted \(argument.count) chars>"))
                redactNext = false
                continue
            }
            if let (flag, value) = Self.inlineValue(argument) {
                rendered.append(flag + "=" + Self.quote("<redacted \(value.count) chars>"))
                continue
            }
            redactNext = Self.redactedValueFlags.contains(argument)
            var text = redacted(argument)
            if let limit = argumentLimit, text.count > limit {
                text = String(text.prefix(limit)) + "…"
            }
            rendered.append(Self.quote(text))
        }
        return rendered.joined(separator: " ")
    }

    /// `--prompt=V` split into the flag and its value.
    private static func inlineValue(_ argument: String) -> (String, Substring)? {
        guard let equals = argument.firstIndex(of: "=") else { return nil }
        let flag = String(argument[..<equals])
        guard redactedValueFlags.contains(flag) else { return nil }
        return (flag, argument[argument.index(after: equals)...])
    }

    private func redacted(_ argument: String) -> String {
        var text = argument
        for secret in secrets {
            if text == secret { return "<redacted>" }
            if text.contains(secret) {
                text = text.replacingOccurrences(of: secret, with: "<redacted>")
            }
        }
        return text
    }

    private static let safeCharacters = Set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_@%+=:,./-")

    /// POSIX shell quoting: bare when every character is safe, else single-quoted with `'` written as `'\''`.
    public static func quote(_ argument: String) -> String {
        if !argument.isEmpty, argument.allSatisfy({ safeCharacters.contains($0) }) { return argument }
        return "'" + argument.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}
