import Foundation

/// Everything the app asks of BranchBox. Implemented by `CLIBackend` (BranchBoxCLI) and
/// `PreviewBackend` (BranchBoxPreview); the stores only ever see this protocol.
///
/// Every throwing method throws only `BackendError`. Cancellation is `Task` cancellation, and a
/// cancelled method throws `.cancelled` only after the underlying process group has exited.
public protocol BranchBoxBackend: Sendable {
    // Identity & environment
    func identity() async throws -> BackendIdentity
    func doctor(_ project: ProjectRef?) async -> DoctorReport                 // never throws; per-check status

    // Projects
    func resolveProject(at folder: URL) async throws -> ProjectResolution      // normalizes to the MAIN worktree
    func detect(_ folder: URL) async throws -> DetectReport
    func readConfig(_ project: ProjectRef) async throws -> ProjectConfigDocument
    func applyConfig(_ patch: ConfigPatch, to project: ProjectRef, dryRun: Bool) async throws -> ConfigApplyResult   // .config
    func setTunnelCredentials(_ request: TunnelCredentialsRequest, in project: ProjectRef) async throws -> TunnelCredentialsResult // .tunnelCredentials
    func initProject(_ request: InitRequest, progress: @escaping ProgressSink) async throws -> InitReport

    // Feature reads
    func listFeatures(in project: ProjectRef, includeRemoved: Bool) async throws -> FeatureListing   // + strays
    func listBranches(in project: ProjectRef) async throws -> BranchList
    func previewName(_ input: String, in project: ProjectRef) async throws -> NamePreview
    func planTeardown(_ request: TeardownRequest) async throws -> TeardownPlanDocument               // CLI --dry-run or app preflight
    func devcontainerStatus(for feature: FeatureRef) async throws -> DevcontainerStatus

    // Feature mutations (cancellable; progress streamed)
    func startFeature(_ request: StartFeatureRequest, progress: @escaping ProgressSink) async throws -> StartSummary
    func teardownFeature(_ request: TeardownRequest, progress: @escaping ProgressSink) async throws -> TeardownOutcome
    func exec(_ request: ExecRequest, progress: @escaping ProgressSink) async throws -> ExecResult     // non-zero inner exit is DATA
    func devcontainer(_ action: DevcontainerAction, for feature: FeatureRef, progress: @escaping ProgressSink) async throws -> DevcontainerResult
    func syncDevcontainers(_ request: SyncRequest, progress: @escaping ProgressSink) async throws -> SyncReport
    func openTunnel(_ feature: FeatureRef, progress: @escaping ProgressSink) async throws -> TunnelChange
    func removeTunnel(_ feature: FeatureRef, force: Bool, progress: @escaping ProgressSink) async throws -> TunnelChange
    func deleteBranch(_ branch: String, in project: ProjectRef, force: Bool) async throws
    func removeStray(_ stray: StrayWorktree, in project: ProjectRef, discardChanges: Bool) async throws

    /// Shell-escaped, redacted command line for "Copy as Command"; nil for non-CLI backends.
    func previewCommandLine(_ request: OperationRequestContext) -> String?
}
