//! The native host's poller: what the thread that runs the jobs that wait sleeps in, woken by the sockets its jobs
//! watch, by its timer, or by another thread (ADR-0018, ADR-0033).
//!
//! The system's poller is kqueue on macOS and epoll on Linux, each written
//! over `libc`. A host with neither watches no socket, and sleeps on a lock.

#[cfg(target_os = "linux")]
mod epoll;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod kqueue;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
mod parked;
#[cfg(test)]
mod tests;

#[cfg(target_os = "linux")]
use epoll::System;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use kqueue::System;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
use parked::System;

use aethereal_tcp::{Interest, Readiness, Readying};
use core::ffi::c_int;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

/// The poller the thread that runs the jobs that wait sleeps in, which a host serves its jobs as their [`Readiness`]
/// (ADR-0034). Each wait for a socket is armed in the system's poller once, and forgotten once it has reported or is
/// dropped.
pub struct Poller {
    system: System,
    /// The waits armed, by the socket and what it is waited on for: at most one of each.
    watches: Mutex<BTreeMap<(c_int, Interest), Watch>>,
    /// The number the next wait takes, so a wait that took another's place is told apart from it.
    next: AtomicU64,
}

/// One wait armed: its number, and where it stands.
struct Watch {
    number: u64,
    state: Watching,
}

/// Where a wait armed stands.
enum Watching {
    /// Not yet reported: the waker of the job that waits.
    Waiting(Waker),
    /// Reported, and not yet seen by the job.
    Ready,
}

/// How long the thread sleeps in the poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sleep {
    /// Until a socket is ready, or it is woken.
    Forever,
    /// As long as this at the most.
    Within(Duration),
}

/// What a socket may be waited on for at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Interests {
    readable: bool,
    writable: bool,
}

/// What the system's poller reported of one socket.
#[derive(Debug, Clone, Copy)]
struct Report {
    socket: c_int,
    readable: bool,
    writable: bool,
}

impl Interests {
    /// Whether `interest` is among these.
    const fn holds(self, interest: Interest) -> bool {
        match interest {
            Interest::Readable => self.readable,
            Interest::Writable => self.writable,
        }
    }

    /// These and `interest`.
    const fn with(self, interest: Interest) -> Self {
        match interest {
            Interest::Readable => Self { readable: true, ..self },
            Interest::Writable => Self { writable: true, ..self },
        }
    }
}

impl Poller {
    /// Whether this host's system watches sockets: a host with no system poller serves no [`Readiness`].
    pub const WATCHES_SOCKETS: bool = System::WATCHES_SOCKETS;

    /// A poller with nothing armed.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make its system's poller, as when it cannot make a thread.
    #[must_use]
    pub fn new() -> Self {
        let Ok(system) = System::new() else {
            panic!("the host makes its poller");
        };

        Self { system, watches: Mutex::new(BTreeMap::new()), next: AtomicU64::new(0) }
    }

    /// Sleeps as `sleep` says, or until a socket armed is ready or [`notify`](Self::notify) is called, then wakes the
    /// job of each wait reported.
    pub(super) fn sleep(&self, sleep: Sleep) {
        let mut reports = Vec::new();
        self.system.wait(sleep, &mut reports);
        let mut watches = self.watches();
        for report in reports {
            let fired = Interests { readable: report.readable, writable: report.writable };
            for interest in [Interest::Readable, Interest::Writable] {
                let is_fired = fired.holds(interest);
                if !is_fired {
                    continue;
                }
                let Some(watch) = watches.get_mut(&(report.socket, interest)) else {
                    continue;
                };
                if let Watching::Waiting(waker) = &watch.state {
                    waker.wake_by_ref();
                }
                watch.state = Watching::Ready;
            }
            // A system that disarms the whole socket on any report arms again what still waits on it.
            self.arm(&watches, report.socket);
        }
        drop(watches);
    }

    /// Wakes the thread sleeping in the poller, or the next to sleep in it, from any thread.
    pub(super) fn notify(&self) {
        self.system.notify();
    }

