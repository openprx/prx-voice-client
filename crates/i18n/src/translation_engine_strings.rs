//! Translation strings of the translation engine choice (system translation or a language model): the settings rows and their status lines, the provider problems that belong to it and the strings the Call translation window shows for the engine, English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. The keys of
//! `call.issue.provider.*` are chosen from the typed `ProviderIssue` of a call issue; the
//! `callwin.engine.*` and `callwin.action.*` keys are for the Call translation window to render.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    (
        "call.issue.provider.language_pack_missing",
        "System translation needs the language pack for this pair. Download it in System Settings, General, Language & Region, Translation Languages, or switch to the language model.",
    ),
    (
        "call.issue.provider.os_too_old",
        "System translation needs macOS 26 or later. Switch to the language model.",
    ),
    (
        "call.issue.provider.server_failed",
        "The local language model server could not start or stopped answering. See the reason below, or switch to system translation.",
    ),
    ("callwin.engine.label", "Translation engine"),
    ("callwin.engine.system", "System translation"),
    ("callwin.engine.llm", "Language model"),
    (
        "callwin.engine.no_context",
        "translates each sentence on its own",
    ),
    (
        "callwin.action.switch_to_llm",
        "Switch to the language model",
    ),
    (
        "callwin.action.switch_to_system",
        "Switch to system translation",
    ),
    (
        "callwin.action.open_translation_languages",
        "Open translation languages",
    ),
    ("settings.engine.title", "Translation engine"),
    (
        "settings.engine.desc",
        "Which engine turns what is said into the other language.",
    ),
    ("settings.engine.system", "System translation (default)"),
    (
        "settings.engine.system.desc",
        "Fast and light. Translates each sentence on its own, without the conversation as context.",
    ),
    ("settings.engine.llm", "Language model (Gemma 4 E4B)"),
    (
        "settings.engine.llm.desc",
        "Slower and uses about 5 GB of memory. Understands the conversation around a sentence.",
    ),
    ("settings.engine.status", "Status"),
    (
        "settings.engine.system.ready",
        "Ready: the language packs are installed.",
    ),
    (
        "settings.engine.system.checking",
        "Checking the language packs...",
    ),
    (
        "settings.engine.system.pack_missing",
        "A language pack is missing for {0}. Download it in System Settings, General, Language & Region, Translation Languages.",
    ),
    (
        "settings.engine.system.unsupported",
        "System translation does not support {0}.",
    ),
    (
        "settings.engine.system.os_too_old",
        "System translation needs macOS 26 or later. Choose the language model.",
    ),
    (
        "settings.engine.system.not_built",
        "This build does not include system translation. Choose the language model.",
    ),
    (
        "settings.engine.system.failed",
        "System translation failed: {0}",
    ),
    (
        "settings.engine.llm.stopped",
        "The model server is stopped. It starts when it is needed.",
    ),
    (
        "settings.engine.llm.starting",
        "Starting the model server...",
    ),
    (
        "settings.engine.llm.ready",
        "Ready: the model server is running.",
    ),
    (
        "settings.engine.llm.adopted",
        "Ready: using the model server already running at {0}.",
    ),
    (
        "settings.engine.llm.failed",
        "The model server could not start: {0}",
    ),
    (
        "settings.engine.llm.stopping",
        "Stopping the model server...",
    ),
    (
        "settings.engine.action.open_languages",
        "Open System Settings",
    ),
    ("settings.engine.action.retry", "Try again"),
    (
        "settings.engine.mirror",
        "Translation engine: {0}. Change it under Call translation.",
    ),
    (
        "assistant.issue.server_failed",
        "The local language model server could not start or stopped answering. See the reason below.",
    ),
    (
        "settings.engine.chip.stopped",
        "Stopped",
    ),
    (
        "settings.engine.chip.starting",
        "Starting",
    ),
    (
        "settings.engine.chip.ready",
        "Ready",
    ),
    (
        "settings.engine.chip.stopping",
        "Stopping",
    ),
    (
        "settings.engine.chip.failed",
        "Failed",
    ),
    (
        "settings.engine.chip.checking",
        "Checking",
    ),
    (
        "settings.engine.chip.pack_missing",
        "Pack missing",
    ),
    (
        "settings.engine.chip.unavailable",
        "Unavailable",
    ),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    (
        "call.issue.provider.language_pack_missing",
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{9700}\u{8981}\u{8fd9}\u{4e2a}\u{8bed}\u{8a00}\u{5bf9}\u{7684}\u{8bed}\u{8a00}\u{5305}\u{3002}\u{8bf7}\u{5728}\u{201c}\u{7cfb}\u{7edf}\u{8bbe}\u{7f6e} > \u{901a}\u{7528} > \u{8bed}\u{8a00}\u{4e0e}\u{5730}\u{533a} > \u{7ffb}\u{8bd1}\u{8bed}\u{8a00}\u{201d}\u{4e2d}\u{4e0b}\u{8f7d}\u{ff0c}\u{6216}\u{5207}\u{6362}\u{5230}\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{3002}",
    ),
    (
        "call.issue.provider.os_too_old",
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{9700}\u{8981} macOS 26 \u{6216}\u{66f4}\u{65b0}\u{7684}\u{7cfb}\u{7edf}\u{3002}\u{8bf7}\u{5207}\u{6362}\u{5230}\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{3002}",
    ),
    (
        "call.issue.provider.server_failed",
        "\u{672c}\u{5730}\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{670d}\u{52a1}\u{65e0}\u{6cd5}\u{542f}\u{52a8}\u{6216}\u{5df2}\u{4e0d}\u{518d}\u{54cd}\u{5e94}\u{3002}\u{8bf7}\u{67e5}\u{770b}\u{4e0b}\u{65b9}\u{7684}\u{539f}\u{56e0}\u{ff0c}\u{6216}\u{5207}\u{6362}\u{5230}\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{3002}",
    ),
    ("callwin.engine.label", "\u{7ffb}\u{8bd1}\u{5f15}\u{64ce}"),
    ("callwin.engine.system", "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}"),
    ("callwin.engine.llm", "\u{8bed}\u{8a00}\u{6a21}\u{578b}"),
    (
        "callwin.engine.no_context",
        "\u{9010}\u{53e5}\u{72ec}\u{7acb}\u{7ffb}\u{8bd1}",
    ),
    (
        "callwin.action.switch_to_llm",
        "\u{5207}\u{6362}\u{5230}\u{8bed}\u{8a00}\u{6a21}\u{578b}",
    ),
    (
        "callwin.action.switch_to_system",
        "\u{5207}\u{6362}\u{5230}\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}",
    ),
    (
        "callwin.action.open_translation_languages",
        "\u{6253}\u{5f00}\u{7ffb}\u{8bd1}\u{8bed}\u{8a00}",
    ),
    ("settings.engine.title", "\u{7ffb}\u{8bd1}\u{5f15}\u{64ce}"),
    (
        "settings.engine.desc",
        "\u{7531}\u{54ea}\u{4e2a}\u{5f15}\u{64ce}\u{628a}\u{8bf4}\u{7684}\u{8bdd}\u{7ffb}\u{8bd1}\u{6210}\u{53e6}\u{4e00}\u{79cd}\u{8bed}\u{8a00}\u{3002}",
    ),
    (
        "settings.engine.system",
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{ff08}\u{9ed8}\u{8ba4}\u{ff09}",
    ),
    (
        "settings.engine.system.desc",
        "\u{53c8}\u{5feb}\u{53c8}\u{7701}\u{8d44}\u{6e90}\u{3002}\u{9010}\u{53e5}\u{72ec}\u{7acb}\u{7ffb}\u{8bd1}\u{ff0c}\u{4e0d}\u{53c2}\u{8003}\u{5bf9}\u{8bdd}\u{4e0a}\u{4e0b}\u{6587}\u{3002}",
    ),
    (
        "settings.engine.llm",
        "\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{ff08}Gemma 4 E4B\u{ff09}",
    ),
    (
        "settings.engine.llm.desc",
        "\u{8f83}\u{6162}\u{ff0c}\u{5360}\u{7528}\u{7ea6} 5 GB \u{5185}\u{5b58}\u{3002}\u{80fd}\u{7ed3}\u{5408}\u{5bf9}\u{8bdd}\u{4e0a}\u{4e0b}\u{6587}\u{7406}\u{89e3}\u{53e5}\u{5b50}\u{3002}",
    ),
    ("settings.engine.status", "\u{72b6}\u{6001}"),
    (
        "settings.engine.system.ready",
        "\u{5c31}\u{7eea}\u{ff1a}\u{8bed}\u{8a00}\u{5305}\u{5df2}\u{5b89}\u{88c5}\u{3002}",
    ),
    (
        "settings.engine.system.checking",
        "\u{6b63}\u{5728}\u{68c0}\u{67e5}\u{8bed}\u{8a00}\u{5305}...",
    ),
    (
        "settings.engine.system.pack_missing",
        "\u{7f3a}\u{5c11} {0} \u{7684}\u{8bed}\u{8a00}\u{5305}\u{3002}\u{8bf7}\u{5728}\u{201c}\u{7cfb}\u{7edf}\u{8bbe}\u{7f6e} > \u{901a}\u{7528} > \u{8bed}\u{8a00}\u{4e0e}\u{5730}\u{533a} > \u{7ffb}\u{8bd1}\u{8bed}\u{8a00}\u{201d}\u{4e2d}\u{4e0b}\u{8f7d}\u{3002}",
    ),
    (
        "settings.engine.system.unsupported",
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{4e0d}\u{652f}\u{6301} {0}\u{3002}",
    ),
    (
        "settings.engine.system.os_too_old",
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{9700}\u{8981} macOS 26 \u{6216}\u{66f4}\u{65b0}\u{7684}\u{7cfb}\u{7edf}\u{3002}\u{8bf7}\u{9009}\u{62e9}\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{3002}",
    ),
    (
        "settings.engine.system.not_built",
        "\u{6b64}\u{7248}\u{672c}\u{4e0d}\u{5305}\u{542b}\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{3002}\u{8bf7}\u{9009}\u{62e9}\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{3002}",
    ),
    (
        "settings.engine.system.failed",
        "\u{7cfb}\u{7edf}\u{7ffb}\u{8bd1}\u{5931}\u{8d25}\u{ff1a}{0}",
    ),
    (
        "settings.engine.llm.stopped",
        "\u{6a21}\u{578b}\u{670d}\u{52a1}\u{5df2}\u{505c}\u{6b62}\u{ff0c}\u{9700}\u{8981}\u{65f6}\u{4f1a}\u{81ea}\u{52a8}\u{542f}\u{52a8}\u{3002}",
    ),
    (
        "settings.engine.llm.starting",
        "\u{6b63}\u{5728}\u{542f}\u{52a8}\u{6a21}\u{578b}\u{670d}\u{52a1}...",
    ),
    (
        "settings.engine.llm.ready",
        "\u{5c31}\u{7eea}\u{ff1a}\u{6a21}\u{578b}\u{670d}\u{52a1}\u{6b63}\u{5728}\u{8fd0}\u{884c}\u{3002}",
    ),
    (
        "settings.engine.llm.adopted",
        "\u{5c31}\u{7eea}\u{ff1a}\u{6b63}\u{5728}\u{4f7f}\u{7528} {0} \u{4e0a}\u{5df2}\u{5728}\u{8fd0}\u{884c}\u{7684}\u{6a21}\u{578b}\u{670d}\u{52a1}\u{3002}",
    ),
    (
        "settings.engine.llm.failed",
        "\u{6a21}\u{578b}\u{670d}\u{52a1}\u{65e0}\u{6cd5}\u{542f}\u{52a8}\u{ff1a}{0}",
    ),
    (
        "settings.engine.llm.stopping",
        "\u{6b63}\u{5728}\u{505c}\u{6b62}\u{6a21}\u{578b}\u{670d}\u{52a1}...",
    ),
    (
        "settings.engine.action.open_languages",
        "\u{6253}\u{5f00}\u{7cfb}\u{7edf}\u{8bbe}\u{7f6e}",
    ),
    ("settings.engine.action.retry", "\u{91cd}\u{8bd5}"),
    (
        "settings.engine.mirror",
        "\u{7ffb}\u{8bd1}\u{5f15}\u{64ce}\u{ff1a}{0}\u{3002}\u{53ef}\u{5728}\u{201c}\u{901a}\u{8bdd}\u{7ffb}\u{8bd1}\u{201d}\u{4e2d}\u{66f4}\u{6539}\u{3002}",
    ),
    (
        "assistant.issue.server_failed",
        "\u{672c}\u{5730}\u{8bed}\u{8a00}\u{6a21}\u{578b}\u{670d}\u{52a1}\u{65e0}\u{6cd5}\u{542f}\u{52a8}\u{6216}\u{5df2}\u{4e0d}\u{518d}\u{54cd}\u{5e94}\u{3002}\u{8bf7}\u{67e5}\u{770b}\u{4e0b}\u{65b9}\u{7684}\u{539f}\u{56e0}\u{3002}",
    ),
    (
        "settings.engine.chip.stopped",
        "\u{5df2}\u{505c}\u{6b62}",
    ),
    (
        "settings.engine.chip.starting",
        "\u{542f}\u{52a8}\u{4e2d}",
    ),
    (
        "settings.engine.chip.ready",
        "\u{5c31}\u{7eea}",
    ),
    (
        "settings.engine.chip.stopping",
        "\u{505c}\u{6b62}\u{4e2d}",
    ),
    (
        "settings.engine.chip.failed",
        "\u{5931}\u{8d25}",
    ),
    (
        "settings.engine.chip.checking",
        "\u{68c0}\u{67e5}\u{4e2d}",
    ),
    (
        "settings.engine.chip.pack_missing",
        "\u{7f3a}\u{5c11}\u{8bed}\u{8a00}\u{5305}",
    ),
    (
        "settings.engine.chip.unavailable",
        "\u{4e0d}\u{53ef}\u{7528}",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_hold_the_same_keys_in_the_same_order() {
        let en: Vec<&str> = EN.iter().map(|(k, _)| *k).collect();
        let zh: Vec<&str> = ZH_CN.iter().map(|(k, _)| *k).collect();
        assert_eq!(en, zh);
        assert!(ZH_CN.iter().all(|(_, v)| !v.is_empty()));
    }

    #[test]
    fn every_placeholder_of_the_english_text_is_in_the_chinese_text() {
        for ((key, en), (_, zh)) in EN.iter().zip(ZH_CN) {
            for p in ["{0}", "{1}"] {
                assert_eq!(en.contains(p), zh.contains(p), "{key}: {p}");
            }
        }
    }
}
