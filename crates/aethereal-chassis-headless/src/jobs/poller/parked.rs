//! The poller on a host with no system poller: it watches no socket, and sleeps on a lock until it is woken or its time
//! has passed.

use super::{Interests, Report, Sleep};
use core::ffi::c_int;
use std::io;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// Whether the poller has been woken since it last slept, and what a sleeping thread waits on.
pub(super) struct System {
    woken: Mutex<bool>,
    waking: Condvar,
}

impl System {
    /// A lock watches no socket.
    pub(super) const WATCHES_SOCKETS: bool = false;

    /// A poller not yet woken.
    #[expect(clippy::unnecessary_wraps, reason = "every system's poller is made the same way, and the others can fail")]
    pub(super) const fn new() -> io::Result<Self> {
        Ok(Self { woken: Mutex::new(false), waking: Condvar::new() })
    }

    /// Refuses every socket: this host watches none.
    #[expect(
        clippy::unused_self,
        reason = "every system's poller arms a socket the same way, and the others use theirs"
    )]
    pub(super) fn arm(&self, _: c_int, _: Interests) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Waits as `sleep` says, or until [`notify`](Self::notify) is called. No socket is reported.
    pub(super) fn wait(&self, sleep: Sleep, _: &mut Vec<Report>) {
        let woken = self.woken();
        let mut woken = match sleep {
            Sleep::Forever => self.waking.wait_while(woken, |woken| !*woken).unwrap_or_else(PoisonError::into_inner),
            Sleep::Within(within) => {
                let waited = self.waking.wait_timeout_while(woken, within, |woken| !*woken);
                waited.unwrap_or_else(PoisonError::into_inner).0
            }
        };
        *woken = false;
    }

    /// Wakes the thread sleeping in [`wait`](Self::wait), or the next to wait.
    pub(super) fn notify(&self) {
        *self.woken() = true;
        self.waking.notify_one();
    }

    /// Whether it has been woken, whatever a thread that panicked holding the lock left.
    fn woken(&self) -> MutexGuard<'_, bool> {
        self.woken.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
