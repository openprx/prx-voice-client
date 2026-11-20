// Core Audio process-object enumeration shim (macOS 14.4+), C ABI.
//
// Reads, for the application processes known to the audio system, whether they
// run audio input or output and which devices they use. Read-only: no tap,
// device or stream is created and no audio is recorded. A port of the approach
// of bench/callapp/procaudio.c. Rust side: crates/audio/src/call_activity/native.rs.

#ifndef PRXVC_PROCS_H
#define PRXVC_PROCS_H

#include <stdint.h>
#include <sys/types.h> // pid_t

#ifdef __cplusplus
extern "C" {
#endif

#define PRXVC_PROC_MAX_DEVICES 16
#define PRXVC_PROC_STR 128

// One audio device a process uses.
typedef struct {
    uint32_t object_id;
    char uid[PRXVC_PROC_STR];  // NUL-terminated, "" if unknown
    char name[PRXVC_PROC_STR]; // NUL-terminated, "" if unknown
    uint32_t input_channels;   // channels of the device's input streams
    uint32_t output_channels;  // channels of the device's output streams
} PrxvcProcDevice;

// One Core Audio process object.
typedef struct {
    uint32_t object_id;
    int32_t pid;
    char bundle_id[256]; // NUL-terminated, "" if none
    int32_t is_running;        // kAudioProcessPropertyIsRunning
    int32_t is_running_input;  // kAudioProcessPropertyIsRunningInput
    int32_t is_running_output; // kAudioProcessPropertyIsRunningOutput
    uint32_t input_count;      // valid entries of `inputs` (capped at the array size)
    uint32_t output_count;
    PrxvcProcDevice inputs[PRXVC_PROC_MAX_DEVICES];
    PrxvcProcDevice outputs[PRXVC_PROC_MAX_DEVICES];
} PrxvcProcRow;

// Called once per matching process object, on the calling thread; `row` is
// valid only for the duration of the call.
typedef void (*PrxvcProcRowCb)(void *ctx, const PrxvcProcRow *row);

// Visit the process objects that match the filter: `bundle_id` (non-NULL) matches
// by bundle ID, otherwise `pid` (> 0) matches by pid, otherwise every process
// object. `with_devices` zero skips the device queries (the counts are 0).
// Returns 0 or the Core Audio OSStatus of the failed list query (for example
// when the OS has no process objects, before macOS 14.4).
int32_t prxvc_procs_enumerate(const char *bundle_id, pid_t pid, int32_t with_devices,
                              PrxvcProcRowCb cb, void *ctx);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_PROCS_H
