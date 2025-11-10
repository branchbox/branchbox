import Foundation
import GRPC
import NIOCore
import SwiftProtobuf

extension GRPCPayload where Self: SwiftProtobuf.Message {
    public init(serializedByteBuffer: inout NIOCore.ByteBuffer) throws {
        let data = serializedByteBuffer.readData(length: serializedByteBuffer.readableBytes) ?? Data()
        self = try Self(serializedData: data)
    }

    public func serialize(into buffer: inout NIOCore.ByteBuffer) throws {
        let data = try self.serializedData()
        buffer.writeBytes(data)
    }
}

struct Branchbox_Agent_ListRequest: SwiftProtobuf.Message, GRPCPayload {
    var repoPath: String = ""
    var includeRemoved: Bool = false
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                repoPath = try decoder.decodeSingularStringField()
            case 2:
                includeRemoved = try decoder.decodeSingularBoolField()
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !repoPath.isEmpty {
            try visitor.visitSingularStringField(value: repoPath, fieldNumber: 1)
        }
        if includeRemoved {
            try visitor.visitSingularBoolField(value: includeRemoved, fieldNumber: 2)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_ListRequest, rhs: Branchbox_Agent_ListRequest) -> Bool {
        lhs.repoPath == rhs.repoPath &&
            lhs.includeRemoved == rhs.includeRemoved &&
            lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_ListResponse: SwiftProtobuf.Message, GRPCPayload {
    var features: [Branchbox_Agent_Feature] = []
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                try decoder.decodeRepeatedMessageField(value: &features)
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !features.isEmpty {
            try visitor.visitRepeatedMessageField(value: features, fieldNumber: 1)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_ListResponse, rhs: Branchbox_Agent_ListResponse) -> Bool {
        lhs.features == rhs.features && lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_Feature: SwiftProtobuf.Message, GRPCPayload, Equatable {
    var workFeature: String = ""
    var branchName: String = ""
    var worktreePath: String = ""
    var status: String = ""
    var featureURL: String = ""
    var tunnelStatus: String = ""
    var promptSeed: String = ""
    var startMode: String = ""
    var updatedAt: String = ""
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                workFeature = try decoder.decodeSingularStringField()
            case 2:
                branchName = try decoder.decodeSingularStringField()
            case 3:
                worktreePath = try decoder.decodeSingularStringField()
            case 4:
                status = try decoder.decodeSingularStringField()
            case 5:
                featureURL = try decoder.decodeSingularStringField()
            case 10:
                tunnelStatus = try decoder.decodeSingularStringField()
            case 18:
                promptSeed = try decoder.decodeSingularStringField()
            case 19:
                startMode = try decoder.decodeSingularStringField()
            case 16:
                updatedAt = try decoder.decodeSingularStringField()
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !workFeature.isEmpty {
            try visitor.visitSingularStringField(value: workFeature, fieldNumber: 1)
        }
        if !branchName.isEmpty {
            try visitor.visitSingularStringField(value: branchName, fieldNumber: 2)
        }
        if !worktreePath.isEmpty {
            try visitor.visitSingularStringField(value: worktreePath, fieldNumber: 3)
        }
        if !status.isEmpty {
            try visitor.visitSingularStringField(value: status, fieldNumber: 4)
        }
        if !featureURL.isEmpty {
            try visitor.visitSingularStringField(value: featureURL, fieldNumber: 5)
        }
        if !tunnelStatus.isEmpty {
            try visitor.visitSingularStringField(value: tunnelStatus, fieldNumber: 10)
        }
        if !promptSeed.isEmpty {
            try visitor.visitSingularStringField(value: promptSeed, fieldNumber: 18)
        }
        if !startMode.isEmpty {
            try visitor.visitSingularStringField(value: startMode, fieldNumber: 19)
        }
        if !updatedAt.isEmpty {
            try visitor.visitSingularStringField(value: updatedAt, fieldNumber: 16)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_Feature, rhs: Branchbox_Agent_Feature) -> Bool {
        lhs.workFeature == rhs.workFeature &&
            lhs.branchName == rhs.branchName &&
            lhs.worktreePath == rhs.worktreePath &&
            lhs.status == rhs.status &&
            lhs.featureURL == rhs.featureURL &&
            lhs.tunnelStatus == rhs.tunnelStatus &&
            lhs.promptSeed == rhs.promptSeed &&
            lhs.startMode == rhs.startMode &&
            lhs.updatedAt == rhs.updatedAt &&
            lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_StartRequest: SwiftProtobuf.Message, GRPCPayload {
    var repoPath: String = ""
    var name: String = ""
    var title: String = ""
    var baseBranch: String = ""
    var branchPrefix: String = ""
    var reuse: Bool = false
    var telemetry: Bool = false
    var skipModules: [String] = []
    var mode: String = "full"
    var promptSeed: String = ""
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                repoPath = try decoder.decodeSingularStringField()
            case 2:
                name = try decoder.decodeSingularStringField()
            case 3:
                title = try decoder.decodeSingularStringField()
            case 4:
                baseBranch = try decoder.decodeSingularStringField()
            case 5:
                branchPrefix = try decoder.decodeSingularStringField()
            case 6:
                reuse = try decoder.decodeSingularBoolField()
            case 7:
                telemetry = try decoder.decodeSingularBoolField()
            case 8:
                try decoder.decodeRepeatedStringField(value: &skipModules)
            case 9:
                mode = try decoder.decodeSingularStringField()
            case 10:
                promptSeed = try decoder.decodeSingularStringField()
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !repoPath.isEmpty {
            try visitor.visitSingularStringField(value: repoPath, fieldNumber: 1)
        }
        if !name.isEmpty {
            try visitor.visitSingularStringField(value: name, fieldNumber: 2)
        }
        if !title.isEmpty {
            try visitor.visitSingularStringField(value: title, fieldNumber: 3)
        }
        if !baseBranch.isEmpty {
            try visitor.visitSingularStringField(value: baseBranch, fieldNumber: 4)
        }
        if !branchPrefix.isEmpty {
            try visitor.visitSingularStringField(value: branchPrefix, fieldNumber: 5)
        }
        if reuse {
            try visitor.visitSingularBoolField(value: reuse, fieldNumber: 6)
        }
        if telemetry {
            try visitor.visitSingularBoolField(value: telemetry, fieldNumber: 7)
        }
        if !skipModules.isEmpty {
            try visitor.visitRepeatedStringField(value: skipModules, fieldNumber: 8)
        }
        if !mode.isEmpty {
            try visitor.visitSingularStringField(value: mode, fieldNumber: 9)
        }
        if !promptSeed.isEmpty {
            try visitor.visitSingularStringField(value: promptSeed, fieldNumber: 10)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_StartRequest, rhs: Branchbox_Agent_StartRequest) -> Bool {
        lhs.repoPath == rhs.repoPath &&
            lhs.name == rhs.name &&
            lhs.title == rhs.title &&
            lhs.baseBranch == rhs.baseBranch &&
            lhs.branchPrefix == rhs.branchPrefix &&
            lhs.reuse == rhs.reuse &&
            lhs.telemetry == rhs.telemetry &&
            lhs.skipModules == rhs.skipModules &&
            lhs.mode == rhs.mode &&
            lhs.promptSeed == rhs.promptSeed &&
            lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_StartResponse: SwiftProtobuf.Message, GRPCPayload {
    var summary: Branchbox_Agent_StartSummary?
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                var value = Branchbox_Agent_StartSummary()
                try decoder.decodeSingularMessageField(value: &value)
                summary = value
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if var summary {
            try visitor.visitSingularMessageField(value: summary, fieldNumber: 1)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_StartResponse, rhs: Branchbox_Agent_StartResponse) -> Bool {
        lhs.summary == rhs.summary && lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_StartSummary: SwiftProtobuf.Message, GRPCPayload {
    var workFeature: String = ""
    var branchName: String = ""
    var worktreePath: String = ""
    var featureURL: String = ""
    var composeProjectName: String = ""
    var color: String = ""
    var promptSeed: String = ""
    var warnings: [String] = []
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                workFeature = try decoder.decodeSingularStringField()
            case 2:
                branchName = try decoder.decodeSingularStringField()
            case 3:
                worktreePath = try decoder.decodeSingularStringField()
            case 4:
                featureURL = try decoder.decodeSingularStringField()
            case 5:
                composeProjectName = try decoder.decodeSingularStringField()
            case 8:
                color = try decoder.decodeSingularStringField()
            case 9:
                promptSeed = try decoder.decodeSingularStringField()
            case 10:
                try decoder.decodeRepeatedStringField(value: &warnings)
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !workFeature.isEmpty {
            try visitor.visitSingularStringField(value: workFeature, fieldNumber: 1)
        }
        if !branchName.isEmpty {
            try visitor.visitSingularStringField(value: branchName, fieldNumber: 2)
        }
        if !worktreePath.isEmpty {
            try visitor.visitSingularStringField(value: worktreePath, fieldNumber: 3)
        }
        if !featureURL.isEmpty {
            try visitor.visitSingularStringField(value: featureURL, fieldNumber: 4)
        }
        if !composeProjectName.isEmpty {
            try visitor.visitSingularStringField(value: composeProjectName, fieldNumber: 5)
        }
        if !color.isEmpty {
            try visitor.visitSingularStringField(value: color, fieldNumber: 8)
        }
        if !promptSeed.isEmpty {
            try visitor.visitSingularStringField(value: promptSeed, fieldNumber: 9)
        }
        if !warnings.isEmpty {
            try visitor.visitRepeatedStringField(value: warnings, fieldNumber: 10)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_StartSummary, rhs: Branchbox_Agent_StartSummary) -> Bool {
        lhs.workFeature == rhs.workFeature &&
            lhs.branchName == rhs.branchName &&
            lhs.worktreePath == rhs.worktreePath &&
            lhs.featureURL == rhs.featureURL &&
            lhs.composeProjectName == rhs.composeProjectName &&
            lhs.color == rhs.color &&
            lhs.promptSeed == rhs.promptSeed &&
            lhs.warnings == rhs.warnings &&
            lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_TeardownRequest: SwiftProtobuf.Message, GRPCPayload {
    var repoPath: String = ""
    var name: String = ""
    var branchPrefix: String = ""
    var deleteBranch: Bool = false
    var force: Bool = false
    var completeSpec: Bool = false
    var telemetry: Bool = false
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                repoPath = try decoder.decodeSingularStringField()
            case 2:
                name = try decoder.decodeSingularStringField()
            case 3:
                branchPrefix = try decoder.decodeSingularStringField()
            case 4:
                deleteBranch = try decoder.decodeSingularBoolField()
            case 5:
                force = try decoder.decodeSingularBoolField()
            case 6:
                completeSpec = try decoder.decodeSingularBoolField()
            case 7:
                telemetry = try decoder.decodeSingularBoolField()
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !repoPath.isEmpty {
            try visitor.visitSingularStringField(value: repoPath, fieldNumber: 1)
        }
        if !name.isEmpty {
            try visitor.visitSingularStringField(value: name, fieldNumber: 2)
        }
        if !branchPrefix.isEmpty {
            try visitor.visitSingularStringField(value: branchPrefix, fieldNumber: 3)
        }
        if deleteBranch {
            try visitor.visitSingularBoolField(value: deleteBranch, fieldNumber: 4)
        }
        if force {
            try visitor.visitSingularBoolField(value: force, fieldNumber: 5)
        }
        if completeSpec {
            try visitor.visitSingularBoolField(value: completeSpec, fieldNumber: 6)
        }
        if telemetry {
            try visitor.visitSingularBoolField(value: telemetry, fieldNumber: 7)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_TeardownRequest, rhs: Branchbox_Agent_TeardownRequest) -> Bool {
        lhs.repoPath == rhs.repoPath &&
            lhs.name == rhs.name &&
            lhs.branchPrefix == rhs.branchPrefix &&
            lhs.deleteBranch == rhs.deleteBranch &&
            lhs.force == rhs.force &&
            lhs.completeSpec == rhs.completeSpec &&
            lhs.telemetry == rhs.telemetry &&
            lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_TeardownResponse: SwiftProtobuf.Message, GRPCPayload {
    var summary: Branchbox_Agent_TeardownSummary?
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                var value = Branchbox_Agent_TeardownSummary()
                try decoder.decodeSingularMessageField(value: &value)
                summary = value
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if var summary {
            try visitor.visitSingularMessageField(value: summary, fieldNumber: 1)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_TeardownResponse, rhs: Branchbox_Agent_TeardownResponse) -> Bool {
        lhs.summary == rhs.summary && lhs.unknownFields == rhs.unknownFields
    }
}

struct Branchbox_Agent_TeardownSummary: SwiftProtobuf.Message, GRPCPayload {
    var workFeature: String = ""
    var branchName: String = ""
    var worktreeRemoved: Bool = false
    var branchDeleted: Bool = false
    var warnings: [String] = []
    var unknownFields = SwiftProtobuf.UnknownStorage()

    init() {}

    mutating func decodeMessage<D: SwiftProtobuf.Decoder>(decoder: inout D) throws {
        while let fieldNumber = try decoder.nextFieldNumber() {
            switch fieldNumber {
            case 1:
                workFeature = try decoder.decodeSingularStringField()
            case 2:
                branchName = try decoder.decodeSingularStringField()
            case 3:
                worktreeRemoved = try decoder.decodeSingularBoolField()
            case 4:
                branchDeleted = try decoder.decodeSingularBoolField()
            case 5:
                try decoder.decodeRepeatedStringField(value: &warnings)
            default:
                try decoder.skipField()
            }
        }
    }

    func traverse<V: SwiftProtobuf.Visitor>(visitor: inout V) throws {
        if !workFeature.isEmpty {
            try visitor.visitSingularStringField(value: workFeature, fieldNumber: 1)
        }
        if !branchName.isEmpty {
            try visitor.visitSingularStringField(value: branchName, fieldNumber: 2)
        }
        if worktreeRemoved {
            try visitor.visitSingularBoolField(value: worktreeRemoved, fieldNumber: 3)
        }
        if branchDeleted {
            try visitor.visitSingularBoolField(value: branchDeleted, fieldNumber: 4)
        }
        if !warnings.isEmpty {
            try visitor.visitRepeatedStringField(value: warnings, fieldNumber: 5)
        }
        try unknownFields.traverse(visitor: &visitor)
    }

    static func == (lhs: Branchbox_Agent_TeardownSummary, rhs: Branchbox_Agent_TeardownSummary) -> Bool {
        lhs.workFeature == rhs.workFeature &&
            lhs.branchName == rhs.branchName &&
            lhs.worktreeRemoved == rhs.worktreeRemoved &&
            lhs.branchDeleted == rhs.branchDeleted &&
            lhs.warnings == rhs.warnings &&
            lhs.unknownFields == rhs.unknownFields
    }
}
