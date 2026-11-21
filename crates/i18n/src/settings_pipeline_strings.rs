//! Translation strings of the Speech pipeline page (design 2.2 and 4.8): roles, what leaves the machine, the sentence for each provider problem, and the storage and download rows, English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. The keys of
//! `settings.pipeline.problem.<code>` are chosen from the typed problem of the provider registry
//! by the app.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("settings.pipeline.setup", "Setup"),
    (
        "settings.pipeline.setup.desc",
        "All on this computer, hybrid, or all cloud.",
    ),
    ("settings.reason.presets", "Waiting for the preset chooser."),
    ("settings.pipeline.preset.local", "All on this computer"),
    ("settings.pipeline.preset.hybrid", "Hybrid"),
    ("settings.pipeline.preset.cloud", "All cloud"),
    ("settings.pipeline.preset.custom", "Custom"),
    ("settings.pipeline.leaving", "Data leaving this computer"),
    (
        "settings.pipeline.leaving.desc",
        "What is sent to other computers during a session.",
    ),
    (
        "settings.pipeline.leaving.nothing",
        "Nothing leaves this computer",
    ),
    (
        "settings.pipeline.leaving.offline",
        "Offline mode: nothing leaves this computer",
    ),
    ("settings.pipeline.leaving.some", "Sent to {0}"),
    ("settings.pipeline.leaving.unknown", "Not known yet"),
    ("settings.pipeline.roles", "Providers for each step"),
    ("settings.pipeline.choose", "Choose providers"),
    (
        "settings.pipeline.choose.desc",
        "Pick a local model or a server for each step.",
    ),
    (
        "settings.reason.provider_editor",
        "Waiting for the provider editor.",
    ),
    ("settings.pipeline.loading", "Checking the providers…"),
    ("settings.pipeline.problems", "Needs attention"),
    ("settings.pipeline.ready", "Ready"),
    ("settings.pipeline.fallback", "If it fails: {0}"),
    ("settings.pipeline.role.vad", "Speech detection"),
    ("settings.pipeline.role.asr", "Speech recognition"),
    ("settings.pipeline.role.refine", "Recognition refinement (two-pass mode only; off by default)"),
    ("settings.pipeline.role.mt", "Translation"),
    ("settings.pipeline.role.tts", "Voice"),
    ("settings.pipeline.role.dialogue", "Assistant dialogue"),
    ("settings.lang.zh", "Chinese"),
    ("settings.lang.en", "English"),
    ("settings.pipeline.egress.local", "Runs on this computer"),
    ("settings.pipeline.egress.audio", "Sends audio to {0}"),
    ("settings.pipeline.egress.text", "Sends text to {0}"),
    (
        "settings.pipeline.egress.audio_lan",
        "Sends audio to {0} on your network",
    ),
    (
        "settings.pipeline.egress.text_lan",
        "Sends text to {0} on your network",
    ),
    ("settings.pipeline.cloud", "Cloud and server providers"),
    (
        "settings.pipeline.cloud.none",
        "No cloud or server providers are set up.",
    ),
    ("settings.pipeline.add_provider", "Add provider"),
    (
        "settings.pipeline.add_provider.desc",
        "Connect a cloud service or a server on your network.",
    ),
    ("settings.pipeline.models", "Local models"),
    ("settings.pipeline.model_list", "Model list"),
    (
        "settings.pipeline.model_list.desc",
        "Every model, its state, download and removal.",
    ),
    (
        "settings.reason.model_manager",
        "Waiting for the model manager.",
    ),
    ("settings.pipeline.storage", "Storage and downloads"),
    (
        "settings.pipeline.problem.other",
        "A provider problem ({0}). See the log for details.",
    ),
    (
        "settings.pipeline.problem.store_unreadable",
        "The model store cannot be read: {0}",
    ),
    (
        "settings.pipeline.problem.invalid_language",
        "{0} is “{1}”, which is not a supported language. Choose Chinese or English.",
    ),
    (
        "settings.pipeline.problem.language_not_set",
        "The language setting {0} is empty and could not be derived. Choose your languages under Call translation.",
    ),
    (
        "settings.pipeline.problem.invalid_setting",
        "{0} is “{1}”, which is not a known value.",
    ),
    (
        "settings.pipeline.problem.no_provider",
        "No provider is chosen for this step and there is no default.",
    ),
    (
        "settings.pipeline.problem.unknown_provider",
        "The chosen provider “{0}” does not exist. Choose another one.",
    ),
    (
        "settings.pipeline.problem.unknown_protocol",
        "Provider “{0}” uses the unknown protocol “{1}”.",
    ),
    (
        "settings.pipeline.problem.unknown_template",
        "Provider “{0}” uses the unknown template “{1}”.",
    ),
    (
        "settings.pipeline.problem.wrong_kind",
        "“{0}” cannot do this step. Choose another provider.",
    ),
    (
        "settings.pipeline.problem.model_not_installed",
        "Model “{0}” is not installed ({1}). Download it, then try again.",
    ),
    (
        "settings.pipeline.problem.model_installing",
        "Model “{0}” is still being installed. Wait until it finishes.",
    ),
    (
        "settings.pipeline.problem.model_failed_verification",
        "Model “{0}” failed its check and may be damaged. Download it again.",
    ),
    (
        "settings.pipeline.problem.model_failed",
        "Model “{0}” cannot be used: {1}",
    ),
    (
        "settings.pipeline.problem.model_files_incomplete",
        "Model “{0}” is missing its {1} file. Download it again.",
    ),
    (
        "settings.pipeline.problem.runtime_not_built_in",
        "“{0}” needs the {1} engine, which this build does not include. Use a build with the speech runtimes.",
    ),
    (
        "settings.pipeline.problem.not_streaming",
        "“{0}” cannot work live: {1}",
    ),
    (
        "settings.pipeline.problem.language_unsupported",
        "“{0}” does not support the language {1}.",
    ),
    (
        "settings.pipeline.problem.pair_unsupported",
        "“{0}” does not translate from {1} to {2}.",
    ),
    (
        "settings.pipeline.problem.credential_missing",
        "“{0}” needs an API key and none is saved.",
    ),
    (
        "settings.pipeline.problem.credential_unreadable",
        "The API key of “{0}” could not be read: {1}",
    ),
    (
        "settings.pipeline.problem.endpoint_blocked",
        "Offline mode blocks “{0}” at {1}. Turn off offline mode or allow the host.",
    ),
    (
        "settings.pipeline.problem.invalid_endpoint",
        "“{0}” has an unusable address: {1}",
    ),
    (
        "settings.pipeline.problem.invalid_params",
        "“{0}”: setting {1} {2}",
    ),
    (
        "settings.pipeline.problem.voice_profile_not_set",
        "No voice profile is chosen for speaking in your own voice. Set up your voice, or turn that off.",
    ),
    (
        "settings.pipeline.problem.voice_profile_missing",
        "Voice profile “{0}” is missing. Record it again or restore it.",
    ),
    (
        "settings.pipeline.problem.voice_profile_unreadable",
        "Voice profile “{0}” cannot be read: {1}",
    ),
    (
        "settings.pipeline.problem.voice_not_prepared",
        "Voice profile “{0}” has not been prepared for the speech model. Prepare it.",
    ),
    (
        "settings.pipeline.problem.voice_prepared_for_other_model",
        "Voice profile “{0}” was prepared for a different speech model. Prepare it again.",
    ),
    (
        "settings.pipeline.problem.voice_preset_unknown",
        "The preset voice “{0}” does not exist for this model. Choose another voice.",
    ),
    (
        "settings.pipeline.problem.voice_preset_file_missing",
        "The recording of preset voice “{0}” is missing.",
    ),
    (
        "settings.pipeline.problem.voice_preset_unreadable",
        "The preset voice “{0}” cannot be read: {1}",
    ),
    (
        "settings.pipeline.problem.voice_preset_language",
        "The preset voice “{0}” is for {1}, but {2} is needed.",
    ),
    (
        "settings.pipeline.problem.fallback_is_primary",
        "The fallback “{0}” is the same as the main provider. Choose a different fallback.",
    ),
    ("settings.models.preload", "Load models at startup"),
    (
        "settings.models.preload.desc",
        "Off loads models when a session starts.",
    ),
    ("settings.models.warmup", "Warm up after loading"),
    (
        "settings.models.warmup.desc",
        "Off skips the warm-up; the first phrase is slower.",
    ),
    ("settings.models.source", "Download source"),
    (
        "settings.models.source.desc",
        "Where models are downloaded from.",
    ),
    ("settings.models.source.hf", "Hugging Face"),
    ("settings.models.source.ms", "ModelScope"),
    ("settings.models.source.custom", "Custom…"),
    ("settings.models.source.address", "Download address"),
    (
        "settings.models.source.custom.desc",
        "A Hugging Face compatible address starting with https://.",
    ),
    (
        "settings.models.source.custom.invalid",
        "Enter an address starting with https://.",
    ),
    ("settings.models.concurrent", "Downloads at a time"),
    (
        "settings.models.concurrent.desc",
        "How many models download at once.",
    ),
    ("settings.models.dir", "Model storage location"),
    ("settings.models.dir.default", "Default"),
    ("settings.models.dir.custom", "Custom"),
    ("settings.models.dir.unknown", "Not known yet"),
    ("settings.models.change_dir", "Change location"),
    (
        "settings.models.change_dir.desc",
        "Move the installed models to another folder.",
    ),
    ("settings.models.catalogue_url", "Model list source"),
    (
        "settings.models.catalogue_url.desc",
        "Where a newer model list is fetched from.",
    ),
    ("settings.models.update", "Update the model list"),
    (
        "settings.models.update.desc",
        "Check for new models automatically, or only when you ask.",
    ),
    ("settings.models.update.auto", "Automatically (daily)"),
    ("settings.models.update.manual", "Only when I ask"),
    ("settings.models.check_now", "Check for model updates"),
    (
        "settings.models.check_now.desc",
        "Look for a newer model list now.",
    ),
    (
        "settings.reason.catalogue_update",
        "Waiting for the model list update.",
    ),
    ("settings.models.unused", "Unused models"),
    (
        "settings.models.unused.desc",
        "Remove models no step uses, to free disk space.",
    ),
    ("settings.pipeline.assistant", "Voice assistant"),
    ("settings.pipeline.assistant.desc", "What runs when you talk to the assistant: the same speech models as a call, and its own dialogue backend."),
    ("settings.pipeline.problem.not_available_yet", "{1} is not available yet. Choose another dialogue backend on the Assistant page."),
    ("settings.pipeline.problem.server_not_answering", "The server at {0} does not answer ({1}). Start it, or choose another dialogue backend."),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("settings.pipeline.setup", "整体方案"),
    (
        "settings.pipeline.setup.desc",
        "全部在本机、混合，或全部使用云端。",
    ),
    ("settings.reason.presets", "等待方案选择功能。"),
    ("settings.pipeline.preset.local", "全部在本机"),
    ("settings.pipeline.preset.hybrid", "混合"),
    ("settings.pipeline.preset.cloud", "全部云端"),
    ("settings.pipeline.preset.custom", "自定义"),
    ("settings.pipeline.leaving", "离开这台电脑的数据"),
    (
        "settings.pipeline.leaving.desc",
        "会话期间发送到其他电脑的内容。",
    ),
    ("settings.pipeline.leaving.nothing", "没有数据离开这台电脑"),
    (
        "settings.pipeline.leaving.offline",
        "离线模式：没有数据离开这台电脑",
    ),
    ("settings.pipeline.leaving.some", "会发送到 {0}"),
    ("settings.pipeline.leaving.unknown", "尚不清楚"),
    ("settings.pipeline.roles", "各环节的处理方式"),
    ("settings.pipeline.choose", "选择处理方式"),
    (
        "settings.pipeline.choose.desc",
        "为每个环节选择本地模型或服务器。",
    ),
    (
        "settings.reason.provider_editor",
        "等待服务提供方编辑功能。",
    ),
    ("settings.pipeline.loading", "正在检查各环节…"),
    ("settings.pipeline.problems", "需要处理"),
    ("settings.pipeline.ready", "就绪"),
    ("settings.pipeline.fallback", "出错时改用：{0}"),
    ("settings.pipeline.role.vad", "语音检测"),
    ("settings.pipeline.role.asr", "语音识别"),
    ("settings.pipeline.role.refine", "识别精修（仅两遍识别模式使用，默认关闭）"),
    ("settings.pipeline.role.mt", "翻译"),
    ("settings.pipeline.role.tts", "语音合成"),
    ("settings.pipeline.role.dialogue", "助手对话"),
    ("settings.lang.zh", "中文"),
    ("settings.lang.en", "英文"),
    ("settings.pipeline.egress.local", "在本机运行"),
    ("settings.pipeline.egress.audio", "把音频发送到 {0}"),
    ("settings.pipeline.egress.text", "把文字发送到 {0}"),
    (
        "settings.pipeline.egress.audio_lan",
        "把音频发送到局域网内的 {0}",
    ),
    (
        "settings.pipeline.egress.text_lan",
        "把文字发送到局域网内的 {0}",
    ),
    ("settings.pipeline.cloud", "云端与服务器"),
    (
        "settings.pipeline.cloud.none",
        "尚未配置云端或服务器提供方。",
    ),
    ("settings.pipeline.add_provider", "添加提供方"),
    (
        "settings.pipeline.add_provider.desc",
        "接入云服务或局域网内的服务器。",
    ),
    ("settings.pipeline.models", "本地模型"),
    ("settings.pipeline.model_list", "模型列表"),
    (
        "settings.pipeline.model_list.desc",
        "所有模型的状态、下载与删除。",
    ),
    ("settings.reason.model_manager", "等待模型管理功能。"),
    ("settings.pipeline.storage", "存储与下载"),
    (
        "settings.pipeline.problem.other",
        "提供方出现问题（{0}），详情见日志。",
    ),
    (
        "settings.pipeline.problem.store_unreadable",
        "无法读取模型存储位置：{0}",
    ),
    (
        "settings.pipeline.problem.invalid_language",
        "“{0}”的值“{1}”不是支持的语言，请选择中文或英文。",
    ),
    (
        "settings.pipeline.problem.language_not_set",
        "语言设置 {0} 为空，且无法推断。请在“通话翻译”中选择语言。",
    ),
    (
        "settings.pipeline.problem.invalid_setting",
        "“{0}”的值“{1}”无法识别。",
    ),
    (
        "settings.pipeline.problem.no_provider",
        "这个环节没有选定处理方式，也没有默认值。",
    ),
    (
        "settings.pipeline.problem.unknown_provider",
        "所选的“{0}”不存在，请改选其他。",
    ),
    (
        "settings.pipeline.problem.unknown_protocol",
        "提供方“{0}”使用了未知的协议“{1}”。",
    ),
    (
        "settings.pipeline.problem.unknown_template",
        "提供方“{0}”使用了未知的模板“{1}”。",
    ),
    (
        "settings.pipeline.problem.wrong_kind",
        "“{0}”无法承担这个环节，请改选其他。",
    ),
    (
        "settings.pipeline.problem.model_not_installed",
        "模型“{0}”尚未安装（{1}），请先下载。",
    ),
    (
        "settings.pipeline.problem.model_installing",
        "模型“{0}”还在安装中，请等它完成。",
    ),
    (
        "settings.pipeline.problem.model_failed_verification",
        "模型“{0}”没有通过校验，可能已损坏，请重新下载。",
    ),
    (
        "settings.pipeline.problem.model_failed",
        "模型“{0}”无法使用：{1}",
    ),
    (
        "settings.pipeline.problem.model_files_incomplete",
        "模型“{0}”缺少 {1} 文件，请重新下载。",
    ),
    (
        "settings.pipeline.problem.runtime_not_built_in",
        "“{0}”需要 {1} 引擎，而这个版本不包含它。请使用带语音运行库的版本。",
    ),
    (
        "settings.pipeline.problem.not_streaming",
        "“{0}”无法实时工作：{1}",
    ),
    (
        "settings.pipeline.problem.language_unsupported",
        "“{0}”不支持 {1}。",
    ),
    (
        "settings.pipeline.problem.pair_unsupported",
        "“{0}”不支持 {1} 到 {2} 的翻译。",
    ),
    (
        "settings.pipeline.problem.credential_missing",
        "“{0}”需要 API 密钥，但还没有保存。",
    ),
    (
        "settings.pipeline.problem.credential_unreadable",
        "无法读取“{0}”的 API 密钥：{1}",
    ),
    (
        "settings.pipeline.problem.endpoint_blocked",
        "离线模式阻止了对“{0}”（{1}）的访问。请关闭离线模式，或把该主机加入允许列表。",
    ),
    (
        "settings.pipeline.problem.invalid_endpoint",
        "“{0}”的地址无法使用：{1}",
    ),
    (
        "settings.pipeline.problem.invalid_params",
        "“{0}”：参数 {1} {2}",
    ),
    (
        "settings.pipeline.problem.voice_profile_not_set",
        "没有为“用我的声音说话”选定声音档案。请设置你的声音，或关闭这项功能。",
    ),
    (
        "settings.pipeline.problem.voice_profile_missing",
        "声音档案“{0}”不存在。请重新录制或恢复它。",
    ),
    (
        "settings.pipeline.problem.voice_profile_unreadable",
        "无法读取声音档案“{0}”：{1}",
    ),
    (
        "settings.pipeline.problem.voice_not_prepared",
        "声音档案“{0}”还没有为语音模型准备，请先准备。",
    ),
    (
        "settings.pipeline.problem.voice_prepared_for_other_model",
        "声音档案“{0}”是为另一个语音模型准备的，请重新准备。",
    ),
    (
        "settings.pipeline.problem.voice_preset_unknown",
        "这个模型没有预设声音“{0}”，请改选其他声音。",
    ),
    (
        "settings.pipeline.problem.voice_preset_file_missing",
        "预设声音“{0}”的录音文件缺失。",
    ),
    (
        "settings.pipeline.problem.voice_preset_unreadable",
        "无法读取预设声音“{0}”：{1}",
    ),
    (
        "settings.pipeline.problem.voice_preset_language",
        "预设声音“{0}”适用于 {1}，但这里需要 {2}。",
    ),
    (
        "settings.pipeline.problem.fallback_is_primary",
        "备用的“{0}”与主要提供方相同，请改选不同的备用项。",
    ),
    ("settings.models.preload", "启动时加载模型"),
    (
        "settings.models.preload.desc",
        "关闭后，在会话开始时才加载模型。",
    ),
    ("settings.models.warmup", "加载后预热"),
    (
        "settings.models.warmup.desc",
        "关闭则跳过预热，第一句会慢一些。",
    ),
    ("settings.models.source", "下载来源"),
    ("settings.models.source.desc", "模型从哪里下载。"),
    ("settings.models.source.hf", "Hugging Face"),
    ("settings.models.source.ms", "ModelScope"),
    ("settings.models.source.custom", "自定义…"),
    ("settings.models.source.address", "下载地址"),
    (
        "settings.models.source.custom.desc",
        "以 https:// 开头、兼容 Hugging Face 的地址。",
    ),
    (
        "settings.models.source.custom.invalid",
        "请输入以 https:// 开头的地址。",
    ),
    ("settings.models.concurrent", "同时下载数"),
    ("settings.models.concurrent.desc", "同时下载几个模型。"),
    ("settings.models.dir", "模型存放位置"),
    ("settings.models.dir.default", "默认"),
    ("settings.models.dir.custom", "自定义"),
    ("settings.models.dir.unknown", "尚未获取"),
    ("settings.models.change_dir", "更改位置"),
    (
        "settings.models.change_dir.desc",
        "把已安装的模型移到其他文件夹。",
    ),
    ("settings.models.catalogue_url", "模型列表来源"),
    (
        "settings.models.catalogue_url.desc",
        "从这里获取更新的模型列表。",
    ),
    ("settings.models.update", "更新模型列表"),
    (
        "settings.models.update.desc",
        "自动检查新模型，或只在你需要时检查。",
    ),
    ("settings.models.update.auto", "自动（每天）"),
    ("settings.models.update.manual", "仅在我需要时"),
    ("settings.models.check_now", "检查模型更新"),
    (
        "settings.models.check_now.desc",
        "现在检查是否有更新的模型列表。",
    ),
    ("settings.reason.catalogue_update", "等待模型列表更新功能。"),
    ("settings.models.unused", "未使用的模型"),
    (
        "settings.models.unused.desc",
        "删除没有任何环节使用的模型，释放磁盘空间。",
    ),
    ("settings.pipeline.assistant", "语音助手"),
    ("settings.pipeline.assistant.desc", "与助手对话时运行的内容：与通话相同的语音模型，加上助手自己的对话后端。"),
    ("settings.pipeline.problem.not_available_yet", "{1}暂不可用。请在“助手”页选择其他对话后端。"),
    ("settings.pipeline.problem.server_not_answering", "{0} 处的服务器没有响应（{1}）。请启动它，或选择其他对话后端。"),
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
