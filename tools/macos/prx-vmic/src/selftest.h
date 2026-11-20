// Loopback self-test: sink -> driver -> virtual microphone, with objective numbers.
#ifndef PRX_SELFTEST_H
#define PRX_SELFTEST_H
// Returns 0 on PASS, 3 on FAIL, 2 if a device is missing, 1 on other errors.
int selftest_run(const char *sink_key, const char *mic_key, const char *wav_out);
// Prints peak / RMS / dominant frequency of a WAV file.
int analyze_file(const char *path);
#endif
