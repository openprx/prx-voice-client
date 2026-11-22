//! Tests of the registry: resolution from fixture configurations and a fixture catalogue,
//! construction and lifecycle over fake engines, the offline policy against a recording
//! listener, "test provider" and the disclosure. No network beyond loopback, no native code.

mod dialogue;
mod fixtures;
mod lifecycle;
mod local_voice;
mod no_mock;
mod offline;
mod problems;
mod resolution;
mod system_translation;
mod test_provider;
