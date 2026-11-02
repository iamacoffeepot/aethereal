//! What wakes the host when something comes in while it sleeps between steps: a push to a source's feed, or a job
//! finished (ADR-0010, ADR-0018).

use super::Engine;
use std::sync::{Arc, PoisonError, RwLock};
use std::task::Waker;

impl Engine {
    /// The engine, waking `wake` whenever a source is pushed or a job finishes, as [`set_wake`](Self::set_wake) does.
    pub(super) fn with_wake(mut self, wake: Waker) -> Self {
        self.set_wake(wake);

        self
    }

    /// Wakes `wake` from now on whenever a source is pushed or a job finishes, once what it brings is waiting for the
    /// next [`bring`](Self::bring), in place of what it woke before (ADR-0010, ADR-0018). Every feed made and every
    /// job handed out wakes it, those from before it was set among them. A host that sleeps between steps wakes on it
    /// to bring in at once what came in; an engine given none wakes nothing.
    pub fn set_wake(&mut self, wake: Waker) {
        self.bell.set(wake);
    }
}

/// The waker the host gave the engine, shared by every feed the engine makes and every job it hands out, so one given
/// after they were made reaches them all the same. Until the host gives one, it wakes nothing.
#[derive(Clone)]
pub(super) struct Bell {
    waker: Arc<RwLock<Waker>>,
}

impl Bell {
    /// A bell that wakes nothing, until the host gives it a waker.
    pub(super) fn new() -> Self {
        Self { waker: Arc::new(RwLock::new(Waker::noop().clone())) }
    }

    /// Wakes `waker` from now on, in place of what it woke before.
    pub(super) fn set(&self, waker: Waker) {
        // A waker that panicked while it woke left the one it held, which is still a waker.
        *self.waker.write().unwrap_or_else(PoisonError::into_inner) = waker;
    }

    /// Wakes the host.
    pub(super) fn ring(&self) {
        self.waker.read().unwrap_or_else(PoisonError::into_inner).wake_by_ref();
    }
}
