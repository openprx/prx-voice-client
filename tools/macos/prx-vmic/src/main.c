// prx-vmic: feeder / recorder / self-test for the PRX Voice virtual microphone.
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "hal.h"
#include "io.h"
#include "props.h"
#include "selftest.h"
#include "wav.h"

static void usage(void) {
    fputs("usage:\n"
          "  prx-vmic list\n"
          "  prx-vmic props  [--device UID_OR_NAME]   (live transport type etc. of the microphone)\n"
          "  prx-vmic tone   [--seconds N] [--freq HZ] [--level 0..1] [--device UID_OR_NAME]\n"
          "  prx-vmic play   FILE.wav [--device UID_OR_NAME]\n"
          "  prx-vmic record --seconds N --out FILE.wav [--device UID_OR_NAME]\n"
          "  prx-vmic analyze FILE.wav\n"
          "  prx-vmic gen    FILE.wav [--seconds N] [--freq HZ]   (write a test tone WAV, no device needed)\n"
          "  prx-vmic selftest [--out FILE.wav] [--sink UID_OR_NAME] [--mic UID_OR_NAME]\n"
          "default sink: " PRX_SINK_UID "\ndefault mic : " PRX_MIC_UID "\n"
          "exit codes: 0 ok, 1 error, 2 device not installed, 3 self-test failed\n"
          "Recording needs microphone permission for the terminal app that launches this tool.\n",
          stderr);
}

static const char *opt(int argc, char **argv, const char *name, const char *def) {
    for (int i = 2; i + 1 < argc; i++) if (strcmp(argv[i], name) == 0) return argv[i + 1];
    return def;
}

static int open_device(const char *key, AudioObjectPropertyScope scope, HalDevice *d, HalFormat *f) {
    if (hal_find(key, d) != 0) {
        fprintf(stderr, "error: device '%s' not installed (for the PRX driver run driver/macos/install.sh; see `prx-vmic list`)\n", key);
        return 2;
    }
    if (hal_format(d, scope, f) != 0) return 1;
    printf("device : %s [%s] id %u\n", d->name, d->uid, (unsigned)d->id);
    printf("format : %.0f Hz, %u ch, float32 linear PCM, latency %u, safety offset %u, buffer %u frames\n", f->rate,
           (unsigned)f->channels, f->latency, f->safety, f->buffer);
    return 0;
}

static int do_play(const char *key, const float *src, size_t n, double src_rate) {
    HalDevice d;
    HalFormat f;
    int rc = open_device(key, kAudioObjectPropertyScopeOutput, &d, &f);
    if (rc) return rc;
    size_t m = n;
    float *conv = NULL;
    if (fabs(src_rate - f.rate) > 0.5) {
        conv = resample_linear(src, n, src_rate, f.rate, &m);
        src = conv;
        printf("resampled %.0f Hz -> %.0f Hz (linear)\n", src_rate, f.rate);
    }
    PlayJob job = {.src = src, .rate = f.rate, .n = m, .marker = -1};
    void *h;
    if (player_start(&d, &f, &job, &h) != 0) return 1;
    printf("playing %.2f s ...\n", (double)m / f.rate);
    player_finish(h);
    printf("done   : %llu callbacks, underruns %llu, overruns %llu\n", (unsigned long long)job.callbacks,
           (unsigned long long)job.underruns, (unsigned long long)job.overruns);
    free(conv);
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 2) { usage(); return 1; }
    const char *cmd = argv[1];
    if (strcmp(cmd, "list") == 0) return hal_list();
    if (strcmp(cmd, "props") == 0) return props_run(opt(argc, argv, "--device", PRX_MIC_UID));
    if (strcmp(cmd, "analyze") == 0 && argc >= 3) return analyze_file(argv[2]);
    if (strcmp(cmd, "selftest") == 0)
        return selftest_run(opt(argc, argv, "--sink", PRX_SINK_UID), opt(argc, argv, "--mic", PRX_MIC_UID),
                            opt(argc, argv, "--out", NULL));
    const char *dev = opt(argc, argv, "--device", NULL);
    if (strcmp(cmd, "gen") == 0 && argc >= 3) {
        double secs = atof(opt(argc, argv, "--seconds", "3")), freq = atof(opt(argc, argv, "--freq", "440"));
        size_t n = (size_t)(secs * 48000);
        float *x = malloc(n * sizeof(float));
        for (size_t i = 0; i < n; i++) x[i] = (float)(0.5 * sin(2 * M_PI * freq * (double)i / 48000.0));
        int rc = wav_write_mono_f32(argv[2], x, n, 48000.0);
        free(x);
        return rc;
    }
    if (strcmp(cmd, "tone") == 0) {
        double secs = atof(opt(argc, argv, "--seconds", "5"));
        double freq = atof(opt(argc, argv, "--freq", "440"));
        double level = atof(opt(argc, argv, "--level", "0.5"));
        if (secs <= 0 || secs > 3600 || freq <= 0 || level <= 0 || level > 1) { usage(); return 1; }
        size_t n = (size_t)(secs * 48000);
        float *x = malloc(n * sizeof(float));
        for (size_t i = 0; i < n; i++) x[i] = (float)(level * sin(2 * M_PI * freq * (double)i / 48000.0));
        int rc = do_play(dev ? dev : PRX_SINK_UID, x, n, 48000.0);
        free(x);
        return rc;
    }
    if (strcmp(cmd, "play") == 0 && argc >= 3) {
        float *x; size_t n; double r;
        if (wav_read_mono(argv[2], &x, &n, &r) != 0) return 1;
        int rc = do_play(dev ? dev : PRX_SINK_UID, x, n, r);
        free(x);
        return rc;
    }
    if (strcmp(cmd, "record") == 0) {
        double secs = atof(opt(argc, argv, "--seconds", "0"));
        const char *out = opt(argc, argv, "--out", NULL);
        if (secs <= 0 || secs > 3600 || !out) { usage(); return 1; }
        HalDevice d;
        HalFormat f;
        int rc = open_device(dev ? dev : PRX_MIC_UID, kAudioObjectPropertyScopeInput, &d, &f);
        if (rc) return rc;
        Capture cap;
        void *h;
        if (capture_start(&d, &f, secs, &cap, &h) != 0) return 1;
        printf("recording %.1f s (microphone permission needed for this terminal app) ...\n", secs);
        usleep((useconds_t)(secs * 1e6));
        capture_stop(h);
        if (cap.n > (size_t)(secs * f.rate)) cap.n = (size_t)(secs * f.rate);
        if (wav_write_mono_f32(out, cap.buf, cap.n, f.rate) != 0) return 1;
        printf("saved  : %s (%zu frames)\n", out, cap.n);
        return analyze_file(out);
    }
    usage();
    return 1;
}
