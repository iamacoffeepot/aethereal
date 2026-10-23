//! What a host keeps of the file capability (ADR-0032).

use crate::load::Load;
use aethereal_actor::{JobType, Storage};
use std::sync::Arc;

/// What the file capability's bootstrap serves its host: the host's storage, and the job the capability runs to read
/// it. The host makes its runners with both (ADR-0029: `Engine::set_jobs`), so a program lists the capability once and
/// never lists its job.
pub struct FileHosting {
    storage: Arc<dyn Storage>,
    job: JobType,
}

impl FileHosting {
    /// What a host serving `storage` keeps.
    #[must_use]
    pub fn new(storage: impl Storage + 'static) -> Self {
        Self { storage: Arc::new(storage), job: JobType::of::<Load>() }
    }

    /// The host's storage, and the job the capability runs, for its runners.
    #[must_use]
    pub fn into_parts(self) -> (Arc<dyn Storage>, JobType) {
        (self.storage, self.job)
    }
}
