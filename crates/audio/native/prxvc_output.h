// Core Audio output-stream shim for devices cpal cannot see (hidden devices, such
// as the virtual microphone's sink, are not in the device list and can only be
// reached by UID). C ABI consumed by crates/audio/src/core_audio_output.rs.

#ifndef PRXVC_OUTPUT_H
#define PRXVC_OUTPUT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Called on the device's IO thread with an interleaved float32 buffer of
// `frames` frames of `channels` channels to fill. Must not block or allocate.
typedef void (*PrxvcRenderFn)(void *user, float *out, uint32_t frames, uint32_t channels);

typedef struct PrxvcOutput PrxvcOutput;

// Format of the first output stream of the device with this UID (found by UID,
// hidden devices included). Returns 0 and fills `rate` and `channels`, or a
// negative value: -1 not found, -2 no output stream, -3 not 32-bit float
// interleaved linear PCM, other negative values are Core Audio statuses.
int32_t prxvc_output_format(const char *uid, double *rate, uint32_t *channels);

// Start rendering `render(user, ...)` into the device with this UID at the
// device's current format. Returns NULL and sets `*status` (as above) on failure.
PrxvcOutput *prxvc_output_open(const char *uid, PrxvcRenderFn render, void *user, int32_t *status);

// Stop the device and release the IO proc. After it returns `render` is not
// called again. Accepts NULL.
void prxvc_output_close(PrxvcOutput *output);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_OUTPUT_H
