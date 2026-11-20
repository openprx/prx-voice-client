#include "io.h"

#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define RING_CAP 16384u  // power of two

typedef struct {
    AudioObjectID dev;
    AudioDeviceIOProcID proc;
    PlayJob *job;
    float ring[RING_CAP];
    _Atomic uint64_t w, r;
    _Atomic int done;
    pthread_t thread;
} Player;

static OSStatus play_proc(AudioObjectID dev, const AudioTimeStamp *now, const AudioBufferList *in,
                          const AudioTimeStamp *intime, AudioBufferList *out, const AudioTimeStamp *outtime,
                          void *cd) {
    (void)dev, (void)now, (void)in, (void)intime;
    Player *p = cd;
    PlayJob *job = p->job;
    if (out == NULL || out->mNumberBuffers == 0) return noErr;
    UInt32 nch0 = out->mBuffers[0].mNumberChannels;
    UInt32 frames = out->mBuffers[0].mDataByteSize / (4 * nch0);
    uint64_t w = atomic_load_explicit(&p->w, memory_order_acquire);
    uint64_t r = atomic_load_explicit(&p->r, memory_order_relaxed);
    uint64_t avail = w - r;
    UInt32 take = avail < frames ? (UInt32)avail : frames;
    if (take < frames && !atomic_load(&p->done)) job->underruns++;
    job->callbacks++;
    if (job->marker >= 0 && (uint64_t)job->marker >= job->frames_played &&
        (uint64_t)job->marker < job->frames_played + take && job->marker_out_host == 0) {
        // Time of the marker frame: output time of the buffer plus its offset within it.
        uint64_t off_ns = (uint64_t)((double)(job->marker - (long)job->frames_played) * 1e9 / job->rate);
        job->marker_out_host = outtime->mHostTime + hal_ns_to_host(off_ns);
    }
    for (UInt32 b = 0; b < out->mNumberBuffers; b++) {
        float *dst = out->mBuffers[b].mData;
        UInt32 nch = out->mBuffers[b].mNumberChannels;
        for (UInt32 i = 0; i < frames; i++) {
            float v = i < take ? p->ring[(r + i) & (RING_CAP - 1)] : 0.0f;
            for (UInt32 c = 0; c < nch; c++) dst[i * nch + c] = v;
        }
    }
    atomic_store_explicit(&p->r, r + take, memory_order_release);
    job->frames_played += take;
    return noErr;
}

static void sleep_until_ns(uint64_t target_ns) {
    uint64_t now = hal_host_ns(hal_now_host());
    if (target_ns > now) {
        struct timespec ts = {(time_t)((target_ns - now) / 1000000000ull), (long)((target_ns - now) % 1000000000ull)};
        nanosleep(&ts, NULL);
    }
}

static void *producer(void *arg) {
    Player *p = arg;
    PlayJob *job = p->job;
    // The first two chunks are pushed before the device starts (see player_start); pace the rest.
    uint64_t t0 = hal_host_ns(hal_now_host());
    size_t pos = 2 * PLAY_CHUNK;
    uint64_t k = 0;
    while (pos < job->n) {
        k++;
        // Push chunk k just before the device would have consumed it, with a small lead.
        sleep_until_ns(t0 + (uint64_t)((double)(k * PLAY_CHUNK) * 1e9 / job->rate));
        size_t len = job->n - pos < PLAY_CHUNK ? job->n - pos : PLAY_CHUNK;
        uint64_t w = atomic_load_explicit(&p->w, memory_order_relaxed);
        uint64_t r = atomic_load_explicit(&p->r, memory_order_acquire);
        if (w - r + len > RING_CAP) {
            job->overruns++;
        } else {
            if (job->marker >= 0 && (size_t)job->marker == pos) job->marker_write_host = hal_now_host();
            for (size_t i = 0; i < len; i++) p->ring[(w + i) & (RING_CAP - 1)] = job->src[pos + i];
            atomic_store_explicit(&p->w, w + len, memory_order_release);
        }
        pos += len;
    }
    atomic_store(&p->done, 1);
    return NULL;
}

