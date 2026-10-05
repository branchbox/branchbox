import AppKit
import BranchBoxKit
import SwiftUI

/// The editable state behind Project Settings: one field per `config get` key descriptor (§5.10), the values the
/// CLI reported, and the user's edits. `patch` holds exactly the keys whose value changed, so saving never
/// rewrites a key the user did not touch.
///
/// Legacy CLIs have no key table; their form is built from the effective config with the built-in descriptor list
/// and is read-only (`editable == false`).
struct ConfigForm: Hashable {
    /// Project Settings' tabs, in order.
    enum Tab: String, CaseIterable, Identifiable, Hashable {
        case features, teardown, runtime, sharing, codingAgent
        var id: String { rawValue }

        var title: String {
            switch self {
            case .features: "Features"
            case .teardown: "Teardown"
            case .runtime: "Runtime"
            case .sharing: "Sharing"
            case .codingAgent: "Coding Agent"
            }
        }

        /// The tab a dotted key belongs to; keys of sections this app version does not know go to Features.
        static func of(_ key: String) -> Tab {
            if key.hasPrefix("feature.teardown.") { return .teardown }
            if key.hasPrefix("runtime.") { return .runtime }
            if key.hasPrefix("tunnel.") { return .sharing }
            if key.hasPrefix("editor.") { return .codingAgent }
            return .features
        }
    }

    /// A suggested value for a free-text key, with the words it reads as.
    struct Suggestion: Hashable, Identifiable {
        let value: String
        let label: String
        var id: String { value }
    }

    /// How a descriptor's `type` is edited. A few free-text keys get a picker of known values plus Custom….
    enum Kind: Hashable {
        case bool, choice([String]), text, list
        /// A picker of `options`, a "not set" entry reading `unset`, and Custom… with a text field (`customLabel`).
        case suggested(options: [Suggestion], unset: String, customLabel: String)

        init(_ descriptor: ConfigKeyDescriptor) {
            if descriptor.type == "string", let suggested = ConfigForm.suggestedKinds[descriptor.key] {
                self = suggested
                return
            }
            switch descriptor.type {
            case "bool": self = .bool
            case "enum": self = .choice(descriptor.allowed)
            case "string_list": self = .list
            default: self = .text
            }
        }
    }

    struct Field: Identifiable, Hashable {
        let descriptor: ConfigKeyDescriptor
        var id: String { descriptor.key }
        var key: String { descriptor.key }
        var kind: Kind { Kind(descriptor) }
        var tab: Tab { Tab.of(descriptor.key) }
        var label: String { ConfigForm.label(for: descriptor.key) }
        /// The app's own words for the key; the CLI's description only for keys this app version doesn't know.
        var help: String { ConfigForm.description(for: descriptor.key, cli: descriptor.description) }
        /// VS Code–specific keys, shown under their own heading in the Coding Agent tab.
        var isEditorIntegration: Bool { ConfigForm.editorIntegrationKeys.contains(descriptor.key) }
    }

    let editable: Bool
    let fields: [Field]
    /// What the CLI reported for each key; `.null` means not set.
    let original: [String: JSONValue]
    private(set) var values: [String: JSONValue]

    /// Keys the form never edits: the token file is written by `tunnel credentials set`, never by hand.
    static let managedKeys: Set<String> = ["tunnel.providers.cloudflared.api_token_path"]

    init(document: ProjectConfigDocument) {
        let descriptors = document.keys.isEmpty ? Self.builtInDescriptors(for: document.effective) : document.keys
        editable = document.editable && !document.keys.isEmpty
        fields = descriptors.filter { !Self.managedKeys.contains($0.key) }.map(Field.init)
        var original: [String: JSONValue] = [:]
        for descriptor in descriptors {
            original[descriptor.key] = descriptor.value ?? .null
        }
        self.original = original
        values = original
    }

    func fields(in tab: Tab) -> [Field] { fields.filter { $0.tab == tab } }

    func value(_ key: String) -> JSONValue { values[key] ?? .null }

    /// Sets `key`. A blank text or an empty list means "not set". Ignored on a read-only form.
    mutating func set(_ key: String, _ value: JSONValue) {
        guard editable else { return }
        values[key] = Self.normalized(value)
    }

