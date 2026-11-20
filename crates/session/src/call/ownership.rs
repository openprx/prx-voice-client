//! Who owns the microphone path: call mode and an assistant session are mutually exclusive.
//!
//! Both want the physical microphone (the assistant for recognition, call mode
//! for the outgoing path), and call mode also moves the system default input.
//! Letting both run would feed two consumers from one device and leave the
//! assistant listening to the virtual microphone. The rule: whoever claims the
//! microphone first keeps it until its claim is dropped; the other is refused
//! with [`MicBusy`], and the refusal is shown to the user.

use std::sync::{Arc, Mutex, PoisonError};

/// A user of the microphone path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MicOwner {
    /// Call mode.
    Call,
    /// An assistant session.
    Assistant,
}

/// The microphone is held by someone else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the microphone is in use by {held_by:?}")]
pub struct MicBusy {
    /// The current owner.
    pub held_by: MicOwner,
}

/// The shared record of the owner. Clones refer to the same record.
#[derive(Clone, Debug, Default)]
pub struct MicOwnership {
    owner: Arc<Mutex<Option<MicOwner>>>,
}

/// A held claim; the microphone is released when it is dropped.
#[derive(Debug)]
pub struct MicClaim {
    owner: MicOwner,
    ownership: MicOwnership,
}

impl MicOwnership {
    /// A record with no owner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim the microphone for `owner`.
    ///
    /// # Errors
    /// [`MicBusy`] naming the current owner (also when it is `owner` itself:
    /// a second claim for the same use is a mistake to surface, not to allow).
    pub fn claim(&self, owner: MicOwner) -> Result<MicClaim, MicBusy> {
        let mut slot = self.owner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held_by) = *slot {
            return Err(MicBusy { held_by });
        }
        *slot = Some(owner);
        Ok(MicClaim {
            owner,
            ownership: self.clone(),
        })
    }

    /// The current owner, if any.
    pub fn holder(&self) -> Option<MicOwner> {
        *self.owner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl MicClaim {
    /// The owner this claim is for.
    pub fn owner(&self) -> MicOwner {
        self.owner
    }
}

impl Drop for MicClaim {
    fn drop(&mut self) {
        let mut slot = self
            .ownership
            .owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *slot == Some(self.owner) {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_claim_is_refused_and_names_the_holder() {
        let m = MicOwnership::new();
        let call = m.claim(MicOwner::Call).expect("free");
        assert_eq!(m.holder(), Some(MicOwner::Call));
        assert_eq!(
            m.claim(MicOwner::Assistant).unwrap_err(),
            MicBusy {
                held_by: MicOwner::Call
            }
        );
        assert_eq!(m.claim(MicOwner::Call).unwrap_err().held_by, MicOwner::Call);
        drop(call);
        assert_eq!(m.holder(), None);
        let assistant = m.claim(MicOwner::Assistant).expect("released");
        assert_eq!(assistant.owner(), MicOwner::Assistant);
        assert_eq!(
            m.claim(MicOwner::Call).unwrap_err().held_by,
            MicOwner::Assistant
        );
    }

    #[test]
    fn clones_share_one_record() {
        let a = MicOwnership::new();
        let b = a.clone();
        let _claim = a.claim(MicOwner::Assistant).unwrap();
        assert_eq!(b.holder(), Some(MicOwner::Assistant));
    }
}
