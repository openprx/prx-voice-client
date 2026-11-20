// Minimal WAV reader (PCM16/24/32, float32) and float32 mono writer.
#ifndef PRX_WAV_H
#define PRX_WAV_H
#include <stddef.h>

// Reads a WAV file, mixes it down to mono float. Caller frees *samples. Returns 0 on success.
int wav_read_mono(const char *path, float **samples, size_t *n, double *rate);
int wav_write_mono_f32(const char *path, const float *samples, size_t n, double rate);
// Linear-interpolation resampler; returns a new buffer (caller frees) of *out_n samples.
float *resample_linear(const float *in, size_t n, double from, double to, size_t *out_n);

#endif
