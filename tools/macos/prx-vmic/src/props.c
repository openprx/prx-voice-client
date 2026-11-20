#include "props.h"

#include <stdio.h>

#include "hal.h"

static const char *fourcc(UInt32 v, char *b) {
    b[0] = (char)(v >> 24); b[1] = (char)(v >> 16); b[2] = (char)(v >> 8); b[3] = (char)v; b[4] = 0;
    for (int i = 0; i < 4; i++) if (b[i] < 32 || b[i] > 126) { snprintf(b, 16, "0x%08x", (unsigned)v); break; }
    return b;
}

static int get_u32(AudioObjectID id, AudioObjectPropertySelector sel, AudioObjectPropertyScope scope, UInt32 *v) {
    AudioObjectPropertyAddress a = {sel, scope, kAudioObjectPropertyElementMain};
    UInt32 sz = sizeof *v;
    return AudioObjectGetPropertyData(id, &a, 0, NULL, &sz, v) == noErr;
}

// Variant name for a transport type, as used by driver/macos/build.sh --transport.
static const char *variant_of(UInt32 t) {
    switch (t) {
        case kAudioDeviceTransportTypeVirtual: return "virtual";
        case kAudioDeviceTransportTypeUSB: return "usb";
        case kAudioDeviceTransportTypeBuiltIn: return "builtin";
        case kAudioDeviceTransportTypePCI: return "other";
        default: return "unrecognised";
    }
}

int props_run(const char *key) {
    HalDevice d;
    if (hal_find(key, &d) != 0) {
        fprintf(stderr, "error: device '%s' not installed\n", key);
        return 2;
    }
    UInt32 tt = 0, hidden = 0, alive = 0, cdi = 0, clock = 0;
    int have_tt = get_u32(d.id, kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal, &tt);
    get_u32(d.id, kAudioDevicePropertyIsHidden, kAudioObjectPropertyScopeGlobal, &hidden);
    get_u32(d.id, kAudioDevicePropertyDeviceIsAlive, kAudioObjectPropertyScopeGlobal, &alive);
    get_u32(d.id, kAudioDevicePropertyDeviceCanBeDefaultDevice, kAudioObjectPropertyScopeInput, &cdi);
    get_u32(d.id, kAudioDevicePropertyClockDomain, kAudioObjectPropertyScopeGlobal, &clock);
    char tb[16];
    printf("device   : %s [%s] id %u\n", d.name, d.uid, (unsigned)d.id);
    printf("transport=%s variant=%s\n", have_tt ? fourcc(tt, tb) : "(unreadable)", have_tt ? variant_of(tt) : "unknown");
    printf("hidden=%u alive=%u canBeDefaultInput=%u clockDomain=0x%x\n", hidden, alive, cdi, (unsigned)clock);
    return 0;
}
