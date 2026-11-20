//! Where the registry asks whether a catalogue model is ready.

use prxvc_models::{ModelHandle, ModelId, NotReady, ReadyModel};

/// The registry's view of the model manager: the verified files of a model, or why it has none.
pub trait ModelSource: Send + Sync {
    /// The ready model with catalogue id `id`.
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady>;
}

impl ModelSource for ModelHandle {
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady> {
        ModelHandle::ready(self, id)
    }
}
