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

// Whether the device has at least one channel in `scope` (input or output).
static bool prxvc_has_channels(AudioObjectID dev, AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress addr = {kAudioDevicePropertyStreamConfiguration, scope,
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

static bool prxvc_has_input(AudioObjectID dev) {
    return prxvc_has_channels(dev, kAudioObjectPropertyScopeInput);
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

static uint32_t prxvc_u32_property(AudioObjectID obj, AudioObjectPropertySelector sel, uint32_t fallback) {
    AudioObjectPropertyAddress addr = {sel, kAudioObjectPropertyScopeGlobal,
                                       kAudioObjectPropertyElementMain};
    UInt32 value = 0;
    UInt32 size = sizeof(value);
    if (AudioObjectGetPropertyData(obj, &addr, 0, NULL, &size, &value) != noErr) {
        return fallback;
    }
    return value;
}

static void prxvc_fill_device(AudioObjectID dev, AudioObjectID default_input, PrxvcDevice *d) {
    memset(d, 0, sizeof(*d));
    d->device_id = dev;
    d->has_input = prxvc_has_channels(dev, kAudioObjectPropertyScopeInput) ? 1 : 0;
    d->has_output = prxvc_has_channels(dev, kAudioObjectPropertyScopeOutput) ? 1 : 0;
    prxvc_copy_string_property(dev, kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal,
                               d->uid);
    prxvc_copy_string_property(dev, kAudioDevicePropertyDeviceNameCFString,
                               kAudioDevicePropertyScopeOutput, d->name);
    d->transport = prxvc_transport_class(
        prxvc_u32_property(dev, kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeUnknown));
    d->is_alive = prxvc_u32_property(dev, kAudioDevicePropertyDeviceIsAlive, 1) != 0 ? 1 : 0;
    d->is_hidden = prxvc_u32_property(dev, kAudioDevicePropertyIsHidden, 0) != 0 ? 1 : 0;
    d->is_default_input = (dev == default_input) ? 1 : 0;
}

int32_t prxvc_device_by_uid(const char *uid, PrxvcDevice *out) {
    if (uid == NULL || out == NULL) {
        return -1;
    }
    CFStringRef key = CFStringCreateWithCString(NULL, uid, kCFStringEncodingUTF8);
    if (key == NULL) {
        return -1;
    }
    AudioObjectPropertyAddress addr = {kAudioHardwarePropertyTranslateUIDToDevice,
                                       kAudioObjectPropertyScopeGlobal,
                                       kAudioObjectPropertyElementMain};
    AudioObjectID dev = kAudioObjectUnknown;
    UInt32 size = sizeof(dev);
    OSStatus st = AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, sizeof(key), &key,
                                             &size, &dev);
    CFRelease(key);
    if (st != noErr || dev == kAudioObjectUnknown) {
        return -1;
    }
    prxvc_fill_device(dev, kAudioObjectUnknown, out);
    return 0;
}

int32_t prxvc_list_devices(PrxvcDevice *out, int32_t capacity) {
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
    if (size == 0) {
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
    UInt32 count = size / (UInt32)sizeof(AudioObjectID);
    for (UInt32 i = 0; i < count && written < capacity; i++) {
        prxvc_fill_device(ids[i], default_input, &out[written]);
        written++;
    }
    free(ids);
    return written;
}
