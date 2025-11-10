import Foundation
import GRPC

struct Branchbox_Agent_FeatureServiceClient: GRPCClient {
    let channel: GRPCChannel
    var defaultCallOptions: CallOptions

    init(channel: GRPCChannel, defaultCallOptions: CallOptions = CallOptions()) {
        self.channel = channel
        self.defaultCallOptions = defaultCallOptions
    }

    func list(
        _ request: Branchbox_Agent_ListRequest,
        callOptions: CallOptions? = nil
    ) -> UnaryCall<Branchbox_Agent_ListRequest, Branchbox_Agent_ListResponse> {
        self.makeUnaryCall(
            path: "/branchbox.agent.FeatureService/List",
            request: request,
            callOptions: callOptions ?? defaultCallOptions,
            interceptors: []
        )
    }

    func start(
        _ request: Branchbox_Agent_StartRequest,
        callOptions: CallOptions? = nil
    ) -> UnaryCall<Branchbox_Agent_StartRequest, Branchbox_Agent_StartResponse> {
        self.makeUnaryCall(
            path: "/branchbox.agent.FeatureService/Start",
            request: request,
            callOptions: callOptions ?? defaultCallOptions,
            interceptors: []
        )
    }

    func teardown(
        _ request: Branchbox_Agent_TeardownRequest,
        callOptions: CallOptions? = nil
    ) -> UnaryCall<Branchbox_Agent_TeardownRequest, Branchbox_Agent_TeardownResponse> {
        self.makeUnaryCall(
            path: "/branchbox.agent.FeatureService/Teardown",
            request: request,
            callOptions: callOptions ?? defaultCallOptions,
            interceptors: []
        )
    }

    func status(
        _ request: Branchbox_Agent_StatusRequest,
        callOptions: CallOptions? = nil
    ) -> UnaryCall<Branchbox_Agent_StatusRequest, Branchbox_Agent_StatusResponse> {
        self.makeUnaryCall(
            path: "/branchbox.agent.FeatureService/Status",
            request: request,
            callOptions: callOptions ?? defaultCallOptions,
            interceptors: []
        )
    }
}
