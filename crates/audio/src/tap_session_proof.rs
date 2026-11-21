//! Whether system audio recording consent has been proven in this application run: any tap delivered a non-zero sample.
//!
//! A tap without consent delivers exact zeros; a tap that delivered a real sample once proves
//! the consent for the rest of the run, so a restart of call mode must not suspect it again.
//! Process-wide on purpose: the proof outlives the single tap and call that produced it.

use std::sync::atomic::{AtomicBool, Ordering};

static PROVEN: AtomicBool = AtomicBool::new(false);

/// Whether a tap of this run has delivered a non-zero sample.
pub fn consent_proven() -> bool {
    PROVEN.load(Ordering::Acquire)
}

/// Remember that a tap of this run has delivered a non-zero sample.
pub fn mark_consent_proven() {
    PROVEN.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proof_stays_for_the_rest_of_the_run() {
        // The only test that touches the flag: nothing else in this crate's tests reads it.
        mark_consent_proven();
        assert!(consent_proven());
        mark_consent_proven();
        assert!(consent_proven());
    }
}
