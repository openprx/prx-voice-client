//! Translation strings of translated calls (provider problems, the headphone warning, captions, latency and counters in the debug view), English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. The keys of
//! `call.issue.provider.*` are chosen from the typed `ProviderIssue` of a call issue.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    (
        "call.issue.provider.model_not_installed",
        "A model is not installed. Install it, then start again.",
    ),
    (
        "call.issue.provider.model_installing",
        "A model is still being installed. Wait until it is done.",
    ),
    (
        "call.issue.provider.model_damaged",
        "An installed model is damaged. Install it again.",
    ),
    (
        "call.issue.provider.runtime_not_built_in",
        "This build cannot run a model the call needs. Use a build with the speech runtimes.",
    ),
    (
        "call.issue.provider.voice_profile_missing",
        "Your voice profile is missing. Record or install it.",
    ),
    (
        "call.issue.provider.voice_not_prepared",
        "Your voice has not been prepared for the current speech model. Prepare it.",
    ),
    (
        "call.issue.provider.voice_preset_missing",
        "The voice for the other person's speech is missing. Install a preset voice and select it.",
    ),
    (
        "call.issue.provider.server_unreachable",
        "The translation server does not answer. Start it, then start again.",
    ),
    (
        "call.issue.provider.unsupported",
        "A provider cannot handle this language pair or live use. Choose another provider.",
    ),
    (
        "call.issue.provider.offline",
        "Offline mode blocks a provider the call needs. Allow it or choose a local one.",
    ),
    (
        "call.issue.provider.setting",
        "A setting is wrong. Correct it in the settings.",
    ),
    (
        "call.issue.provider.start_failed",
        "A provider could not start.",
    ),
    (
        "call.issue.speaker_output",
        "The sound goes out through the built-in speakers, so the microphone would pick up the translation and translate it again. Use headphones.",
    ),
    (
        "call.issue.local_output_unavailable",
        "The local output could not be opened. The call application plays the other person itself, so nothing is translated for you.",
    ),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    (
        "call.issue.provider.model_not_installed",
        "\u{6709}\u{6a21}\u{578b}\u{5c1a}\u{672a}\u{5b89}\u{88c5}\u{3002}\u{8bf7}\u{5148}\u{5b89}\u{88c5}\u{ff0c}\u{7136}\u{540e}\u{91cd}\u{65b0}\u{5f00}\u{59cb}\u{3002}",
    ),
    (
        "call.issue.provider.model_installing",
        "\u{6709}\u{6a21}\u{578b}\u{4ecd}\u{5728}\u{5b89}\u{88c5}\u{4e2d}\u{3002}\u{8bf7}\u{7b49}\u{5f85}\u{5b8c}\u{6210}\u{3002}",
    ),
    (
        "call.issue.provider.model_damaged",
        "\u{5df2}\u{5b89}\u{88c5}\u{7684}\u{6a21}\u{578b}\u{5df2}\u{635f}\u{574f}\u{3002}\u{8bf7}\u{91cd}\u{65b0}\u{5b89}\u{88c5}\u{3002}",
    ),
    (
        "call.issue.provider.runtime_not_built_in",
        "\u{6b64}\u{7248}\u{672c}\u{65e0}\u{6cd5}\u{8fd0}\u{884c}\u{901a}\u{8bdd}\u{6240}\u{9700}\u{7684}\u{6a21}\u{578b}\u{3002}\u{8bf7}\u{4f7f}\u{7528}\u{5e26}\u{6709}\u{8bed}\u{97f3}\u{8fd0}\u{884c}\u{5e93}\u{7684}\u{7248}\u{672c}\u{3002}",
    ),
    (
        "call.issue.provider.voice_profile_missing",
        "\u{627e}\u{4e0d}\u{5230}\u{4f60}\u{7684}\u{58f0}\u{97f3}\u{6863}\u{6848}\u{3002}\u{8bf7}\u{5f55}\u{5236}\u{6216}\u{5b89}\u{88c5}\u{5b83}\u{3002}",
    ),
    (
        "call.issue.provider.voice_not_prepared",
        "\u{4f60}\u{7684}\u{58f0}\u{97f3}\u{5c1a}\u{672a}\u{4e3a}\u{5f53}\u{524d}\u{8bed}\u{97f3}\u{6a21}\u{578b}\u{51c6}\u{5907}\u{597d}\u{3002}\u{8bf7}\u{5148}\u{51c6}\u{5907}\u{3002}",
    ),
    (
        "call.issue.provider.voice_preset_missing",
        "\u{5bf9}\u{65b9}\u{8bed}\u{97f3}\u{6240}\u{7528}\u{7684}\u{58f0}\u{97f3}\u{7f3a}\u{5931}\u{3002}\u{8bf7}\u{5b89}\u{88c5}\u{4e00}\u{4e2a}\u{9884}\u{8bbe}\u{58f0}\u{97f3}\u{5e76}\u{9009}\u{4e2d}\u{5b83}\u{3002}",
    ),
    (
        "call.issue.provider.server_unreachable",
        "\u{7ffb}\u{8bd1}\u{670d}\u{52a1}\u{5668}\u{6ca1}\u{6709}\u{54cd}\u{5e94}\u{3002}\u{8bf7}\u{542f}\u{52a8}\u{5b83}\u{ff0c}\u{7136}\u{540e}\u{91cd}\u{65b0}\u{5f00}\u{59cb}\u{3002}",
    ),
    (
        "call.issue.provider.unsupported",
        "\u{6709}\u{63d0}\u{4f9b}\u{65b9}\u{4e0d}\u{652f}\u{6301}\u{6b64}\u{8bed}\u{8a00}\u{5bf9}\u{6216}\u{5b9e}\u{65f6}\u{4f7f}\u{7528}\u{3002}\u{8bf7}\u{9009}\u{62e9}\u{5176}\u{4ed6}\u{63d0}\u{4f9b}\u{65b9}\u{3002}",
    ),
    (
        "call.issue.provider.offline",
        "\u{79bb}\u{7ebf}\u{6a21}\u{5f0f}\u{963b}\u{6b62}\u{4e86}\u{901a}\u{8bdd}\u{6240}\u{9700}\u{7684}\u{63d0}\u{4f9b}\u{65b9}\u{3002}\u{8bf7}\u{5141}\u{8bb8}\u{5b83}\u{6216}\u{9009}\u{62e9}\u{672c}\u{5730}\u{63d0}\u{4f9b}\u{65b9}\u{3002}",
    ),
    (
        "call.issue.provider.setting",
        "\u{6709}\u{8bbe}\u{7f6e}\u{4e0d}\u{6b63}\u{786e}\u{3002}\u{8bf7}\u{5728}\u{8bbe}\u{7f6e}\u{4e2d}\u{66f4}\u{6b63}\u{3002}",
    ),
    (
        "call.issue.provider.start_failed",
        "\u{6709}\u{63d0}\u{4f9b}\u{65b9}\u{65e0}\u{6cd5}\u{542f}\u{52a8}\u{3002}",
    ),
    (
        "call.issue.speaker_output",
        "\u{58f0}\u{97f3}\u{4ece}\u{5185}\u{7f6e}\u{626c}\u{58f0}\u{5668}\u{64ad}\u{51fa}\u{ff0c}\u{9ea6}\u{514b}\u{98ce}\u{4f1a}\u{6536}\u{5230}\u{8bd1}\u{6587}\u{5e76}\u{518d}\u{6b21}\u{7ffb}\u{8bd1}\u{3002}\u{8bf7}\u{4f7f}\u{7528}\u{8033}\u{673a}\u{3002}",
    ),
    (
        "call.issue.local_output_unavailable",
        "\u{65e0}\u{6cd5}\u{6253}\u{5f00}\u{672c}\u{5730}\u{8f93}\u{51fa}\u{3002}\u{901a}\u{8bdd}\u{5e94}\u{7528}\u{4f1a}\u{81ea}\u{884c}\u{64ad}\u{653e}\u{5bf9}\u{65b9}\u{7684}\u{58f0}\u{97f3}\u{ff0c}\u{56e0}\u{6b64}\u{4e0d}\u{4f1a}\u{4e3a}\u{4f60}\u{7ffb}\u{8bd1}\u{3002}",
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
