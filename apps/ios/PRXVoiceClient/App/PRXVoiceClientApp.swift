import SwiftUI

@main
struct PRXVoiceClientApp: App {
    @StateObject private var sessionState = SessionStateManager()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(sessionState)
                .onAppear {
                    sessionState.initialize()
                }
        }
    }
}
