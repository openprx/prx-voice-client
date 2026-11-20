//! The sections of the settings window, their order and their stable page keys.

/// A page of the settings window, in the order of the section list (design 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsSection {
    /// Mode, interface language, theme and main-window behaviour.
    #[default]
    General,
    /// Languages, call application, what the user hears.
    CallTranslation,
    /// The voice assistant and its server.
    Assistant,
    /// Devices, echo protection and the virtual microphone.
    Audio,
    /// The user's voice and the voices used for the other party.
    Voice,
    /// Providers per role, what leaves the machine, problems.
    SpeechPipeline,
    /// Operating-system permissions and prerequisites, offline mode.
    Permissions,
    /// Global keyboard shortcuts.
    Shortcuts,
    /// History, transcripts and logs.
    Privacy,
    /// Developer tools, diagnostics and tuning.
    Advanced,
    /// Version, licences and where the data lives.
    About,
}

impl SettingsSection {
    /// Every section in list order.
    pub const ALL: [SettingsSection; 11] = [
        Self::General,
        Self::CallTranslation,
        Self::Assistant,
        Self::Audio,
        Self::Voice,
        Self::SpeechPipeline,
        Self::Permissions,
        Self::Shortcuts,
        Self::Privacy,
        Self::Advanced,
        Self::About,
    ];

    /// The key stored in `ui.settings_window.page`.
    pub fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::CallTranslation => "call",
            Self::Assistant => "assistant",
            Self::Audio => "audio",
            Self::Voice => "voice",
            Self::SpeechPipeline => "pipeline",
            Self::Permissions => "permissions",
            Self::Shortcuts => "shortcuts",
            Self::Privacy => "privacy",
            Self::Advanced => "advanced",
            Self::About => "about",
        }
    }

    /// The section stored under `key`; `None` for an unknown key.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Translated name for the section list and the heading.
    pub fn label(self) -> String {
        prxvc_i18n::t(&format!("settings.nav.{}", self.key()))
    }

    /// Position in [`ALL`](Self::ALL).
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The next section, wrapping around.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// The previous section, wrapping around.
    pub fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_are_unique() {
        for s in SettingsSection::ALL {
            assert_eq!(SettingsSection::from_key(s.key()), Some(s));
        }
        let mut keys: Vec<_> = SettingsSection::ALL.iter().map(|s| s.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SettingsSection::ALL.len());
        assert_eq!(SettingsSection::from_key("nope"), None);
    }

    #[test]
    fn neighbours_wrap_around() {
        assert_eq!(SettingsSection::About.next(), SettingsSection::General);
        assert_eq!(SettingsSection::General.previous(), SettingsSection::About);
        assert_eq!(
            SettingsSection::General.next(),
            SettingsSection::CallTranslation
        );
    }
}
