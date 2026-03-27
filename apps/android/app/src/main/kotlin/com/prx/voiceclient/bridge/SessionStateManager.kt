package com.prx.voiceclient.bridge

import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateListOf
import kotlinx.coroutines.*

/**
 * Manages session state and communicates with Rust Engine via RustBridge.
 */
class SessionStateManager {

    private val _currentState = mutableStateOf("Idle")
    val currentState: State<String> = _currentState

    private val _isActive = mutableStateOf(false)
    val isActive: State<Boolean> = _isActive

    private val _sessionId = mutableStateOf<String?>(null)
    val sessionId: State<String?> = _sessionId

    private val _transcriptLines = mutableStateListOf<TranscriptLine>()
    val transcriptLines: List<TranscriptLine> = _transcriptLines

    private val _errorMessage = mutableStateOf<String?>(null)
    val errorMessage: State<String?> = _errorMessage

    private var pollJob: Job? = null
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    data class TranscriptLine(
        val role: String,
        val text: String,
        val isFinal: Boolean
    )

    fun initialize() {
        RustBridge.initialize().onFailure { e ->
            _errorMessage.value = e.message
        }.onSuccess {
            _currentState.value = "Ready"
        }
    }

    fun startSession() {
        scope.launch {
            RustBridge.createSession().onSuccess { sid ->
                _sessionId.value = sid
                RustBridge.startSession(sid)
                _isActive.value = true
                _currentState.value = "Listening"
                startPolling()
            }.onFailure { e ->
                _errorMessage.value = e.message
            }
        }
    }

    fun stopSession() {
        val sid = _sessionId.value ?: return
        scope.launch {
            RustBridge.stopSession(sid)
            _isActive.value = false
            _currentState.value = "Completed"
            stopPolling()
            _sessionId.value = null
            _transcriptLines.clear()
        }
    }

    fun interrupt() {
        val sid = _sessionId.value ?: return
        scope.launch {
            RustBridge.interrupt(sid)
        }
    }

    private fun startPolling() {
        pollJob = scope.launch {
            while (isActive.value) {
                val events = RustBridge.pollEvents()
                for (event in events) {
                    processEvent(event)
                }
                delay(100)
            }
        }
    }

    private fun stopPolling() {
        pollJob?.cancel()
        pollJob = null
    }

    private fun processEvent(event: org.json.JSONObject) {
        val eventType = event.optString("event_type", "")
        when {
            eventType.contains("StateChanged") -> {
                val to = event.optString("to", "")
                if (to.isNotEmpty()) {
                    _currentState.value = to
                    _isActive.value = to != "Completed" && to != "Failed"
                }
            }
            eventType.contains("InterimTranscript") -> {
                val text = event.optString("text", "")
                if (text.isNotEmpty()) {
                    _transcriptLines.add(TranscriptLine("user", text, false))
                }
            }
            eventType.contains("FinalTranscript") -> {
                val text = event.optString("text", "")
                if (text.isNotEmpty()) {
                    _transcriptLines.add(TranscriptLine("user", text, true))
                }
            }
            eventType.contains("AssistantReply") -> {
                val delta = event.optString("delta", "")
                if (delta.isNotEmpty()) {
                    _transcriptLines.add(TranscriptLine("assistant", delta, true))
                }
            }
        }
    }

    fun cleanup() {
        stopPolling()
        scope.cancel()
        RustBridge.shutdown()
    }
}
