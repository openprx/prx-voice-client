//! Translation strings shared by the settings window and the pages General, Shortcuts, Privacy and data, Advanced and About (design 2.2), plus the tray state line and the orb menu entries, English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. Keys are
//! `settings.<area>.<item>`; `.desc` is the helper line of a row and `settings.reason.*` the visible
//! reason of a row that is not available yet. The other pages are in `settings_session_strings`,
//! `settings_pipeline_strings` and `settings_permission_strings`.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("tray.state.idle", "Idle"),
    ("tray.state.listening", "Listening"),
    ("tray.state.speaking", "Speaking"),
    ("tray.state.translating", "Translating"),
    ("tray.state.paused", "Paused"),
    ("tray.state.attention", "Needs attention: {0}"),
    ("settings.nav.general", "General"),
    ("settings.nav.call", "Call translation"),
    ("settings.nav.assistant", "Assistant"),
    ("settings.nav.audio", "Audio"),
    ("settings.nav.voice", "Voice"),
    ("settings.nav.pipeline", "Speech pipeline"),
    ("settings.nav.permissions", "Permissions"),
    ("settings.nav.shortcuts", "Shortcuts"),
    ("settings.nav.privacy", "Privacy and data"),
    ("settings.nav.advanced", "Advanced"),
    ("settings.nav.about", "About"),
    ("settings.section_select", "Section"),
    ("settings.menu.pipeline", "Speech pipeline"),
    ("settings.menu.permissions", "Permissions"),
    ("settings.unavailable", "Not available yet"),
    ("settings.value_now", "Now: {0}"),
    (
        "settings.note.saved_only",
        "Saved now; the app does not use it yet.",
    ),
    ("settings.note.next_session", "Applies to the next session."),
    (
        "settings.note.next_download",
        "Applies to the next download.",
    ),
    (
        "settings.notice.unreadable",
        "Your settings file could not be read. It was kept as {0} and defaults are in use.",
    ),
    (
        "settings.notice.save_failed",
        "The change could not be saved, so the old value is still in effect: {0}",
    ),
    ("common.on", "On"),
    ("common.off", "Off"),
    ("settings.mode", "Mode"),
    (
        "settings.mode.desc",
        "Decides what the start button of the main window does.",
    ),
    ("settings.mode.call", "Call translation"),
    ("settings.mode.assistant", "Voice assistant"),
    ("settings.general.language", "Interface language"),
    ("settings.language.system", "System"),
    ("settings.general.captions", "Show live captions"),
    ("settings.compact", "Compact main window"),
    (
        "settings.compact.desc",
        "Hide captions and show only the orb and its status.",
    ),
    ("settings.always_on_top", "Keep main window on top"),
    (
        "settings.always_on_top.desc",
        "Keep the orb above other windows.",
    ),
    ("settings.launch_at_login", "Launch at login"),
    (
        "settings.launch_at_login.desc",
        "Start PRX Voice when you sign in to this computer.",
    ),
    (
        "settings.reason.login_item",
        "Waiting for the system's login-item registration.",
    ),
    ("settings.shortcuts.enabled", "Global shortcuts"),
    ("settings.shortcuts.toggle", "Start or stop session"),
    ("settings.shortcuts.toggle.desc", "Works from any app."),
    ("settings.shortcuts.ptt", "Push to talk"),
    (
        "settings.shortcuts.ptt.desc",
        "Hold to speak when Outgoing input is push-to-talk.",
    ),
    ("settings.shortcuts.interrupt", "Stop speaking"),
    (
        "settings.shortcuts.interrupt.desc",
        "Interrupts the assistant or the current translation.",
    ),
    ("settings.shortcuts.none", "None"),
    (
        "settings.reason.recorder",
        "Waiting for the shortcut recorder.",
    ),
    ("settings.shortcuts.mode", "Assistant shortcut behaviour"),
    (
        "settings.shortcuts.mode.desc",
        "Toggle starts and stops with each press; hold to talk listens only while the key is down.",
    ),
    ("settings.shortcuts.mode.toggle", "Toggle"),
    ("settings.shortcuts.mode.hold", "Hold to talk"),
    ("settings.privacy.history", "Keep session history"),
    (
        "settings.privacy.transcripts",
        "Keep transcripts in history",
    ),
    (
        "settings.privacy.transcripts.desc",
        "Store what was said, not only that a session happened.",
    ),
    (
        "settings.privacy.transcripts.needs_history",
        "Needs session history.",
    ),
    (
        "settings.privacy.log_sensitive",
        "Include spoken text in logs",
    ),
    (
        "settings.privacy.log_sensitive.desc",
        "Logs may then contain what you and the other party said.",
    ),
    ("settings.privacy.delete_history", "Delete all history"),
    (
        "settings.privacy.delete_history.desc",
        "Remove every saved session from this computer.",
    ),
    (
        "settings.reason.delete_history",
        "Waiting for the delete function.",
    ),
    ("settings.advanced.developer", "Developer tools"),
    (
        "settings.advanced.developer.desc",
        "Show the debug window in the orb's menu.",
    ),
    ("settings.advanced.debug_panel", "Show debug panel"),
    (
        "settings.advanced.debug_panel.desc",
        "Show the debug panel with the developer tools.",
    ),
    (
        "settings.advanced.log_level.desc",
        "How much detail the log records.",
    ),
    (
        "settings.advanced.barge_in",
        "Skip translations that fall behind",
    ),
    (
        "settings.advanced.barge_in.desc",
        "When you keep talking, queued translations that are too late are dropped.",
    ),
    ("settings.advanced.export", "Export diagnostics"),
    (
        "settings.advanced.export.desc",
        "Write a diagnostics bundle for a bug report.",
    ),
    ("settings.advanced.export.action", "Export"),
    ("settings.advanced.capture_format", "Capture format"),
    (
        "settings.advanced.capture_format.desc",
        "Fixed by the speech models; it cannot be changed.",
    ),
    ("settings.advanced.capture_format.value", "{0} kHz, {1} ch"),
    ("settings.advanced.setup_again", "Run setup again"),
    (
        "settings.advanced.setup_again.desc",
        "Go through first-run setup again; settings, models and permissions are kept.",
    ),
    (
        "settings.reason.first_run",
        "Waiting for the first-run setup.",
    ),
    ("settings.advanced.experimental", "Experimental"),
    (
        "settings.advanced.experimental.none",
        "There are no experimental settings in this version.",
    ),
    ("settings.about.version", "Version"),
    (
        "settings.about.version.desc",
        "The version of PRX Voice you are running.",
    ),
    ("settings.about.version.value", "PRX Voice {0}"),
    ("settings.about.updates", "Updates"),
    (
        "settings.about.updates.desc",
        "Check whether a newer version is available.",
    ),
    ("settings.reason.updates", "Waiting for the update check."),
    ("settings.about.model_licences", "Model licences"),
    (
        "settings.about.model_licences.desc",
        "The licence and source of every installed model.",
    ),
    (
        "settings.reason.model_licences",
        "Waiting for the model manager.",
    ),
    ("settings.about.notices", "Open-source licences"),
    ("settings.about.notices.action", "Show file"),
    ("settings.about.notices.chip", "Notices"),
    (
        "settings.about.notices.missing",
        "The notices file was not found next to this copy of the app.",
    ),
    ("settings.about.data_dir", "Data folder"),
    ("settings.about.folder", "Folder"),
    ("settings.about.reveal.finder", "Reveal in Finder"),
    ("settings.about.reveal.files", "Show in file manager"),
    ("settings.about.log", "Log file"),
    ("settings.about.log.file", "File"),
    (
        "settings.about.log.terminal",
        "Written to the terminal that started PRX Voice.",
    ),
    ("settings.about.log.terminal.chip", "Terminal"),
    ("settings.about.config", "Settings file"),
    ("settings.about.file", "File"),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("tray.state.idle", "空闲"),
    ("tray.state.listening", "正在聆听"),
    ("tray.state.speaking", "正在说话"),
    ("tray.state.translating", "正在翻译"),
    ("tray.state.paused", "已暂停"),
    ("tray.state.attention", "需要处理：{0}"),
    ("settings.nav.general", "通用"),
    ("settings.nav.call", "通话翻译"),
    ("settings.nav.assistant", "语音助手"),
    ("settings.nav.audio", "音频"),
    ("settings.nav.voice", "声音"),
    ("settings.nav.pipeline", "语音处理"),
    ("settings.nav.permissions", "权限"),
    ("settings.nav.shortcuts", "快捷键"),
    ("settings.nav.privacy", "隐私与数据"),
    ("settings.nav.advanced", "高级"),
    ("settings.nav.about", "关于"),
    ("settings.section_select", "设置分区"),
    ("settings.menu.pipeline", "语音处理"),
    ("settings.menu.permissions", "权限"),
    ("settings.unavailable", "暂不可用"),
    ("settings.value_now", "当前：{0}"),
    (
        "settings.note.saved_only",
        "已保存，应用暂时还不会用到这项设置。",
    ),
    ("settings.note.next_session", "下次会话开始时生效。"),
    ("settings.note.next_download", "下次下载时生效。"),
    (
        "settings.notice.unreadable",
        "无法读取设置文件。原文件已保留为 {0}，当前使用默认设置。",
    ),
    (
        "settings.notice.save_failed",
        "无法保存这项更改，仍使用原来的值：{0}",
    ),
    ("common.on", "开"),
    ("common.off", "关"),
    ("settings.mode", "使用模式"),
    ("settings.mode.desc", "决定主窗口的开始按钮做什么。"),
    ("settings.mode.call", "通话翻译"),
    ("settings.mode.assistant", "语音助手"),
    ("settings.general.language", "界面语言"),
    ("settings.language.system", "跟随系统"),
    ("settings.general.captions", "显示实时字幕"),
    ("settings.compact", "精简主窗口"),
    ("settings.compact.desc", "隐藏字幕，只显示光球和状态。"),
    ("settings.always_on_top", "主窗口置顶"),
    (
        "settings.always_on_top.desc",
        "让光球始终显示在其他窗口之上。",
    ),
    ("settings.launch_at_login", "登录时启动"),
    (
        "settings.launch_at_login.desc",
        "登录这台电脑时自动启动 PRX Voice。",
    ),
    ("settings.reason.login_item", "等待接入系统的登录项注册。"),
    ("settings.shortcuts.enabled", "全局快捷键"),
    ("settings.shortcuts.toggle", "开始或结束会话"),
    ("settings.shortcuts.toggle.desc", "在任何应用中都可使用。"),
    ("settings.shortcuts.ptt", "按键说话"),
    (
        "settings.shortcuts.ptt.desc",
        "“我说话的方式”设为按键说话时，按住它说话。",
    ),
    ("settings.shortcuts.interrupt", "打断播放"),
    (
        "settings.shortcuts.interrupt.desc",
        "打断助手或当前的译文播放。",
    ),
    ("settings.shortcuts.none", "未设置"),
    ("settings.reason.recorder", "等待快捷键录入功能。"),
    ("settings.shortcuts.mode", "助手快捷键的行为"),
    (
        "settings.shortcuts.mode.desc",
        "切换：每按一次开始或结束；按住说话：只在按住按键时收音。",
    ),
    ("settings.shortcuts.mode.toggle", "切换"),
    ("settings.shortcuts.mode.hold", "按住说话"),
    ("settings.privacy.history", "保留会话历史"),
    ("settings.privacy.transcripts", "在历史中保留文字记录"),
    (
        "settings.privacy.transcripts.desc",
        "保存说过的内容，而不只是记录有过会话。",
    ),
    (
        "settings.privacy.transcripts.needs_history",
        "需要先开启会话历史。",
    ),
    ("settings.privacy.log_sensitive", "日志中包含说话内容"),
    (
        "settings.privacy.log_sensitive.desc",
        "开启后，日志里可能包含你和对方说过的话。",
    ),
    ("settings.privacy.delete_history", "删除全部历史"),
    (
        "settings.privacy.delete_history.desc",
        "从这台电脑上删除所有已保存的会话。",
    ),
    ("settings.reason.delete_history", "等待删除功能。"),
    ("settings.advanced.developer", "开发者工具"),
    (
        "settings.advanced.developer.desc",
        "在光球菜单中显示调试窗口。",
    ),
    ("settings.advanced.debug_panel", "显示调试面板"),
    (
        "settings.advanced.debug_panel.desc",
        "开启开发者工具时显示调试面板。",
    ),
    ("settings.advanced.log_level.desc", "日志记录的详细程度。"),
    ("settings.advanced.barge_in", "跳过落后的译文"),
    (
        "settings.advanced.barge_in.desc",
        "你继续说话时，丢弃已经太晚的排队译文。",
    ),
    ("settings.advanced.export", "导出诊断信息"),
    (
        "settings.advanced.export.desc",
        "生成用于反馈问题的诊断包。",
    ),
    ("settings.advanced.export.action", "导出"),
    ("settings.advanced.capture_format", "采集格式"),
    (
        "settings.advanced.capture_format.desc",
        "由语音模型决定，不能修改。",
    ),
    (
        "settings.advanced.capture_format.value",
        "{0} kHz，{1} 声道",
    ),
    ("settings.advanced.setup_again", "重新运行设置向导"),
    (
        "settings.advanced.setup_again.desc",
        "再走一遍首次设置；设置、模型和权限都会保留。",
    ),
    ("settings.reason.first_run", "等待首次设置向导功能。"),
    ("settings.advanced.experimental", "实验功能"),
    (
        "settings.advanced.experimental.none",
        "这个版本没有实验功能。",
    ),
    ("settings.about.version", "版本"),
    (
        "settings.about.version.desc",
        "你正在运行的 PRX Voice 版本。",
    ),
    ("settings.about.version.value", "PRX Voice {0}"),
    ("settings.about.updates", "检查更新"),
    ("settings.about.updates.desc", "检查是否有新版本。"),
    ("settings.reason.updates", "等待更新检查功能。"),
    ("settings.about.model_licences", "模型许可证"),
    (
        "settings.about.model_licences.desc",
        "每个已安装模型的许可证和来源。",
    ),
    ("settings.reason.model_licences", "等待模型管理功能。"),
    ("settings.about.notices", "开源许可证"),
    ("settings.about.notices.action", "查看文件"),
    ("settings.about.notices.chip", "声明文件"),
    (
        "settings.about.notices.missing",
        "在这份应用旁没有找到声明文件。",
    ),
    ("settings.about.data_dir", "数据文件夹"),
    ("settings.about.folder", "文件夹"),
    ("settings.about.reveal.finder", "在 Finder 中显示"),
    ("settings.about.reveal.files", "在文件管理器中显示"),
    ("settings.about.log", "日志文件"),
    ("settings.about.log.file", "文件"),
    ("settings.about.log.terminal", "写入启动 PRX Voice 的终端。"),
    ("settings.about.log.terminal.chip", "终端"),
    ("settings.about.config", "设置文件"),
    ("settings.about.file", "文件"),
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
