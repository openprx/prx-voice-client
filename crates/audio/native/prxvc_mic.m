// Microphone consent shim (see prxvc_mic.h).

#import <AVFoundation/AVFoundation.h>
#include "prxvc_mic.h"

int32_t prxvc_mic_permission_status(void) {
    switch ([AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio]) {
        case AVAuthorizationStatusAuthorized:
            return PRXVC_MIC_GRANTED;
        case AVAuthorizationStatusNotDetermined:
            return PRXVC_MIC_UNDETERMINED;
        default:
            return PRXVC_MIC_DENIED;
    }
}

int32_t prxvc_mic_bundle_declares_usage(void) {
    id value = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSMicrophoneUsageDescription"];
    return value != nil ? 1 : 0;
}

int32_t prxvc_mic_request(int32_t timeout_ms) {
    if (!prxvc_mic_bundle_declares_usage()) {
        return -2;
    }
    __block int32_t answer = -1;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio
                             completionHandler:^(BOOL granted) {
        answer = granted ? 1 : 0;
        dispatch_semaphore_signal(done);
    }];
    dispatch_time_t limit = dispatch_time(DISPATCH_TIME_NOW, (int64_t)timeout_ms * NSEC_PER_MSEC);
    if (dispatch_semaphore_wait(done, limit) != 0) {
        return -1;
    }
    return answer;
}
