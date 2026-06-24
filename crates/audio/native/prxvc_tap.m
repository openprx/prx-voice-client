// Core Audio process-tap shim (macOS 14.4+). See prxvc_tap.h.
//
// Sequence (mirrors the Core Audio process-tap API; cf. insidegui/AudioCap):
//   1. PID  -> AudioObjectID via kAudioHardwarePropertyTranslatePIDToProcessObject
//   2. CATapDescription (stereo mixdown of that process), read its UUID
//   3. AudioHardwareCreateProcessTap
//   4. read kAudioTapPropertyFormat (AudioStreamBasicDescription)
//   5. AudioHardwareCreateAggregateDevice (private, tap list = our tap UUID)
//   6. AudioDeviceCreateIOProcIDWithBlock on a serial dispatch queue
//   7. AudioDeviceStart -> callback fires with float32 PCM
//   stop: AudioDeviceStop / DestroyIOProcID / DestroyAggregateDevice / DestroyProcessTap

#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreAudio/CATapDescription.h>
#import <dispatch/dispatch.h>

#include "prxvc_tap.h"

struct PrxvcTap {
    AudioObjectID tap_id;
    AudioObjectID aggregate_id;
    AudioDeviceIOProcID io_proc_id;
    dispatch_queue_t queue;
    PrxvcTapAudioCb cb;
    void *ctx;
    double sample_rate;
    uint32_t channels;
};

// Translate a unix pid into a Core Audio process AudioObjectID.
static AudioObjectID prxvc_process_object_for_pid(pid_t pid) {
    AudioObjectID object = kAudioObjectUnknown;
    AudioObjectPropertyAddress addr = {
        .mSelector = kAudioHardwarePropertyTranslatePIDToProcessObject,
        .mScope = kAudioObjectPropertyScopeGlobal,
        .mElement = kAudioObjectPropertyElementMain,
    };
    UInt32 size = sizeof(AudioObjectID);
    pid_t query = pid;
    OSStatus st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr,
                                             sizeof(query), &query, &size, &object);
    if (st != noErr) {
        NSLog(@"[prxvc_tap] TranslatePIDToProcessObject failed: %d", (int)st);
        return kAudioObjectUnknown;
    }
    return object;
}

