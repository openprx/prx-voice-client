//! Garbage collection of the store: plan what can be deleted and apply it
//! (design 4.3). Single responsibility: deciding and deleting unneeded data.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::catalogue::{Catalogue, ModelId};
use crate::types::{ActiveSet, StoreError};

use super::core::*;
use super::fsutil::*;
use super::types::*;

const STALE_PARTIAL_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

impl ModelStore {
    /// proposes `UnusedModel` for an active or loaded one.
    pub fn gc_plan(&self, catalogue: &Catalogue, active: &ActiveSet, loaded: &[ModelId]) -> GcPlan {
        let mut items = Vec::new();
        let is_loaded = |id: &ModelId| loaded.contains(id);

        // Store side: tmp leftovers, old revisions, unused models.
        for model_dir in sorted_children(&self.store_dir()) {
            let id = ModelId(file_name_str(&model_dir));
            if self.is_claimed(&id) {
                continue;
            }
            let revs = sorted_children(&model_dir);
            for rev in &revs {
                let name = file_name_str(rev);
                if name.ends_with(TMP_SUFFIX) {
                    items.push(GcItem {
                        kind: GcKind::TmpDir,
                        path: rev.clone(),
                        bytes: dir_size(rev),
                        model_id: Some(id.clone()),
                    });
                }
            }
            if is_loaded(&id) {
                continue;
            }
            let current_key = catalogue
                .get(&id)
                .filter(|e| self.ready(e).is_some())
                .map(|e| e.revision_key());
            if let Some(current) = &current_key {
                for rev in &revs {
                    let name = file_name_str(rev);
                    if &name != current
                        && !name.ends_with(TMP_SUFFIX)
                        && !name.ends_with(SET_ASIDE_SUFFIX)
                        && Self::intact_receipt(rev).is_some()
                    {
                        items.push(GcItem {
                            kind: GcKind::OldRevision,
                            path: rev.clone(),
                            bytes: dir_size(rev),
                            model_id: Some(id.clone()),
                        });
                    }
                }
            }
            if !active.contains(&id) && revs.iter().any(|r| Self::intact_receipt(r).is_some()) {
                items.push(GcItem {
                    kind: GcKind::UnusedModel,
                    path: model_dir.clone(),
                    bytes: dir_size(&model_dir),
                    model_id: Some(id.clone()),
                });
            }
        }

        // Partial side: orphans and stale files.
        let now = SystemTime::now();
        for model_dir in sorted_children(&self.partial_root()) {
            let id = ModelId(file_name_str(&model_dir));
            if self.is_claimed(&id) {
                continue;
            }
            for rev_dir in sorted_children(&model_dir) {
                let rev_key = file_name_str(&rev_dir);
                for p in sorted_children(&rev_dir) {
                    let name = file_name_str(&p);
                    let bytes = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                    let orphan = |items: &mut Vec<GcItem>, path: PathBuf| {
                        items.push(GcItem {
                            kind: GcKind::OrphanPartial,
                            path,
                            bytes,
                            model_id: Some(id.clone()),
                        })
                    };
                    if let Some(stem) = name.strip_suffix(".part.json") {
                        if !rev_dir.join(format!("{stem}.part")).exists() {
                            orphan(&mut items, p.clone());
                        }
                    } else if let Some(stem) = name.strip_suffix(".part") {
                        let side: Option<Sidecar> = fs::read(rev_dir.join(format!("{name}.json")))
                            .ok()
                            .and_then(|d| serde_json::from_slice(&d).ok());
                        let matches = catalogue.get(&id).is_some_and(|e| {
                            e.revision_key() == rev_key
                                && side.as_ref().is_some_and(|s| {
                                    e.files.iter().any(|f| {
                                        f.name == stem && f.size == s.size && f.sha256 == s.sha256
                                    })
                                })
                        });
                        if !matches {
                            orphan(&mut items, p.clone());
                        } else if fs::metadata(&p)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| now.duration_since(t).ok())
                            .is_some_and(|age| age > STALE_PARTIAL_AGE)
                        {
                            items.push(GcItem {
                                kind: GcKind::StalePartial,
                                path: p.clone(),
                                bytes,
                                model_id: Some(id.clone()),
                            });
                        }
                    }
                }
            }
        }
        GcPlan { items }
    }

    /// Delete everything in the plan. Returns bytes freed.
    pub fn gc_apply(&self, plan: &GcPlan) -> Result<u64, StoreError> {
        self.require_writable()?;
        let mut freed = 0;
        for item in &plan.items {
            if let Some(id) = &item.model_id
                && self.is_claimed(id)
            {
                continue;
            }
            match item.kind {
                GcKind::UnusedModel => self.remove_model_dir(&item.path)?,
                _ => {
                    remove_path(&item.path).map_err(|e| StoreError::io(&item.path, e))?;
                    if matches!(item.kind, GcKind::OrphanPartial | GcKind::StalePartial) {
                        let side = PathBuf::from(format!("{}.json", item.path.display()));
                        let _ = fs::remove_file(side);
                    }
                }
            }
            freed += item.bytes;
            if let Some(parent) = item.path.parent() {
                let _ = fs::remove_dir(parent);
                if let Some(grand) = parent.parent()
                    && grand != self.store_dir()
                    && grand != self.partial_root()
                    && grand != self.root
                {
                    let _ = fs::remove_dir(grand);
                }
            }
        }
        Ok(freed)
    }
}
