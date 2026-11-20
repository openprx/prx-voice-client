//! Versioned migration of `config.toml` (design section 2.5).
//!
//! A file carries a top-level `schema_version`. A file without one is the
//! unversioned file written by every build before this module existed
//! (version 0). [`migrate`] runs, in order, every step from the file's version
//! up to [`CURRENT_SCHEMA_VERSION`] on the parsed `toml::Table`, before it is
//! deserialised into the typed config. Steps are idempotent. The migrated
//! table is written back only by the next save.
//!
//! To add a schema version: bump [`CURRENT_SCHEMA_VERSION`] and append one
//! entry to [`CHAIN`].

use crate::ConfigError;

/// The schema version this build reads and writes.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

type Step = fn(&mut toml::Table) -> Result<(), ConfigError>;

/// One link of the chain: the steps that take a file from version
/// `from` to `from + 1`.
struct Link {
    from: u32,
    steps: &'static [(&'static str, Step)],
}

/// The ordered chain. `CHAIN[i].from == i`.
const CHAIN: &[Link] = &[
    Link {
        from: 0,
        steps: &[
            ("echo_control to echo_mode", echo_control_to_echo_mode),
            ("pin save_transcript of existing files", pin_legacy_save_transcript),
        ],
    },
    Link {
        from: 1,
        steps: &[("pin the call-translation keys of [call]", pin_call_translation_keys)],
    },
];

/// Schema version recorded in `table`; 0 when absent.
pub fn file_version(table: &toml::Table) -> Result<u32, ConfigError> {
    match table.get("schema_version") {
        None => Ok(0),
        Some(toml::Value::Integer(v)) => u32::try_from(*v)
            .map_err(|_| ConfigError::MigrationError(format!("invalid schema_version {v}"))),
        Some(other) => Err(ConfigError::MigrationError(format!(
            "schema_version must be an integer, found {}",
            other.type_str()
        ))),
    }
}

/// Upgrade `table` to [`CURRENT_SCHEMA_VERSION`].
///
/// Returns the version the table had when it needed an upgrade, or `None`
/// when it was current (or newer: a file from a newer build is left as it is).
pub fn migrate(table: &mut toml::Table) -> Result<Option<u32>, ConfigError> {
    let from = file_version(table)?;
    if from >= CURRENT_SCHEMA_VERSION {
        return Ok(None);
    }
    for version in from..CURRENT_SCHEMA_VERSION {
        let link = CHAIN
            .iter()
            .find(|l| l.from == version)
            .ok_or_else(|| ConfigError::MigrationError(format!("no migration from schema {version}")))?;
        for (name, step) in link.steps {
            step(table).map_err(|e| ConfigError::MigrationError(format!("{name}: {e}")))?;
        }
        table.insert(
            "schema_version".to_owned(),
            toml::Value::Integer(i64::from(version) + 1),
        );
    }
    Ok(Some(from))
}

/// `audio.echo_control` becomes `audio.echo_mode` (`true` is `"auto"`, `false`
/// is `"off"`), unless `echo_mode` is already there. `echo_control` is kept:
/// older builds still read it.
pub(crate) fn echo_control_to_echo_mode(table: &mut toml::Table) -> Result<(), ConfigError> {
    let Some(audio) = table.get_mut("audio").and_then(|v| v.as_table_mut()) else {
        return Ok(());
    };
    if audio.contains_key("echo_mode") {
        return Ok(());
    }
    if let Some(toml::Value::Boolean(on)) = audio.get("echo_control") {
        let mode = if *on { "auto" } else { "off" };
        audio.insert("echo_mode".to_owned(), toml::Value::String(mode.to_owned()));
    }
    Ok(())
}

/// The default of `privacy.save_transcript` changed from `true` to `false` for
/// new installs only. A file that already exists was written under the old
/// default, so a missing key is pinned to `true`: the behaviour that user has
/// today does not change. A key that is present is not touched.
pub(crate) fn pin_legacy_save_transcript(table: &mut toml::Table) -> Result<(), ConfigError> {
    let privacy = table
        .entry("privacy")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let privacy = privacy
        .as_table_mut()
        .ok_or_else(|| ConfigError::MigrationError("`privacy` is not a table".to_owned()))?;
    privacy
        .entry("save_transcript")
        .or_insert(toml::Value::Boolean(true));
    Ok(())
}

