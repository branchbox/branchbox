import Foundation

/// Accepts `exit_code` (`feature exec --json`) and `exitCode` (`devcontainer exec --json`). A non-zero
/// inner exit code is data: both commands print the payload and then exit 1.
public struct ExecResult: Decodable, Sendable, Hashable { public let exitCode: Int32; public let stdout: String; public let stderr: String
    public let outcome: String?
    public init(exitCode: Int32, stdout: String = "", stderr: String = "", outcome: String? = nil) {
        self.exitCode = exitCode
        self.stdout = stdout
        self.stderr = stderr
        self.outcome = outcome
    }

    private enum CodingKeys: String, CodingKey { case exitCodeSnake = "exit_code", exitCodeCamel = "exitCode", stdout, stderr, outcome }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        // The exit code is what makes this an exec result, so one of the two spellings is required.
        if let code = c.lenient(Int32.self, forKey: .exitCodeSnake) {
            exitCode = code
        } else {
            exitCode = try c.decode(Int32.self, forKey: .exitCodeCamel)
        }
        stdout = c.lenient(String.self, forKey: .stdout) ?? ""
        stderr = c.lenient(String.self, forKey: .stderr) ?? ""
        outcome = c.lenient(String.self, forKey: .outcome)
    }
}
