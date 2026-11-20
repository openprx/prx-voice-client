// Core Audio input-device enumeration shim — C ABI surface consumed by Rust FFI.
//
// cpal exposes neither the device UID nor the transport type, which the
// input-device policy needs. See crates/audio/src/core_audio_devices.rs for the
// Rust side.

#ifndef PRXVC_DEVICES_H
#define PRXVC_DEVICES_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Transport classification, mirrored by `InputTransport` in Rust.
enum {
    PRXVC_TRANSPORT_UNKNOWN = 0,
    PRXVC_TRANSPORT_BUILT_IN = 1,
    PRXVC_TRANSPORT_USB = 2,
    PRXVC_TRANSPORT_BLUETOOTH = 3,
    PRXVC_TRANSPORT_VIRTUAL = 4,
    PRXVC_TRANSPORT_AGGREGATE = 5,
    PRXVC_TRANSPORT_OTHER = 6,
};

#define PRXVC_DEVICE_STRING_LEN 256

// One input-capable device. Strings are NUL-terminated UTF-8, truncated to fit.
typedef struct {
    uint32_t device_id;
    char uid[PRXVC_DEVICE_STRING_LEN];
    char name[PRXVC_DEVICE_STRING_LEN];
    uint32_t transport;
    int32_t is_default; // non-zero for the system default input
} PrxvcInputDevice;

// Fill `out` with up to `capacity` input-capable devices (devices with at least
// one input channel). Returns the number written, or a negative Core Audio
// OSStatus on failure. Not for use on an audio IO thread.
int32_t prxvc_list_input_devices(PrxvcInputDevice *out, int32_t capacity);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_DEVICES_H
