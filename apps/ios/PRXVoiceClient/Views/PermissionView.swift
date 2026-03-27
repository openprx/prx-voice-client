import SwiftUI

struct PermissionView: View {
    let onGranted: () -> Void

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "mic.fill")
                .font(.system(size: 60))
                .foregroundColor(.blue)

            Text("Microphone Access Required")
                .font(.title2)

            Text("PRX Voice Client needs microphone access for voice conversations.")
                .multilineTextAlignment(.center)
                .foregroundColor(.secondary)
                .padding(.horizontal)

            Button("Grant Access") {
                Task {
                    let granted = await AudioManager.shared.checkMicrophonePermission()
                    if granted {
                        onGranted()
                    }
                }
            }
            .buttonStyle(.borderedProminent)
        }
        .padding()
    }
}
