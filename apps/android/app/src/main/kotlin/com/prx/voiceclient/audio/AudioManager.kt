package com.prx.voiceclient.audio

import android.content.Context
import android.media.AudioFocusRequest
import android.media.AudioManager

/**
 * Manages Android audio focus and routing for voice sessions.
 */
class AudioSessionManager(private val context: Context) {

    private val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private var focusRequest: AudioFocusRequest? = null

    fun requestAudioFocus(): Boolean {
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
            .setOnAudioFocusChangeListener { focusChange ->
                when (focusChange) {
                    AudioManager.AUDIOFOCUS_LOSS -> releaseAudioFocus()
                    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> { /* pause */ }
                    AudioManager.AUDIOFOCUS_GAIN -> { /* resume */ }
                }
            }
            .build()

        focusRequest = request
        return audioManager.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    fun releaseAudioFocus() {
        focusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
        focusRequest = null
    }

    fun isBluetoothConnected(): Boolean {
        return audioManager.isBluetoothScoOn || audioManager.isBluetoothA2dpOn
    }

    fun getCurrentOutputDevice(): String {
        val devices = audioManager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        return devices.firstOrNull()?.productName?.toString() ?: "Speaker"
    }
}