    /// Only the keys whose value differs from what the CLI reported, in field order.
    var patch: ConfigPatch {
        ConfigPatch(changes: fields.compactMap { field in
            let new = value(field.key)
            guard new != (original[field.key] ?? .null) else { return nil }
            return ConfigChange(key: field.key, value: new == .null ? nil : new)
        })
    }

    var hasChanges: Bool { !patch.changes.isEmpty }

    /// The app-side check of a key's current value (the CLI validates again on save).
    func problem(for key: String) -> String? {
        guard key == "feature.branch_prefix", case .string(let prefix) = value(key) else { return nil }
        return NameRules.branchPrefixProblem(prefix)
    }

    var problems: [String: String] {
        Dictionary(uniqueKeysWithValues: fields.compactMap { field in problem(for: field.key).map { (field.key, $0) } })
    }

    /// Restores the reported values.
    mutating func revert() { values = original }

    // MARK: Presentation

    /// A curated label for the keys BranchBox ships; other keys get their last component in words.
    static func label(for key: String) -> String {
        if let known = labels[key] { return known }
        let last = key.split(separator: ".").last.map(String.init) ?? key
        let words = last.replacingOccurrences(of: "_", with: " ")
        return words.prefix(1).uppercased() + words.dropFirst()
    }

    private static let labels: [String: String] = [
        "runtime.provider": "Default runtime",
        "runtime.sbx.run_services": "Services to run in Docker Sandboxes",
        "feature.branch_prefix": "Branch prefix",
        "feature.teardown.delete_branch_by_default": "Delete the branch when tearing down",
        "feature.teardown.force_delete_unmerged_by_default": "Also delete branches with unmerged commits",
        "feature.teardown.prompt_force_delete_unmerged": "Ask first when tearing down from the command line",
        "tunnel.enabled": "Share features through a tunnel",
        "tunnel.default_provider": "Tunnel provider",
        "tunnel.providers.cloudflared.account_id": "Cloudflare account ID",
        "tunnel.providers.cloudflared.tunnel_name_prefix": "Tunnel name prefix",
        "tunnel.providers.cloudflared.dns_zone": "DNS zone",
        "tunnel.providers.cloudflared.service_url": "Service URL",
        "tunnel.providers.cloudflared.manual_instructions": "Set up tunnels by hand",
        "editor.default_agent": "Coding agent",
        "editor.auto_launch_agent_terminal": "Open the agent when VS Code attaches",
        "editor.preferred_sidebar_view": "Sidebar view to show",
        "editor.hide_secondary_sidebar": "Hide VS Code's secondary sidebar",
    ]

    /// Plain descriptions for the keys BranchBox ships. The CLI's `config get` descriptions name flags and keys
    /// (`--runtime`, `dns_zone`, `git branch -D`); the form never shows those for a key it knows.
    static func description(for key: String, cli: String) -> String {
        descriptions[key] ?? cli
    }

    private static let descriptions: [String: String] = [
        "runtime.provider": "Used when you don't pick a runtime while starting a feature.",
        "runtime.sbx.run_services": "Docker Compose services to start inside each Docker Sandbox.",
        "feature.branch_prefix": "New features start on a branch named prefix/feature, for example feature/checkout.",
        "feature.teardown.delete_branch_by_default": "Tear Down starts with Delete branch selected. Branches with work that isn't merged are kept unless you choose otherwise.",
        "feature.teardown.force_delete_unmerged_by_default": "Tear Down also deletes branches whose commits aren't merged anywhere. That work is lost.",
        "feature.teardown.prompt_force_delete_unmerged": "For branchbox in Terminal: ask before deleting a branch whose commits aren't merged. The app always asks.",
        "tunnel.enabled": "New features get a public address that you can share.",
        "tunnel.default_provider": "The service that hosts the shared addresses.",
        "tunnel.providers.cloudflared.account_id": "Shown on your account's overview page in the Cloudflare dashboard.",
        "tunnel.providers.cloudflared.tunnel_name_prefix": "Tunnels are named prefix-feature. With a DNS zone, addresses look like prefix-feature.zone.",
        "tunnel.providers.cloudflared.dns_zone": "A domain in your Cloudflare account that feature addresses are created under.",
        "tunnel.providers.cloudflared.service_url": "The address inside the feature that visitors reach, for example http://app:5001.",
        "tunnel.providers.cloudflared.manual_instructions": "Show setup steps to follow yourself instead of creating tunnels automatically.",
        "editor.default_agent": "The agent Launch opens in this project's features. It takes precedence over App Settings.",
        "editor.auto_launch_agent_terminal": "Opens a terminal running the agent when VS Code opens a feature's dev container.",
        "editor.preferred_sidebar_view": "The VS Code sidebar view shown when a feature's dev container opens.",
        "editor.hide_secondary_sidebar": "Keeps VS Code's right-hand sidebar closed when a feature's dev container opens.",
    ]

