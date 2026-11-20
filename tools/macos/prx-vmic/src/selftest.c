#include "selftest.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "dsp.h"
#include "hal.h"
#include "io.h"
#include "wav.h"

#define TONE_HZ 440.0
#define TONE_AMP 0.5
#define BLOCK_MS 10

typedef struct { size_t tone_start, tone_end, click; } Layout;

// Scenario: 1.0 s silence, 2.0 s tone, 1.0 s silence, 1 ms click, 1.0 s silence.
static float *build_scenario(double rate, size_t *n, Layout *lay) {
    size_t s1 = (size_t)rate, tone = (size_t)(2 * rate), s2 = (size_t)rate;
    size_t click_at = ((s1 + tone + s2) / PLAY_CHUNK + 1) * PLAY_CHUNK;
    size_t total = click_at + (size_t)rate;
    float *x = calloc(total, sizeof(float));
    size_t fade = (size_t)(0.01 * rate);
    for (size_t i = 0; i < tone; i++) {
        double env = i < fade ? (double)i / fade : (i > tone - fade ? (double)(tone - i) / fade : 1.0);
        x[s1 + i] = (float)(TONE_AMP * env * sin(2 * M_PI * TONE_HZ * (double)i / rate));
    }
    for (size_t i = 0; i < (size_t)(0.001 * rate); i++) x[click_at + i] = 0.8f;
    lay->tone_start = s1; lay->tone_end = s1 + tone; lay->click = click_at;
    *n = total;
    return x;
}

static const char *verdict(int ok) { return ok ? "ok" : "FAIL"; }

