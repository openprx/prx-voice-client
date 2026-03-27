package com.prx.voiceclient.model

data class VoiceSession(
    val id: String,
    val createdAt: Long,
    val endedAt: Long? = null,
    val state: String,
    val turnCount: Int = 0,
    val transcriptPreview: String? = null
)