    /// Keys that only affect VS Code's dev container window.
    static let editorIntegrationKeys: Set<String> = [
        "editor.auto_launch_agent_terminal", "editor.preferred_sidebar_view", "editor.hide_secondary_sidebar",
    ]

    /// Free-text keys edited as a picker of known values (matching App Settings › Coding Agent for the agent).
    static let suggestedKinds: [String: Kind] = [
        "editor.default_agent": .suggested(options: [Suggestion(value: "claude", label: "Claude Code"),
                                                     Suggestion(value: "codex", label: "Codex")],
                                           unset: "Same as App Settings", customLabel: "Command"),
        "editor.preferred_sidebar_view": .suggested(options: [
            Suggestion(value: "workbench.view.explorer", label: "Explorer"),
            Suggestion(value: "workbench.view.scm", label: "Source Control"),
            Suggestion(value: "workbench.view.search", label: "Search"),
            Suggestion(value: "workbench.view.debug", label: "Run and Debug"),
            Suggestion(value: "workbench.view.extensions", label: "Extensions"),
        ], unset: "VS Code's choice", customLabel: "View ID"),
    ]

    /// How an enum value reads in a picker.
    static func choiceLabel(_ value: String, key: String) -> String {
        if key == "runtime.provider" { return RuntimeProvider(raw: value).label }
        if value == "cloudflared" { return "Cloudflare Tunnel" }
        return value
    }

    /// Why an enum value can't be picked on this Mac, or nil when it can.
    static func unavailableReason(_ value: String, key: String) -> String? {
        guard key == "runtime.provider" else { return nil }
        switch value {
        case "local-vm": return "Needs Linux with KVM"
        case "in-guest": return "Only inside an isolated dev container"
        default: return nil
        }
    }

    /// A value as text, for the review list and read-only rows.
    static func display(_ value: JSONValue?) -> String {
        switch value {
        case nil, .null?: "Not set"
        case .bool(let flag)?: flag ? "On" : "Off"
        case .number(let number)?: number.formatted()
        case .string(let text)?: text.isEmpty ? "Not set" : text
        case .array(let items)?: items.isEmpty ? "None" : items.map { display($0) }.joined(separator: ", ")
        case .object?: "…"
        }
    }

    private static func normalized(_ value: JSONValue) -> JSONValue {
        switch value {
        case .string(let text) where text.trimmingCharacters(in: .whitespaces).isEmpty: .null
        case .array(let items) where items.isEmpty: .null
        default: value
        }
    }

