// System sleep and wake notifications through IOKit, delivered on a private dispatch queue (no
// run loop and no main thread needed). C ABI consumed by crates/audio/src/routing/power.rs.

#ifndef PRXVC_POWER_H
#define PRXVC_POWER_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    PRXVC_POWER_WILL_SLEEP = 1,
    PRXVC_POWER_DID_WAKE = 2,
};

// Called on a private serial dispatch queue. It must only record the notification and return
// quickly: the system waits for it before it goes to sleep (this shim never vetoes sleep).
typedef void (*prxvc_power_cb)(int32_t kind, void *ctx);

typedef struct PrxvcPowerMonitor PrxvcPowerMonitor;

// Start the monitor. Returns NULL and sets `*status` (when non-NULL) on failure.
PrxvcPowerMonitor *prxvc_power_start(prxvc_power_cb cb, void *ctx, int32_t *status);

// Stop the monitor. When this returns no callback is running or will run, so `ctx` may be freed.
// Must not be called from inside the callback.
void prxvc_power_stop(PrxvcPowerMonitor *monitor);

#ifdef __cplusplus
}
#endif

#endif // PRXVC_POWER_H
