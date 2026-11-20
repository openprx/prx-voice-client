// Core Audio output-stream shim. See prxvc_output.h.

#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#include <stdlib.h>
#include <string.h>

#include "prxvc_output.h"

struct PrxvcOutput {
    AudioObjectID device;
    AudioDeviceIOProcID proc;
    PrxvcRenderFn render;
    void *user;
};

static AudioObjectID prxvc_device_for_uid(const char *uid) {
    CFStringRef key = CFStringCreateWithCString(NULL, uid, kCFStringEncodingUTF8);
    if (key == NULL) {
        return kAudioObjectUnknown;
    }
    AudioObjectPropertyAddress addr = {kAudioHardwarePropertyTranslateUIDToDevice,
                                       kAudioObjectPropertyScopeGlobal,
                                       kAudioObjectPropertyElementMain};
    AudioObjectID dev = kAudioObjectUnknown;
    UInt32 size = sizeof(dev);
    OSStatus st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, sizeof(key), &key,
                                             &size, &dev);
    CFRelease(key);
    return (st == noErr) ? dev : kAudioObjectUnknown;
}

int32_t prxvc_output_format(const char *uid, double *rate, uint32_t *channels) {
    AudioObjectID dev = prxvc_device_for_uid(uid);
    if (dev == kAudioObjectUnknown) {
        return -1;
    }
    AudioObjectPropertyAddress saddr = {kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput,
                                        kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(dev, &saddr, 0, NULL, &size) != noErr ||
        size < sizeof(AudioObjectID)) {
        return -2;
    }
    AudioObjectID *streams = (AudioObjectID *)malloc(size);
    if (streams == NULL) {
        return -1;
    }
    OSStatus st = AudioObjectGetPropertyData(dev, &saddr, 0, NULL, &size, streams);
    AudioObjectID stream = (st == noErr) ? streams[0] : kAudioObjectUnknown;
    free(streams);
    if (st != noErr) {
        return (int32_t)st;
    }
    AudioStreamBasicDescription f;
    AudioObjectPropertyAddress faddr = {kAudioStreamPropertyVirtualFormat,
                                        kAudioObjectPropertyScopeGlobal,
                                        kAudioObjectPropertyElementMain};
    UInt32 fsize = sizeof(f);
    st = AudioObjectGetPropertyData(stream, &faddr, 0, NULL, &fsize, &f);
    if (st != noErr) {
        return (int32_t)st;
    }
    BOOL is_float = (f.mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    BOOL non_interleaved = (f.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    if (f.mFormatID != kAudioFormatLinearPCM || !is_float || f.mBitsPerChannel != 32 ||
        (non_interleaved && f.mChannelsPerFrame > 1)) {
        return -3;
    }
    *rate = f.mSampleRate;
    *channels = f.mChannelsPerFrame;
    return 0;
}

static OSStatus prxvc_ioproc(AudioObjectID device, const AudioTimeStamp *now,
                             const AudioBufferList *input, const AudioTimeStamp *input_time,
                             AudioBufferList *output, const AudioTimeStamp *output_time,
                             void *client) {
    (void)device; (void)now; (void)input; (void)input_time; (void)output_time;
    PrxvcOutput *o = (PrxvcOutput *)client;
    for (UInt32 i = 0; i < output->mNumberBuffers; i++) {
        AudioBuffer *b = &output->mBuffers[i];
        if (i == 0 && b->mNumberChannels > 0 && b->mData != NULL) {
            uint32_t frames = b->mDataByteSize / (uint32_t)(sizeof(float) * b->mNumberChannels);
            o->render(o->user, (float *)b->mData, frames, b->mNumberChannels);
        } else if (b->mData != NULL) {
            memset(b->mData, 0, b->mDataByteSize);
        }
    }
    return noErr;
}

PrxvcOutput *prxvc_output_open(const char *uid, PrxvcRenderFn render, void *user, int32_t *status) {
    double rate = 0;
    uint32_t channels = 0;
    int32_t fmt = prxvc_output_format(uid, &rate, &channels);
    if (fmt != 0) {
        *status = fmt;
        return NULL;
    }
    PrxvcOutput *o = (PrxvcOutput *)calloc(1, sizeof(*o));
    if (o == NULL) {
        *status = -1;
        return NULL;
    }
    o->device = prxvc_device_for_uid(uid);
    o->render = render;
    o->user = user;
    OSStatus st = AudioDeviceCreateIOProcID(o->device, prxvc_ioproc, o, &o->proc);
    if (st != noErr) {
        free(o);
        *status = (int32_t)st;
        return NULL;
    }
    st = AudioDeviceStart(o->device, o->proc);
    if (st != noErr) {
        AudioDeviceDestroyIOProcID(o->device, o->proc);
        free(o);
        *status = (int32_t)st;
        return NULL;
    }
    *status = 0;
    return o;
}

void prxvc_output_close(PrxvcOutput *o) {
    if (o == NULL) {
        return;
    }
    AudioDeviceStop(o->device, o->proc);
    AudioDeviceDestroyIOProcID(o->device, o->proc);
    free(o);
}
