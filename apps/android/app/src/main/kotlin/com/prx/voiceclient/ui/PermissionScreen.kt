package com.prx.voiceclient.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
fun PermissionScreen(onRequestPermission: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Icon(Icons.Default.Mic, "Microphone", modifier = Modifier.size(64.dp))
        Spacer(Modifier.height(16.dp))
        Text("Microphone Access Required", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        Text("PRX Voice Client needs microphone access for voice conversations.")
        Spacer(Modifier.height(24.dp))
        Button(onClick = onRequestPermission) {
            Text("Grant Access")
        }
    }
}
