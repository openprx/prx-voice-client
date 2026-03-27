package com.prx.voiceclient

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import com.prx.voiceclient.bridge.SessionStateManager
import com.prx.voiceclient.ui.MainNavigation

class MainActivity : ComponentActivity() {
    private val sessionState = SessionStateManager()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        sessionState.initialize()

        setContent {
            MainNavigation(sessionState)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        sessionState.cleanup()
    }
}
