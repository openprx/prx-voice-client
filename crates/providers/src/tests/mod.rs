//! Tests of the registry: resolution from fixture configurations and a fixture catalogue,
//! construction and lifecycle over fake engines, the offline policy against a recording
//! listener, "test provider" and the disclosure. No network beyond loopback, no native code.

mod fixtures;
mod lifecycle;
mod no_mock;
mod offline;
mod problems;
mod resolution;
mod test_provider;
