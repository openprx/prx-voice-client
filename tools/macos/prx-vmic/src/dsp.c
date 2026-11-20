#include "dsp.h"

#include <math.h>
#include <stdlib.h>

double dsp_rms(const float *x, size_t n) {
    if (n == 0) return 0;
    double acc = 0;
    for (size_t i = 0; i < n; i++) acc += (double)x[i] * x[i];
    return sqrt(acc / (double)n);
}

double dsp_peak(const float *x, size_t n) {
    double p = 0;
    for (size_t i = 0; i < n; i++) p = fmax(p, fabs((double)x[i]));
    return p;
}

static void fft(double *re, double *im, size_t n) {
    for (size_t i = 1, j = 0; i < n; i++) {
        size_t bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) {
            double t = re[i]; re[i] = re[j]; re[j] = t;
            t = im[i]; im[i] = im[j]; im[j] = t;
        }
    }
    for (size_t len = 2; len <= n; len <<= 1) {
        double ang = -2.0 * M_PI / (double)len;
        for (size_t i = 0; i < n; i += len) {
            for (size_t k = 0; k < len / 2; k++) {
                double wr = cos(ang * (double)k), wi = sin(ang * (double)k);
                size_t a = i + k, b = i + k + len / 2;
                double xr = re[b] * wr - im[b] * wi, xi = re[b] * wi + im[b] * wr;
                re[b] = re[a] - xr; im[b] = im[a] - xi;
                re[a] += xr; im[a] += xi;
            }
        }
    }
}

double dsp_dominant_freq(const float *x, size_t n, double rate) {
    size_t N = 1;
    while (N * 2 <= n && N < 65536) N *= 2;
    if (N < 16) return 0;
    double *re = calloc(N, sizeof(double)), *im = calloc(N, sizeof(double));
    for (size_t i = 0; i < N; i++) re[i] = x[i] * (0.5 - 0.5 * cos(2.0 * M_PI * (double)i / (double)(N - 1)));
    fft(re, im, N);
    size_t best = 1;
    double bm = 0;
    for (size_t k = 1; k < N / 2; k++) {
        double m = re[k] * re[k] + im[k] * im[k];
        if (m > bm) { bm = m; best = k; }
    }
    double a = log(re[best - 1] * re[best - 1] + im[best - 1] * im[best - 1] + 1e-30) * 0.5;
    double b = log(bm + 1e-30) * 0.5;
    double c = log(re[best + 1] * re[best + 1] + im[best + 1] * im[best + 1] + 1e-30) * 0.5;
    double d = a - 2 * b + c;
    double delta = d != 0 ? 0.5 * (a - c) / d : 0;
    free(re); free(im);
    return ((double)best + delta) * rate / (double)N;
}
