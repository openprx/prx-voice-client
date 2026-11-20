// `prx-vmic props`: prints the live properties of the PRX microphone as seen by other applications.
#ifndef PRX_PROPS_H
#define PRX_PROPS_H
// Returns 0 on success, 2 if the device is not installed.
int props_run(const char *key);
#endif
