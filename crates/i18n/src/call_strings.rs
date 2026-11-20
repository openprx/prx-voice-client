//! Translation strings of the call-mode control (debug view and call issues), English and Simplified Chinese.
//!
//! The two tables hold the same keys; a test checks it. Keys are `call.<area>.<item>`;
//! the issue messages are `call.issue.<code>` and are chosen by the application
//! from the typed error code of the coordinator.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("call.title", "Call mode (temporary control)"),
    ("call.start", "Start call mode"),
    ("call.stop", "Stop call mode"),
    ("call.pause", "Pause"),
    ("call.resume", "Resume"),
    ("call.reapply", "Use PRX Voice Microphone again"),
    ("call.state", "State"),
    ("call.state.idle", "Idle"),
    ("call.state.initializing", "Starting"),
    ("call.state.audio_ready", "Audio ready (passthrough)"),
    ("call.state.translating", "Translating"),
    ("call.state.paused", "Paused"),
    ("call.state.degraded", "Degraded"),
    ("call.state.recovering", "Recovering"),
    ("call.state.stopping", "Stopping"),
    ("call.they_hear", "The other person hears"),
    ("call.you_hear", "You hear"),
    ("call.hears.translation", "the translation"),
    ("call.hears.original", "the original voice"),
    ("call.hears.nothing", "nothing"),
    (
        "call.hears.original_by_app",
        "the original, played by the call application",
    ),
    ("call.microphone", "Microphone"),
    ("call.virtual_sink", "Virtual microphone sink"),
    ("call.app", "Call application"),
    ("call.default_input", "Default input"),
    ("call.default_input.ours", "PRX Voice Microphone"),
    ("call.default_input.displaced", "changed to {0}"),
    ("call.default_input.off", "not in call mode"),
    ("call.activity", "Call detection"),
    ("call.activity.unknown", "not read yet"),
    ("call.activity.unavailable", "not available"),
    (
        "call.activity.not_running",
        "the application is not running",
    ),
    ("call.activity.idle", "the application is not recording"),
    (
        "call.activity.on_ours",
        "a call is using the PRX Voice Microphone",
    ),
    (
        "call.activity.on_other",
        "a call is using another microphone: {0}",
    ),
    ("call.path.outgoing", "Outgoing path (microphone)"),
    ("call.path.incoming", "Incoming path (call application tap)"),
    ("call.path.device", "Device"),
    ("call.path.off", "not running"),
    ("call.path.level", "level {0} dBFS, peak {1} dBFS"),
    (
        "call.path.counters",
        "frames {0}, overruns {1}, drops {2}, underruns {3}",
    ),
    ("call.issues", "Issues"),
    ("call.no_issues", "No issues"),
    ("call.last_error", "Last error"),
    (
        "call.refused.assistant_running",
        "Call mode cannot start while an assistant session is running. Stop the session first.",
    ),
    (
        "call.refused.call_running",
        "The assistant cannot start while call mode is on. Stop call mode first.",
    ),
    (
        "call.issue.not_in_bundle",
        "Run PRX Voice from the app bundle: outside it the call application's audio cannot be captured.",
    ),
    ("call.issue.no_call_app", "No call application is selected."),
    (
        "call.issue.mic_consent_denied",
        "Microphone access is denied. Allow it in System Settings, Privacy & Security, Microphone.",
    ),
    (
        "call.issue.mic_consent_unavailable",
        "Microphone access could not be requested. Start PRX Voice from its app bundle and answer the system prompt.",
    ),
    (
        "call.issue.capture_consent_denied",
        "System audio recording is denied. Allow it in System Settings, Privacy & Security, Screen & System Audio Recording.",
    ),
    (
        "call.issue.capture_consent_unavailable",
        "System audio recording could not be requested. Start PRX Voice from its app bundle and answer the system prompt.",
    ),
    (
        "call.issue.driver_missing",
        "The PRX Voice microphone driver is not installed.",
    ),
    (
        "call.issue.driver_not_loaded",
        "The microphone driver is installed but the audio service has not loaded it.",
    ),
    (
        "call.issue.driver_stale",
        "The installed microphone driver is out of date. Update it.",
    ),
    (
        "call.issue.driver_wrong_variant",
        "The installed microphone driver is not the variant this call application needs.",
    ),
    (
        "call.issue.driver_unusable",
        "The microphone driver cannot be checked.",
    ),
    (
        "call.issue.no_microphone",
        "No usable microphone was found.",
    ),
    (
        "call.issue.endpoint_open_failed",
        "An audio device could not be opened.",
    ),
    (
        "call.issue.target_not_running",
        "The call application is not running.",
    ),
    (
        "call.issue.routing_journal_present",
        "A record of an earlier call mode is in the way. Restart PRX Voice to clean it up.",
    ),
    (
        "call.issue.routing_switch_failed",
        "The system microphone could not be switched.",
    ),
    (
        "call.issue.routing_unavailable",
        "The system audio settings cannot be read or changed.",
    ),
    (
        "call.issue.virtual_mic_lost",
        "The virtual microphone disappeared. Your microphone was restored.",
    ),
    (
        "call.issue.default_input_displaced",
        "The other person can't hear translations: the system microphone was changed.",
    ),
    (
        "call.issue.capture_no_signal",
        "No sound from the call application: check System Audio Recording permission.",
    ),
    (
        "call.issue.capture_stalled",
        "The capture of the call application stopped delivering audio.",
    ),
    (
        "call.issue.target_gone",
        "The call application closed; waiting for it.",
    ),
    ("call.issue.endpoint_lost", "An audio device was lost."),
    (
        "call.issue.microphone_unavailable",
        "No microphone is available now; capture resumes when one appears.",
    ),
    ("call.issue.microphone_changed", "The microphone changed."),
    (
        "call.issue.recovery_timeout",
        "Audio devices did not come back in time.",
    ),
    (
        "call.issue.call_on_other_microphone",
        "The call application uses another microphone; the other person hears it instead of PRX Voice.",
    ),
    (
        "call.issue.activity_unavailable",
        "Call detection is not available.",
    ),
    (
        "call.issue.direction_degraded",
        "A direction runs in a reduced way.",
    ),
    (
        "call.issue.restore_failed",
        "Your previous microphone could not be restored; it will be restored at the next start.",
    ),
    (
        "call.issue.stop_leak",
        "Something was still running after the stop.",
    ),
    (
        "call.issue.command_rejected",
        "That is not possible in the current state.",
    ),
    (
        "call.issue.assistant_active",
        "Call mode cannot start while an assistant session is running.",
    ),
    (
        "call.issue.translation_unavailable",
        "Translation is not available in this build.",
    ),
    ("call.issue.internal", "An internal error occurred."),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    (
        "call.title",
        "\u{901a}\u{8bdd}\u{6a21}\u{5f0f}\u{ff08}\u{4e34}\u{65f6}\u{63a7}\u{5236}\u{ff09}",
    ),
    (
        "call.start",
        "\u{5f00}\u{59cb}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}",
    ),
    (
        "call.stop",
        "\u{505c}\u{6b62}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}",
    ),
    ("call.pause", "\u{6682}\u{505c}"),
    ("call.resume", "\u{7ee7}\u{7eed}"),
    (
        "call.reapply",
        "\u{91cd}\u{65b0}\u{4f7f}\u{7528} PRX Voice \u{9ea6}\u{514b}\u{98ce}",
    ),
    ("call.state", "\u{72b6}\u{6001}"),
    ("call.state.idle", "\u{7a7a}\u{95f2}"),
    (
        "call.state.initializing",
        "\u{6b63}\u{5728}\u{542f}\u{52a8}",
    ),
    (
        "call.state.audio_ready",
        "\u{97f3}\u{9891}\u{5c31}\u{7eea}\u{ff08}\u{76f4}\u{901a}\u{ff09}",
    ),
    ("call.state.translating", "\u{7ffb}\u{8bd1}\u{4e2d}"),
    ("call.state.paused", "\u{5df2}\u{6682}\u{505c}"),
    ("call.state.degraded", "\u{964d}\u{7ea7}\u{8fd0}\u{884c}"),
    ("call.state.recovering", "\u{6b63}\u{5728}\u{6062}\u{590d}"),
    ("call.state.stopping", "\u{6b63}\u{5728}\u{505c}\u{6b62}"),
    ("call.they_hear", "\u{5bf9}\u{65b9}\u{542c}\u{5230}"),
    ("call.you_hear", "\u{4f60}\u{542c}\u{5230}"),
    ("call.hears.translation", "\u{8bd1}\u{6587}"),
    ("call.hears.original", "\u{539f}\u{58f0}"),
    ("call.hears.nothing", "\u{65e0}\u{58f0}"),
    (
        "call.hears.original_by_app",
        "\u{539f}\u{58f0}\u{ff08}\u{7531}\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{64ad}\u{653e}\u{ff09}",
    ),
    ("call.microphone", "\u{9ea6}\u{514b}\u{98ce}"),
    (
        "call.virtual_sink",
        "\u{865a}\u{62df}\u{9ea6}\u{514b}\u{98ce}\u{8f93}\u{5165}\u{7aef}",
    ),
    ("call.app", "\u{901a}\u{8bdd}\u{5e94}\u{7528}"),
    (
        "call.default_input",
        "\u{9ed8}\u{8ba4}\u{8f93}\u{5165}\u{8bbe}\u{5907}",
    ),
    (
        "call.default_input.ours",
        "PRX Voice \u{9ea6}\u{514b}\u{98ce}",
    ),
    (
        "call.default_input.displaced",
        "\u{5df2}\u{88ab}\u{66f4}\u{6539}\u{4e3a} {0}",
    ),
    (
        "call.default_input.off",
        "\u{672a}\u{5904}\u{4e8e}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}",
    ),
    ("call.activity", "\u{901a}\u{8bdd}\u{68c0}\u{6d4b}"),
    ("call.activity.unknown", "\u{5c1a}\u{672a}\u{8bfb}\u{53d6}"),
    ("call.activity.unavailable", "\u{4e0d}\u{53ef}\u{7528}"),
    (
        "call.activity.not_running",
        "\u{5e94}\u{7528}\u{672a}\u{8fd0}\u{884c}",
    ),
    (
        "call.activity.idle",
        "\u{5e94}\u{7528}\u{672a}\u{5728}\u{5f55}\u{97f3}",
    ),
    (
        "call.activity.on_ours",
        "\u{901a}\u{8bdd}\u{6b63}\u{5728}\u{4f7f}\u{7528} PRX Voice \u{9ea6}\u{514b}\u{98ce}",
    ),
    (
        "call.activity.on_other",
        "\u{901a}\u{8bdd}\u{6b63}\u{5728}\u{4f7f}\u{7528}\u{5176}\u{4ed6}\u{9ea6}\u{514b}\u{98ce}\u{ff1a}{0}",
    ),
    (
        "call.path.outgoing",
        "\u{53d1}\u{51fa}\u{8def}\u{5f84}\u{ff08}\u{9ea6}\u{514b}\u{98ce}\u{ff09}",
    ),
    (
        "call.path.incoming",
        "\u{63a5}\u{6536}\u{8def}\u{5f84}\u{ff08}\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{91c7}\u{96c6}\u{ff09}",
    ),
    ("call.path.device", "\u{8bbe}\u{5907}"),
    ("call.path.off", "\u{672a}\u{8fd0}\u{884c}"),
    (
        "call.path.level",
        "\u{7535}\u{5e73} {0} dBFS\u{ff0c}\u{5cf0}\u{503c} {1} dBFS",
    ),
    (
        "call.path.counters",
        "\u{5e27} {0}\u{ff0c}\u{6ea2}\u{51fa} {1}\u{ff0c}\u{4e22}\u{5f03} {2}\u{ff0c}\u{6b20}\u{8f7d} {3}",
    ),
    ("call.issues", "\u{95ee}\u{9898}"),
    ("call.no_issues", "\u{6ca1}\u{6709}\u{95ee}\u{9898}"),
    (
        "call.last_error",
        "\u{6700}\u{8fd1}\u{7684}\u{9519}\u{8bef}",
    ),
    (
        "call.refused.assistant_running",
        "\u{52a9}\u{624b}\u{4f1a}\u{8bdd}\u{8fd0}\u{884c}\u{65f6}\u{65e0}\u{6cd5}\u{5f00}\u{59cb}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}\u{3002}\u{8bf7}\u{5148}\u{505c}\u{6b62}\u{4f1a}\u{8bdd}\u{3002}",
    ),
    (
        "call.refused.call_running",
        "\u{901a}\u{8bdd}\u{6a21}\u{5f0f}\u{5f00}\u{542f}\u{65f6}\u{65e0}\u{6cd5}\u{5f00}\u{59cb}\u{52a9}\u{624b}\u{4f1a}\u{8bdd}\u{3002}\u{8bf7}\u{5148}\u{505c}\u{6b62}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}\u{3002}",
    ),
    (
        "call.issue.not_in_bundle",
        "\u{8bf7}\u{4ece}\u{5e94}\u{7528}\u{5305}\u{8fd0}\u{884c} PRX Voice\u{ff1a}\u{5426}\u{5219}\u{65e0}\u{6cd5}\u{91c7}\u{96c6}\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{7684}\u{58f0}\u{97f3}\u{3002}",
    ),
    (
        "call.issue.no_call_app",
        "\u{672a}\u{9009}\u{62e9}\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{3002}",
    ),
    (
        "call.issue.mic_consent_denied",
        "\u{9ea6}\u{514b}\u{98ce}\u{6743}\u{9650}\u{88ab}\u{62d2}\u{7edd}\u{3002}\u{8bf7}\u{5728}\u{7cfb}\u{7edf}\u{8bbe}\u{7f6e}\u{7684}\u{201c}\u{9690}\u{79c1}\u{4e0e}\u{5b89}\u{5168}\u{6027}\u{201d}\u{4e2d}\u{5141}\u{8bb8}\u{9ea6}\u{514b}\u{98ce}\u{3002}",
    ),
    (
        "call.issue.mic_consent_unavailable",
        "\u{65e0}\u{6cd5}\u{8bf7}\u{6c42}\u{9ea6}\u{514b}\u{98ce}\u{6743}\u{9650}\u{3002}\u{8bf7}\u{4ece}\u{5e94}\u{7528}\u{5305}\u{542f}\u{52a8} PRX Voice \u{5e76}\u{56de}\u{5e94}\u{7cfb}\u{7edf}\u{63d0}\u{793a}\u{3002}",
    ),
    (
        "call.issue.capture_consent_denied",
        "\u{7cfb}\u{7edf}\u{97f3}\u{9891}\u{5f55}\u{5236}\u{6743}\u{9650}\u{88ab}\u{62d2}\u{7edd}\u{3002}\u{8bf7}\u{5728}\u{7cfb}\u{7edf}\u{8bbe}\u{7f6e}\u{7684}\u{201c}\u{9690}\u{79c1}\u{4e0e}\u{5b89}\u{5168}\u{6027}\u{201d}\u{4e2d}\u{5141}\u{8bb8}\u{3002}",
    ),
    (
        "call.issue.capture_consent_unavailable",
        "\u{65e0}\u{6cd5}\u{8bf7}\u{6c42}\u{7cfb}\u{7edf}\u{97f3}\u{9891}\u{5f55}\u{5236}\u{6743}\u{9650}\u{3002}\u{8bf7}\u{4ece}\u{5e94}\u{7528}\u{5305}\u{542f}\u{52a8} PRX Voice \u{5e76}\u{56de}\u{5e94}\u{7cfb}\u{7edf}\u{63d0}\u{793a}\u{3002}",
    ),
    (
        "call.issue.driver_missing",
        "\u{672a}\u{5b89}\u{88c5} PRX Voice \u{9ea6}\u{514b}\u{98ce}\u{9a71}\u{52a8}\u{3002}",
    ),
    (
        "call.issue.driver_not_loaded",
        "\u{9ea6}\u{514b}\u{98ce}\u{9a71}\u{52a8}\u{5df2}\u{5b89}\u{88c5}\u{ff0c}\u{4f46}\u{97f3}\u{9891}\u{670d}\u{52a1}\u{5c1a}\u{672a}\u{52a0}\u{8f7d}\u{3002}",
    ),
    (
        "call.issue.driver_stale",
        "\u{5df2}\u{5b89}\u{88c5}\u{7684}\u{9ea6}\u{514b}\u{98ce}\u{9a71}\u{52a8}\u{7248}\u{672c}\u{8fc7}\u{65e7}\u{3002}\u{8bf7}\u{66f4}\u{65b0}\u{3002}",
    ),
    (
        "call.issue.driver_wrong_variant",
        "\u{5df2}\u{5b89}\u{88c5}\u{7684}\u{9ea6}\u{514b}\u{98ce}\u{9a71}\u{52a8}\u{4e0d}\u{662f}\u{8be5}\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{6240}\u{9700}\u{7684}\u{7248}\u{672c}\u{3002}",
    ),
    (
        "call.issue.driver_unusable",
        "\u{65e0}\u{6cd5}\u{68c0}\u{67e5}\u{9ea6}\u{514b}\u{98ce}\u{9a71}\u{52a8}\u{3002}",
    ),
    (
        "call.issue.no_microphone",
        "\u{6ca1}\u{6709}\u{53ef}\u{7528}\u{7684}\u{9ea6}\u{514b}\u{98ce}\u{3002}",
    ),
    (
        "call.issue.endpoint_open_failed",
        "\u{65e0}\u{6cd5}\u{6253}\u{5f00}\u{97f3}\u{9891}\u{8bbe}\u{5907}\u{3002}",
    ),
    (
        "call.issue.target_not_running",
        "\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{672a}\u{8fd0}\u{884c}\u{3002}",
    ),
    (
        "call.issue.routing_journal_present",
        "\u{5b58}\u{5728}\u{4e0a}\u{6b21}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}\u{7684}\u{9057}\u{7559}\u{8bb0}\u{5f55}\u{3002}\u{8bf7}\u{91cd}\u{542f} PRX Voice \u{4ee5}\u{6e05}\u{7406}\u{3002}",
    ),
    (
        "call.issue.routing_switch_failed",
        "\u{65e0}\u{6cd5}\u{5207}\u{6362}\u{7cfb}\u{7edf}\u{9ea6}\u{514b}\u{98ce}\u{3002}",
    ),
    (
        "call.issue.routing_unavailable",
        "\u{65e0}\u{6cd5}\u{8bfb}\u{53d6}\u{6216}\u{66f4}\u{6539}\u{7cfb}\u{7edf}\u{97f3}\u{9891}\u{8bbe}\u{7f6e}\u{3002}",
    ),
    (
        "call.issue.virtual_mic_lost",
        "\u{865a}\u{62df}\u{9ea6}\u{514b}\u{98ce}\u{5df2}\u{6d88}\u{5931}\u{3002}\u{5df2}\u{6062}\u{590d}\u{4f60}\u{7684}\u{9ea6}\u{514b}\u{98ce}\u{3002}",
    ),
    (
        "call.issue.default_input_displaced",
        "\u{5bf9}\u{65b9}\u{542c}\u{4e0d}\u{5230}\u{8bd1}\u{6587}\u{ff1a}\u{7cfb}\u{7edf}\u{9ea6}\u{514b}\u{98ce}\u{5df2}\u{88ab}\u{66f4}\u{6539}\u{3002}",
    ),
    (
        "call.issue.capture_no_signal",
        "\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{6ca1}\u{6709}\u{58f0}\u{97f3}\u{ff1a}\u{8bf7}\u{68c0}\u{67e5}\u{7cfb}\u{7edf}\u{97f3}\u{9891}\u{5f55}\u{5236}\u{6743}\u{9650}\u{3002}",
    ),
    (
        "call.issue.capture_stalled",
        "\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{7684}\u{91c7}\u{96c6}\u{505c}\u{6b62}\u{4e86}\u{3002}",
    ),
    (
        "call.issue.target_gone",
        "\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{5df2}\u{5173}\u{95ed}\u{ff0c}\u{6b63}\u{5728}\u{7b49}\u{5f85}\u{3002}",
    ),
    (
        "call.issue.endpoint_lost",
        "\u{97f3}\u{9891}\u{8bbe}\u{5907}\u{5df2}\u{4e22}\u{5931}\u{3002}",
    ),
    (
        "call.issue.microphone_unavailable",
        "\u{5f53}\u{524d}\u{6ca1}\u{6709}\u{53ef}\u{7528}\u{7684}\u{9ea6}\u{514b}\u{98ce}\u{ff1b}\u{51fa}\u{73b0}\u{540e}\u{4f1a}\u{81ea}\u{52a8}\u{6062}\u{590d}\u{3002}",
    ),
    (
        "call.issue.microphone_changed",
        "\u{9ea6}\u{514b}\u{98ce}\u{5df2}\u{66f4}\u{6362}\u{3002}",
    ),
    (
        "call.issue.recovery_timeout",
        "\u{97f3}\u{9891}\u{8bbe}\u{5907}\u{672a}\u{80fd}\u{53ca}\u{65f6}\u{6062}\u{590d}\u{3002}",
    ),
    (
        "call.issue.call_on_other_microphone",
        "\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{4f7f}\u{7528}\u{4e86}\u{5176}\u{4ed6}\u{9ea6}\u{514b}\u{98ce}\u{ff1b}\u{5bf9}\u{65b9}\u{542c}\u{5230}\u{7684}\u{662f}\u{5b83}\u{800c}\u{4e0d}\u{662f} PRX Voice\u{3002}",
    ),
    (
        "call.issue.activity_unavailable",
        "\u{901a}\u{8bdd}\u{68c0}\u{6d4b}\u{4e0d}\u{53ef}\u{7528}\u{3002}",
    ),
    (
        "call.issue.direction_degraded",
        "\u{67d0}\u{4e2a}\u{65b9}\u{5411}\u{964d}\u{7ea7}\u{8fd0}\u{884c}\u{3002}",
    ),
    (
        "call.issue.restore_failed",
        "\u{65e0}\u{6cd5}\u{6062}\u{590d}\u{4f60}\u{4e4b}\u{524d}\u{7684}\u{9ea6}\u{514b}\u{98ce}\u{ff1b}\u{5c06}\u{5728}\u{4e0b}\u{6b21}\u{542f}\u{52a8}\u{65f6}\u{6062}\u{590d}\u{3002}",
    ),
    (
        "call.issue.stop_leak",
        "\u{505c}\u{6b62}\u{540e}\u{4ecd}\u{6709}\u{90e8}\u{5206}\u{5728}\u{8fd0}\u{884c}\u{3002}",
    ),
    (
        "call.issue.command_rejected",
        "\u{5f53}\u{524d}\u{72b6}\u{6001}\u{4e0b}\u{65e0}\u{6cd5}\u{6267}\u{884c}\u{3002}",
    ),
    (
        "call.issue.assistant_active",
        "\u{52a9}\u{624b}\u{4f1a}\u{8bdd}\u{8fd0}\u{884c}\u{65f6}\u{65e0}\u{6cd5}\u{5f00}\u{59cb}\u{901a}\u{8bdd}\u{6a21}\u{5f0f}\u{3002}",
    ),
    (
        "call.issue.translation_unavailable",
        "\u{6b64}\u{7248}\u{672c}\u{4e0d}\u{63d0}\u{4f9b}\u{7ffb}\u{8bd1}\u{3002}",
    ),
    (
        "call.issue.internal",
        "\u{53d1}\u{751f}\u{5185}\u{90e8}\u{9519}\u{8bef}\u{3002}",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_have_the_same_keys_in_the_same_order() {
        let en: Vec<_> = EN.iter().map(|(k, _)| *k).collect();
        let zh: Vec<_> = ZH_CN.iter().map(|(k, _)| *k).collect();
        assert_eq!(en, zh);
    }

    #[test]
    fn keys_are_unique_and_no_value_is_empty() {
        let mut seen = std::collections::HashSet::new();
        for (k, v) in EN.iter().chain(ZH_CN) {
            assert!(!v.is_empty(), "{k}");
        }
        for (k, _) in EN {
            assert!(seen.insert(*k), "duplicate key {k}");
        }
    }
}
