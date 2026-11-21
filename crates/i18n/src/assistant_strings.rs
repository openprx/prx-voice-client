//! Translation strings of the voice assistant's issues and the buttons that fix them, English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. `assistant.issue.<code>`
//! is the message the orb stage shows for a typed problem or fault of the assistant, chosen by the
//! application; `assistant.action.*` are the labels of the buttons that open the settings page the
//! fix lives on.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("assistant.action.open_assistant", "Open Assistant settings"),
    ("assistant.action.open_speech", "Open Speech pipeline"),
    ("assistant.action.open_audio", "Open Audio settings"),
    ("assistant.action.open_permissions", "Open Permissions"),
    (
        "assistant.issue.call_running",
        "Call translation is using the microphone. End it first, then talk to the assistant.",
    ),
    (
        "assistant.issue.model_missing",
        "A speech model the assistant needs is not installed.",
    ),
    (
        "assistant.issue.model_installing",
        "A speech model is still being installed. Try again when it has finished.",
    ),
    (
        "assistant.issue.model_damaged",
        "A speech model is damaged. Install it again.",
    ),
    (
        "assistant.issue.runtime_missing",
        "This copy of the app was built without the speech runtime the assistant needs.",
    ),
    (
        "assistant.issue.unsupported",
        "A provider chosen for the assistant cannot do what it needs (it does not stream, or not in this language).",
    ),
    (
        "assistant.issue.offline",
        "Offline mode blocks the dialogue backend. Turn offline mode off or choose a local backend.",
    ),
    (
        "assistant.issue.setting",
        "A setting of the assistant is not valid. Check the dialogue backend and its credentials.",
    ),
    (
        "assistant.issue.voice_missing",
        "The assistant's voice is not set up. Choose a preset voice or enrol your own.",
    ),
    (
        "assistant.issue.voice_not_prepared",
        "Your voice has not been prepared for the current voice model yet.",
    ),
    (
        "assistant.issue.dialogue_down",
        "The assistant's server is not answering. Start the local server, or choose another dialogue backend.",
    ),
    (
        "assistant.issue.dialogue_timeout",
        "The assistant took too long to answer. Try again, or choose another dialogue backend.",
    ),
    (
        "assistant.issue.dialogue_rejected",
        "The dialogue backend refused the request. Check its address, model and credential.",
    ),
    (
        "assistant.issue.dialogue_empty",
        "The dialogue backend sent no answer. Try again.",
    ),
    (
        "assistant.issue.dialogue_not_available_yet",
        "That dialogue backend is not available yet. Choose another one.",
    ),
    (
        "assistant.issue.synthesis_failed",
        "The voice could not speak the answer. Check the voice model.",
    ),
    (
        "assistant.issue.no_audio",
        "The answer had nothing to say aloud.",
    ),
    (
        "assistant.issue.listening_failed",
        "Speech recognition stopped working. Check the speech models.",
    ),
    (
        "assistant.issue.microphone_unavailable",
        "The microphone cannot be opened. Check the permission and the input device.",
    ),
    (
        "assistant.issue.microphone_lost",
        "The microphone was lost. Check the input device.",
    ),
    (
        "assistant.issue.microphone_rate",
        "The microphone delivers audio the speech detector cannot use.",
    ),
    (
        "assistant.issue.output_failed",
        "The sound output refused the answer. Check the output device.",
    ),
    (
        "assistant.issue.start_failed",
        "The assistant could not start.",
    ),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("assistant.action.open_assistant", "打开助手设置"),
    ("assistant.action.open_speech", "打开语音处理"),
    ("assistant.action.open_audio", "打开音频设置"),
    ("assistant.action.open_permissions", "打开权限"),
    (
        "assistant.issue.call_running",
        "通话翻译正在使用麦克风。请先结束通话翻译，再和助手对话。",
    ),
    (
        "assistant.issue.model_missing",
        "助手需要的语音模型尚未安装。",
    ),
    (
        "assistant.issue.model_installing",
        "语音模型仍在安装中，请在安装完成后再试。",
    ),
    (
        "assistant.issue.model_damaged",
        "语音模型已损坏，请重新安装。",
    ),
    (
        "assistant.issue.runtime_missing",
        "此版本的应用没有包含助手所需的语音运行库。",
    ),
    (
        "assistant.issue.unsupported",
        "为助手选择的某个服务无法满足需求（不支持流式输出，或不支持该语言）。",
    ),
    (
        "assistant.issue.offline",
        "离线模式拦截了对话后端。请关闭离线模式，或选择本机后端。",
    ),
    (
        "assistant.issue.setting",
        "助手的某项设置无效。请检查对话后端及其凭据。",
    ),
    (
        "assistant.issue.voice_missing",
        "助手的声音尚未设置。请选择预设声音，或录入你自己的声音。",
    ),
    (
        "assistant.issue.voice_not_prepared",
        "你的声音还没有针对当前语音模型准备好。",
    ),
    (
        "assistant.issue.dialogue_down",
        "助手的服务器没有响应。请启动本机服务器，或选择其他对话后端。",
    ),
    (
        "assistant.issue.dialogue_timeout",
        "助手回答超时。请重试，或选择其他对话后端。",
    ),
    (
        "assistant.issue.dialogue_rejected",
        "对话后端拒绝了请求。请检查它的地址、模型和凭据。",
    ),
    (
        "assistant.issue.dialogue_empty",
        "对话后端没有给出回答。请重试。",
    ),
    (
        "assistant.issue.dialogue_not_available_yet",
        "该对话后端暂不可用，请选择其他后端。",
    ),
    (
        "assistant.issue.synthesis_failed",
        "声音无法朗读这条回答。请检查语音合成模型。",
    ),
    ("assistant.issue.no_audio", "这条回答没有可朗读的内容。"),
    (
        "assistant.issue.listening_failed",
        "语音识别停止工作。请检查语音模型。",
    ),
    (
        "assistant.issue.microphone_unavailable",
        "无法打开麦克风。请检查权限和输入设备。",
    ),
    (
        "assistant.issue.microphone_lost",
        "麦克风已断开。请检查输入设备。",
    ),
    (
        "assistant.issue.microphone_rate",
        "麦克风输出的音频，语音检测无法使用。",
    ),
    (
        "assistant.issue.output_failed",
        "声音输出拒绝了这条回答。请检查输出设备。",
    ),
    ("assistant.issue.start_failed", "助手无法启动。"),
];
