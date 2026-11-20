// Signal analysis helpers for the loopback self-test.
#ifndef PRX_DSP_H
#define PRX_DSP_H
#include <stddef.h>

double dsp_rms(const float *x, size_t n);
double dsp_peak(const float *x, size_t n);
// Dominant frequency (Hz) of up to 65536 samples, Hann window, parabolic peak interpolation.
double dsp_dominant_freq(const float *x, size_t n, double rate);

#endif