    /// The keys a legacy CLI's config.json can hold, read from its effective config (§6.2 readConfig fallback).
    static func builtInDescriptors(for config: ProjectConfig) -> [ConfigKeyDescriptor] {
        func string(_ text: String?) -> JSONValue { text.map(JSONValue.string) ?? .null }
        let cloudflared = config.cloudflared
        return [
            ConfigKeyDescriptor(key: "feature.branch_prefix", type: "string", value: .string(config.branchPrefix),
                                description: "Prefix of the branch a new feature gets."),
            ConfigKeyDescriptor(key: "feature.teardown.delete_branch_by_default", type: "bool",
                                value: .bool(config.deleteBranchByDefault)),
            ConfigKeyDescriptor(key: "feature.teardown.force_delete_unmerged_by_default", type: "bool",
                                value: .bool(config.forceDeleteUnmergedByDefault)),
            ConfigKeyDescriptor(key: "feature.teardown.prompt_force_delete_unmerged", type: "bool",
                                value: .bool(config.promptForceDeleteUnmerged)),
            ConfigKeyDescriptor(key: "runtime.provider", type: "enum", allowed: ["container", "sbx", "local-vm", "in-guest"],
                                value: .string(config.runtimeProvider.raw)),
            ConfigKeyDescriptor(key: "runtime.sbx.run_services", type: "string_list",
                                value: .array(config.sbxRunServices.map(JSONValue.string))),
            ConfigKeyDescriptor(key: "tunnel.enabled", type: "bool", value: .bool(config.tunnelEnabled)),
            ConfigKeyDescriptor(key: "tunnel.providers.cloudflared.account_id", type: "string",
                                value: string(cloudflared?.accountID)),
            ConfigKeyDescriptor(key: "tunnel.providers.cloudflared.tunnel_name_prefix", type: "string",
                                value: string(cloudflared?.tunnelNamePrefix)),
            ConfigKeyDescriptor(key: "tunnel.providers.cloudflared.dns_zone", type: "string", value: string(cloudflared?.dnsZone)),
            ConfigKeyDescriptor(key: "tunnel.providers.cloudflared.service_url", type: "string",
                                value: string(cloudflared?.serviceURL)),
            ConfigKeyDescriptor(key: "tunnel.providers.cloudflared.manual_instructions", type: "bool",
                                value: .bool(cloudflared?.manualInstructions ?? false)),
            ConfigKeyDescriptor(key: "editor.default_agent", type: "string", value: string(config.editorDefaultAgent)),
            ConfigKeyDescriptor(key: "editor.auto_launch_agent_terminal", type: "bool", value: .bool(config.autoLaunchAgentTerminal)),
        ]
    }
}

/// One config key as a form row: a Toggle, Picker, TextField or token field, with the key's description and an
/// inline error (local validation, or the CLI's `config_invalid` for this key).
struct ConfigFieldRow: View {
    let field: ConfigForm.Field
    @Binding var form: ConfigForm
    var error: String?

    /// Custom… is picked in a suggested-values picker; the text field shows until a listed value is chosen.
    @State private var editingCustom = false
    static let unsetTag = ""
    static let customTag = "\u{0}custom"

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            control
                .disabled(!form.editable)
            if let error {
                Label(error, systemImage: "exclamationmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityLabel("Error: \(error)")
            }
        }
        .accessibilityIdentifier("projectSettings.field.\(field.key)")
    }

    @ViewBuilder private var control: some View {
        switch field.kind {
        case .bool:
            Toggle(isOn: boolBinding) { labelStack }
        case .choice(let allowed):
            Picker(selection: stringBinding) {
                ForEach(allowed, id: \.self) { value in
                    let reason = ConfigForm.unavailableReason(value, key: field.key)
                    Text(reason.map { "\(ConfigForm.choiceLabel(value, key: field.key)) — \($0)" }
                         ?? ConfigForm.choiceLabel(value, key: field.key))
                        .tag(value)
                        .selectionDisabled(reason != nil && value != stringBinding.wrappedValue)
                }
            } label: {
                labelStack
            }
        case .text:
            LabeledContent {
                TextField(field.label, text: stringBinding, prompt: Text(placeholder))
                    .textFieldStyle(.roundedBorder)
                    .labelsHidden()
                    .frame(width: 240)
            } label: {
                labelStack
            }
        case .suggested(let options, let unset, let customLabel):
            Picker(selection: suggestionBinding(options)) {
                Text(unset).tag(Self.unsetTag)
                ForEach(options) { Text($0.label).tag($0.value) }
                if let custom = customValue(options) {
                    Text(custom).tag(custom)
                }
                Divider()
                Text("Custom…").tag(Self.customTag)
            } label: {
                labelStack
            }
            if editingCustom {
                LabeledContent(customLabel) {
                    TextField(customLabel, text: stringBinding, prompt: Text("Not set"))
                        .textFieldStyle(.roundedBorder)
                        .font(.body.monospaced())
                        .labelsHidden()
                        .frame(width: 240)
                }
            }
        case .list:
            VStack(alignment: .leading, spacing: 8) {
                labelStack
                ConfigListEditor(items: listBinding, noun: field.key.hasSuffix("services") ? "service" : "entry")
            }
        }
    }

    private var labelStack: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(field.label)
            if !field.help.isEmpty {
                Text(Self.markdown(field.help))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var placeholder: String {
        if case .string(let text)? = field.descriptor.defaultValue, !text.isEmpty { return text }
        return "Not set"
    }

    /// Descriptions use `code` spans; render them, and fall back to the plain text.
    static func markdown(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }

    private var boolBinding: Binding<Bool> {
        Binding(get: {
            if case .bool(let flag) = form.value(field.key) { return flag }
            if case .bool(let flag)? = field.descriptor.defaultValue { return flag }
            return false
        }, set: { form.set(field.key, .bool($0)) })
    }

    /// The current value when it isn't one of the listed options (it stays pickable).
    private func customValue(_ options: [ConfigForm.Suggestion]) -> String? {
        guard case .string(let text) = form.value(field.key), !text.isEmpty,
              !options.contains(where: { $0.value == text }) else { return nil }
        return text
    }

    private func suggestionBinding(_ options: [ConfigForm.Suggestion]) -> Binding<String> {
        Binding(get: {
            if editingCustom { return Self.customTag }
            if case .string(let text) = form.value(field.key) { return text }
            return Self.unsetTag
        }, set: { tag in
            switch tag {
            case Self.customTag:
                editingCustom = true
            case Self.unsetTag:
                editingCustom = false
                form.set(field.key, .null)
            default:
                editingCustom = false
                form.set(field.key, .string(tag))
            }
        })
    }

    private var stringBinding: Binding<String> {
        Binding(get: {
            if case .string(let text) = form.value(field.key) { return text }
            return ""
        }, set: { form.set(field.key, .string($0)) })
    }

    private var listBinding: Binding<[String]> {
        Binding(get: {
            guard case .array(let items) = form.value(field.key) else { return [] }
            return items.compactMap { if case .string(let text) = $0 { text } else { nil } }
        }, set: { form.set(field.key, .array($0.map(JSONValue.string))) })
    }
}

