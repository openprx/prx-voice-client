/*
 * C ABI of libprxvc_apple_translation.dylib: Apple's on-device Translation framework behind a
 * blocking C interface. Implemented in Swift (the native directory); no Swift type crosses this boundary.
 *
 * Threading contract
 * ------------------
 *  - Every function is thread-safe and may be called from any thread except where noted.
 *  - prxat_translate and prxat_prepare BLOCK the calling thread until the result is known, the
 *    timeout passed or prxat_cancel was called. Never call them from a device (audio) callback.
 *  - Calls on one session are serialised: a second prxat_translate waits for the first.
 *  - prxat_cancel may be called from any thread while a call is blocked; the blocked call returns
 *    PRXAT_CANCELLED promptly. A cancel that arrives while no call is running is forgotten.
 *  - After PRXAT_TIMEOUT the session replaces its internal framework session, so the next call
 *    starts clean even if the framework never finished the abandoned one.
 *  - prxat_destroy must not race with a running call on the same session (cancel it and wait for
 *    the call to return first).
 *  - The framework delivers its work through the MAIN dispatch queue. The process's main thread must
 *    therefore run its run loop (an application does; a command-line program or a test runner calls
 *    prxat_run_main_loop on its main thread and does its work on another thread). Without it the
 *    framework would never answer; prxat_availability, prxat_create and prxat_prepare detect this
 *    in about a second and return PRXAT_NO_MAIN_LOOP. Calling a blocking function on the main
 *    thread itself returns the same code.
 *  - Strings are UTF-8, NUL terminated. A string returned through an out parameter is owned by the
 *    caller and released with prxat_free.
 */
#ifndef PRXVC_APPLE_TRANSLATION_H
#define PRXVC_APPLE_TRANSLATION_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PRXAT_ABI_VERSION 1

/* Result codes. 0 is success; every other code comes with a message when a buffer is given. */
#define PRXAT_OK 0
#define PRXAT_OS_TOO_OLD 1        /* needs macOS 26 (the framework is absent or too old) */
#define PRXAT_PACK_NOT_INSTALLED 2 /* the language pair is supported but its pack is not downloaded */
#define PRXAT_PAIR_UNSUPPORTED 3  /* the framework does not translate this pair */
#define PRXAT_TIMEOUT 4
#define PRXAT_CANCELLED 5
#define PRXAT_FRAMEWORK_ERROR 6   /* any other framework failure; see the message */
#define PRXAT_INVALID_ARGUMENT 7
#define PRXAT_NO_MAIN_LOOP 8      /* the main thread does not service the main dispatch queue */

/* Translation strategy of a session. */
#define PRXAT_STRATEGY_DEFAULT 0
#define PRXAT_STRATEGY_LOW_LATENCY 1  /* needs macOS 26.4; older systems use the default */
#define PRXAT_STRATEGY_HIGH_FIDELITY 2

typedef struct prxat_session prxat_session;

/* PRXAT_ABI_VERSION of the library, to check against the header the caller was built with. */
int32_t prxat_abi_version(void);

/* PRXAT_OK when this operating system can run the framework (macOS 26 or later). */
int32_t prxat_os_support(void);

/* Whether the pack for source -> target is installed. Language tags are BCP 47 ("zh-Hans", "en").
 * Returns PRXAT_OK (installed), PRXAT_PACK_NOT_INSTALLED, PRXAT_PAIR_UNSUPPORTED, PRXAT_OS_TOO_OLD,
 * PRXAT_TIMEOUT or PRXAT_FRAMEWORK_ERROR. msg (may be NULL) receives a short description. */
int32_t prxat_availability(const char *source, const char *target, char *msg, size_t msg_cap);

/* A session for the pair. Returns NULL and sets *code (may be NULL) and msg on failure; checks the
 * availability first, so a session exists only for an installed pair. */
prxat_session *prxat_create(const char *source, const char *target, int32_t strategy,
                            int32_t *code, char *msg, size_t msg_cap);

/* Load the pair's resources (absorbs the cold first call). */
int32_t prxat_prepare(prxat_session *session, int32_t timeout_ms, char *msg, size_t msg_cap);

/* Translate one text as a whole. On PRXAT_OK *out is a new string to release with prxat_free. */
int32_t prxat_translate(prxat_session *session, const char *text, int32_t timeout_ms, char **out,
                        char *msg, size_t msg_cap);

/* Make the blocked call of the session return PRXAT_CANCELLED. */
void prxat_cancel(prxat_session *session);

/* Run the main run loop until prxat_stop_main_loop; call it on the main thread only. */
void prxat_run_main_loop(void);
void prxat_stop_main_loop(void);

void prxat_destroy(prxat_session *session);
void prxat_free(char *text);

#ifdef __cplusplus
}
#endif
#endif