int selftest_run(const char *sink_key, const char *mic_key, const char *wav_out) {
    HalDevice sink, mic;
    if (hal_find(sink_key, &sink) != 0) {
        fprintf(stderr, "error: output device '%s' not installed (run driver/macos/install.sh first)\n", sink_key);
        return 2;
    }
    if (hal_find(mic_key, &mic) != 0) {
        fprintf(stderr, "error: input device '%s' not installed (run driver/macos/install.sh first)\n", mic_key);
        return 2;
    }
    HalFormat of, inf;
    if (hal_format(&sink, kAudioObjectPropertyScopeOutput, &of) != 0 ||
        hal_format(&mic, kAudioObjectPropertyScopeInput, &inf) != 0) return 1;
    printf("sink : %s [%s] id %u, %.0f Hz, %u ch, float32, latency %u, safety %u, buffer %u frames\n", sink.name,
           sink.uid, (unsigned)sink.id, of.rate, (unsigned)of.channels, of.latency, of.safety, of.buffer);
    printf("mic  : %s [%s] id %u, %.0f Hz, %u ch, float32, latency %u, safety %u, buffer %u frames\n", mic.name,
           mic.uid, (unsigned)mic.id, inf.rate, (unsigned)inf.channels, inf.latency, inf.safety, inf.buffer);
    printf("note : the first capture start asks for microphone permission for the launching terminal app\n");

    size_t n;
    Layout lay;
    float *src = build_scenario(of.rate, &n, &lay);
    PlayJob job = {.src = src, .rate = of.rate, .n = n, .marker = (long)lay.click};
    Capture cap;
    void *ch, *ph;
    if (capture_start(&mic, &inf, (double)n / of.rate + 1.0, &cap, &ch) != 0) return 1;
    if (player_start(&sink, &of, &job, &ph) != 0) { capture_stop(ch); return 1; }
    player_finish(ph);
    capture_stop(ch);

    printf("play : %llu callbacks, %llu underruns, %llu overruns\n", (unsigned long long)job.callbacks,
           (unsigned long long)job.underruns, (unsigned long long)job.overruns);
    printf("rec  : %zu frames, %llu callbacks\n", cap.n, (unsigned long long)cap.callbacks);
    if (wav_out) {
        wav_write_mono_f32(wav_out, cap.buf, cap.n, cap.rate);
        printf("saved: %s\n", wav_out);
    }
    double rate = cap.rate;
    if (dsp_peak(cap.buf, cap.n) < 1e-6) {
        printf("RESULT: FAIL - capture is digital silence. Either microphone permission was denied to this\n"
               "        terminal app (System Settings > Privacy & Security > Microphone), or no audio reaches the device.\n");
        return 3;
    }

    // Tone: first run of >= 50 consecutive 10 ms blocks above 0.1 RMS.
    size_t blk = (size_t)(rate * BLOCK_MS / 1000), nb = cap.n / blk;
    size_t run = 0, ts = 0, te = 0;
    for (size_t b = 0; b < nb && te == 0; b++) {
        if (dsp_rms(cap.buf + b * blk, blk) > 0.1) { if (run++ == 0) ts = b; }
        else { if (run >= 50) te = b; run = 0; }
    }
    if (te == 0 && run >= 50) te = nb;
    if (te == 0) {
        printf("RESULT: FAIL - no sustained tone found in the recording\n");
        return 3;
    }
    size_t trim = (size_t)(0.2 * rate);
    size_t is = ts * blk + trim, ie = te * blk - trim;
    const float *mid = cap.buf + is;
    size_t mn = ie > is ? ie - is : 0;
    double freq = dsp_dominant_freq(mid, mn, rate);
    double rms = dsp_rms(mid, mn);
    size_t drop = 0;
    for (size_t b = is / blk; b < ie / blk; b++) if (dsp_rms(cap.buf + b * blk, blk) < 0.25) drop++;
    double expect = TONE_AMP / sqrt(2.0);
    double db = 20 * log10(rms / expect);
    double tone_len = (double)(te - ts) * BLOCK_MS / 1000.0;

    // Click: first sample above 0.3 well after the tone.
    size_t from = te * blk + (size_t)(0.3 * rate), ci = 0;
    for (size_t i = from; i < cap.n; i++) if (fabsf(cap.buf[i]) > 0.3f) { ci = i; break; }
    double lat_write = -1, lat_out = -1;
    if (ci) {
        uint64_t t = capture_host_time(&cap, ci);
        if (job.marker_write_host && t > job.marker_write_host) lat_write = hal_host_ns(t - job.marker_write_host) / 1e6;
        if (job.marker_out_host && t > job.marker_out_host) lat_out = hal_host_ns(t - job.marker_out_host) / 1e6;
    }

    int f_ok = fabs(freq - TONE_HZ) < 1.0, r_ok = fabs(db) < 1.5, d_ok = drop == 0;
    int l_ok = lat_write > 0 && lat_write < 100.0;
    printf("tone : dominant %.2f Hz (expect %.0f) [%s]\n", freq, TONE_HZ, verdict(f_ok));
    printf("       RMS %.4f = %.1f dBFS (expect %.4f, delta %+.2f dB) [%s]\n", rms, 20 * log10(rms + 1e-12), expect, db,
           verdict(r_ok));
    printf("       length %.2f s (expect 2.00), dropout blocks %zu [%s]\n", tone_len, drop, verdict(d_ok));
    if (ci) {
        printf("click: found at %.3f s; end-to-end latency %.1f ms from the write into the sink ring,\n"
               "       %.1f ms from the sink render time [%s]\n",
               (double)ci / rate, lat_write, lat_out, verdict(l_ok));
    } else {
        printf("click: not found [FAIL]\n");
    }
    int pass = f_ok && r_ok && d_ok && l_ok;
    printf("RESULT: %s\n", pass ? "PASS" : "FAIL");
    free(src); free(cap.buf); free(cap.cbs);
    return pass ? 0 : 3;
}

int analyze_file(const char *path) {
    float *x; size_t n; double rate;
    if (wav_read_mono(path, &x, &n, &rate) != 0) return 1;
    size_t blk = (size_t)(rate * BLOCK_MS / 1000), nb = n / blk;
    double mx = 0;
    for (size_t b = 0; b < nb; b++) mx = fmax(mx, dsp_rms(x + b * blk, blk));
    size_t start = 0;
    for (size_t b = 0; b < nb; b++) if (dsp_rms(x + b * blk, blk) > 0.1 * mx) { start = b * blk; break; }
    printf("%s: %zu frames, %.0f Hz, %.2f s, peak %.4f, RMS %.4f (%.1f dBFS), dominant %.2f Hz\n", path, n, rate,
           (double)n / rate, dsp_peak(x, n), dsp_rms(x, n), 20 * log10(dsp_rms(x, n) + 1e-12),
           dsp_dominant_freq(x + start, n - start, rate));
    free(x);
    return 0;
}
