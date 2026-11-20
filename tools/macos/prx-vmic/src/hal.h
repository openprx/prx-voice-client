// Core Audio helpers: device lookup by UID or name, format query.
#ifndef PRX_HAL_H
#define PRX_HAL_H
#include <CoreAudio/CoreAudio.h>
#include <stdint.h>

#define PRX_MIC_UID "org.openprx.prx-voice-client.microphone"
#define PRX_SINK_UID "org.openprx.prx-voice-client.microphone.sink"

typedef struct {
    AudioObjectID id;
    char name[128];
    char uid[160];
} HalDevice;

typedef struct {
    double rate;       // nominal sample rate
    UInt32 channels;   // channels in the first stream of the scope
    UInt32 bits;
    UInt32 latency;    // device latency, frames
    UInt32 safety;     // safety offset, frames
    UInt32 buffer;     // IO buffer size, frames
} HalFormat;

// Finds a device by exact UID (hidden devices included), then by case-insensitive
// name substring. Returns 0 on success, -1 if no such device.
int hal_find(const char *key, HalDevice *out);
// Reads the stream format of `dev` in `scope`. Returns 0 only for 32-bit float linear PCM.
int hal_format(const HalDevice *dev, AudioObjectPropertyScope scope, HalFormat *out);
// Prints every audio device with its UID and channel counts.
int hal_list(void);
uint64_t hal_host_ns(uint64_t host_ticks);
uint64_t hal_now_host(void);
uint64_t hal_ns_to_host(uint64_t ns);

#endif
