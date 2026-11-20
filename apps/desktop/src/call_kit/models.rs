//! The model store as the provider registry's source of models: what is installed and verified, read from the store on disk.

use std::sync::Arc;

use prxvc_models::{
    Catalogue, FailReason, InstallState, ModelId, ModelStore, NotReady, ReadyModel,
    StoreInstallState,
};
use prxvc_providers::ModelSource;

/// Answers "is this model ready" from the store's receipts, without the model manager's threads
/// (the manager is not part of the application yet). Nothing is downloaded or changed.
pub(crate) struct StoreModels {
    store: ModelStore,
    catalogue: Arc<Catalogue>,
}

impl StoreModels {
    /// Open the store at `models_dir`. A store another process holds opens read-only, which is
    /// all this needs.
    pub(crate) fn open(
        models_dir: std::path::PathBuf,
        catalogue: Arc<Catalogue>,
    ) -> Result<Self, String> {
        let store = ModelStore::open(models_dir).map_err(|e| e.to_string())?;
        Ok(Self { store, catalogue })
    }
}

impl ModelSource for StoreModels {
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady> {
        let entry = self.catalogue.get(id).ok_or(NotReady::NoSelection)?;
        if let Some(ready) = self.store.ready(entry) {
            return Ok(ready);
        }
        match self.store.install_state(entry) {
            StoreInstallState::Corrupt => Err(NotReady::Failed {
                id: id.clone(),
                reason: FailReason::ChecksumMismatch {
                    file: entry
                        .files
                        .first()
                        .map(|f| f.name.clone())
                        .unwrap_or_default(),
                },
            }),
            StoreInstallState::Partial { done } => Err(NotReady::InProgress {
                id: id.clone(),
                state: InstallState::Paused {
                    done,
                    total: entry.total_size(),
                },
            }),
            _ => Err(NotReady::NotInstalled {
                id: id.clone(),
                size: entry.total_size(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_store_has_no_model_ready() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let catalogue = Arc::new(Catalogue::embedded());
        let models =
            StoreModels::open(dir.path().to_path_buf(), Arc::clone(&catalogue)).expect("opens");
        let id = catalogue.models().first().expect("a model").id.clone();
        assert!(matches!(
            models.ready(&id),
            Err(NotReady::NotInstalled { .. })
        ));
        assert!(matches!(
            models.ready(&ModelId::new("no-such-model")),
            Err(NotReady::NoSelection)
        ));
    }
}
