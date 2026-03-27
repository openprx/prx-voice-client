#ifndef BridgingHeader_h
#define BridgingHeader_h

// PRX Voice Client Rust FFI
const char* prxvc_initialize(const char* config_json);
int prxvc_shutdown(void);
const char* prxvc_create_session(const char* config_json);
const char* prxvc_start_session(const char* session_id);
const char* prxvc_stop_session(const char* session_id);
const char* prxvc_interrupt(const char* session_id);
const char* prxvc_poll_events(void);
const char* prxvc_get_state(const char* session_id);
void prxvc_free_string(char* ptr);

#endif
