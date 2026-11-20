// Core Audio process-tap shim (macOS 14.4+). See prxvc_tap.h.
//
// Sequence (mirrors the Core Audio process-tap API; cf. insidegui/AudioCap):
//   1. target -> CATapDescription (include list of one application, stereo
//      mixdown; PID form, or bundle-ID form with process restore on macOS 26+)
//   2. mute behaviour from the caller, tap marked private
//   3. AudioHardwareCreateProcessTap, read kAudioTapPropertyFormat
//   4. AudioHardwareCreateAggregateDevice (private, tap list = our tap UUID).
//      No TapAutoStart key: measured, with it an idle target never starts IO.
//   5. AudioDeviceCreateIOProcID with a plain C proc (runs on the HAL IO thread)
//   start: AudioDeviceStart; stop: AudioDeviceStop / DestroyIOProcID;
//   destroy: DestroyAggregateDevice / DestroyProcessTap

#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreAudio/CATapDescription.h>
#import <CoreAudio/HostTime.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

#include "prxvc_tap.h"

struct PrxvcTap {
    AudioObjectID tap_id;
    AudioObjectID aggregate_id;
    AudioDeviceIOProcID io_proc_id;
    PrxvcTapAudioCb cb;
    void *ctx;
    int running;
};

static void prxvc_fail(PrxvcTapResult *r, int32_t status, OSStatus os, const char *step) {
    if (r == NULL) {
        return;
    }
    r->status = status;
    r->os_status = (int32_t)os;
    strlcpy(r->step, step, sizeof(r->step));
}

// The pid's Core Audio process object, or kAudioObjectUnknown.
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
    return st == noErr ? object : kAudioObjectUnknown;
}

// The Core Audio process object whose bundle ID equals `bundle_id`, or
// kAudioObjectUnknown. Writes the process pid to *pid_out when non-NULL.
static AudioObjectID prxvc_process_object_for_bundle(const char *bundle_id, pid_t *pid_out) {
    AudioObjectPropertyAddress list_addr = {
        .mSelector = kAudioHardwarePropertyProcessObjectList,
        .mScope = kAudioObjectPropertyScopeGlobal,
        .mElement = kAudioObjectPropertyElementMain,
    };
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &list_addr, 0, NULL, &size) !=
            noErr ||
        size == 0) {
        return kAudioObjectUnknown;
    }
    AudioObjectID *ids = (AudioObjectID *)malloc(size);
    if (ids == NULL) {
        return kAudioObjectUnknown;
    }
    AudioObjectID found = kAudioObjectUnknown;
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &list_addr, 0, NULL, &size, ids) ==
        noErr) {
        UInt32 count = size / (UInt32)sizeof(AudioObjectID);
        for (UInt32 i = 0; i < count && found == kAudioObjectUnknown; i++) {
            AudioObjectPropertyAddress b_addr = {
                .mSelector = kAudioProcessPropertyBundleID,
                .mScope = kAudioObjectPropertyScopeGlobal,
                .mElement = kAudioObjectPropertyElementMain,
            };
            CFStringRef bid = NULL;
            UInt32 bsize = sizeof(bid);
            if (AudioObjectGetPropertyData(ids[i], &b_addr, 0, NULL, &bsize, &bid) != noErr ||
                bid == NULL) {
                continue;
            }
            char buf[256];
            BOOL ok = CFStringGetCString(bid, buf, sizeof(buf), kCFStringEncodingUTF8);
            CFRelease(bid);
            if (ok && strcmp(buf, bundle_id) == 0) {
                found = ids[i];
            }
        }
        if (found != kAudioObjectUnknown && pid_out != NULL) {
            AudioObjectPropertyAddress p_addr = {
                .mSelector = kAudioProcessPropertyPID,
                .mScope = kAudioObjectPropertyScopeGlobal,
                .mElement = kAudioObjectPropertyElementMain,
            };
            pid_t p = 0;
            UInt32 psize = sizeof(p);
            if (AudioObjectGetPropertyData(found, &p_addr, 0, NULL, &psize, &p) == noErr) {
                *pid_out = p;
            }
        }
    }
    free(ids);
    return found;
}

