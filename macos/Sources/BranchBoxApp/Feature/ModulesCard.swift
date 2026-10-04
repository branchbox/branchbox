import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The setup checklist: one row per module outcome (status, duration, notes, forced).
struct ModulesCard: View {
    let record: FeatureRecord

    var body: some View {
        FeatureCard("Setup", systemImage: "checklist") {
            if record.moduleOutcomes.isEmpty {
                Text(record.startMode == StartFeatureRequest.Mode.minimal.rawValue
                     ? "Quick features skip the setup modules."
                     : "No setup steps were recorded for this feature.")
                    .foregroundStyle(.secondary)
            } else {
                ModuleChecklist(outcomes: record.moduleOutcomes)
            }
        }
    }
}

#Preview("Setup") {
    ModulesCard(record: PreviewSamples.features[0])
        .padding()
        .frame(width: 420)
}
