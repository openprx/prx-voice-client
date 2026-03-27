package com.prx.voiceclient.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.prx.voiceclient.bridge.SessionStateManager

@Composable
fun SessionScreen(sessionState: SessionStateManager) {
    val state by sessionState.currentState
    val isActive by sessionState.isActive

    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        // Status indicator
        Row(verticalAlignment = Alignment.CenterVertically) {
            Surface(
                shape = MaterialTheme.shapes.small,
                color = statusColor(state),
                modifier = Modifier.size(12.dp)
            ) {}
            Spacer(Modifier.width(8.dp))
            Text(state, style = MaterialTheme.typography.headlineSmall)
        }

        Spacer(Modifier.height(16.dp))

        // Transcript
        LazyColumn(
            modifier = Modifier.weight(1f).fillMaxWidth()
        ) {
            items(sessionState.transcriptLines) { line ->
                Row(Modifier.padding(vertical = 4.dp)) {
                    Text(
                        if (line.role == "user") "You: " else "AI: ",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.secondary
                    )
                    Text(line.text, style = MaterialTheme.typography.bodyMedium)
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // Controls
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            if (!isActive) {
                Button(
                    onClick = { sessionState.startSession() },
                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF4CAF50))
                ) {
                    Text("Start")
                }
            } else {
                Button(
                    onClick = { sessionState.interrupt() },
                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFFF9800))
                ) {
                    Text("Interrupt")
                }
                Button(
                    onClick = { sessionState.stopSession() },
                    colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFF44336))
                ) {
                    Text("Stop")
                }
            }
        }

        Spacer(Modifier.height(16.dp))
    }
}

fun statusColor(state: String): Color {
    return when (state) {
        "Listening", "UserSpeaking" -> Color(0xFF4CAF50)
        "Recognizing", "Thinking" -> Color(0xFFFFC107)
        "AssistantSpeaking" -> Color(0xFF2196F3)
        "Failed" -> Color(0xFFF44336)
        "Recovering" -> Color(0xFFFF9800)
        else -> Color.Gray
    }
}
