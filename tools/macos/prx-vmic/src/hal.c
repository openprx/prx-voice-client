#include "hal.h"

#include <CoreFoundation/CoreFoundation.h>
#include <mach/mach_time.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <ctype.h>

uint64_t hal_ns_to_host(uint64_t ns) {
    static mach_timebase_info_data_t tb;
    if (tb.denom == 0) mach_timebase_info(&tb);
    return ns * tb.denom / tb.numer;
}

uint64_t hal_now_host(void) { return mach_absolute_time(); }

uint64_t hal_host_ns(uint64_t ticks) {
    static mach_timebase_info_data_t tb;
    if (tb.denom == 0) mach_timebase_info(&tb);
    return ticks * tb.numer / tb.denom;
}

static void cf_to_buf(CFStringRef s, char *buf, size_t n) {
    buf[0] = 0;
    if (s != NULL) {
        if (!CFStringGetCString(s, buf, (CFIndex)n, kCFStringEncodingUTF8)) buf[0] = 0;
        CFRelease(s);
    }
}

static void get_string(AudioObjectID id, AudioObjectPropertySelector sel, char *buf, size_t n) {
    AudioObjectPropertyAddress a = {sel, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    CFStringRef s = NULL;
    UInt32 sz = sizeof s;
    if (AudioObjectGetPropertyData(id, &a, 0, NULL, &sz, &s) != noErr) s = NULL;
    cf_to_buf(s, buf, n);
}

static void fill_device(AudioObjectID id, HalDevice *d) {
    d->id = id;
    get_string(id, kAudioObjectPropertyName, d->name, sizeof d->name);
    get_string(id, kAudioDevicePropertyDeviceUID, d->uid, sizeof d->uid);
}

static UInt32 channel_count(AudioObjectID id, AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress a = {kAudioDevicePropertyStreamConfiguration, scope, kAudioObjectPropertyElementMain};
    UInt32 sz = 0;
    if (AudioObjectGetPropertyDataSize(id, &a, 0, NULL, &sz) != noErr || sz == 0) return 0;
    AudioBufferList *bl = malloc(sz);
    UInt32 total = 0;
    if (bl != NULL && AudioObjectGetPropertyData(id, &a, 0, NULL, &sz, bl) == noErr) {
        for (UInt32 i = 0; i < bl->mNumberBuffers; i++) total += bl->mBuffers[i].mNumberChannels;
    }
    free(bl);
    return total;
}

static AudioObjectID *all_devices(UInt32 *count) {
    AudioObjectPropertyAddress a = {kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal,
                                    kAudioObjectPropertyElementMain};
    UInt32 sz = 0;
    *count = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &a, 0, NULL, &sz) != noErr || sz == 0) return NULL;
    AudioObjectID *ids = malloc(sz);
    if (ids == NULL || AudioObjectGetPropertyData(kAudioObjectSystemObject, &a, 0, NULL, &sz, ids) != noErr) {
        free(ids);
        return NULL;
    }
    *count = sz / sizeof(AudioObjectID);
    return ids;
}

static int icontains(const char *hay, const char *needle) {
    size_t n = strlen(needle);
    if (n == 0) return 0;
    for (; *hay; hay++) {
        if (strncasecmp(hay, needle, n) == 0) return 1;
    }
    return 0;
}

int hal_find(const char *key, HalDevice *out) {
    AudioObjectPropertyAddress a = {kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyScopeGlobal,
                                    kAudioObjectPropertyElementMain};
    CFStringRef uid = CFStringCreateWithCString(NULL, key, kCFStringEncodingUTF8);
    AudioObjectID dev = kAudioObjectUnknown;
    UInt32 sz = sizeof dev;
    OSStatus st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &a, sizeof uid, &uid, &sz, &dev);
    CFRelease(uid);
    if (st == noErr && dev != kAudioObjectUnknown) {
        fill_device(dev, out);
        return 0;
    }
    UInt32 n;
    AudioObjectID *ids = all_devices(&n);
    for (UInt32 i = 0; i < n; i++) {
        HalDevice d;
        fill_device(ids[i], &d);
        if (icontains(d.name, key)) {
            *out = d;
            free(ids);
            return 0;
        }
    }
    free(ids);
    return -1;
}