int32_t prxvc_tap_target_running(const char *bundle_id, pid_t pid, pid_t *pid_out) {
    if (bundle_id != NULL) {
        return prxvc_process_object_for_bundle(bundle_id, pid_out) != kAudioObjectUnknown;
    }
    if (pid_out != NULL) {
        *pid_out = pid;
    }
    return prxvc_process_object_for_pid(pid) != kAudioObjectUnknown;
}

int32_t prxvc_tap_bundle_form_available(void) {
#if defined(__MAC_26_0) && __MAC_OS_X_VERSION_MAX_ALLOWED >= 260000
    if (@available(macOS 26.0, *)) {
        return 1;
    }
#endif
    return 0;
}

// Handle of the private TCC framework, or NULL when it cannot be loaded.
static void *prxvc_tcc_handle(void) {
    static void *handle;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        handle = dlopen("/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW);
    });
    return handle;
}

// TCC's preflight for System Audio Recording: 0 granted, 1 denied. The symbol
// is a private API of TCC.framework, looked up at run time; when it is absent
// the answer is "unknown".
int32_t prxvc_tap_permission_status(void) {
    static int (*preflight)(CFStringRef, CFDictionaryRef);
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        void *handle = prxvc_tcc_handle();
        if (handle != NULL) {
            preflight = (int (*)(CFStringRef, CFDictionaryRef))dlsym(handle, "TCCAccessPreflight");
        }
    });
    if (preflight == NULL) {
        return PRXVC_PERMISSION_UNKNOWN;
    }
    switch (preflight(CFSTR("kTCCServiceAudioCapture"), NULL)) {
    case 0:
        return PRXVC_PERMISSION_GRANTED;
    case 1:
        return PRXVC_PERMISSION_DENIED;
    default:
        return PRXVC_PERMISSION_UNDETERMINED;
    }
}

int32_t prxvc_tap_bundle_declares_audio_capture(void) {
    id value = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSAudioCaptureUsageDescription"];
    return [value isKindOfClass:[NSString class]] && [(NSString *)value length] > 0;
}

// TCC's request for System Audio Recording (private TCCAccessRequest, the call
// the reference application and the callcap probe use). The completion block
// runs on a TCC queue; the semaphore and the answer live on the heap, so a
// block that fires after the timeout is harmless.
int32_t prxvc_tap_request_consent(int32_t timeout_ms) {
    void *handle = prxvc_tcc_handle();
    if (handle == NULL) {
        return PRXVC_CONSENT_UNAVAILABLE;
    }
    void (*request)(CFStringRef, CFDictionaryRef, void (^)(Boolean)) =
        (void (*)(CFStringRef, CFDictionaryRef, void (^)(Boolean)))dlsym(handle, "TCCAccessRequest");
    if (request == NULL) {
        return PRXVC_CONSENT_UNAVAILABLE;
    }
    __block int32_t answer = PRXVC_CONSENT_NO_ANSWER;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    request(CFSTR("kTCCServiceAudioCapture"), NULL, ^(Boolean granted) {
        answer = granted ? PRXVC_CONSENT_GRANTED : PRXVC_CONSENT_REFUSED;
        dispatch_semaphore_signal(done);
    });
    int64_t wait_ns = (int64_t)(timeout_ms < 0 ? 0 : timeout_ms) * (int64_t)NSEC_PER_MSEC;
    if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, wait_ns)) != 0) {
        return PRXVC_CONSENT_NO_ANSWER;
    }
    return answer;
}

uint64_t prxvc_tap_host_time_now_ns(void) {
    return AudioConvertHostTimeToNanos(AudioGetCurrentHostTime());
}

