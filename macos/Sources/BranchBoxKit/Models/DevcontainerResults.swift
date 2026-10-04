import Foundation

/// `devcontainer up/down/build --json` (camelCase). With Docker unavailable the CLI prints
/// `{"outcome":"error","message":"Docker is not available"}` on stdout and exits 1, so callers decode
/// stdout on failure too.
public struct DevcontainerResult: Decodable, Sendable, Hashable { public let outcome: String; public let containerID: String?
    public let remoteUser: String?; public let remoteWorkspaceFolder: String?; public let composeProjectName: String?
    public let removedContainers: [String]; public let imageName: String?; public let message: String?
    public init(outcome: String, containerID: String? = nil, remoteUser: String? = nil, remoteWorkspaceFolder: String? = nil,
                composeProjectName: String? = nil, removedContainers: [String] = [], imageName: String? = nil,
                message: String? = nil) {
        self.outcome = outcome
        self.containerID = containerID
        self.remoteUser = remoteUser
        self.remoteWorkspaceFolder = remoteWorkspaceFolder
        self.composeProjectName = composeProjectName
        self.removedContainers = removedContainers
        self.imageName = imageName
        self.message = message
    }

    /// The runtime reported `"outcome": "error"` (the payload is printed with exit 1).
    public var isError: Bool { outcome == "error" }

    /// The payload carried an outcome. Any JSON object decodes (every field is lenient), so an error envelope or a
    /// legacy `{"error": "..."}` also decodes, with an empty outcome; callers check the envelope first (§6.3) and
    /// treat an unrecognized payload as a decode failure rather than a success.
    public var isRecognized: Bool { !outcome.isEmpty }

    private enum CodingKeys: String, CodingKey {
        case outcome, containerID = "containerId", remoteUser, remoteWorkspaceFolder, composeProjectName
        case removedContainers, imageName, message, error
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        outcome = c.lenient(String.self, forKey: .outcome) ?? ""
        containerID = c.lenient(String.self, forKey: .containerID)
        remoteUser = c.lenient(String.self, forKey: .remoteUser)
        remoteWorkspaceFolder = c.lenient(String.self, forKey: .remoteWorkspaceFolder)
        composeProjectName = c.lenient(String.self, forKey: .composeProjectName)
        removedContainers = c.lossyArray(String.self, forKey: .removedContainers)
        imageName = c.lenient(String.self, forKey: .imageName)
        // Some devcontainer subcommands report failure as {"error": "..."} instead.
        message = c.lenient(String.self, forKey: .message) ?? c.lenient(String.self, forKey: .error)
    }
}

public struct DevcontainerStatus: Sendable, Hashable {
    public enum State: String, Sendable, Hashable { case running, stopped, notCreated, unknown }
    public let state: State; public let containerID: String?; public let service: DevcontainerServiceInfo?
    public init(state: State, containerID: String? = nil, service: DevcontainerServiceInfo? = nil) {
        self.state = state
        self.containerID = containerID
        self.service = service
    }
}

public struct DevcontainerServiceInfo: Decodable, Sendable, Hashable { public let serviceName: String?; public let port: Int?
    public let serviceURL: String?; public let containerUser: String?    // devcontainer detect --json (snake_case)
    public init(serviceName: String?, port: Int?, serviceURL: String?, containerUser: String?) {
        self.serviceName = serviceName
        self.port = port
        self.serviceURL = serviceURL
        self.containerUser = containerUser
    }

    /// The payload named a service. `devcontainer detect --json` without a `.devcontainer` prints
    /// `{"error": "..."}` (exit 1), which decodes with every field nil.
    public var isRecognized: Bool { serviceName != nil }

    private enum CodingKeys: String, CodingKey {
        case serviceName = "service_name", port, serviceURL = "service_url", containerUser = "container_user"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        serviceName = c.lenient(String.self, forKey: .serviceName)
        port = c.lenient(Int.self, forKey: .port)
        serviceURL = c.lenient(String.self, forKey: .serviceURL)
        containerUser = c.lenient(String.self, forKey: .containerUser)
    }
}
