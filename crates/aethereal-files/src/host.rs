//! What a host keeps of the file capability (ADR-0032).

use aethereal_actor::Storage;
use std::sync::Arc;

/// What the file capability's bootstrap serves its host: the host's storage, which the host makes its runners with
/// (ADR-0029: `Engine::set_jobs`), so the capability's jobs read files from it. Its jobs carry their own code, so the
/// host is handed none of it (ADR-0018).
pub struct FileHosting {
    storage: Arc<dyn Storage>,
}

impl FileHosting {
    /// What a host serving `storage` keeps.
    #[must_use]
    pub fn new(storage: impl Storage + 'static) -> Self {
        Self { storage: Arc::new(storage) }
    }

    /// The host's storage, for its runners.
    #[must_use]
    pub fn into_storage(self) -> Arc<dyn Storage> {
        self.storage
    }
}
