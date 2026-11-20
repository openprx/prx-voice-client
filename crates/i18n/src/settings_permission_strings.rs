//! Translation strings of the Permissions page (design 3.5): status words, the rows, and the advice for each typed driver error code, English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. The keys of
//! `settings.driver.error.<code>` are chosen from the code of the app's typed driver error.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("settings.perm.needed_for", "Needed for: {0}"),
    ("settings.perm.without", "Without it: {0}"),
    ("settings.status.allowed", "Allowed"),
    ("settings.status.installed", "Installed"),
    ("settings.status.installed.versioned", "Installed {0}"),
    ("settings.status.not_allowed", "Not allowed"),
    ("settings.status.not_asked", "Not asked yet"),
    ("settings.status.not_checked", "Not checked yet"),
    ("settings.status.not_installed", "Not installed"),
    ("settings.status.not_needed", "Not needed"),
    ("settings.status.not_used", "Not used"),
    ("settings.status.update_available", "Update available"),
    ("settings.status.waiting", "Waiting for System Settings"),
    ("settings.status.wrong_variant", "Wrong variant"),
    ("settings.status.needs_repair", "Needs repair"),
    ("settings.perm.microphone", "Microphone"),
    ("settings.perm.microphone.for", "Hearing you, in both modes"),
    ("settings.perm.microphone.without", "No session can start"),
    ("settings.perm.call_audio", "Call audio"),
    (
        "settings.perm.call_audio.for",
        "Hearing the other person in call translation",
    ),
    (
        "settings.perm.call_audio.without",
        "The other person's speech is not translated; yours still is",
    ),
    ("settings.perm.allow", "Allow"),
    ("settings.perm.check_now", "Check now"),
    ("settings.perm.check_again", "Check again"),
    ("settings.perm.open_system_settings", "Open System Settings"),
    ("settings.perm.shortcuts", "Keyboard shortcuts"),
    (
        "settings.perm.shortcuts.desc",
        "Ordinary shortcuts need no Accessibility or Input Monitoring permission.",
    ),
    ("settings.perm.driver", "Virtual microphone"),
    (
        "settings.perm.driver.for",
        "Letting the other person hear your translated voice",
    ),
    (
        "settings.perm.driver.without",
        "You can hear translations, but the other person cannot",
    ),
    ("settings.driver.install", "Install"),
    ("settings.driver.update", "Update"),
    ("settings.driver.repair", "Repair"),
    ("settings.driver.uninstall", "Uninstall"),
    ("settings.driver.working", "Working…"),
    ("settings.driver.variant", "Installed variant: {0}"),
    (
        "settings.driver.no_bundle",
        "This copy of PRX Voice carries no driver. Start the app from PRX Voice.app to install it.",
    ),
    (
        "settings.driver.uninstall.confirm",
        "Remove the PRX Voice microphone? Call apps using it will lose their microphone.",
    ),
    ("settings.driver.uninstall.yes", "Remove"),
    (
        "settings.driver.done.install",
        "The microphone driver is installed.",
    ),
    (
        "settings.driver.done.uninstall",
        "The microphone driver was removed.",
    ),
    (
        "settings.driver.error.bundle_not_found",
        "This copy of PRX Voice has no driver inside. Start the app from PRX Voice.app.",
    ),
    (
        "settings.driver.error.source_rejected",
        "The driver inside the app failed its checks. Reinstall PRX Voice from a fresh download.",
    ),
    (
        "settings.driver.error.cancelled",
        "Nothing was changed. Try again and approve the administrator prompt.",
    ),
    (
        "settings.driver.error.authorization_failed",
        "The administrator authorization failed. Check the account password and that an administrator is signed in.",
    ),
    (
        "settings.driver.error.copy_failed",
        "The driver could not be copied into the system. Nothing was started; try again, and report this if it repeats.",
    ),
    (
        "settings.driver.error.restart_failed",
        "The driver was copied but the audio service did not restart. Restart the Mac, then check again.",
    ),
    (
        "settings.driver.error.device_did_not_appear",
        "The driver was copied but macOS did not load it. Restart the Mac; if the device is still missing, report it.",
    ),
    (
        "settings.driver.error.device_still_present",
        "The driver was removed but macOS still lists the device. Wait a few seconds or restart the Mac.",
    ),
    (
        "settings.driver.error.unsupported",
        "The virtual microphone driver is available on macOS only.",
    ),
    (
        "settings.driver.error.other",
        "The driver operation failed ({0}). See the log for details.",
    ),
    ("settings.perm.notifications", "Notifications"),
    (
        "settings.perm.notifications.desc",
        "PRX Voice reports everything in its own windows.",
    ),
    (
        "settings.perm.network.for",
        "Model downloads, update checks, cloud providers and a remote assistant server",
    ),
    (
        "settings.perm.network.without",
        "Only local models and local servers work",
    ),
    ("settings.perm.offline", "Offline mode"),
    (
        "settings.perm.offline.desc",
        "Blocks all network traffic except this computer, local provider servers and the hosts below.",
    ),
    ("settings.perm.allow_hosts", "Allowed local hosts"),
    (
        "settings.perm.allow_hosts.desc",
        "Extra hosts that stay reachable in offline mode, for example 192.168.1.20.",
    ),
    ("settings.perm.allow_hosts.remove", "Remove"),
    ("settings.perm.allow_hosts.remove_named", "Remove {0}"),
    ("settings.perm.allow_hosts.add_label", "Add a host"),
    ("settings.perm.allow_hosts.add", "Add"),
    ("settings.perm.reachable", "Will still be reached"),
    (
        "settings.perm.reachable.loopback",
        "127.0.0.1 and localhost (this computer)",
    ),
    (
        "settings.perm.bundle",
        "Running outside the app bundle. macOS gives microphone and audio-capture permission to the app that started PRX Voice (for example Terminal) and shows that name in System Settings. Start PRX Voice from its .app so the permissions belong to it.",
    ),
    (
        "settings.perm.outcome.not_in_bundle",
        "macOS can only ask for this permission from the app bundle. Start PRX Voice from its .app.",
    ),
    (
        "settings.perm.outcome.refused",
        "Refused. macOS will not ask again; open System Settings to allow it.",
    ),
    (
        "settings.perm.outcome.no_answer",
        "The system prompt was not answered in time. Try again.",
    ),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("settings.perm.needed_for", "用途：{0}"),
    ("settings.perm.without", "没有它：{0}"),
    ("settings.status.allowed", "已允许"),
    ("settings.status.installed", "已安装"),
    ("settings.status.installed.versioned", "已安装 {0}"),
    ("settings.status.not_allowed", "未允许"),
    ("settings.status.not_asked", "尚未询问"),
    ("settings.status.not_checked", "尚未检查"),
    ("settings.status.not_installed", "未安装"),
    ("settings.status.not_needed", "无需权限"),
    ("settings.status.not_used", "未使用"),
    ("settings.status.update_available", "有可用更新"),
    ("settings.status.waiting", "等待系统设置"),
    ("settings.status.wrong_variant", "类型不符"),
    ("settings.status.needs_repair", "需要修复"),
    ("settings.perm.microphone", "麦克风"),
    (
        "settings.perm.microphone.for",
        "听到你说话（两种模式都需要）",
    ),
    ("settings.perm.microphone.without", "无法开始任何会话"),
    ("settings.perm.call_audio", "通话音频"),
    ("settings.perm.call_audio.for", "在通话翻译中听到对方的声音"),
    (
        "settings.perm.call_audio.without",
        "无法翻译对方的话，你说的话仍可翻译",
    ),
    ("settings.perm.allow", "允许"),
    ("settings.perm.check_now", "立即检查"),
    ("settings.perm.check_again", "重新检查"),
    ("settings.perm.open_system_settings", "打开系统设置"),
    ("settings.perm.shortcuts", "键盘快捷键"),
    (
        "settings.perm.shortcuts.desc",
        "普通快捷键不需要“辅助功能”或“输入监控”权限。",
    ),
    ("settings.perm.driver", "虚拟麦克风"),
    ("settings.perm.driver.for", "让对方听到你的译文语音"),
    ("settings.perm.driver.without", "你能听到译文，但对方听不到"),
    ("settings.driver.install", "安装"),
    ("settings.driver.update", "更新"),
    ("settings.driver.repair", "修复"),
    ("settings.driver.uninstall", "卸载"),
    ("settings.driver.working", "处理中…"),
    ("settings.driver.variant", "已安装的类型：{0}"),
    (
        "settings.driver.no_bundle",
        "这份 PRX Voice 不含驱动。请从 PRX Voice.app 启动后再安装。",
    ),
    (
        "settings.driver.uninstall.confirm",
        "要移除 PRX Voice 麦克风吗？正在使用它的通话应用会失去麦克风。",
    ),
    ("settings.driver.uninstall.yes", "移除"),
    ("settings.driver.done.install", "麦克风驱动已安装。"),
    ("settings.driver.done.uninstall", "麦克风驱动已移除。"),
    (
        "settings.driver.error.bundle_not_found",
        "这份 PRX Voice 里没有驱动。请从 PRX Voice.app 启动。",
    ),
    (
        "settings.driver.error.source_rejected",
        "应用内的驱动没有通过检查。请重新下载并安装 PRX Voice。",
    ),
    (
        "settings.driver.error.cancelled",
        "没有做任何更改。请重试，并在管理员授权窗口中允许。",
    ),
    (
        "settings.driver.error.authorization_failed",
        "管理员授权失败。请检查账户密码，并确认当前是管理员账户。",
    ),
    (
        "settings.driver.error.copy_failed",
        "无法把驱动复制到系统中，也没有启动任何内容。请重试，如反复出现请反馈。",
    ),
    (
        "settings.driver.error.restart_failed",
        "驱动已复制，但音频服务没有重启。请重启 Mac 后重新检查。",
    ),
    (
        "settings.driver.error.device_did_not_appear",
        "驱动已复制，但 macOS 没有加载它。请重启 Mac；若设备仍未出现，请反馈。",
    ),
    (
        "settings.driver.error.device_still_present",
        "驱动已移除，但 macOS 仍显示该设备。请等几秒或重启 Mac。",
    ),
    (
        "settings.driver.error.unsupported",
        "虚拟麦克风驱动仅支持 macOS。",
    ),
    (
        "settings.driver.error.other",
        "驱动操作失败（{0}），详情见日志。",
    ),
    ("settings.perm.notifications", "通知"),
    (
        "settings.perm.notifications.desc",
        "PRX Voice 的所有提示都显示在自己的窗口里。",
    ),
    (
        "settings.perm.network.for",
        "模型下载、更新检查、云端服务和远程助手服务器",
    ),
    (
        "settings.perm.network.without",
        "只能使用本地模型和本地服务器",
    ),
    ("settings.perm.offline", "离线模式"),
    (
        "settings.perm.offline.desc",
        "除本机、本地服务器和下面列出的主机外，阻止所有网络访问。",
    ),
    ("settings.perm.allow_hosts", "允许访问的本地主机"),
    (
        "settings.perm.allow_hosts.desc",
        "离线模式下仍可访问的额外主机，例如 192.168.1.20。",
    ),
    ("settings.perm.allow_hosts.remove", "移除"),
    ("settings.perm.allow_hosts.remove_named", "移除 {0}"),
    ("settings.perm.allow_hosts.add_label", "添加主机"),
    ("settings.perm.allow_hosts.add", "添加"),
    ("settings.perm.reachable", "仍会访问"),
    (
        "settings.perm.reachable.loopback",
        "127.0.0.1 和 localhost（本机）",
    ),
    (
        "settings.perm.bundle",
        "当前不是从应用包（.app）中运行。macOS 会把麦克风和音频捕获权限记在启动 PRX Voice 的应用（例如“终端”）名下，并在系统设置中显示那个名字。请从 PRX Voice.app 启动，权限才会属于它。",
    ),
    (
        "settings.perm.outcome.not_in_bundle",
        "macOS 只会向应用包请求这项权限。请从 PRX Voice.app 启动。",
    ),
    (
        "settings.perm.outcome.refused",
        "已被拒绝。macOS 不会再次询问，请到系统设置中允许。",
    ),
    (
        "settings.perm.outcome.no_answer",
        "系统提示没有及时回应，请重试。",
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

    #[test]
    fn placeholders_agree_between_the_languages() {
        let count = |s: &str| (0..6).filter(|i| s.contains(&format!("{{{i}}}"))).count();
        for ((k, en), (_, zh)) in EN.iter().zip(ZH_CN) {
            assert_eq!(count(en), count(zh), "placeholders differ in {k}");
        }
    }
}
