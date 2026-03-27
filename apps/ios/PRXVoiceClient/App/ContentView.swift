import SwiftUI

struct ContentView: View {
    @State private var selectedTab = 0

    var body: some View {
        TabView(selection: $selectedTab) {
            SessionView()
                .tabItem { Label("Session", systemImage: "waveform") }
                .tag(0)

            HistoryView()
                .tabItem { Label("History", systemImage: "clock") }
                .tag(1)

            SettingsView()
                .tabItem { Label("Settings", systemImage: "gear") }
                .tag(2)
        }
    }
}
