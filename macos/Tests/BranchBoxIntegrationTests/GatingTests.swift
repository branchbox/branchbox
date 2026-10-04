import Foundation
import Testing

// Integration suites run against a real CLI only when BRANCHBOX_IT=1, using BRANCHBOX_IT_CLI or the CLI found
// on PATH and the usual install folders (DESIGN §13.2). SW-1 adds the smoke suite and VER-1 the full one; each
// gates itself the same way, so a plain `swift test` skips them.

@Suite struct GatingTests {
    @Test(.enabled(if: ProcessInfo.processInfo.environment["BRANCHBOX_IT"] == "1"))
    func cliUnderTestIsConfiguredOrResolvable() {
        let environment = ProcessInfo.processInfo.environment
        let files = FileManager.default
        if let configured = environment["BRANCHBOX_IT_CLI"], !configured.isEmpty {
            #expect(files.isExecutableFile(atPath: configured), "BRANCHBOX_IT_CLI=\(configured) is not an executable file")
            return
        }
        let home = files.homeDirectoryForCurrentUser.path
        let folders = (environment["PATH"] ?? "").split(separator: ":").map(String.init)
            + ["/opt/homebrew/bin", "/usr/local/bin", "\(home)/.cargo/bin", "\(home)/.local/bin"]
        let found = folders.map { "\($0)/branchbox" }.first { files.isExecutableFile(atPath: $0) }
        #expect(found != nil, "Set BRANCHBOX_IT_CLI or install branchbox; no executable branchbox in \(folders.joined(separator: ":"))")
    }
}