int player_start(const HalDevice *dev, const HalFormat *fmt, PlayJob *job, void **handle) {
    (void)fmt;
    Player *p = calloc(1, sizeof *p);
    if (p == NULL) return -1;
    p->dev = dev->id;
    p->job = job;
    job->underruns = job->overruns = job->callbacks = job->frames_played = 0;
    job->marker_write_host = job->marker_out_host = 0;
    size_t pre = job->n < 2 * PLAY_CHUNK ? job->n : 2 * PLAY_CHUNK;
    for (size_t i = 0; i < pre; i++) p->ring[i] = job->src[i];
    atomic_store(&p->w, pre);
    if (pre == job->n) atomic_store(&p->done, 1);
    OSStatus st = AudioDeviceCreateIOProcID(dev->id, play_proc, p, &p->proc);
    if (st == noErr) st = AudioDeviceStart(dev->id, p->proc);
    if (st != noErr) {
        fprintf(stderr, "error: cannot start playback on '%s' (OSStatus %d)\n", dev->name, (int)st);
        free(p);
        return -1;
    }
    if (!atomic_load(&p->done)) pthread_create(&p->thread, NULL, producer, p);
    *handle = p;
    return 0;
}

int player_finish(void *handle) {
    Player *p = handle;
    if (!atomic_load(&p->done) || p->thread) pthread_join(p->thread, NULL);
    // Let the device drain the ring (at most 1 s).
    for (int i = 0; i < 100 && p->job->frames_played < p->job->n; i++) usleep(10000);
    usleep(50000);
    AudioDeviceStop(p->dev, p->proc);
    AudioDeviceDestroyIOProcID(p->dev, p->proc);
    free(p);
    return 0;
}

// ---- capture ----

static OSStatus cap_proc(AudioObjectID dev, const AudioTimeStamp *now, const AudioBufferList *in,
                         const AudioTimeStamp *intime, AudioBufferList *out, const AudioTimeStamp *outtime,
                         void *cd) {
    (void)dev, (void)now, (void)out, (void)outtime;
    Capture *c = cd;
    if (in == NULL || in->mNumberBuffers == 0) return noErr;
    UInt32 nch = in->mBuffers[0].mNumberChannels;
    UInt32 frames = in->mBuffers[0].mDataByteSize / (4 * nch);
    const float *src = in->mBuffers[0].mData;
    c->callbacks++;
    if (c->ncb < c->cbcap) {
        c->cbs[c->ncb].idx = c->n;
        c->cbs[c->ncb].host = intime->mHostTime;
        c->ncb++;
    }
    for (UInt32 i = 0; i < frames && c->n < c->cap; i++) c->buf[c->n++] = src[i * nch];
    return noErr;
}

typedef struct {
    AudioObjectID dev;
    AudioDeviceIOProcID proc;
} CapHandle;

int capture_start(const HalDevice *dev, const HalFormat *fmt, double seconds, Capture *cap, void **handle) {
    memset(cap, 0, sizeof *cap);
    cap->rate = fmt->rate;
    cap->cap = (size_t)(seconds * fmt->rate) + (size_t)fmt->rate;  // one second of slack
    cap->buf = calloc(cap->cap, sizeof(float));
    cap->cbcap = (size_t)(seconds * 2000) + 4000;
    cap->cbs = calloc(cap->cbcap, sizeof cap->cbs[0]);
    CapHandle *h = calloc(1, sizeof *h);
    if (!cap->buf || !cap->cbs || !h) return -1;
    h->dev = dev->id;
    OSStatus st = AudioDeviceCreateIOProcID(dev->id, cap_proc, cap, &h->proc);
    if (st == noErr) st = AudioDeviceStart(dev->id, h->proc);
    if (st != noErr) {
        fprintf(stderr, "error: cannot start capture on '%s' (OSStatus %d)\n", dev->name, (int)st);
        free(h);
        return -1;
    }
    *handle = h;
    return 0;
}

void capture_stop(void *handle) {
    CapHandle *h = handle;
    AudioDeviceStop(h->dev, h->proc);
    AudioDeviceDestroyIOProcID(h->dev, h->proc);
    free(h);
}

uint64_t capture_host_time(const Capture *c, size_t idx) {
    size_t k = 0;
    for (size_t i = 0; i < c->ncb; i++) {
        if (c->cbs[i].idx <= idx) k = i; else break;
    }
    uint64_t base = c->cbs[k].host;
    uint64_t off_ns = (uint64_t)((double)(idx - c->cbs[k].idx) * 1e9 / c->rate);
    return base + hal_ns_to_host(off_ns);
}