    /// Arms `socket` for what still waits on it among `watches`, and nothing else.
    fn arm(&self, watches: &BTreeMap<(c_int, Interest), Watch>, socket: c_int) {
        // A socket that cannot be armed has closed, or was never one: its waits are reported, or dropped, as they are.
        let _ = self.system.arm(socket, waiting_on(watches, socket));
    }

    /// The waits armed, whatever a thread that panicked holding them left.
    fn watches(&self) -> MutexGuard<'_, BTreeMap<(c_int, Interest), Watch>> {
        self.watches.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What `socket` is still waited on for among `watches`: its waits not yet reported.
fn waiting_on(watches: &BTreeMap<(c_int, Interest), Watch>, socket: c_int) -> Interests {
    [Interest::Readable, Interest::Writable]
        .into_iter()
        .filter(|interest| watches.get(&(socket, *interest)).is_some_and(Watch::waits))
        .fold(Interests::default(), Interests::with)
}

impl Watch {
    /// Whether it has not yet been reported.
    const fn waits(&self) -> bool {
        matches!(self.state, Watching::Waiting(_))
    }
}

impl Default for Poller {
    fn default() -> Self {
        Self::new()
    }
}

impl Readiness for Poller {
    fn ready(&self, socket: u64, interest: Interest) -> Readying<'_> {
        // A number no descriptor has is ready at once: the socket call made then says what it is.
        let stage = c_int::try_from(socket).map_or(Stage::Done, |socket| Stage::Unarmed { socket, interest });

        Box::pin(Ready { poller: self, stage })
    }
}

/// A wait for a socket, as the job that waits polls it.
struct Ready<'a> {
    poller: &'a Poller,
    stage: Stage,
}

/// Where a wait for a socket stands, as its job sees it.
#[derive(Clone, Copy)]
enum Stage {
    /// Not yet polled: nothing is armed.
    Unarmed { socket: c_int, interest: Interest },
    /// Armed under this number.
    Armed { socket: c_int, interest: Interest, number: u64 },
    /// Reported, or never armed: ready from now on.
    Done,
}

impl Future for Ready<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        let poller = self.poller;
        let mut watches = poller.watches();
        let stage = match self.stage {
            Stage::Done => return Poll::Ready(()),
            Stage::Unarmed { socket, interest } => {
                let number = poller.next.fetch_add(1, Ordering::Relaxed);
                let watch = Watch { number, state: Watching::Waiting(context.waker().clone()) };
                watches.insert((socket, interest), watch);
                let armed = poller.system.arm(socket, waiting_on(&watches, socket));
                if armed.is_err() {
                    watches.remove(&(socket, interest));
                    Stage::Done
                } else {
                    Stage::Armed { socket, interest, number }
                }
            }
            Stage::Armed { socket, interest, number } => match watches.get_mut(&(socket, interest)) {
                Some(watch) if watch.number == number => match watch.state {
                    Watching::Ready => {
                        watches.remove(&(socket, interest));
                        Stage::Done
                    }
                    Watching::Waiting(_) => {
                        watch.state = Watching::Waiting(context.waker().clone());
                        Stage::Armed { socket, interest, number }
                    }
                },
                // Another wait took its place: it is reported as ready, and its socket call says whether it was.
                Some(_) | None => Stage::Done,
            },
        };
        drop(watches);
        self.stage = stage;

        match stage {
            Stage::Done => Poll::Ready(()),
            Stage::Unarmed { .. } | Stage::Armed { .. } => Poll::Pending,
        }
    }
}

impl Drop for Ready<'_> {
    /// A wait dropped before it was reported is forgotten, and its socket armed for what still waits on it.
    fn drop(&mut self) {
        let Stage::Armed { socket, interest, number } = self.stage else {
            return;
        };
        let mut watches = self.poller.watches();
        let is_own = watches.get(&(socket, interest)).is_some_and(|watch| watch.number == number);
        if is_own {
            watches.remove(&(socket, interest));
            self.poller.arm(&watches, socket);
        }
        drop(watches);
    }
}
