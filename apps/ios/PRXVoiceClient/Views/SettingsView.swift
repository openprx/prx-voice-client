import SwiftUI

struct SettingsView: View {
    @AppStorage("language") private var language = "en"
    @AppStorage("autoPlay") private var autoPlay = true
    @AppStorage("saveHistory") private var saveHistory = true

    var body: some View {
        NavigationStack {
            Form {
                Section("Language") {
                    Picker("Language", selection: $language) {
                        Text("English").tag("en")
                        Text("中文").tag("zh-CN")
                    }
                }

                Section("Audio") {
                    Toggle("Auto-play replies", isOn: $autoPlay)

                    HStack {
                        Text("Audio Route")
                        Spacer()
                        Text(AudioManager.shared.currentRoute())
                            .foregroundColor(.secondary)
                    }
                }

                Section("Privacy") {
                    Toggle("Save history", isOn: $saveHistory)
                }

                Section("About") {
                    HStack {
                        Text("Version")
                        Spacer()
                        Text("0.1.0")
                            .foregroundColor(.secondary)
                    }
                }
            }
            .navigationTitle("Settings")
        }
    }
}
