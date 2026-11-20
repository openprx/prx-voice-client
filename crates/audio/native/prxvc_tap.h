// Core Audio process-tap shim — C ABI surface consumed by Rust FFI.
//
// Captures the audio *rendered by a specific application* on macOS 14.4+ via
// the Core Audio process-tap API and hands the raw buffers of every IO cycle to
// a C callback. All format interpretation happens on the Rust side. See
// crates/audio/src/core_audio_tap.rs for the Rust side.
//
// Lifecycle: prxvc_tap_create -> prxvc_tap_start -> prxvc_tap_stop ->
// prxvc_tap_destroy. create does not start IO, so the caller can validate the
// stream format before any audio is delivered.

#ifndef PRXVC_TAP_H
#define PRXVC_TAP_H

#include <stdint.h>
#include <sys/types.h> // pid_t

#ifdef __cplusplus
extern "C" {
#endif

// Opaque handle to a tap. Created by prxvc_tap_create, freed by
// prxvc_tap_destroy.
typedef struct PrxvcTap PrxvcTap;

// Mute behaviour; values equal CATapMuteBehavior.
enum {
    PRXVC_TAP_UNMUTED = 0,
    PRXVC_TAP_MUTED = 1,
    PRXVC_TAP_MUTED_WHEN_TAPPED = 2,
};

// Result codes of prxvc_tap_create.
enum {
    PRXVC_TAP_OK = 0,
    PRXVC_TAP_ERR_UNSUPPORTED_OS = 1,
    PRXVC_TAP_ERR_PROCESS_NOT_FOUND = 2,
    PRXVC_TAP_ERR_CORE_AUDIO = 3, // see PrxvcTapResult.os_status / .step
    PRXVC_TAP_ERR_INTERNAL = 4,
};

// Result of prxvc_tap_permission_status.
enum {
    PRXVC_PERMISSION_GRANTED = 0,
    PRXVC_PERMISSION_DENIED = 1,
    PRXVC_PERMISSION_UNDETERMINED = 2,
    PRXVC_PERMISSION_UNKNOWN = -1, // the system gives no answer
};

// What to tap and how.
typedef struct {
    pid_t pid;               // target when bundle_id is NULL
    const char *bundle_id;   // target application by bundle ID, or NULL
    int32_t prefer_bundle_form; // non-zero: use the bundle-ID form with process
                                // restore when the OS provides it (macOS 26+)
    int32_t mute;            // PRXVC_TAP_*
} PrxvcTapConfig;

// Stream format of the tap (the fields of AudioStreamBasicDescription that
// matter).
typedef struct {
    double sample_rate;
    uint32_t format_id;
    uint32_t format_flags;
    uint32_t channels;
    uint32_t bits_per_channel;
    uint32_t bytes_per_frame;
} PrxvcTapFormat;

// Outcome of prxvc_tap_create.
typedef struct {
    int32_t status;          // PRXVC_TAP_*
    int32_t os_status;       // Core Audio OSStatus when status is CORE_AUDIO
    char step[48];           // failing step, NUL-terminated
    int32_t used_bundle_form; // non-zero when the bundle-ID form is in use
} PrxvcTapResult;

// One buffer of an IO cycle. `data` holds `byte_size` bytes of float32 samples
// with `channels` interleaved channels.
typedef struct {
    uint32_t channels;
    uint32_t byte_size;
    const void *data;
} PrxvcTapBuffer;

// Largest number of buffers passed to the callback; extra ones are dropped.
#define PRXVC_TAP_MAX_BUFFERS 8

// Audio delivery callback.
//
// Invoked from the Core Audio IO (HAL) thread, a real-time thread: the
// implementation must not allocate, lock, log or block.
//   ctx          : opaque pointer passed to prxvc_tap_create.
//   buffer_count : number of valid entries in `buffers` (1..PRXVC_TAP_MAX_BUFFERS).
//   buffers      : valid only for the duration of the call.
//   host_time    : host time of the input data, 0 if unknown.
typedef void (*PrxvcTapAudioCb)(void *ctx, uint32_t buffer_count,
                                const PrxvcTapBuffer *buffers, uint64_t host_time);

// Create the process tap and its private aggregate device, without starting IO.
// On success returns a handle and fills *format; on failure returns NULL and
// describes the failure in *result. Never called from an IO thread.
PrxvcTap *prxvc_tap_create(const PrxvcTapConfig *config, PrxvcTapAudioCb cb, void *ctx,
                           PrxvcTapFormat *format, PrxvcTapResult *result);

// Start IO. Returns 0 or a Core Audio OSStatus.
int32_t prxvc_tap_start(PrxvcTap *tap);

// Stop IO. After this returns, `cb` is not invoked again. Safe with NULL.
void prxvc_tap_stop(PrxvcTap *tap);

// Stop (if running) and release all Core Audio resources. Safe with NULL.
void prxvc_tap_destroy(PrxvcTap *tap);

// Whether the target application has a Core Audio process object (is running
// and known to the audio system). With a bundle_id (non-NULL) matches by bundle
// ID and writes the pid to *pid_out when non-NULL; otherwise checks `pid`.
int32_t prxvc_tap_target_running(const char *bundle_id, pid_t pid, pid_t *pid_out);

// Whether the OS provides the bundle-ID tap form with process restore (macOS 26+).
int32_t prxvc_tap_bundle_form_available(void);

// The user's answer for "System Audio Recording" (PRXVC_PERMISSION_*), from the
// system's permission preflight. Does not prompt.
int32_t prxvc_tap_permission_status(void);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_TAP_H
