#include "wav.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint32_t rd32(const uint8_t *p) { return p[0] | (p[1] << 8) | (p[2] << 16) | ((uint32_t)p[3] << 24); }
static uint16_t rd16(const uint8_t *p) { return (uint16_t)(p[0] | (p[1] << 8)); }

int wav_read_mono(const char *path, float **samples, size_t *n, double *rate) {
    FILE *f = fopen(path, "rb");
    if (f == NULL) {
        fprintf(stderr, "error: cannot open %s\n", path);
        return -1;
    }
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *b = malloc((size_t)size);
    if (b == NULL || fread(b, 1, (size_t)size, f) != (size_t)size) {
        fclose(f);
        free(b);
        return -1;
    }
    fclose(f);
    if (size < 12 || memcmp(b, "RIFF", 4) != 0 || memcmp(b + 8, "WAVE", 4) != 0) {
        fprintf(stderr, "error: %s is not a RIFF/WAVE file\n", path);
        free(b);
        return -1;
    }
    unsigned fmt = 0, ch = 0, bits = 0;
    uint32_t sr = 0;
    const uint8_t *data = NULL;
    size_t dlen = 0;
    size_t pos = 12;
    while (pos + 8 <= (size_t)size) {
        size_t len = rd32(b + pos + 4);
        const uint8_t *c = b + pos + 8;
        if (pos + 8 + len > (size_t)size) len = (size_t)size - pos - 8;
        if (memcmp(b + pos, "fmt ", 4) == 0 && len >= 16) {
            fmt = rd16(c);
            ch = rd16(c + 2);
            sr = rd32(c + 4);
            bits = rd16(c + 14);
            if (fmt == 0xFFFE && len >= 26) fmt = rd16(c + 24);  // WAVE_FORMAT_EXTENSIBLE sub-format
        } else if (memcmp(b + pos, "data", 4) == 0) {
            data = c;
            dlen = len;
        }
        pos += 8 + len + (len & 1);
    }
    int ok = data != NULL && ch > 0 && ((fmt == 1 && (bits == 16 || bits == 24 || bits == 32)) || (fmt == 3 && bits == 32));
    if (!ok) {
        fprintf(stderr, "error: unsupported WAV (format %u, %u bits, %u ch)\n", fmt, bits, ch);
        free(b);
        return -1;
    }
    size_t bps = bits / 8;
    size_t frames = dlen / (bps * ch);
    float *out = malloc(frames * sizeof(float));
    if (out == NULL) {
        free(b);
        return -1;
    }
    for (size_t i = 0; i < frames; i++) {
        double acc = 0;
        for (unsigned c = 0; c < ch; c++) {
            const uint8_t *p = data + (i * ch + c) * bps;
            double v;
            if (fmt == 3) {
                uint32_t u = rd32(p);
                float fv;
                memcpy(&fv, &u, 4);
                v = fv;
            } else if (bits == 16) {
                v = (int16_t)rd16(p) / 32768.0;
            } else if (bits == 24) {
                int32_t s = (int32_t)((uint32_t)p[0] << 8 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 24);
                v = s / 2147483648.0;
            } else {
                v = (int32_t)rd32(p) / 2147483648.0;
            }
            acc += v;
        }
        out[i] = (float)(acc / ch);
    }
    free(b);
    *samples = out;
    *n = frames;
    *rate = sr;
    return 0;
}

static void w32(FILE *f, uint32_t v) {
    uint8_t p[4] = {v & 255, (v >> 8) & 255, (v >> 16) & 255, (v >> 24) & 255};
    fwrite(p, 1, 4, f);
}
static void w16(FILE *f, uint16_t v) {
    uint8_t p[2] = {v & 255, v >> 8};
    fwrite(p, 1, 2, f);
}

int wav_write_mono_f32(const char *path, const float *s, size_t n, double rate) {
    FILE *f = fopen(path, "wb");
    if (f == NULL) {
        fprintf(stderr, "error: cannot write %s\n", path);
        return -1;
    }
    uint32_t bytes = (uint32_t)(n * 4);
    fwrite("RIFF", 1, 4, f);
    w32(f, 36 + bytes);
    fwrite("WAVEfmt ", 1, 8, f);
    w32(f, 16);
    w16(f, 3);
    w16(f, 1);
    w32(f, (uint32_t)rate);
    w32(f, (uint32_t)rate * 4);
    w16(f, 4);
    w16(f, 32);
    fwrite("data", 1, 4, f);
    w32(f, bytes);
    fwrite(s, 4, n, f);
    return fclose(f) == 0 ? 0 : -1;
}

float *resample_linear(const float *in, size_t n, double from, double to, size_t *out_n) {
    size_t m = (size_t)((double)n * to / from);
    float *out = malloc((m ? m : 1) * sizeof(float));
    if (out == NULL) return NULL;
    double step = from / to;
    for (size_t i = 0; i < m; i++) {
        double pos = (double)i * step;
        size_t k = (size_t)pos;
        double frac = pos - (double)k;
        float a = in[k < n ? k : n - 1];
        float c = in[k + 1 < n ? k + 1 : n - 1];
        out[i] = (float)(a + (c - a) * frac);
    }
    *out_n = m;
    return out;
}
