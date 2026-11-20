// Core Audio process-object enumeration (macOS 14.4+). See prxvc_procs.h.

#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#include <stdlib.h>
#include <string.h>

#include "prxvc_procs.h"

static AudioObjectPropertyAddress prxvc_addr(AudioObjectPropertySelector sel,
                                             AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress a = {sel, scope, kAudioObjectPropertyElementMain};
    return a;
}

static int32_t prxvc_get_u32(AudioObjectID obj, AudioObjectPropertySelector sel, UInt32 *out) {
    AudioObjectPropertyAddress a = prxvc_addr(sel, kAudioObjectPropertyScopeGlobal);
    UInt32 size = sizeof(*out);
    return AudioObjectGetPropertyData(obj, &a, 0, NULL, &size, out) == noErr;
}

static void prxvc_get_string(AudioObjectID obj, AudioObjectPropertySelector sel, char *buf,
                             size_t len) {
    buf[0] = 0;
    CFStringRef s = NULL;
    AudioObjectPropertyAddress a = prxvc_addr(sel, kAudioObjectPropertyScopeGlobal);
    UInt32 size = sizeof(s);
    if (AudioObjectGetPropertyData(obj, &a, 0, NULL, &size, &s) == noErr && s != NULL) {
        if (!CFStringGetCString(s, buf, (CFIndex)len, kCFStringEncodingUTF8)) {
            buf[0] = 0;
        }
        CFRelease(s);
    }
}

// Channels of the device's streams in one scope (0 when it has none).
static uint32_t prxvc_channels(AudioObjectID device, AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress a = prxvc_addr(kAudioDevicePropertyStreamConfiguration, scope);
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(device, &a, 0, NULL, &size) != noErr || size == 0) {
        return 0;
    }
    AudioBufferList *list = (AudioBufferList *)malloc(size);
    if (list == NULL) {
        return 0;
    }
    uint32_t channels = 0;
    if (AudioObjectGetPropertyData(device, &a, 0, NULL, &size, list) == noErr) {
        for (UInt32 i = 0; i < list->mNumberBuffers; i++) {
            channels += list->mBuffers[i].mNumberChannels;
        }
    }
    free(list);
    return channels;
}

// Fill `out` with the devices the process uses in `scope`; returns the count.
static uint32_t prxvc_fill_devices(AudioObjectID proc, AudioObjectPropertyScope scope,
                                   PrxvcProcDevice *out) {
    AudioObjectPropertyAddress a = prxvc_addr(kAudioProcessPropertyDevices, scope);
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(proc, &a, 0, NULL, &size) != noErr || size == 0) {
        return 0;
    }
    AudioObjectID *ids = (AudioObjectID *)malloc(size);
    if (ids == NULL) {
        return 0;
    }
    uint32_t n = 0;
    if (AudioObjectGetPropertyData(proc, &a, 0, NULL, &size, ids) == noErr) {
        UInt32 count = size / (UInt32)sizeof(AudioObjectID);
        for (UInt32 i = 0; i < count && n < PRXVC_PROC_MAX_DEVICES; i++) {
            PrxvcProcDevice *d = &out[n++];
            memset(d, 0, sizeof(*d));
            d->object_id = ids[i];
            prxvc_get_string(ids[i], kAudioDevicePropertyDeviceUID, d->uid, sizeof(d->uid));
            prxvc_get_string(ids[i], kAudioObjectPropertyName, d->name, sizeof(d->name));
            d->input_channels = prxvc_channels(ids[i], kAudioObjectPropertyScopeInput);
            d->output_channels = prxvc_channels(ids[i], kAudioObjectPropertyScopeOutput);
        }
    }
    free(ids);
    return n;
}

int32_t prxvc_procs_enumerate(const char *bundle_id, pid_t pid, int32_t with_devices,
                              PrxvcProcRowCb cb, void *ctx) {
    if (cb == NULL) {
        return -1;
    }
    AudioObjectPropertyAddress list_addr =
        prxvc_addr(kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal);
    UInt32 size = 0;
    OSStatus st = AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &list_addr, 0, NULL, &size);
    if (st != noErr) {
        return (int32_t)st;
    }
    if (size == 0) {
        return 0;
    }
    AudioObjectID *ids = (AudioObjectID *)malloc(size);
    PrxvcProcRow *row = (PrxvcProcRow *)malloc(sizeof(PrxvcProcRow));
    if (ids == NULL || row == NULL) {
        free(ids);
        free(row);
        return -1;
    }
    st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &list_addr, 0, NULL, &size, ids);
    if (st == noErr) {
        UInt32 count = size / (UInt32)sizeof(AudioObjectID);
        for (UInt32 i = 0; i < count; i++) {
            UInt32 value = 0;
            memset(row, 0, sizeof(*row));
            row->object_id = ids[i];
            if (prxvc_get_u32(ids[i], kAudioProcessPropertyPID, &value)) {
                row->pid = (int32_t)value;
            }
            prxvc_get_string(ids[i], kAudioProcessPropertyBundleID, row->bundle_id,
                             sizeof(row->bundle_id));
            if (bundle_id != NULL) {
                if (strcmp(row->bundle_id, bundle_id) != 0) {
                    continue;
                }
            } else if (pid > 0 && row->pid != pid) {
                continue;
            }
            row->is_running = prxvc_get_u32(ids[i], kAudioProcessPropertyIsRunning, &value) && value != 0;
            row->is_running_input =
                prxvc_get_u32(ids[i], kAudioProcessPropertyIsRunningInput, &value) && value != 0;
            row->is_running_output =
                prxvc_get_u32(ids[i], kAudioProcessPropertyIsRunningOutput, &value) && value != 0;
            if (with_devices) {
                row->input_count = prxvc_fill_devices(ids[i], kAudioObjectPropertyScopeInput, row->inputs);
                row->output_count =
                    prxvc_fill_devices(ids[i], kAudioObjectPropertyScopeOutput, row->outputs);
            }
            cb(ctx, row);
        }
    }
    free(ids);
    free(row);
    return (int32_t)st;
}
