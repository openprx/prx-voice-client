// Core Audio default-device shim. See prxvc_defaults.h.

#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#include <string.h>

#include "prxvc_defaults.h"

static AudioObjectID prxvc_translate_uid(const char *uid) {
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
    return st == noErr ? dev : kAudioObjectUnknown;
}

static bool prxvc_device_has_input(AudioObjectID dev) {
    AudioObjectPropertyAddress addr = {kAudioDevicePropertyStreamConfiguration,
                                       kAudioObjectPropertyScopeInput,
                                       kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(dev, &addr, 0, NULL, &size) != noErr || size == 0) {
        return false;
    }
    AudioBufferList *list = (AudioBufferList *)malloc(size);
    if (list == NULL) {
        return false;
    }
    bool has = false;
    if (AudioObjectGetPropertyData(dev, &addr, 0, NULL, &size, list) == noErr) {
        for (UInt32 i = 0; i < list->mNumberBuffers; i++) {
            if (list->mBuffers[i].mNumberChannels > 0) {
                has = true;
                break;
            }
        }
    }
    free(list);
    return has;
}

int32_t prxvc_default_device_uid(int32_t is_input, char *uid_out, int32_t capacity) {
    if (uid_out == NULL || capacity <= 0) {
        return -1;
    }
    uid_out[0] = '\0';
    AudioObjectPropertyAddress addr = {is_input ? kAudioHardwarePropertyDefaultInputDevice
                                                : kAudioHardwarePropertyDefaultOutputDevice,
                                       kAudioObjectPropertyScopeGlobal,
                                       kAudioObjectPropertyElementMain};
    AudioObjectID dev = kAudioObjectUnknown;
    UInt32 size = sizeof(dev);
    OSStatus st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, &size, &dev);
    if (st != noErr) {
        return (int32_t)st;
    }
    if (dev == kAudioObjectUnknown) {
        return -1;
    }
    AudioObjectPropertyAddress uaddr = {kAudioDevicePropertyDeviceUID,
                                        kAudioObjectPropertyScopeGlobal,
                                        kAudioObjectPropertyElementMain};
    CFStringRef str = NULL;
    UInt32 ssize = sizeof(str);
    st = AudioObjectGetPropertyData(dev, &uaddr, 0, NULL, &ssize, &str);
    if (st != noErr || str == NULL) {
        return st != noErr ? (int32_t)st : -1;
    }
    bool ok = CFStringGetCString(str, uid_out, capacity, kCFStringEncodingUTF8);
    CFRelease(str);
    if (!ok) {
        uid_out[0] = '\0';
        return -1;
    }
    return 0;
}

int32_t prxvc_set_default_input_uid(const char *uid) {
    if (uid == NULL) {
        return -1;
    }
    AudioObjectID dev = prxvc_translate_uid(uid);
    if (dev == kAudioObjectUnknown) {
        return -1;
    }
    if (!prxvc_device_has_input(dev)) {
        return -2;
    }
    AudioObjectPropertyAddress addr = {kAudioHardwarePropertyDefaultInputDevice,
                                       kAudioObjectPropertyScopeGlobal,
                                       kAudioObjectPropertyElementMain};
    OSStatus st = AudioObjectSetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, sizeof(dev), &dev);
    return (int32_t)st;
}

@interface PrxvcListenerHolder : NSObject
@property(nonatomic) dispatch_queue_t queue;
@property(nonatomic, strong) NSMutableArray *blocks;
@end

@implementation PrxvcListenerHolder
@end

static const AudioObjectPropertySelector kPrxvcSelectors[3] = {
    kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices,
};
static const int32_t kPrxvcKinds[3] = {
    PRXVC_CHANGE_DEFAULT_INPUT,
    PRXVC_CHANGE_DEFAULT_OUTPUT,
    PRXVC_CHANGE_DEVICE_LIST,
};

static AudioObjectPropertyAddress prxvc_address(AudioObjectPropertySelector sel) {
    AudioObjectPropertyAddress a = {sel, kAudioObjectPropertyScopeGlobal,
                                    kAudioObjectPropertyElementMain};
    return a;
}

static void prxvc_remove_listeners(PrxvcListenerHolder *holder) {
    for (NSUInteger i = 0; i < holder.blocks.count; i++) {
        AudioObjectPropertyAddress addr = prxvc_address(kPrxvcSelectors[i]);
        AudioObjectRemovePropertyListenerBlock(kAudioObjectSystemObject, &addr, holder.queue,
                                               holder.blocks[i]);
    }
}

PrxvcChangeListener *prxvc_change_listener_start(prxvc_change_cb cb, void *ctx, int32_t *status) {
    if (status != NULL) {
        *status = 0;
    }
    if (cb == NULL) {
        if (status != NULL) {
            *status = -1;
        }
        return NULL;
    }
    PrxvcListenerHolder *holder = [[PrxvcListenerHolder alloc] init];
    holder.queue = dispatch_queue_create("org.openprx.prx-voice-client.routing.hal", DISPATCH_QUEUE_SERIAL);
    holder.blocks = [NSMutableArray arrayWithCapacity:3];
    // One block per selector, so the kind is known without inspecting the addresses.
    for (int i = 0; i < 3; i++) {
        int32_t kind = kPrxvcKinds[i];
        AudioObjectPropertyListenerBlock block = [^(UInt32 count, const AudioObjectPropertyAddress *addrs) {
            (void)count;
            (void)addrs;
            cb(kind, ctx);
        } copy];
        AudioObjectPropertyAddress addr = prxvc_address(kPrxvcSelectors[i]);
        OSStatus st = AudioObjectAddPropertyListenerBlock(kAudioObjectSystemObject, &addr, holder.queue, block);
        if (st != noErr) {
            prxvc_remove_listeners(holder);
            dispatch_sync(holder.queue, ^{
            });
            if (status != NULL) {
                *status = (int32_t)st;
            }
            return NULL;
        }
        [holder.blocks addObject:block];
    }
    return (PrxvcChangeListener *)CFBridgingRetain(holder);
}

void prxvc_change_listener_stop(PrxvcChangeListener *listener) {
    if (listener == NULL) {
        return;
    }
    PrxvcListenerHolder *holder = (PrxvcListenerHolder *)CFBridgingRelease((CFTypeRef)listener);
    prxvc_remove_listeners(holder);
    // Wait for a callback that is already running on the queue.
    dispatch_sync(holder.queue, ^{
    });
}
