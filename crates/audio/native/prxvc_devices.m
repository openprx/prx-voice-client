// Core Audio input-device enumeration shim. See prxvc_devices.h.

#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#include <stdlib.h>
#include <string.h>

#include "prxvc_devices.h"

static uint32_t prxvc_transport_class(UInt32 t) {
    switch (t) {
    case kAudioDeviceTransportTypeBuiltIn:
        return PRXVC_TRANSPORT_BUILT_IN;
    case kAudioDeviceTransportTypeUSB:
        return PRXVC_TRANSPORT_USB;
    case kAudioDeviceTransportTypeBluetooth:
    case kAudioDeviceTransportTypeBluetoothLE:
        return PRXVC_TRANSPORT_BLUETOOTH;
    case kAudioDeviceTransportTypeVirtual:
        return PRXVC_TRANSPORT_VIRTUAL;
    case kAudioDeviceTransportTypeAggregate:
    case kAudioDeviceTransportTypeAutoAggregate:
        return PRXVC_TRANSPORT_AGGREGATE;
    case kAudioDeviceTransportTypeUnknown:
        return PRXVC_TRANSPORT_UNKNOWN;
    default:
        return PRXVC_TRANSPORT_OTHER;
    }
}

// Copy a CFString property into `dst` (UTF-8, truncated). Leaves "" on failure.
static void prxvc_copy_string_property(AudioObjectID obj, AudioObjectPropertySelector sel,
                                       AudioObjectPropertyScope scope, char *dst) {
    dst[0] = '\0';
    AudioObjectPropertyAddress addr = {sel, scope, kAudioObjectPropertyElementMain};
    CFStringRef str = NULL;
    UInt32 size = sizeof(str);
    if (AudioObjectGetPropertyData(obj, &addr, 0, NULL, &size, &str) != noErr || str == NULL) {
        return;
    }
    if (!CFStringGetCString(str, dst, PRXVC_DEVICE_STRING_LEN, kCFStringEncodingUTF8)) {
        dst[0] = '\0';
    }
    CFRelease(str);
}

// Whether the device has at least one input channel.
static bool prxvc_has_input(AudioObjectID dev) {
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

int32_t prxvc_list_input_devices(PrxvcInputDevice *out, int32_t capacity) {
    if (out == NULL || capacity <= 0) {
        return 0;
    }
    AudioObjectPropertyAddress list_addr = {kAudioHardwarePropertyDevices,
                                            kAudioObjectPropertyScopeGlobal,
                                            kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    OSStatus st = AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &list_addr, 0, NULL, &size);
    if (st != noErr) {
        return (int32_t)st;
    }
    UInt32 count = size / (UInt32)sizeof(AudioObjectID);
    if (count == 0) {
        return 0;
    }
    AudioObjectID *ids = (AudioObjectID *)malloc(size);
    if (ids == NULL) {
        return -1;
    }
    st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &list_addr, 0, NULL, &size, ids);
    if (st != noErr) {
        free(ids);
        return (int32_t)st;
    }

    AudioObjectID default_input = kAudioObjectUnknown;
    AudioObjectPropertyAddress def_addr = {kAudioHardwarePropertyDefaultInputDevice,
                                           kAudioObjectPropertyScopeGlobal,
                                           kAudioObjectPropertyElementMain};
    UInt32 def_size = sizeof(default_input);
    (void)AudioObjectGetPropertyData(kAudioObjectSystemObject, &def_addr, 0, NULL, &def_size,
                                     &default_input);

    int32_t written = 0;
    count = size / (UInt32)sizeof(AudioObjectID);
    for (UInt32 i = 0; i < count && written < capacity; i++) {
        AudioObjectID dev = ids[i];
        if (!prxvc_has_input(dev)) {
            continue;
        }
        PrxvcInputDevice *d = &out[written];
        memset(d, 0, sizeof(*d));
        d->device_id = dev;
        prxvc_copy_string_property(dev, kAudioDevicePropertyDeviceUID,
                                   kAudioObjectPropertyScopeGlobal, d->uid);
        // Same property cpal reads for `Device::name`, so the names match.
        prxvc_copy_string_property(dev, kAudioDevicePropertyDeviceNameCFString,
                                   kAudioDevicePropertyScopeOutput, d->name);
        UInt32 transport = 0;
        UInt32 tsize = sizeof(transport);
        AudioObjectPropertyAddress taddr = {kAudioDevicePropertyTransportType,
                                            kAudioObjectPropertyScopeGlobal,
                                            kAudioObjectPropertyElementMain};
        if (AudioObjectGetPropertyData(dev, &taddr, 0, NULL, &tsize, &transport) == noErr) {
            d->transport = prxvc_transport_class(transport);
        }
        d->is_default = (dev == default_input) ? 1 : 0;
        written++;
    }
    free(ids);
    return written;
}