// IO proc on the HAL IO thread: no allocation, no locks, no logging, no
// Objective-C messaging; the buffer descriptions are copied to the stack.
static OSStatus prxvc_io_proc(AudioObjectID device, const AudioTimeStamp *now,
                              const AudioBufferList *input, const AudioTimeStamp *input_time,
                              AudioBufferList *output, const AudioTimeStamp *output_time,
                              void *client) {
    (void)device;
    (void)now;
    (void)output;
    (void)output_time;
    PrxvcTap *tap = (PrxvcTap *)client;
    if (tap == NULL || input == NULL || input->mNumberBuffers == 0) {
        return noErr;
    }
    PrxvcTapBuffer bufs[PRXVC_TAP_MAX_BUFFERS];
    uint32_t n = input->mNumberBuffers;
    if (n > PRXVC_TAP_MAX_BUFFERS) {
        n = PRXVC_TAP_MAX_BUFFERS;
    }
    for (uint32_t i = 0; i < n; i++) {
        bufs[i].channels = input->mBuffers[i].mNumberChannels;
        bufs[i].byte_size = input->mBuffers[i].mDataByteSize;
        bufs[i].data = input->mBuffers[i].mData;
    }
    uint64_t host_ns = 0;
    if (input_time != NULL && (input_time->mFlags & kAudioTimeStampHostTimeValid) != 0) {
        host_ns = AudioConvertHostTimeToNanos(input_time->mHostTime);
    }
    tap->cb(tap->ctx, n, bufs, host_ns);
    return noErr;
}

// Build the tap description for the configured target, or nil with *result set.
static CATapDescription *prxvc_make_description(const PrxvcTapConfig *config, int *used_bundle_form,
                                                PrxvcTapResult *result)
API_AVAILABLE(macos(14.4)) {
    CATapDescription *desc = nil;
    *used_bundle_form = 0;
    if (config->bundle_id != NULL) {
        NSString *bundle = [NSString stringWithUTF8String:config->bundle_id];
#if defined(__MAC_26_0) && __MAC_OS_X_VERSION_MAX_ALLOWED >= 260000
        if (config->prefer_bundle_form) {
            if (@available(macOS 26.0, *)) {
                desc = [[CATapDescription alloc] initStereoMixdownOfProcesses:@[]];
                desc.bundleIDs = @[ bundle ];
                desc.processRestoreEnabled = YES;
                *used_bundle_form = 1;
            }
        }
#endif
        if (desc == nil) {
            pid_t pid = 0;
            AudioObjectID proc = prxvc_process_object_for_bundle(config->bundle_id, &pid);
            if (proc == kAudioObjectUnknown) {
                prxvc_fail(result, PRXVC_TAP_ERR_PROCESS_NOT_FOUND, 0, "find process by bundle id");
                return nil;
            }
            desc = [[CATapDescription alloc] initStereoMixdownOfProcesses:@[ @(proc) ]];
        }
    } else {
        AudioObjectID proc = prxvc_process_object_for_pid(config->pid);
        if (proc == kAudioObjectUnknown) {
            prxvc_fail(result, PRXVC_TAP_ERR_PROCESS_NOT_FOUND, 0, "translate pid to process");
            return nil;
        }
        desc = [[CATapDescription alloc] initStereoMixdownOfProcesses:@[ @(proc) ]];
    }
    if (desc == nil) {
        prxvc_fail(result, PRXVC_TAP_ERR_INTERNAL, 0, "create tap description");
        return nil;
    }
    switch (config->mute) {
    case PRXVC_TAP_MUTED:
        desc.muteBehavior = CATapMuted;
        break;
    case PRXVC_TAP_MUTED_WHEN_TAPPED:
        desc.muteBehavior = CATapMutedWhenTapped;
        break;
    default:
        desc.muteBehavior = CATapUnmuted;
        break;
    }
    desc.privateTap = YES;
    return desc;
}

