import BranchBoxKit
import Foundation

extension BackendError {
    /// One line naming the cause, for notifications and the operation history. The app's presentation layer
    /// (SW-3) has the full wording and recoveries.
    var briefSummary: String {
        switch self {
        case .cliNotFound:
            "The BranchBox CLI was not found"
        case .cliTooOld(let found, let minimum, _):
            "BranchBox CLI \(found) is too old; version \(minimum) or later is required"
        case .cliUnusable(_, let reason):
            reason
        case .launchFailed(let executable, let reason):
            "Could not launch \(executable): \(reason)"
        case .projectInvalid(.missing(let path)), .projectInvalid(.workingDirectoryMissing(let path)):
            "The folder \(path) does not exist"
        case .projectInvalid(.notGitRepository(let path)):
            "\(path) is not a git repository"
        case .projectInvalid(.notInitialized(let path)):
            "BranchBox is not set up in \(path)"
        case .refused(let refusal):
            refusal.message
        case .partial(let failure):
            failure.remaining.message
        case .commandFailed(let diagnostics):
            diagnostics.summary
        case .decodeFailed(let what, let detail, _):
            "Could not read \(what): \(detail)"
        case .registryCorrupted(let path, _):
            "The feature registry \(path) cannot be read"
        case .unsupported(let capability, let minimumCLI):
            "This needs BranchBox CLI \(minimumCLI) or later (\(capability.rawValue))"
        case .timedOut(let operation, let after, _):
            "\(operation) did not finish within \(after.seconds.formatted(.number.precision(.fractionLength(0...1)))) s"
        case .cancelled(let note):
            note ?? "Stopped"
        }
    }
}
