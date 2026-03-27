package com.prx.voiceclient.ui

import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.navigation.compose.*
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import com.prx.voiceclient.bridge.SessionStateManager

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainNavigation(sessionState: SessionStateManager) {
    val navController = rememberNavController()
    var selectedRoute by remember { mutableStateOf("session") }

    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    icon = { Icon(Icons.Default.Mic, "Session") },
                    label = { Text("Session") },
                    selected = selectedRoute == "session",
                    onClick = {
                        selectedRoute = "session"
                        navController.navigate("session") {
                            popUpTo("session") { inclusive = true }
                        }
                    }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Default.History, "History") },
                    label = { Text("History") },
                    selected = selectedRoute == "history",
                    onClick = {
                        selectedRoute = "history"
                        navController.navigate("history") {
                            popUpTo("session")
                        }
                    }
                )
                NavigationBarItem(
                    icon = { Icon(Icons.Default.Settings, "Settings") },
                    label = { Text("Settings") },
                    selected = selectedRoute == "settings",
                    onClick = {
                        selectedRoute = "settings"
                        navController.navigate("settings") {
                            popUpTo("session")
                        }
                    }
                )
            }
        }
    ) { padding ->
        NavHost(navController, startDestination = "session", Modifier.padding(padding)) {
            composable("session") { SessionScreen(sessionState) }
            composable("history") { HistoryScreen() }
            composable("settings") { SettingsScreen() }
        }
    }
}
