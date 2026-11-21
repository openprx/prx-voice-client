//! Translation strings of the settings pages Call translation, Assistant, Audio and Voice (design 2.2), English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("settings.call.self_language", "My language"),
    (
        "settings.call.self_language.desc",
        "The language you speak in calls.",
    ),
    ("settings.call.peer_language", "Other party's language"),
    (
        "settings.call.peer_language.desc",
        "The language the other person speaks. It must differ from yours.",
    ),
    ("settings.call.lang.follow", "Match interface"),
    ("settings.call.lang.zh", "中文"),
    ("settings.call.lang.en", "English"),
    (
        "settings.call.languages_differ",
        "The two languages must differ. The change was not saved.",
    ),
    ("settings.call.target_app", "Call app"),
    (
        "settings.call.target_app.desc",
        "The app whose sound is translated.",
    ),
    ("settings.call.target_app.choose", "Choose when starting"),
    (
        "settings.reason.running_apps",
        "Waiting for the list of running apps that play audio.",
    ),
    ("settings.call.local_output", "What I hear"),
    (
        "settings.call.local_output.desc",
        "Choose what plays on your side during a translated call.",
    ),
    ("settings.call.output.translation", "Translation only"),
    (
        "settings.call.output.translation.desc",
        "You hear only the translated speech.",
    ),
    ("settings.call.output.both", "Original and translation"),
    (
        "settings.call.output.both.desc",
        "The original voice is lowered under the translation.",
    ),
    ("settings.call.output.original", "Original only"),
    (
        "settings.call.output.original.desc",
        "You hear the other person without translation.",
    ),
    ("settings.call.capture", "Outgoing input"),
    (
        "settings.call.capture.desc",
        "Hands-free translates whatever you say; push-to-talk translates only while you hold the key.",
    ),
    ("settings.call.capture.hands_free", "Hands-free"),
    ("settings.call.capture.ptt", "Push-to-talk"),
    (
        "settings.reason.ptt_needs_shortcut",
        "Set a push-to-talk shortcut first (see Shortcuts).",
    ),
    (
        "settings.call.half_duplex",
        "Pause outgoing translation while I listen",
    ),
    (
        "settings.call.half_duplex.desc",
        "Avoids feedback when the translation plays through speakers.",
    ),
    (
        "settings.reason.half_duplex",
        "Waiting for a setting of its own; for now it follows Echo protection in Audio.",
    ),
    (
        "settings.call.captions_source",
        "Show original text in captions",
    ),
    (
        "settings.call.captions_source.desc",
        "Show what was said next to its translation.",
    ),
    (
        "settings.call.disclosure",
        "Tell the other party the call is translated",
    ),
    (
        "settings.call.disclosure.desc",
        "Plays a short spoken notice, in their language, when you first speak in a call: the call is translated live and they hear a synthesized voice. It plays while your first sentence is being translated. Applies from the next start; the wording is in the config file.",
    ),
    ("settings.assistant.speak", "Speak replies aloud"),
    ("settings.assistant.language", "Assistant language"),
    (
        "settings.assistant.language.desc",
        "The language of the assistant session and of speech recognition.",
    ),
    (
        "settings.server_url.invalid",
        "Enter an address starting with http:// or https://.",
    ),
    ("settings.audio.microphone", "Microphone"),
    (
        "settings.input_device.desc",
        "The microphone PRX Voice listens to.",
    ),
    ("settings.audio.speakers", "Headphones / speakers"),
    (
        "settings.output_device.desc",
        "Where spoken replies and translations play.",
    ),
    ("settings.audio.default", "System default"),
    ("settings.audio.default_named", "System default · {0}"),
    ("settings.audio.not_connected", "{0} (not connected)"),
    ("settings.audio.uses", "In use: {0}"),
    (
        "settings.audio.reason.missing",
        "The chosen microphone is not connected. {0} is used instead.",
    ),
    (
        "settings.audio.reason.avoided",
        "The system default input is a virtual device, so {0} is used instead.",
    ),
    (
        "settings.audio.reason.own_virtual",
        "PRX Voice Microphone is the virtual device other apps listen to. It cannot be your microphone; choose a real one.",
    ),
    (
        "settings.audio.reason.no_device",
        "No microphone was found. Connect one and check again.",
    ),
    (
        "settings.audio.list_error",
        "The device list could not be read: {0}",
    ),
    ("settings.audio.route", "Output is"),
    (
        "settings.audio.route.desc",
        "Whether sound goes to headphones or speakers decides when the microphone is paused.",
    ),
    (
        "settings.audio.route.unknown",
        "Unknown (treated as speakers)",
    ),
    ("settings.audio.echo", "Echo protection"),
    (
        "settings.audio.echo.desc",
        "Keeps the microphone from hearing PRX Voice's own speech.",
    ),
    ("settings.audio.echo.auto", "Auto"),
    ("settings.audio.echo.always", "Always"),
    ("settings.audio.echo.off", "Off"),
    ("settings.audio.virtual_mic", "Virtual microphone"),
    (
        "settings.audio.virtual_mic.desc",
        "The microphone call apps use to hear your translated voice.",
    ),
    ("settings.audio.virtual_mic.manage", "Manage"),
    (
        "settings.audio.virtual_mic_uid",
        "Other virtual microphone (advanced)",
    ),
    (
        "settings.audio.virtual_mic_uid.desc",
        "Leave empty to use PRX Voice Microphone. To use another device, enter its sink UID.",
    ),
    ("settings.audio.test", "Test audio"),
    (
        "settings.audio.test.desc",
        "Play a test sound or check the microphone level.",
    ),
    (
        "settings.reason.audio_test",
        "Waiting for the audio test tools.",
    ),
    ("settings.voice.my_voice", "My voice"),
    (
        "settings.voice.my_voice.desc",
        "Record your voice so translations can be spoken in it.",
    ),
    ("settings.voice.not_set_up", "Not set up"),
    ("settings.voice.recorded", "Recorded"),
    ("settings.voice.set_up", "Set up my voice"),
    ("settings.voice.language_unknown", "language not recorded"),
    (
        "settings.voice.not_prepared",
        "not prepared for the current speech model",
    ),
    ("settings.voice.prepared_for", "prepared for model {0}"),
    (
        "settings.reason.enrolment",
        "Waiting for voice recording, a later step.",
    ),
    (
        "settings.voice.use_clone",
        "Speak my translations in my voice",
    ),
    (
        "settings.voice.use_clone.desc",
        "Your translated speech uses your recorded voice.",
    ),
    (
        "settings.voice.use_clone.needs_profile",
        "Available once your voice is set up.",
    ),
    ("settings.voice.incoming", "Voice for the other party"),
    (
        "settings.voice.incoming.desc",
        "The voice used when the other person's speech is translated for you.",
    ),
    ("settings.voice.fallback", "Fallback voice"),
    (
        "settings.voice.fallback.desc",
        "Used when your own voice is too slow to prepare.",
    ),
    ("settings.voice.preset.default", "Default voice"),
    ("settings.assistant.language.follow", "Same as interface"),
    ("settings.assistant.backend", "Dialogue backend"),
    ("settings.assistant.backend.desc", "Where the assistant's answers come from."),
    ("settings.assistant.backend.local", "Local server"),
    ("settings.assistant.backend.openprx", "OpenPRX agent (not available yet)"),
    ("settings.assistant.backend.loading", "Looking at the dialogue backends."),
    ("settings.assistant.backend.status", "Backend status"),
    ("settings.assistant.backend.status.desc", "Whether the chosen backend can answer now."),
    ("settings.assistant.voice", "Assistant voice"),
    ("settings.assistant.voice.desc", "The voice that speaks the assistant's answers."),
    ("settings.assistant.voice.preset", "A preset voice"),
    ("settings.assistant.voice.clone", "My voice"),
    ("settings.assistant.voice.needs_profile", "My voice needs an enrolled voice profile. Set one up on the Voice page, or choose a preset voice."),
    ("settings.assistant.voice.preset_name", "Preset voice"),
    ("settings.assistant.voice.preset_name.desc", "Which preset speaks. When none is chosen, the preset for the other party (Voice page) speaks."),
    ("settings.assistant.models_note", "Recognition and the voice use the same models as call translation. See Speech pipeline for what runs."),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("settings.call.self_language", "我的语言"),
    ("settings.call.self_language.desc", "你在通话中使用的语言。"),
    ("settings.call.peer_language", "对方的语言"),
    (
        "settings.call.peer_language.desc",
        "对方使用的语言，必须与你的语言不同。",
    ),
    ("settings.call.lang.follow", "跟随界面语言"),
    ("settings.call.lang.zh", "中文"),
    ("settings.call.lang.en", "English"),
    (
        "settings.call.languages_differ",
        "两种语言必须不同，这次更改没有保存。",
    ),
    ("settings.call.target_app", "通话应用"),
    ("settings.call.target_app.desc", "要翻译其声音的应用。"),
    ("settings.call.target_app.choose", "开始时选择"),
    (
        "settings.reason.running_apps",
        "等待“正在播放声音的应用”列表功能。",
    ),
    ("settings.call.local_output", "我听到的内容"),
    (
        "settings.call.local_output.desc",
        "选择翻译通话时你这一侧播放什么。",
    ),
    ("settings.call.output.translation", "只听译文"),
    (
        "settings.call.output.translation.desc",
        "只播放翻译后的语音。",
    ),
    ("settings.call.output.both", "原声加译文"),
    (
        "settings.call.output.both.desc",
        "原声会压低，译文在上面播放。",
    ),
    ("settings.call.output.original", "只听原声"),
    (
        "settings.call.output.original.desc",
        "只播放对方的原声，不翻译。",
    ),
    ("settings.call.capture", "我说话的方式"),
    (
        "settings.call.capture.desc",
        "免提会翻译你说的每一句；按键说话只在你按住按键时翻译。",
    ),
    ("settings.call.capture.hands_free", "免提"),
    ("settings.call.capture.ptt", "按键说话"),
    (
        "settings.reason.ptt_needs_shortcut",
        "请先在“快捷键”中设置按键说话的快捷键。",
    ),
    ("settings.call.half_duplex", "我在听时暂停发出翻译"),
    (
        "settings.call.half_duplex.desc",
        "译文通过扬声器播放时避免回声。",
    ),
    (
        "settings.reason.half_duplex",
        "等待独立的设置项；目前跟随“音频”里的回声保护。",
    ),
    ("settings.call.captions_source", "字幕中显示原文"),
    (
        "settings.call.captions_source.desc",
        "在译文旁边显示说话的原文。",
    ),
    ("settings.call.disclosure", "告知对方通话在翻译"),
    (
        "settings.call.disclosure.desc",
        "在你通话中第一次开口时，用对方的语言播放一段简短的语音提示：通话为实时翻译，他们听到的是合成语音。提示在你的第一句话翻译期间播放。下次开始时生效；提示文字可在配置文件中修改。",
    ),
    ("settings.assistant.speak", "朗读回复"),
    ("settings.assistant.language", "助手语言"),
    (
        "settings.assistant.language.desc",
        "助手会话和语音识别使用的语言。",
    ),
    (
        "settings.server_url.invalid",
        "请输入以 http:// 或 https:// 开头的地址。",
    ),
    ("settings.audio.microphone", "麦克风"),
    ("settings.input_device.desc", "PRX Voice 使用的麦克风。"),
    ("settings.audio.speakers", "耳机 / 扬声器"),
    ("settings.output_device.desc", "回复和译文的播放位置。"),
    ("settings.audio.default", "系统默认"),
    ("settings.audio.default_named", "系统默认 · {0}"),
    ("settings.audio.not_connected", "{0}（未连接）"),
    ("settings.audio.uses", "正在使用：{0}"),
    (
        "settings.audio.reason.missing",
        "所选麦克风未连接，改用 {0}。",
    ),
    (
        "settings.audio.reason.avoided",
        "系统默认输入是虚拟设备，因此改用 {0}。",
    ),
    (
        "settings.audio.reason.own_virtual",
        "“PRX Voice Microphone”是供其他应用收听的虚拟设备，不能用作你的麦克风，请选择真实的麦克风。",
    ),
    (
        "settings.audio.reason.no_device",
        "没有找到麦克风，请接入后重新检查。",
    ),
    ("settings.audio.list_error", "无法读取设备列表：{0}"),
    ("settings.audio.route", "当前输出"),
    (
        "settings.audio.route.desc",
        "输出到耳机还是扬声器，决定何时暂停麦克风。",
    ),
    ("settings.audio.route.unknown", "未知（按扬声器处理）"),
    ("settings.audio.echo", "回声保护"),
    (
        "settings.audio.echo.desc",
        "避免麦克风收到 PRX Voice 自己播放的声音。",
    ),
    ("settings.audio.echo.auto", "自动"),
    ("settings.audio.echo.always", "始终开启"),
    ("settings.audio.echo.off", "关闭"),
    ("settings.audio.virtual_mic", "虚拟麦克风"),
    (
        "settings.audio.virtual_mic.desc",
        "通话应用用来听你译文语音的麦克风。",
    ),
    ("settings.audio.virtual_mic.manage", "管理"),
    ("settings.audio.virtual_mic_uid", "其他虚拟麦克风（高级）"),
    (
        "settings.audio.virtual_mic_uid.desc",
        "留空则使用 PRX Voice Microphone。若要改用其他设备，请填写它的输出端 UID。",
    ),
    ("settings.audio.test", "测试音频"),
    (
        "settings.audio.test.desc",
        "播放测试声音，或检查麦克风电平。",
    ),
    ("settings.reason.audio_test", "等待音频测试功能。"),
    ("settings.voice.my_voice", "我的声音"),
    (
        "settings.voice.my_voice.desc",
        "录制你的声音，让译文用你的声音说出来。",
    ),
    ("settings.voice.not_set_up", "未设置"),
    ("settings.voice.recorded", "已录制"),
    ("settings.voice.set_up", "设置我的声音"),
    ("settings.voice.language_unknown", "未记录语言"),
    ("settings.voice.not_prepared", "尚未为当前语音模型准备"),
    ("settings.voice.prepared_for", "已为模型 {0} 准备"),
    (
        "settings.reason.enrolment",
        "等待录音功能，将在后续版本提供。",
    ),
    ("settings.voice.use_clone", "用我的声音说出译文"),
    (
        "settings.voice.use_clone.desc",
        "译文使用你录制的声音播放。",
    ),
    (
        "settings.voice.use_clone.needs_profile",
        "设置好你的声音后可用。",
    ),
    ("settings.voice.incoming", "对方译文的声音"),
    (
        "settings.voice.incoming.desc",
        "把对方的话翻译给你听时使用的声音。",
    ),
    ("settings.voice.fallback", "备用声音"),
    (
        "settings.voice.fallback.desc",
        "你自己的声音来不及准备时使用。",
    ),
    ("settings.voice.preset.default", "默认声音"),
    ("settings.assistant.language.follow", "与界面一致"),
    ("settings.assistant.backend", "对话后端"),
    ("settings.assistant.backend.desc", "助手的回答由哪里生成。"),
    ("settings.assistant.backend.local", "本机服务器"),
    ("settings.assistant.backend.openprx", "OpenPRX 智能体（暂不可用）"),
    ("settings.assistant.backend.loading", "正在检查对话后端。"),
    ("settings.assistant.backend.status", "后端状态"),
    ("settings.assistant.backend.status.desc", "所选后端现在能否回答。"),
    ("settings.assistant.voice", "助手的声音"),
    ("settings.assistant.voice.desc", "朗读助手回答所用的声音。"),
    ("settings.assistant.voice.preset", "预设声音"),
    ("settings.assistant.voice.clone", "我的声音"),
    ("settings.assistant.voice.needs_profile", "“我的声音”需要先录入声音档案。请在“声音”页设置，或改用预设声音。"),
    ("settings.assistant.voice.preset_name", "预设声音"),
    ("settings.assistant.voice.preset_name.desc", "由哪个预设朗读。未选择时，使用“声音”页中为对方设置的预设。"),
    ("settings.assistant.models_note", "识别和声音与通话翻译使用相同的模型。运行情况见“语音处理”页。"),
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

    #[test]
    fn placeholders_agree_between_the_languages() {
        let count = |s: &str| (0..6).filter(|i| s.contains(&format!("{{{i}}}"))).count();
        for ((k, en), (_, zh)) in EN.iter().zip(ZH_CN) {
            assert_eq!(count(en), count(zh), "placeholders differ in {k}");
        }
    }
}
