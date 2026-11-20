// Microphone consent shim: status and request of the system microphone
// permission (AVFoundation). C ABI surface consumed by crates/audio/src/mic_consent.rs.
//
// Requesting access from a process whose main bundle does not declare
// NSMicrophoneUsageDescription terminates the process, so the request refuses
// to run in that case.

#ifndef PRXVC_MIC_H
#define PRXVC_MIC_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Result of prxvc_mic_permission_status.
enum {
    PRXVC_MIC_GRANTED = 0,
    PRXVC_MIC_DENIED = 1,       // denied or restricted
    PRXVC_MIC_UNDETERMINED = 2,
};

// The current microphone permission. Never prompts.
int32_t prxvc_mic_permission_status(void);

// Non-zero when the main bundle's Info.plist declares NSMicrophoneUsageDescription.
int32_t prxvc_mic_bundle_declares_usage(void);

// Ask for microphone access; blocks until the user answers or timeout_ms
// passes. Returns 1 granted, 0 refused, -1 no answer in time, -2 not allowed
// to ask (the bundle does not declare the usage key).
int32_t prxvc_mic_request(int32_t timeout_ms);

#ifdef __cplusplus
}
#endif

#endif
