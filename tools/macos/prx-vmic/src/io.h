// Real-time playback into and capture from a Core Audio device.
#ifndef PRX_IO_H
#define PRX_IO_H
#include "hal.h"
#include <stddef.h>

// Producer chunk: the source is pushed into the device ring in chunks of this many frames.
#define PLAY_CHUNK 480

typedef struct {
    const float *src;  // mono source at the device rate
    double rate;       // device sample rate
    size_t n;
    long marker;       // source index to time (multiple of PLAY_CHUNK), or -1
    // results
    uint64_t underruns, overruns, callbacks, frames_played;
    uint64_t marker_write_host, marker_out_host;
} PlayJob;

// Plays job->src to `dev` in real time (blocks until drained). Returns 0 on success.
int player_start(const HalDevice *dev, const HalFormat *fmt, PlayJob *job, void **handle);
int player_finish(void *handle);

typedef struct {
    float *buf;        // mono capture of channel 0
    size_t cap;
    size_t n;
    double rate;
    struct { size_t idx; uint64_t host; } *cbs;
    size_t ncb, cbcap;
    uint64_t callbacks;
} Capture;

int capture_start(const HalDevice *dev, const HalFormat *fmt, double seconds, Capture *cap, void **handle);
void capture_stop(void *handle);
// Host time (mach ticks) at which capture sample `idx` was captured.
uint64_t capture_host_time(const Capture *c, size_t idx);

#endif