PrxvcTap *prxvc_tap_create(const PrxvcTapConfig *config, PrxvcTapAudioCb cb, void *ctx,
                           PrxvcTapFormat *format, PrxvcTapResult *result) {
    if (result != NULL) {
        memset(result, 0, sizeof(*result));
    }
    if (config == NULL || cb == NULL || format == NULL) {
        prxvc_fail(result, PRXVC_TAP_ERR_INTERNAL, 0, "bad arguments");
        return NULL;
    }
    if (@available(macOS 14.4, *)) {
        // Warm the host-clock conversion so the IO proc never initialises it.
        (void)AudioConvertHostTimeToNanos(AudioGetCurrentHostTime());
        int used_bundle_form = 0;
        CATapDescription *desc = prxvc_make_description(config, &used_bundle_form, result);
        if (desc == nil) {
            return NULL;
        }
        NSString *tap_uuid = desc.UUID.UUIDString;

        AudioObjectID tap_id = kAudioObjectUnknown;
        OSStatus st = AudioHardwareCreateProcessTap(desc, &tap_id);
        if (st != noErr || tap_id == kAudioObjectUnknown) {
            prxvc_fail(result, PRXVC_TAP_ERR_CORE_AUDIO, st, "create process tap");
            return NULL;
        }

        AudioStreamBasicDescription asbd = {0};
        UInt32 asbd_size = sizeof(asbd);
        AudioObjectPropertyAddress fmt_addr = {
            .mSelector = kAudioTapPropertyFormat,
            .mScope = kAudioObjectPropertyScopeGlobal,
            .mElement = kAudioObjectPropertyElementMain,
        };
        st = AudioObjectGetPropertyData(tap_id, &fmt_addr, 0, NULL, &asbd_size, &asbd);
        if (st != noErr) {
            prxvc_fail(result, PRXVC_TAP_ERR_CORE_AUDIO, st, "read tap format");
            AudioHardwareDestroyProcessTap(tap_id);
            return NULL;
        }

        // Private aggregate device wrapping the tap. Deliberately without
        // kAudioAggregateDeviceTapAutoStartKey (an idle target would then never
        // start the IO proc).
        NSDictionary *agg_desc = @{
            @(kAudioAggregateDeviceUIDKey) : [[NSUUID UUID] UUIDString],
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
            prxvc_fail(result, PRXVC_TAP_ERR_CORE_AUDIO, st, "create aggregate device");
            AudioHardwareDestroyProcessTap(tap_id);
            return NULL;
        }

        PrxvcTap *handle = (PrxvcTap *)calloc(1, sizeof(PrxvcTap));
        if (handle == NULL) {
            prxvc_fail(result, PRXVC_TAP_ERR_INTERNAL, 0, "allocate handle");
            AudioHardwareDestroyAggregateDevice(agg_id);
            AudioHardwareDestroyProcessTap(tap_id);
            return NULL;
        }
        handle->tap_id = tap_id;
        handle->aggregate_id = agg_id;
        handle->cb = cb;
        handle->ctx = ctx;

        st = AudioDeviceCreateIOProcID(agg_id, prxvc_io_proc, handle, &handle->io_proc_id);
        if (st != noErr || handle->io_proc_id == NULL) {
            prxvc_fail(result, PRXVC_TAP_ERR_CORE_AUDIO, st, "create IO proc");
            handle->io_proc_id = NULL;
            prxvc_tap_destroy(handle);
            return NULL;
        }

        format->sample_rate = asbd.mSampleRate;
        format->format_id = asbd.mFormatID;
        format->format_flags = asbd.mFormatFlags;
        format->channels = asbd.mChannelsPerFrame;
        format->bits_per_channel = asbd.mBitsPerChannel;
        format->bytes_per_frame = asbd.mBytesPerFrame;
        if (result != NULL) {
            result->used_bundle_form = used_bundle_form;
        }
        return handle;
    } else {
        prxvc_fail(result, PRXVC_TAP_ERR_UNSUPPORTED_OS, 0, "os version");
        return NULL;
    }
}

int32_t prxvc_tap_start(PrxvcTap *tap) {
    if (tap == NULL || tap->io_proc_id == NULL) {
        return -1;
    }
    OSStatus st = AudioDeviceStart(tap->aggregate_id, tap->io_proc_id);
    if (st == noErr) {
        tap->running = 1;
    }
    return (int32_t)st;
}

void prxvc_tap_stop(PrxvcTap *tap) {
    if (tap == NULL || !tap->running) {
        return;
    }
    // Returns only after the IO proc is no longer running.
    AudioDeviceStop(tap->aggregate_id, tap->io_proc_id);
    tap->running = 0;
}

void prxvc_tap_destroy(PrxvcTap *tap) {
    if (tap == NULL) {
        return;
    }
    prxvc_tap_stop(tap);
    if (tap->aggregate_id != kAudioObjectUnknown && tap->io_proc_id != NULL) {
        AudioDeviceDestroyIOProcID(tap->aggregate_id, tap->io_proc_id);
    }
    if (tap->aggregate_id != kAudioObjectUnknown) {
        AudioHardwareDestroyAggregateDevice(tap->aggregate_id);
    }
    if (tap->tap_id != kAudioObjectUnknown) {
        AudioHardwareDestroyProcessTap(tap->tap_id);
    }
    free(tap);
}
