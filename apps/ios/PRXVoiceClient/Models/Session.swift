import Foundation

struct VoiceSession: Identifiable {
    let id: String
    let createdAt: Date
    var endedAt: Date?
    var state: String
    var turnCount: Int
    var transcriptPreview: String?
}
