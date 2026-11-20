// Core Audio default-device shim: read and set the system default input by UID, read the default
// output, and listen for default-device and device-list changes. C ABI consumed by
// crates/audio/src/core_audio_defaults.rs and crates/audio/src/routing/hal_listener.rs.

#ifndef PRXVC_DEFAULTS_H
#define PRXVC_DEFAULTS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PRXVC_DEFAULTS_STRING_LEN 256

// Kinds delivered to a change callback.
enum {
    PRXVC_CHANGE_DEFAULT_INPUT = 1,
    PRXVC_CHANGE_DEFAULT_OUTPUT = 2,
    PRXVC_CHANGE_DEVICE_LIST = 3,
};

// Write the UID of the system default input (is_input != 0) or output device into `uid_out`
// (NUL-terminated UTF-8, `capacity` bytes). Returns 0, -1 when there is no default device, or a
// negative-or-positive Core Audio OSStatus (non-zero) on failure.
int32_t prxvc_default_device_uid(int32_t is_input, char *uid_out, int32_t capacity);

// Make the device with this UID the system default input. Needs no administrator rights.
// Returns 0 on success, -1 when no device has the UID, -2 when the device has no input channel,
// or the Core Audio OSStatus (non-zero) of the failed call.
int32_t prxvc_set_default_input_uid(const char *uid);

// Called on a private serial dispatch queue (never the main thread, never a real-time thread).
// It must only record the notification and return quickly.
typedef void (*prxvc_change_cb)(int32_t kind, void *ctx);

// Opaque listener handle.
typedef struct PrxvcChangeListener PrxvcChangeListener;

// Start listening on the system object for default input, default output and device-list
// changes. Returns NULL and sets `*status` (when non-NULL) on failure.
PrxvcChangeListener *prxvc_change_listener_start(prxvc_change_cb cb, void *ctx, int32_t *status);

// Stop listening. When this returns no callback is running or will run, so `ctx` may be freed.
// Must not be called from inside the callback.
void prxvc_change_listener_stop(PrxvcChangeListener *listener);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_DEFAULTS_H
