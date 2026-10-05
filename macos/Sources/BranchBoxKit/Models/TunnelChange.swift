import Foundation

/// `tunnel open --json` prints `{work_feature,state,warnings}`; `tunnel remove --json` prints
/// `{work_feature,previous_state,updated_state,warnings}`. `state` holds the tunnel as it is now in both.
public struct TunnelChange: Decodable, Sendable, Hashable { public let workFeature: String; public let state: TunnelState?
    public let previousState: TunnelState?; public let warnings: [String]
    public init(workFeature: String, state: TunnelState?, previousState: TunnelState? = nil, warnings: [String] = []) {
        self.workFeature = workFeature
        self.state = state
        self.previousState = previousState
        self.warnings = warnings
    }

    private enum CodingKeys: String, CodingKey {
        case workFeature = "work_feature", state, previousState = "previous_state", updatedState = "updated_state", warnings
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        workFeature = try c.decode(String.self, forKey: .workFeature)
        state = c.lenient(TunnelState.self, forKey: .state) ?? c.lenient(TunnelState.self, forKey: .updatedState)
        previousState = c.lenient(TunnelState.self, forKey: .previousState)
        warnings = c.lossyArray(String.self, forKey: .warnings)
    }
}

/// `tunnel credentials set --json` (§5.11). The token itself is never echoed back.
public struct TunnelCredentialsResult: Decodable, Sendable, Hashable { public let credentialsPath: String; public let accountID: String?
    public let tokenPresent: Bool
    public init(credentialsPath: String, accountID: String?, tokenPresent: Bool) {
        self.credentialsPath = credentialsPath
        self.accountID = accountID
        self.tokenPresent = tokenPresent
    }

    private enum CodingKeys: String, CodingKey {
        case credentialsPath = "credentials_path", accountID = "account_id", tokenPresent = "token_present"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        credentialsPath = try c.decode(String.self, forKey: .credentialsPath)
        accountID = c.lenient(String.self, forKey: .accountID)
        tokenPresent = c.lenient(Bool.self, forKey: .tokenPresent) ?? false
    }
}
