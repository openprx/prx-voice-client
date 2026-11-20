//! On-disk model store (design 4.3). Layout:
//!
//! ```text
//! models/
//!   store/<model-id>/<revision-key>/{files..., receipt.json}
//!   partial/<model-id>/<revision-key>/<name>.part (+ <name>.part.json)
//!   jobs.json, state.lock
//! ```
//!
//! This file declares the sub-modules: `types` (vocabulary), `fsutil` (file
//! helpers), `core` (the `ModelStore`), `gc`, `legacy` (migration) and
//! `relocate` (store move).

use std::io;
use std::path::Path;
use std::sync::Arc;

mod core;
mod fsutil;
mod gc;
mod legacy;
mod relocate;
mod types;

pub use core::ModelStore;
pub(crate) use fsutil::hash_file;
pub use relocate::MOVE_MARGIN_BYTES;
pub use types::*;

/// Free-space probe for the volume holding a path. Injectable for tests.
pub type FreeSpaceFn = Arc<dyn Fn(&Path) -> io::Result<u64> + Send + Sync>;

pub fn default_free_space() -> FreeSpaceFn {
    Arc::new(|p| fs4::available_space(p))
}
