// Host-side test of the built driver bundle: loads the binary with dlopen, calls the factory and the
// AudioServerPlugIn interface directly (no coreaudiod, no admin rights), and checks identity,
// format, hidden flag, ring loopback and the latency cap.
#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CoreFoundation.h>
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

static int failures;
static AudioServerPlugInIOCycleInfo ci;
#define CHECK(c, ...) do { if (!(c)) { failures++; printf("FAIL: " __VA_ARGS__); printf("\n"); } else { printf("ok  : " __VA_ARGS__); printf("\n"); } } while (0)

static AudioServerPlugInDriverInterface **drv;
static const AudioObjectPropertyAddress ADDR_NAME = {kAudioObjectPropertyName, 0, 0};

static void str_prop(AudioObjectID obj, AudioObjectPropertySelector sel, char *out, size_t n) {
    AudioObjectPropertyAddress a = {sel, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    CFStringRef s = NULL; UInt32 sz = 0;
    (*drv)->GetPropertyData(drv, obj, 0, &a, 0, NULL, sizeof s, &sz, &s);
    out[0] = 0;
    if (s) { CFStringGetCString(s, out, (CFIndex)n, kCFStringEncodingUTF8); }
}
static UInt32 u32_prop(AudioObjectID obj, AudioObjectPropertySelector sel, AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress a = {sel, scope, kAudioObjectPropertyElementMain};
    UInt32 v = 0xdead, sz = 0;
    (*drv)->GetPropertyData(drv, obj, 0, &a, 0, NULL, sizeof v, &sz, &v);
    return v;
}
static AudioObjectID translate(const char *uid) {
    AudioObjectPropertyAddress a = {kAudioPlugInPropertyTranslateUIDToDevice, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    CFStringRef q = CFStringCreateWithCString(NULL, uid, kCFStringEncodingUTF8);
    AudioObjectID id = 0; UInt32 sz = 0;
    (*drv)->GetPropertyData(drv, kAudioObjectPlugInObject, 0, &a, sizeof q, &q, sizeof id, &sz, &id);
    CFRelease(q);
    return id;
}
static OSStatus host_noop(AudioServerPlugInHostRef h, ...) { (void)h; return 0; }

int main(int argc, char **argv) {
    (void)ADDR_NAME;
    if (argc < 2) { fprintf(stderr, "usage: driver_loadtest PATH/TO/Contents/MacOS/PrxVoiceMicrophoneDriver [EXPECTED_TRANSPORT_FOURCC, default virt]\n"); return 1; }
    void *lib = dlopen(argv[1], RTLD_NOW);
    CHECK(lib != NULL, "dlopen binary");
    if (!lib) return 1;
    typedef void *(*Create)(CFAllocatorRef, CFUUIDRef);
    Create create = (Create)dlsym(lib, "PrxVoiceMicrophoneDriverCreate");
    CHECK(create != NULL, "factory symbol PrxVoiceMicrophoneDriverCreate");
    CHECK(dlsym(lib, "HexMicrophoneDriverCreate") == NULL, "no Hex factory symbol");
    drv = create(NULL, kAudioServerPlugInTypeUUID);
    CHECK(drv != NULL, "factory returns driver for AudioServerPlugIn type");
    CHECK(create(NULL, CFUUIDCreate(NULL)) == NULL, "factory rejects other type UUID");
    (*drv)->Initialize(drv, (AudioServerPlugInHostRef)&host_noop);

    char buf[256];
    str_prop(kAudioObjectPlugInObject, kAudioPlugInPropertyBundleID, buf, sizeof buf);
    CHECK(!strcmp(buf, "org.openprx.prx-voice-client.microphone-driver"), "bundle id property '%s'", buf);
    AudioObjectID mic = translate("org.openprx.prx-voice-client.microphone");
    AudioObjectID sink = translate("org.openprx.prx-voice-client.microphone.sink");
    CHECK(mic != 0 && sink != 0 && mic != sink, "UID translation: mic=%u sink=%u", (unsigned)mic, (unsigned)sink);
    CHECK(translate("anomaly.hex.microphone") == 0, "Hex UID is not served");
    str_prop(mic, kAudioObjectPropertyName, buf, sizeof buf);
    CHECK(!strcmp(buf, "PRX Voice Microphone"), "mic name '%s'", buf);
    str_prop(mic, kAudioObjectPropertyManufacturer, buf, sizeof buf);
    CHECK(!strcmp(buf, "OpenPRX"), "manufacturer '%s'", buf);
    str_prop(sink, kAudioObjectPropertyName, buf, sizeof buf);
    CHECK(strstr(buf, "PRX Voice") != NULL, "sink name '%s'", buf);
    CHECK(u32_prop(mic, kAudioDevicePropertyIsHidden, kAudioObjectPropertyScopeGlobal) == 0, "mic is visible");
    CHECK(u32_prop(sink, kAudioDevicePropertyIsHidden, kAudioObjectPropertyScopeGlobal) == 1, "sink is hidden");
    UInt32 expect_tt = kAudioDeviceTransportTypeVirtual;
    if (argc > 2 && strlen(argv[2]) == 4) expect_tt = ((UInt32)(unsigned char)argv[2][0] << 24) | ((UInt32)(unsigned char)argv[2][1] << 16) | ((UInt32)(unsigned char)argv[2][2] << 8) | (UInt32)(unsigned char)argv[2][3];
    UInt32 tmic = u32_prop(mic, kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal);
    UInt32 tsink = u32_prop(sink, kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal);
    CHECK(tmic == expect_tt, "mic transport 0x%08x == expected 0x%08x", (unsigned)tmic, (unsigned)expect_tt);
    CHECK(tsink == tmic, "sink transport equals mic transport (0x%08x)", (unsigned)tsink);
    CHECK(expect_tt != 0 && expect_tt != kAudioDeviceTransportTypeAggregate, "transport is not 0 or aggregate");

    // Streams: mic has 1 input stream, sink has 1 output stream; format 48 kHz mono float32.
    AudioObjectPropertyAddress sa = {kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput, kAudioObjectPropertyElementMain};
    UInt32 sz = 0;
    (*drv)->GetPropertyDataSize(drv, mic, 0, &sa, 0, NULL, &sz);
    CHECK(sz == sizeof(AudioObjectID), "mic input streams: %u bytes", (unsigned)sz);
    sa.mScope = kAudioObjectPropertyScopeOutput;
    (*drv)->GetPropertyDataSize(drv, mic, 0, &sa, 0, NULL, &sz);
    CHECK(sz == 0, "mic has no output stream");
    (*drv)->GetPropertyDataSize(drv, sink, 0, &sa, 0, NULL, &sz);
    CHECK(sz == sizeof(AudioObjectID), "sink output streams: %u bytes", (unsigned)sz);
    AudioObjectID mic_stream = 0;
    sa.mScope = kAudioObjectPropertyScopeInput;
    (*drv)->GetPropertyData(drv, mic, 0, &sa, 0, NULL, sizeof mic_stream, &sz, &mic_stream);
    AudioStreamBasicDescription f; memset(&f, 0, sizeof f);
    AudioObjectPropertyAddress fa = {kAudioStreamPropertyVirtualFormat, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    (*drv)->GetPropertyData(drv, mic_stream, 0, &fa, 0, NULL, sizeof f, &sz, &f);
    CHECK(f.mSampleRate == 48000.0 && f.mChannelsPerFrame == 1 && f.mBitsPerChannel == 32 && (f.mFormatFlags & kAudioFormatFlagIsFloat),
          "mic format %.0f Hz, %u ch, %u bit float", f.mSampleRate, (unsigned)f.mChannelsPerFrame, (unsigned)f.mBitsPerChannel);

    // Loopback: write a ramp into the sink in 512-frame blocks, read the mic in 512-frame blocks.
    (*drv)->StartIO(drv, sink, 1);
    (*drv)->StartIO(drv, mic, 2);
    float in[512], out[512];
    float next = 1.0f, expect = -1.0f;
    int bad = 0, got = 0;
    for (int blk = 0; blk < 200; blk++) {
        for (int i = 0; i < 512; i++) in[i] = next + (float)i;
        next += 512;
        (*drv)->DoIOOperation(drv, sink, 0, 1, kAudioServerPlugInIOOperationWriteMix, 512, &ci, in, NULL);
        (*drv)->DoIOOperation(drv, mic, 0, 2, kAudioServerPlugInIOOperationReadInput, 512, &ci, out, NULL);
        for (int i = 0; i < 512; i++) {
            if (out[i] == 0.0f) continue;
            if (expect < 0) expect = out[i];
            if (out[i] != expect) bad++;
            expect = out[i] + 1.0f;
            got++;
        }
    }
    CHECK(got > 512 * 100 && bad == 0, "ramp loopback: %d samples read, %d discontinuities", got, bad);

    // Latency cap: write 8000 frames without reading, then read; the first sample must be recent.
    for (int k = 0; k < 16; k++) {
        for (int i = 0; i < 512; i++) in[i] = next + (float)i;
        next += 512;
        (*drv)->DoIOOperation(drv, sink, 0, 1, kAudioServerPlugInIOOperationWriteMix, 512, &ci, in, NULL);
    }
    (*drv)->DoIOOperation(drv, mic, 0, 2, kAudioServerPlugInIOOperationReadInput, 512, &ci, out, NULL);
    float lag_frames = (next - 1.0f) - out[0];
    CHECK(lag_frames <= 768 + 512, "latency cap: first sample after a burst lags the newest by %.0f frames (<= 1280)", lag_frames);
    (*drv)->StopIO(drv, mic, 2);
    (*drv)->StopIO(drv, sink, 1);
    printf("%s (%d failures)\n", failures ? "FAILED" : "PASSED", failures);
    return failures ? 1 : 0;
}
