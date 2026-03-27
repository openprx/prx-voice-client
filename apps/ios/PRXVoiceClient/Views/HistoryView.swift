import SwiftUI

struct HistoryView: View {
    var body: some View {
        NavigationStack {
            List {
                Text("No sessions yet")
                    .foregroundColor(.secondary)
            }
            .navigationTitle("History")
        }
    }
}
