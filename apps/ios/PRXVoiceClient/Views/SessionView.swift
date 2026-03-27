import SwiftUI

struct SessionView: View {
    @EnvironmentObject var state: SessionStateManager

    @State private var hasMicPermission = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                // Status indicator
                HStack {
                    Circle()
                        .fill(statusColor)
                        .frame(width: 12, height: 12)
                    Text(state.currentState)
                        .font(.headline)
                }
                .padding()

                // Transcript
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(state.transcriptLines) { line in
                            HStack {
                                Text(line.role == "user" ? "You:" : "AI:")
                                    .font(.caption)
                                    .foregroundColor(.secondary)
                                Text(line.text)
                                    .font(.body)
                            }
                            .padding(.horizontal)
                        }
                    }
                }
                .frame(maxHeight: .infinity)

                // Controls
                HStack(spacing: 30) {
                    if !state.isActive {
                        Button(action: { state.startSession() }) {
                            Label("Start", systemImage: "play.fill")
                                .font(.title2)
                                .padding()
                                .background(Color.green)
                                .foregroundColor(.white)
                                .clipShape(Capsule())
                        }
                    } else {
                        Button(action: { state.interrupt() }) {
                            Label("Interrupt", systemImage: "hand.raised.fill")
                                .padding()
                                .background(Color.orange)
                                .foregroundColor(.white)
                                .clipShape(Capsule())
                        }

                        Button(action: { state.stopSession() }) {
                            Label("Stop", systemImage: "stop.fill")
                                .padding()
                                .background(Color.red)
                                .foregroundColor(.white)
                                .clipShape(Capsule())
                        }
                    }
                }
                .padding(.bottom, 30)
            }
            .navigationTitle("PRX Voice")
            .task {
                hasMicPermission = await AudioManager.shared.checkMicrophonePermission()
            }
        }
    }

    var statusColor: Color {
        switch state.currentState {
        case "Listening", "UserSpeaking": return .green
        case "Recognizing", "Thinking": return .yellow
        case "AssistantSpeaking": return .blue
        case "Failed": return .red
        case "Recovering": return .orange
        default: return .gray
        }
    }
}