PrxvcTap *prxvc_tap_start(pid_t pid, PrxvcTapAudioCb cb, void *ctx) {
    if (cb == NULL) {
        return NULL;
    }
    if (@available(macOS 14.4, *)) {
        // 1. PID -> process object
        AudioObjectID proc = prxvc_process_object_for_pid(pid);
        if (proc == kAudioObjectUnknown) {
            NSLog(@"[prxvc_tap] no audio process object for pid %d", (int)pid);
            return NULL;
        }

        // 2. Tap description: stereo mixdown of the target process only.
        //    muteBehavior = unmuted so the user still hears the other party.
        CATapDescription *desc =
            [[CATapDescription alloc] initStereoMixdownOfProcesses:@[ @(proc) ]];
        if (desc == nil) {
            NSLog(@"[prxvc_tap] CATapDescription init failed");
            return NULL;
        }
        desc.muteBehavior = CATapUnmuted;
        NSString *tap_uuid = desc.UUID.UUIDString;

        // 3. Create the process tap.
        AudioObjectID tap_id = kAudioObjectUnknown;
        OSStatus st = AudioHardwareCreateProcessTap(desc, &tap_id);
        if (st != noErr || tap_id == kAudioObjectUnknown) {
            NSLog(@"[prxvc_tap] AudioHardwareCreateProcessTap failed: %d "
                  @"(check NSAudioCaptureUsageDescription permission)",
                  (int)st);
            return NULL;
        }

        // 4. Read the tap's stream format.
        AudioStreamBasicDescription asbd = {0};
        UInt32 asbd_size = sizeof(asbd);
        AudioObjectPropertyAddress fmt_addr = {
            .mSelector = kAudioTapPropertyFormat,
            .mScope = kAudioObjectPropertyScopeGlobal,
            .mElement = kAudioObjectPropertyElementMain,
        };
        st = AudioObjectGetPropertyData(tap_id, &fmt_addr, 0, NULL, &asbd_size, &asbd);
        if (st != noErr) {
            NSLog(@"[prxvc_tap] read kAudioTapPropertyFormat failed: %d", (int)st);
            AudioHardwareDestroyProcessTap(tap_id);
            return NULL;
        }

        // 5. Private aggregate device wrapping the tap.
        NSString *agg_uid = [[NSUUID UUID] UUIDString];
        NSDictionary *agg_desc = @{
            @(kAudioAggregateDeviceUIDKey) : agg_uid,
            @(kAudioAggregateDeviceIsPrivateKey) : @YES,
            @(kAudioAggregateDeviceIsStackedKey) : @NO,
            @(kAudioAggregateDeviceTapListKey) : @[ @{
                @(kAudioSubTapUIDKey) : tap_uuid,
                @(kAudioSubTapDriftCompensationKey) : @NO,
            } ],
        };
        AudioObjectID agg_id = kAudioObjectUnknown;
        st = AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)agg_desc, &agg_id);
        if (st != noErr || agg_id == kAudioObjectUnknown) {
            NSLog(@"[prxvc_tap] AudioHardwareCreateAggregateDevice failed: %d", (int)st);
            AudioHardwareDestroyProcessTap(tap_id);
            return NULL;
        }

        PrxvcTap *handle = (PrxvcTap *)calloc(1, sizeof(PrxvcTap));
        if (handle == NULL) {
            AudioHardwareDestroyAggregateDevice(agg_id);
            AudioHardwareDestroyProcessTap(tap_id);
            return NULL;
        }
        handle->tap_id = tap_id;
        handle->aggregate_id = agg_id;
        handle->cb = cb;
        handle->ctx = ctx;
        handle->sample_rate = asbd.mSampleRate;
        handle->channels = asbd.mChannelsPerFrame;
        handle->queue =
            dispatch_queue_create("io.prxvc.coreaudio.tap", DISPATCH_QUEUE_SERIAL);

        // 6. IO block: forward the first buffer's float32 samples to the callback.
        AudioDeviceIOBlock block = ^(const AudioTimeStamp *inNow,
                                      const AudioBufferList *inInputData,
                                      const AudioTimeStamp *inInputTime,
                                      AudioBufferList *outOutputData,
                                      const AudioTimeStamp *inOutputTime) {
            (void)inNow;
            (void)inInputTime;
            (void)outOutputData;
            (void)inOutputTime;
            if (inInputData == NULL || inInputData->mNumberBuffers == 0) {
                return;
            }
            const AudioBuffer *buf = &inInputData->mBuffers[0];
            uint32_t ch = buf->mNumberChannels ? buf->mNumberChannels : handle->channels;
            if (buf->mData == NULL || ch == 0) {
                return;
            }
            uint32_t frames = buf->mDataByteSize / (ch * (uint32_t)sizeof(float));
            handle->cb(handle->ctx, (const float *)buf->mData, frames, ch,
                       handle->sample_rate);
        };

        st = AudioDeviceCreateIOProcIDWithBlock(&handle->io_proc_id, agg_id,
                                                handle->queue, block);
        if (st != noErr || handle->io_proc_id == NULL) {
            NSLog(@"[prxvc_tap] AudioDeviceCreateIOProcIDWithBlock failed: %d", (int)st);
            prxvc_tap_stop(handle);
            return NULL;
        }

        // 7. Start.
        st = AudioDeviceStart(agg_id, handle->io_proc_id);
        if (st != noErr) {
            NSLog(@"[prxvc_tap] AudioDeviceStart failed: %d", (int)st);
            prxvc_tap_stop(handle);
            return NULL;
        }

        NSLog(@"[prxvc_tap] started: pid=%d tap=%u agg=%u %.0fHz %uch", (int)pid,
              (unsigned)tap_id, (unsigned)agg_id, handle->sample_rate,
              handle->channels);
        return handle;
    } else {
        NSLog(@"[prxvc_tap] requires macOS 14.4 or later");
        return NULL;
    }
}

void prxvc_tap_stop(PrxvcTap *tap) {
    if (tap == NULL) {
        return;
    }
    if (tap->aggregate_id != kAudioObjectUnknown && tap->io_proc_id != NULL) {
        AudioDeviceStop(tap->aggregate_id, tap->io_proc_id);
        AudioDeviceDestroyIOProcID(tap->aggregate_id, tap->io_proc_id);
    }
    if (tap->aggregate_id != kAudioObjectUnknown) {
        AudioHardwareDestroyAggregateDevice(tap->aggregate_id);
    }
    if (tap->tap_id != kAudioObjectUnknown) {
        AudioHardwareDestroyProcessTap(tap->tap_id);
    }
    tap->queue = nil; // ARC releases the dispatch queue
    free(tap);
}
