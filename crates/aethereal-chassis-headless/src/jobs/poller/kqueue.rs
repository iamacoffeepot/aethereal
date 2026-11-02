//! The poller over kqueue, on macOS: each interest of a socket armed as a filter of its own, reported once.

use super::{Interests, Report, Sleep};
use core::ffi::c_int;
use core::ptr;
use std::io;

/// How many reports one wait takes at the most: the rest wait for the next.
const REPORTS: usize = 64;

/// The ident of the user event that wakes a sleeping thread: user events have idents of their own, apart from sockets.
const NOTIFY: usize = 0;

/// A kqueue, with a user event registered on it that [`notify`](Self::notify) triggers.
pub(super) struct System {
    queue: c_int,
}

impl System {
    /// kqueue watches sockets.
    pub(super) const WATCHES_SOCKETS: bool = true;

    /// A kqueue, with its user event registered.
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: `kqueue` takes nothing, and answers a descriptor or -1.
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return Err(io::Error::last_os_error());
        }
        let system = Self { queue };
        let registered = change(NOTIFY, libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0);
        system.apply(&[registered])?;

        Ok(system)
    }

    /// Arms `socket` for exactly `wanted`, each interest reported once.
    pub(super) fn arm(&self, socket: c_int, wanted: Interests) -> io::Result<()> {
        let ident = usize::try_from(socket).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let filter = |filter: i16, wants: bool| {
            let flags = if wants {
                libc::EV_ADD | libc::EV_ONESHOT
            } else {
                libc::EV_DELETE
            };
            change(ident, filter, flags, 0)
        };
        let changes = [filter(libc::EVFILT_READ, wanted.readable), filter(libc::EVFILT_WRITE, wanted.writable)];

        self.apply(&changes)
    }

    /// Waits as `sleep` says, or until a socket armed is ready or [`notify`](Self::notify) is called, pushing each
    /// report onto `reports`.
    pub(super) fn wait(&self, sleep: Sleep, reports: &mut Vec<Report>) {
        let timeout = match sleep {
            Sleep::Forever => None,
            Sleep::Within(within) => Some(libc::timespec {
                tv_sec: libc::time_t::try_from(within.as_secs()).unwrap_or(libc::time_t::MAX),
                tv_nsec: libc::c_long::from(within.subsec_nanos()),
            }),
        };
        let timeout_at = timeout.as_ref().map_or(ptr::null(), ptr::from_ref);
        let mut events = [change(0, 0, 0, 0); REPORTS];
        let room = c_int::try_from(events.len()).unwrap_or(c_int::MAX);
        // SAFETY: the queue is open, no changes are given, `events` holds `room` entries, and `timeout_at` is null or
        // points at a timespec that outlives the call.
        let count = unsafe { libc::kevent(self.queue, ptr::null(), 0, events.as_mut_ptr(), room, timeout_at) };
        // An interrupted wait reports nothing, and the thread looks again.
        let count = usize::try_from(count).unwrap_or(0);
        for event in &events[..count] {
            let socket = c_int::try_from(event.ident).unwrap_or(c_int::MAX);
            match event.filter {
                libc::EVFILT_READ => reports.push(Report { socket, readable: true, writable: false }),
                libc::EVFILT_WRITE => reports.push(Report { socket, readable: false, writable: true }),
                _ => {}
            }
        }
    }

    /// Wakes the thread sleeping in [`wait`](Self::wait), or the next to wait: the user event stays triggered until a
    /// wait takes it.
    pub(super) fn notify(&self) {
        let triggered = change(NOTIFY, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER);
        // A queue that refuses its own user event has nothing left to wake.
        let _ = self.apply(&[triggered]);
    }

    /// Applies `changes`, at most two, each answered in a receipt of its own, so applying them takes no report a wait
    /// is owed: a deletion of what was not armed is no failure.
    fn apply(&self, changes: &[libc::kevent]) -> io::Result<()> {
        let mut receipted = [change(0, 0, 0, 0); 2];
        let room = changes.len().min(receipted.len());
        for (into, change) in receipted.iter_mut().zip(changes) {
            *into = libc::kevent { flags: change.flags | libc::EV_RECEIPT, ..*change };
        }
        let changes = &receipted[..room];
        let mut receipts = [change(0, 0, 0, 0); 2];
        let asked = c_int::try_from(room).unwrap_or(0);
        // SAFETY: the queue is open, `changes` holds at least `room` entries, `receipts` holds `room` at least, and
        // no timeout is waited on: changes with receipts return once applied.
        let count =
            unsafe { libc::kevent(self.queue, changes.as_ptr(), asked, receipts.as_mut_ptr(), asked, ptr::null()) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let count = usize::try_from(count).unwrap_or(0);
        let failed = receipts[..count]
            .iter()
            .filter(|receipt| receipt.flags & libc::EV_ERROR != 0)
            .map(|receipt| i32::try_from(receipt.data).unwrap_or(libc::EINVAL))
            .find(|&error| error != 0 && error != libc::ENOENT);

        failed.map_or(Ok(()), |error| Err(io::Error::from_raw_os_error(error)))
    }
}

impl Drop for System {
    fn drop(&mut self) {
        // SAFETY: the queue is open, and closed only here.
        let _ = unsafe { libc::close(self.queue) };
    }
}

/// A change to the queue: `flags` applied to the filter `filter` of `ident`, with the filter's own `note`.
const fn change(ident: usize, filter: i16, flags: u16, note: u32) -> libc::kevent {
    libc::kevent { ident, filter, flags, fflags: note, data: 0, udata: ptr::null_mut() }
}
