// Core Audio process-tap shim — C ABI surface consumed by Rust FFI.
//
// Captures the audio *rendered by a specific process* (e.g. a meeting app) on
// macOS 14.4+ via the Core Audio process-tap API, and delivers interleaved
// float32 PCM to a C callback. See crates/audio/src/core_audio_tap.rs for the
// Rust side.

#ifndef PRXVC_TAP_H
#define PRXVC_TAP_H

#include <stdint.h>
#include <sys/types.h> // pid_t

#ifdef __cplusplus
extern "C" {
#endif

// Opaque handle to a running tap. Created by prxvc_tap_start, freed by
// prxvc_tap_stop.
typedef struct PrxvcTap PrxvcTap;

// Audio delivery callback.
//
// Invoked from a Core Audio IO thread — the implementation MUST be fast and
// non-blocking (no locks held across it, no unbounded work).
//   ctx          : opaque pointer passed to prxvc_tap_start.
//   samples      : interleaved float32, length = frame_count * channels.
//   frame_count  : number of frames in this buffer.
//   channels     : channel count (e.g. 2 for a stereo mixdown).
//   sample_rate  : sample rate of the tap stream (e.g. 48000.0).
typedef void (*PrxvcTapAudioCb)(void *ctx,
                                const float *samples,
                                uint32_t frame_count,
                                uint32_t channels,
                                double sample_rate);

// Start capturing audio rendered by process `pid`.
//
// Returns a non-NULL handle on success, or NULL on failure (unsupported macOS
// version, missing NSAudioCaptureUsageDescription permission, invalid pid, or a
// Core Audio error). Errors are logged via NSLog.
PrxvcTap *prxvc_tap_start(pid_t pid, PrxvcTapAudioCb cb, void *ctx);

// Stop the tap and release all Core Audio resources. Safe to call with NULL.
// After this returns, `cb` will not be invoked again.
void prxvc_tap_stop(PrxvcTap *tap);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_TAP_H