int hal_format(const HalDevice *dev, AudioObjectPropertyScope scope, HalFormat *out) {
    memset(out, 0, sizeof *out);
    AudioObjectPropertyAddress sa = {kAudioDevicePropertyStreams, scope, kAudioObjectPropertyElementMain};
    UInt32 sz = 0;
    if (AudioObjectGetPropertyDataSize(dev->id, &sa, 0, NULL, &sz) != noErr || sz < sizeof(AudioObjectID)) {
        fprintf(stderr, "error: device '%s' has no stream in this direction\n", dev->name);
        return -1;
    }
    AudioObjectID *streams = malloc(sz);
    if (streams == NULL || AudioObjectGetPropertyData(dev->id, &sa, 0, NULL, &sz, streams) != noErr) {
        free(streams);
        return -1;
    }
    AudioStreamBasicDescription f;
    AudioObjectPropertyAddress fa = {kAudioStreamPropertyVirtualFormat, kAudioObjectPropertyScopeGlobal,
                                     kAudioObjectPropertyElementMain};
    UInt32 fsz = sizeof f;
    OSStatus st = AudioObjectGetPropertyData(streams[0], &fa, 0, NULL, &fsz, &f);
    free(streams);
    if (st != noErr) {
        fprintf(stderr, "error: cannot read stream format (OSStatus %d)\n", (int)st);
        return -1;
    }
    out->rate = f.mSampleRate;
    out->channels = channel_count(dev->id, scope);
    if (out->channels == 0) out->channels = f.mChannelsPerFrame;
    out->bits = f.mBitsPerChannel;
    UInt32 v;
    AudioObjectPropertyAddress la = {kAudioDevicePropertyLatency, scope, kAudioObjectPropertyElementMain};
    sz = sizeof v;
    if (AudioObjectGetPropertyData(dev->id, &la, 0, NULL, &sz, &v) == noErr) out->latency = v;
    la.mSelector = kAudioDevicePropertySafetyOffset;
    sz = sizeof v;
    if (AudioObjectGetPropertyData(dev->id, &la, 0, NULL, &sz, &v) == noErr) out->safety = v;
    la.mSelector = kAudioDevicePropertyBufferFrameSize;
    la.mScope = kAudioObjectPropertyScopeGlobal;
    sz = sizeof v;
    if (AudioObjectGetPropertyData(dev->id, &la, 0, NULL, &sz, &v) == noErr) out->buffer = v;
    int isfloat = (f.mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    if (f.mFormatID != kAudioFormatLinearPCM || !isfloat || f.mBitsPerChannel != 32) {
        fprintf(stderr, "error: unsupported stream format (id=0x%x flags=0x%x bits=%u); need 32-bit float PCM\n",
                (unsigned)f.mFormatID, (unsigned)f.mFormatFlags, (unsigned)f.mBitsPerChannel);
        return -1;
    }
    return 0;
}

int hal_list(void) {
    UInt32 n;
    AudioObjectID *ids = all_devices(&n);
    printf("%-6s %-4s %-4s %-34s %s\n", "id", "in", "out", "name", "uid");
    for (UInt32 i = 0; i < n; i++) {
        HalDevice d;
        fill_device(ids[i], &d);
        printf("%-6u %-4u %-4u %-34s %s\n", (unsigned)d.id, (unsigned)channel_count(d.id, kAudioObjectPropertyScopeInput),
               (unsigned)channel_count(d.id, kAudioObjectPropertyScopeOutput), d.name, d.uid);
    }
    free(ids);
    HalDevice sink;
    if (hal_find(PRX_SINK_UID, &sink) == 0) printf("(hidden sink present: id %u, %s)\n", (unsigned)sink.id, sink.uid);
    return 0;
}
