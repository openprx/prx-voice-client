//! Strings of the graceful stop of a translated call (the state while the last sentences are played, and the Stop button then), English and Simplified Chinese. Same keys in both tables, in the same order; a test checks it.

/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("callwin.chip.finishing", "Finishing"),
    (
        "callwin.finishing.hint",
        "Playing the last sentences. Press Stop again to end now ({0} s at most).",
    ),
    ("callwin.stop_now", "Stop now"),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("callwin.chip.finishing", "正在收尾"),
    (
        "callwin.finishing.hint",
        "正在播放最后几句。再按一次停止可立即结束（最多 {0} 秒）。",
    ),
    ("callwin.stop_now", "立即停止"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_tables_have_the_same_keys_in_the_same_order() {
        let keys = |t: &[(&str, &str)]| t.iter().map(|(k, _)| (*k).to_owned()).collect::<Vec<_>>();
        assert_eq!(keys(EN), keys(ZH_CN));
    }

    #[test]
    fn the_hint_names_the_limit_in_both_languages() {
        for table in [EN, ZH_CN] {
            let hint = table
                .iter()
                .find(|(k, _)| *k == "callwin.finishing.hint")
                .unwrap()
                .1;
            assert!(hint.contains("{0}"));
        }
    }
}
