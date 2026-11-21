//! Strings of the Call translation window (the dedicated window that runs a translated call), English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. Issue messages and
//! the captions' vocabulary are shared with the debug control (`call_strings`, `translation_strings`).

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    (
        "callwin.title",
        "Call translation",
    ),
    (
        "callwin.state.idle",
        "Not running",
    ),
    (
        "callwin.state.loading",
        "Loading models and audio",
    ),
    (
        "callwin.state.audio_ready",
        "Audio ready, the original voices pass through",
    ),
    (
        "callwin.state.translating",
        "Translating both directions",
    ),
    (
        "callwin.state.paused",
        "Paused, the original voices pass through",
    ),
    (
        "callwin.state.degraded",
        "Reduced: {0}",
    ),
    (
        "callwin.state.degraded_plain",
        "Reduced, see the notes below",
    ),
    (
        "callwin.state.recovering",
        "Recovering the audio",
    ),
    (
        "callwin.state.stopping",
        "Stopping",
    ),
    (
        "callwin.start",
        "Start",
    ),
    (
        "callwin.stop",
        "Stop",
    ),
    (
        "callwin.languages",
        "Languages",
    ),
    (
        "callwin.own_language",
        "You speak",
    ),
    (
        "callwin.peer_language",
        "They speak",
    ),
    (
        "callwin.language_locked",
        "The languages change while call mode is off.",
    ),
    (
        "callwin.lane.out",
        "You, translated for them",
    ),
    (
        "callwin.lane.in",
        "Them, translated for you",
    ),
    (
        "callwin.lane.off",
        "Not running",
    ),
    (
        "callwin.notes",
        "Dropped filler: {0}. Skipped to keep up: {1}.",
    ),
    (
        "callwin.level",
        "Level {0} dBFS",
    ),
    (
        "callwin.latency",
        "Last clause: first translated audio {0} ms after the speech started",
    ),
    (
        "callwin.latency.none",
        "No clause has finished yet.",
    ),
    (
        "callwin.issues.title",
        "Notes",
    ),
    (
        "callwin.issues.none",
        "Nothing needs your attention.",
    ),
    (
        "callwin.action.end_assistant_and_start",
        "End assistant and start",
    ),
    (
        "callwin.action.open_permissions",
        "Open Permissions",
    ),
    (
        "callwin.action.open_audio",
        "Open Audio settings",
    ),
    (
        "callwin.action.open_speech",
        "Open Speech settings",
    ),
    (
        "callwin.debug_link",
        "Open the Call translation window",
    ),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    (
        "callwin.title",
        "\u{901a}\u{8bdd}\u{7ffb}\u{8bd1}",
    ),
    (
        "callwin.state.idle",
        "\u{672a}\u{8fd0}\u{884c}",
    ),
    (
        "callwin.state.loading",
        "\u{6b63}\u{5728}\u{52a0}\u{8f7d}\u{6a21}\u{578b}\u{548c}\u{97f3}\u{9891}",
    ),
    (
        "callwin.state.audio_ready",
        "\u{97f3}\u{9891}\u{5c31}\u{7eea}\u{ff0c}\u{76ee}\u{524d}\u{64ad}\u{653e}\u{539f}\u{58f0}",
    ),
    (
        "callwin.state.translating",
        "\u{6b63}\u{5728}\u{53cc}\u{5411}\u{7ffb}\u{8bd1}",
    ),
    (
        "callwin.state.paused",
        "\u{5df2}\u{6682}\u{505c}\u{ff0c}\u{76ee}\u{524d}\u{64ad}\u{653e}\u{539f}\u{58f0}",
    ),
    (
        "callwin.state.degraded",
        "\u{5df2}\u{964d}\u{7ea7}\u{ff1a}{0}",
    ),
    (
        "callwin.state.degraded_plain",
        "\u{5df2}\u{964d}\u{7ea7}\u{ff0c}\u{89c1}\u{4e0b}\u{65b9}\u{63d0}\u{793a}",
    ),
    (
        "callwin.state.recovering",
        "\u{6b63}\u{5728}\u{6062}\u{590d}\u{97f3}\u{9891}",
    ),
    (
        "callwin.state.stopping",
        "\u{6b63}\u{5728}\u{505c}\u{6b62}",
    ),
    (
        "callwin.start",
        "\u{5f00}\u{59cb}",
    ),
    (
        "callwin.stop",
        "\u{505c}\u{6b62}",
    ),
    (
        "callwin.languages",
        "\u{8bed}\u{8a00}",
    ),
    (
        "callwin.own_language",
        "\u{4f60}\u{8bf4}",
    ),
    (
        "callwin.peer_language",
        "\u{5bf9}\u{65b9}\u{8bf4}",
    ),
    (
        "callwin.language_locked",
        "\u{8bed}\u{8a00}\u{53ea}\u{80fd}\u{5728}\u{901a}\u{8bdd}\u{7ffb}\u{8bd1}\u{5173}\u{95ed}\u{65f6}\u{66f4}\u{6539}\u{3002}",
    ),
    (
        "callwin.lane.out",
        "\u{4f60}\u{8bf4}\u{7684}\u{8bdd}\u{ff0c}\u{8bd1}\u{7ed9}\u{5bf9}\u{65b9}",
    ),
    (
        "callwin.lane.in",
        "\u{5bf9}\u{65b9}\u{8bf4}\u{7684}\u{8bdd}\u{ff0c}\u{8bd1}\u{7ed9}\u{4f60}",
    ),
    (
        "callwin.lane.off",
        "\u{672a}\u{8fd0}\u{884c}",
    ),
    (
        "callwin.notes",
        "\u{5df2}\u{4e22}\u{5f03}\u{8bed}\u{6c14}\u{8bcd} {0} \u{6761}\u{ff0c}\u{4e3a}\u{8ddf}\u{4e0a}\u{8bed}\u{901f}\u{8df3}\u{8fc7} {1} \u{6761}\u{3002}",
    ),
    (
        "callwin.level",
        "\u{7535}\u{5e73} {0} dBFS",
    ),
    (
        "callwin.latency",
        "\u{6700}\u{8fd1}\u{4e00}\u{53e5}\u{ff1a}\u{5f00}\u{53e3}\u{540e} {0} \u{6beb}\u{79d2}\u{51fa}\u{73b0}\u{8bd1}\u{97f3}",
    ),
    (
        "callwin.latency.none",
        "\u{8fd8}\u{6ca1}\u{6709}\u{5b8c}\u{6210}\u{7684}\u{53e5}\u{5b50}\u{3002}",
    ),
    (
        "callwin.issues.title",
        "\u{63d0}\u{793a}",
    ),
    (
        "callwin.issues.none",
        "\u{6ca1}\u{6709}\u{9700}\u{8981}\u{5904}\u{7406}\u{7684}\u{4e8b}\u{9879}\u{3002}",
    ),
    (
        "callwin.action.end_assistant_and_start",
        "\u{7ed3}\u{675f}\u{52a9}\u{624b}\u{5e76}\u{5f00}\u{59cb}",
    ),
    (
        "callwin.action.open_permissions",
        "\u{6253}\u{5f00}\u{6743}\u{9650}\u{9875}",
    ),
    (
        "callwin.action.open_audio",
        "\u{6253}\u{5f00}\u{97f3}\u{9891}\u{8bbe}\u{7f6e}",
    ),
    (
        "callwin.action.open_speech",
        "\u{6253}\u{5f00}\u{8bed}\u{97f3}\u{8bbe}\u{7f6e}",
    ),
    (
        "callwin.debug_link",
        "\u{6253}\u{5f00}\u{901a}\u{8bdd}\u{7ffb}\u{8bd1}\u{7a97}\u{53e3}",
    ),
];
