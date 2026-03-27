package com.prx.voiceclient.bridge

import org.json.JSONObject
import org.json.JSONArray

/**
 * JNI bridge to the Rust prxvc-ffi library.
 * All functions communicate via JSON strings.
 */
object RustBridge {

    init {
        System.loadLibrary("prxvc_ffi")
    }

    // Native methods matching C API
    private external fun nativeInitialize(configJson: String): String?
    private external fun nativeShutdown(): Int
    private external fun nativeCreateSession(configJson: String): String?
    private external fun nativeStartSession(sessionId: String): String?
    private external fun nativeStopSession(sessionId: String): String?
    private external fun nativeInterrupt(sessionId: String): String?
    private external fun nativePollEvents(): String?
    private external fun nativeGetState(sessionId: String): String?

    fun initialize(configJson: String = "{}"): Result<JSONObject> {
        return parseResult(nativeInitialize(configJson))
    }

    fun shutdown() {
        nativeShutdown()
    }

    fun createSession(configJson: String = "{}"): Result<String> {
        return parseResult(nativeCreateSession(configJson)).map { json ->
            json.optString("session_id", "")
        }
    }

    fun startSession(sessionId: String): Result<JSONObject> {
        return parseResult(nativeStartSession(sessionId))
    }

    fun stopSession(sessionId: String): Result<JSONObject> {
        return parseResult(nativeStopSession(sessionId))
    }

    fun interrupt(sessionId: String): Result<JSONObject> {
        return parseResult(nativeInterrupt(sessionId))
    }

    fun pollEvents(): List<JSONObject> {
        val raw = nativePollEvents() ?: return emptyList()
        return try {
            val array = JSONArray(raw)
            (0 until array.length()).map { array.getJSONObject(it) }
        } catch (e: Exception) {
            emptyList()
        }
    }

    fun getSessionState(sessionId: String): Result<JSONObject> {
        return parseResult(nativeGetState(sessionId))
    }

    private fun parseResult(raw: String?): Result<JSONObject> {
        if (raw == null) return Result.failure(RustBridgeException("Null response"))
        return try {
            val json = JSONObject(raw)
            if (json.has("error")) {
                Result.failure(RustBridgeException(json.getString("error")))
            } else {
                Result.success(json)
            }
        } catch (e: Exception) {
            Result.failure(RustBridgeException("Invalid response: ${e.message}"))
        }
    }
}

class RustBridgeException(message: String) : Exception(message)
