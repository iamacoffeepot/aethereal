//! A resource through which an actor asks the host for things, which the host takes between steps (ADR-0019).

use crate::Resource;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use crossbeam_queue::SegQueue;

/// A resource through which the actor that owns it asks the host for things, requests of type `K`: what a capability
/// owns to ask its host system for a window, a device, or an audio output (ADR-0019).
///
/// The chassis makes one with the engine, beside the host's end of it,
/// [`Requests`], and hands it to a root in its config. Its owner opens it in a
/// turn and pushes a request; the host takes what was pushed between steps, on
/// its own thread, where it can make what was asked for, and brings back what
/// it made through a source (ADR-0010). Its requests are pushed onto a queue
/// no one waits on. A push acts on the world outside the turn's effects: one
/// pushed in a turn that then fails is taken all the same.
pub struct Controller<K> {
    asked: Arc<Asked<K>>,
}

/// The host's end of a controller: what it takes the requests from (ADR-0019).
pub struct Requests<K> {
    asked: Arc<Asked<K>>,
}

/// What a controller and its host's end share: the requests pushed and not yet taken, and whether the controller has
/// been dropped.
struct Asked<K> {
    requests: SegQueue<K>,
    dropped: AtomicBool,
}

impl<K> Controller<K> {
    /// A controller, and the host's end of it.
    #[must_use]
    pub fn new() -> (Self, Requests<K>) {
        let asked = Arc::new(Asked { requests: SegQueue::new(), dropped: AtomicBool::new(false) });

        (Self { asked: Arc::clone(&asked) }, Requests { asked })
    }

    /// Asks the host for `request`, which it takes at its next [`Requests::take`].
    pub fn push(&self, request: K) {
        self.asked.requests.push(request);
    }
}

impl<K: Send + 'static> Resource for Controller<K> {}

/// Once its owner drops it, the host is told: it stops what it made for it.
impl<K> Drop for Controller<K> {
    fn drop(&mut self) {
        self.asked.dropped.store(true, Ordering::Release);
    }
}

impl<K> Requests<K> {
    /// Takes the requests pushed before this began, in the order pushed: what is pushed while it takes waits for the
    /// next, so taking is bounded however fast the controller's owner pushes. What was pushed before the controller
    /// was dropped is taken all the same.
    #[must_use]
    pub fn take(&self) -> Vec<K> {
        let ready = self.asked.requests.len();

        (0..ready).map_while(|_| self.asked.requests.pop()).collect()
    }

    /// Whether the controller has been dropped, with its owner or with mail that did not arrive: nothing is pushed to
    /// it from then on.
    #[must_use]
    pub fn is_dropped(&self) -> bool {
        self.asked.dropped.load(Ordering::Acquire)
    }
}
