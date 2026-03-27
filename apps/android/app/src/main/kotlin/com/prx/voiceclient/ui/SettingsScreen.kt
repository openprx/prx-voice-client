package com.prx.voiceclient.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
fun SettingsScreen() {
    var language by remember { mutableStateOf("en") }
    var autoPlay by remember { mutableStateOf(true) }
    var saveHistory by remember { mutableStateOf(true) }

    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Text("Settings", style = MaterialTheme.typography.headlineMedium)

        Spacer(Modifier.height(24.dp))

        // Language
        Text("Language", style = MaterialTheme.typography.titleMedium)
        Row(Modifier.padding(vertical = 8.dp)) {
            FilterChip(selected = language == "en", onClick = { language = "en" }, label = { Text("English") })
            Spacer(Modifier.width(8.dp))
            FilterChip(selected = language == "zh-CN", onClick = { language = "zh-CN" }, label = { Text("\u4e2d\u6587") })
        }

        Spacer(Modifier.height(16.dp))

        // Toggles
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("Auto-play replies")
            Switch(checked = autoPlay, onCheckedChange = { autoPlay = it })
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("Save history")
            Switch(checked = saveHistory, onCheckedChange = { saveHistory = it })
        }

        Spacer(Modifier.height(24.dp))

        Text("Version: 0.1.0", style = MaterialTheme.typography.bodySmall)
    }
}
