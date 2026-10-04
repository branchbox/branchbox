import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The setup checklist: one row per module outcome with its status icon and word, duration, notes and whether
/// it was forced, under the "3 ok · 1 skipped · 0 failed" summary.
struct ModuleChecklist: View {
    let outcomes: [ModuleOutcome]
    var showsSummary = true

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if showsSummary {
                Text(FeaturePresentation.moduleSummary(outcomes))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            ForEach(Array(outcomes.enumerated()), id: \.offset) { _, outcome in
                row(outcome)
            }
        }
    }

    private func row(_ outcome: ModuleOutcome) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: outcome.status.symbol)
                .foregroundStyle(outcome.status.tint.color)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline) {
                    Text(outcome.module)
                        .font(.body.weight(.medium))
                    Text(outcome.status.label)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    if outcome.forced {
                        Text("forced")
                            .font(.caption2)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(.orange.opacity(0.15), in: Capsule())
                    }
                    Spacer(minLength: 8)
                    if let duration = outcome.durationMs, duration >= 1 {   // "0 ms" is noise
                        Text(FeaturePresentation.duration(milliseconds: duration))
                            .font(.caption)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                }
                ForEach(Array(outcome.notes.enumerated()), id: \.offset) { _, note in
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }
}

#Preview("Setup checklists") {
    VStack(alignment: .leading, spacing: 24) {
        ModuleChecklist(outcomes: PreviewSamples.features[0].moduleOutcomes)
        ModuleChecklist(outcomes: PreviewSamples.features.first { $0.workFeature == "sbx-demo" }?.moduleOutcomes ?? [])
        ModuleChecklist(outcomes: [
            ModuleOutcome(module: "compose", status: .failed, durationMs: 1830, notes: ["port 5432 is already in use"], forced: true),
            ModuleOutcome(module: "database", status: .unknown("deferred")),
        ])
        ModuleChecklist(outcomes: [])
    }
    .frame(width: 360)
    .padding()
}
