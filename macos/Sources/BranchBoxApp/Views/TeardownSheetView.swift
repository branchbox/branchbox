import SwiftUI

struct TeardownSheetView: View {
    let feature: FeatureViewData
    @Binding var options: TeardownOptions
    let onConfirm: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Teardown \(feature.workFeature)?").font(.headline)
            Toggle("Force removal", isOn: $options.force)
            Toggle("Complete spec", isOn: $options.completeSpec)
            Toggle("Delete branch", isOn: $options.deleteBranch)
            HStack {
                Button("Cancel", role: .cancel) { onCancel() }
                Spacer()
                Button("Teardown", role: .destructive) { onConfirm() }
            }
        }
        .padding()
        .frame(width: 320)
    }
}
