// System sleep and wake notifications through IOKit. See prxvc_power.h.

#import <Foundation/Foundation.h>
#include <IOKit/IOKitLib.h>
#include <IOKit/IOMessage.h>
#include <IOKit/pwr_mgt/IOPMLib.h>
#include <dispatch/dispatch.h>
#include <stdlib.h>

#include "prxvc_power.h"

struct PrxvcPowerMonitor {
    io_connect_t root_port;
    IONotificationPortRef notify_port;
    io_object_t notifier;
    void *queue; // retained dispatch_queue_t (ARC forbids object fields in C structs)
    prxvc_power_cb cb;
    void *ctx;
};

static void prxvc_power_callback(void *refcon, io_service_t service, natural_t message_type,
                                 void *message_argument) {
    (void)service;
    PrxvcPowerMonitor *m = (PrxvcPowerMonitor *)refcon;
    switch (message_type) {
    case kIOMessageCanSystemSleep:
        // Never veto sleep.
        IOAllowPowerChange(m->root_port, (long)message_argument);
        break;
    case kIOMessageSystemWillSleep:
        m->cb(PRXVC_POWER_WILL_SLEEP, m->ctx);
        IOAllowPowerChange(m->root_port, (long)message_argument);
        break;
    case kIOMessageSystemHasPoweredOn:
        m->cb(PRXVC_POWER_DID_WAKE, m->ctx);
        break;
    default:
        break;
    }
}

PrxvcPowerMonitor *prxvc_power_start(prxvc_power_cb cb, void *ctx, int32_t *status) {
    if (status != NULL) {
        *status = 0;
    }
    if (cb == NULL) {
        if (status != NULL) {
            *status = -1;
        }
        return NULL;
    }
    PrxvcPowerMonitor *m = (PrxvcPowerMonitor *)calloc(1, sizeof(*m));
    if (m == NULL) {
        if (status != NULL) {
            *status = -1;
        }
        return NULL;
    }
    m->cb = cb;
    m->ctx = ctx;
    m->root_port = IORegisterForSystemPower(m, &m->notify_port, prxvc_power_callback, &m->notifier);
    if (m->root_port == MACH_PORT_NULL) {
        if (status != NULL) {
            *status = -2;
        }
        free(m);
        return NULL;
    }
    dispatch_queue_t queue =
        dispatch_queue_create("org.openprx.prx-voice-client.routing.power", DISPATCH_QUEUE_SERIAL);
    m->queue = (void *)CFBridgingRetain(queue);
    IONotificationPortSetDispatchQueue(m->notify_port, queue);
    return m;
}

void prxvc_power_stop(PrxvcPowerMonitor *m) {
    if (m == NULL) {
        return;
    }
    IONotificationPortSetDispatchQueue(m->notify_port, NULL);
    IODeregisterForSystemPower(&m->notifier);
    IOServiceClose(m->root_port);
    IONotificationPortDestroy(m->notify_port);
    // Wait for a callback that is already running on the queue.
    dispatch_queue_t queue = (dispatch_queue_t)CFBridgingRelease(m->queue);
    dispatch_sync(queue, ^{
    });
    free(m);
}