/// Editor for `string_list` keys: each entry is a removable chip, and a field with [Add] appends one (Return adds
/// too). Chips with an × read as a list of separate values, unlike a token field that looks like plain text.
struct ConfigListEditor: View {
    @Binding var items: [String]
    /// What one entry is called ("service"), for the prompt, the Add help and accessibility labels.
    var noun: String = "entry"

    @Environment(\.isEnabled) private var isEnabled
    @State private var draft = ""

    private var trimmedDraft: String { draft.trimmingCharacters(in: .whitespaces) }
    private var canAdd: Bool { !trimmedDraft.isEmpty && !items.contains(trimmedDraft) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if items.isEmpty {
                Text("None")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            } else {
                FlowLayout(spacing: 6) {
                    ForEach(items, id: \.self) { item in chip(item) }
                }
            }
            HStack(spacing: 6) {
                TextField("Add \(noun)", text: $draft, prompt: Text("Add \(noun)"))
                    .textFieldStyle(.roundedBorder)
                    .font(.callout.monospaced())
                    .labelsHidden()
                    .frame(maxWidth: 200)
                    .onSubmit(add)
                Button("Add", action: add)
                    .disabled(!canAdd)
                    .help(items.contains(trimmedDraft) ? "“\(trimmedDraft)” is already in the list" : "Add this \(noun)")
            }
            .controlSize(.small)
        }
        .accessibilityElement(children: .contain)
    }

    private func chip(_ item: String) -> some View {
        HStack(spacing: 4) {
            Text(item)
                .font(.callout.monospaced())
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 200)
                .fixedSize()
                .help(item)
            Button {
                items.removeAll { $0 == item }
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
            .help("Remove \(item)")
            .accessibilityLabel("Remove \(noun) \(item)")
        }
        .padding(.leading, 8)
        .padding(.trailing, 4)
        .padding(.vertical, 3)
        .background(Color.accentColor.opacity(isEnabled ? 0.14 : 0.06), in: RoundedRectangle(cornerRadius: 6))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.accentColor.opacity(isEnabled ? 0.35 : 0.15)))
    }

    private func add() {
        guard canAdd else { return }
        items.append(trimmedDraft)
        draft = ""
    }
}