/// Version 2 adds the call-translation keys of `[call]` (what each party
/// hears while a direction is paused or has failed, call start mode, original
/// gain, barge-in lag). A file that already exists gets them written with the
/// values below, which are the defaults of version 2 and are spelled out here
/// so that this step never changes when a later default does. A key that is
/// present is not touched.
///
/// `[pipeline]` and `providers.active.asr_refine` are deliberately not
/// written: they are tuning and a role selection that is filled in when the
/// user chooses, and the typed config supplies their defaults.
pub(crate) fn pin_call_translation_keys(table: &mut toml::Table) -> Result<(), ConfigError> {
    use toml::Value::{Float, Integer, String as Str};

    let call = table
        .entry("call")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let call = call
        .as_table_mut()
        .ok_or_else(|| ConfigError::MigrationError("`call` is not a table".to_owned()))?;
    let pinned = [
        ("start_mode", Str("manual".to_owned())),
        ("outgoing_when_paused", Str("original".to_owned())),
        ("incoming_when_paused", Str("original".to_owned())),
        ("outgoing_on_failure", Str("original".to_owned())),
        ("incoming_on_failure", Str("original".to_owned())),
        ("original_gain_db", Float(-12.0)),
        ("barge_in_max_lag_ms", Integer(3_000)),
    ];
    for (key, value) in pinned {
        call.entry(key).or_insert(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> toml::Table {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn echo_control_true_gives_auto() {
        let mut t = parse("[audio]\necho_control = true\n");
        echo_control_to_echo_mode(&mut t).unwrap();
        assert_eq!(t["audio"]["echo_mode"].as_str(), Some("auto"));
        assert_eq!(t["audio"]["echo_control"].as_bool(), Some(true));
    }

    #[test]
    fn echo_control_false_gives_off() {
        let mut t = parse("[audio]\necho_control = false\n");
        echo_control_to_echo_mode(&mut t).unwrap();
        assert_eq!(t["audio"]["echo_mode"].as_str(), Some("off"));
    }

    #[test]
    fn existing_echo_mode_wins_and_missing_audio_is_fine() {
        let mut t = parse("[audio]\necho_control = false\necho_mode = \"always\"\n");
        echo_control_to_echo_mode(&mut t).unwrap();
        assert_eq!(t["audio"]["echo_mode"].as_str(), Some("always"));
        let mut empty = parse("");
        echo_control_to_echo_mode(&mut empty).unwrap();
        assert!(empty.is_empty());
        let mut no_key = parse("[audio]\nsample_rate = 16000\n");
        echo_control_to_echo_mode(&mut no_key).unwrap();
        assert!(no_key["audio"].get("echo_mode").is_none());
    }

    #[test]
    fn save_transcript_is_pinned_only_when_missing() {
        let mut missing = parse("[privacy]\nsave_history = false\n");
        pin_legacy_save_transcript(&mut missing).unwrap();
        assert_eq!(missing["privacy"]["save_transcript"].as_bool(), Some(true));
        let mut no_section = parse("");
        pin_legacy_save_transcript(&mut no_section).unwrap();
        assert_eq!(no_section["privacy"]["save_transcript"].as_bool(), Some(true));
        let mut present = parse("[privacy]\nsave_transcript = false\n");
        pin_legacy_save_transcript(&mut present).unwrap();
        assert_eq!(present["privacy"]["save_transcript"].as_bool(), Some(false));
        let mut bad = parse("privacy = 3\n");
        assert!(pin_legacy_save_transcript(&mut bad).is_err());
    }

    #[test]
    fn chain_upgrades_an_unversioned_file_and_is_idempotent() {
        let mut t = parse("[audio]\necho_control = false\n");
        assert_eq!(migrate(&mut t).unwrap(), Some(0));
        assert_eq!(
            t["schema_version"].as_integer(),
            Some(i64::from(CURRENT_SCHEMA_VERSION))
        );
        assert_eq!(t["audio"]["echo_mode"].as_str(), Some("off"));
        assert_eq!(t["privacy"]["save_transcript"].as_bool(), Some(true));
        assert_eq!(t["call"]["start_mode"].as_str(), Some("manual"));
        let once = t.clone();
        assert_eq!(migrate(&mut t).unwrap(), None);
        assert_eq!(t, once);
    }

    #[test]
    fn call_translation_keys_are_pinned_only_when_missing() {
        let mut missing = parse("[call]\nbarge_in = true\nlocal_output_mode = \"both\"\n");
        pin_call_translation_keys(&mut missing).unwrap();
        let call = &missing["call"];
        assert_eq!(call["start_mode"].as_str(), Some("manual"));
        assert_eq!(call["outgoing_when_paused"].as_str(), Some("original"));
        assert_eq!(call["incoming_when_paused"].as_str(), Some("original"));
        assert_eq!(call["outgoing_on_failure"].as_str(), Some("original"));
        assert_eq!(call["incoming_on_failure"].as_str(), Some("original"));
        assert_eq!(call["original_gain_db"].as_float(), Some(-12.0));
        assert_eq!(call["barge_in_max_lag_ms"].as_integer(), Some(3_000));
        assert_eq!(
            call["local_output_mode"].as_str(),
            Some("both"),
            "existing keys are untouched"
        );

        let mut present = parse("[call]\nstart_mode = \"on_call\"\noriginal_gain_db = -6.0\n");
        pin_call_translation_keys(&mut present).unwrap();
        assert_eq!(present["call"]["start_mode"].as_str(), Some("on_call"));
        assert_eq!(present["call"]["original_gain_db"].as_float(), Some(-6.0));
        assert_eq!(
            present["call"]["barge_in_max_lag_ms"].as_integer(),
            Some(3_000)
        );
    }

    #[test]
    fn the_call_step_creates_a_missing_section_and_rejects_a_non_table() {
        let mut empty = parse("");
        pin_call_translation_keys(&mut empty).unwrap();
        assert_eq!(empty["call"]["start_mode"].as_str(), Some("manual"));
        let mut bad = parse("call = 3\n");
        assert!(pin_call_translation_keys(&mut bad).is_err());
    }

    #[test]
    fn the_call_step_is_idempotent() {
        let mut t = parse("[call]\nbarge_in = false\n");
        pin_call_translation_keys(&mut t).unwrap();
        let once = t.clone();
        pin_call_translation_keys(&mut t).unwrap();
        assert_eq!(t, once);
    }

    #[test]
    fn a_version_1_file_takes_only_the_second_link() {
        let mut t = parse(
            "schema_version = 1\n[audio]\necho_control = false\n[privacy]\nsave_transcript = false\n",
        );
        assert_eq!(migrate(&mut t).unwrap(), Some(1));
        assert_eq!(t["schema_version"].as_integer(), Some(2));
        assert!(
            t["audio"].get("echo_mode").is_none(),
            "version 0 steps do not run again"
        );
        assert_eq!(t["privacy"]["save_transcript"].as_bool(), Some(false));
        assert_eq!(t["call"]["outgoing_on_failure"].as_str(), Some("original"));
    }

    #[test]
    fn chain_covers_every_version_up_to_current() {
        for v in 0..CURRENT_SCHEMA_VERSION {
            assert!(CHAIN.iter().any(|l| l.from == v), "no link from {v}");
        }
        assert!(CHAIN.iter().all(|l| l.from < CURRENT_SCHEMA_VERSION));
    }

    #[test]
    fn newer_files_are_left_alone_and_bad_versions_are_errors() {
        let mut newer = parse("schema_version = 99\n[audio]\necho_control = false\n");
        assert_eq!(migrate(&mut newer).unwrap(), None);
        assert!(newer["audio"].get("echo_mode").is_none());
        assert!(file_version(&parse("schema_version = \"x\"")).is_err());
        assert!(file_version(&parse("schema_version = -1")).is_err());
    }
}
