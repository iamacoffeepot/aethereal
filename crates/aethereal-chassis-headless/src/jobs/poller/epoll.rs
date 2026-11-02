//! The poller over epoll, on Linux: a socket armed once for every interest it is waited on for, and disarmed whole on
//! its first report, which is why the poller arms again what still waits on it.

use super::{Interests, Report, Sleep};
use core::ffi::c_int;
use core::ptr;
use std::io;

/// How many reports one wait takes at the most: the rest wait for the next.
const REPORTS: usize = 64;

/// What the eventfd that wakes a sleeping thread is registered under: no socket's descriptor is this.
const NOTIFY: u64 = u64::MAX;

/// An epoll instance, with an eventfd registered on it that [`notify`](Self::notify) writes to.
pub(super) struct System {
    epoll: c_int,
    notify: c_int,
}

impl System {
    /// epoll watches sockets.
    pub(super) const WATCHES_SOCKETS: bool = true;

    /// An epoll instance, with its eventfd registered.
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: `epoll_create1` takes flags alone, and answers a descriptor or -1.
        let epoll = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epoll < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `eventfd` takes a count and flags, and answers a descriptor or -1.
        let notify = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if notify < 0 {
            let error = io::Error::last_os_error();
            // SAFETY: the instance is open, and nothing else holds it.
            let _ = unsafe { libc::close(epoll) };
            return Err(error);
        }
        let system = Self { epoll, notify };
        let mut event = libc::epoll_event { events: libc::EPOLLIN.cast_unsigned(), u64: NOTIFY };
        // SAFETY: both descriptors are open, and `event` outlives the call.
        let added = unsafe { libc::epoll_ctl(epoll, libc::EPOLL_CTL_ADD, notify, &raw mut event) };
        if added < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(system)
    }

    /// Arms `socket` for exactly `wanted`, reported once: modified when it is registered, added when it is not, and
    /// removed when nothing is wanted.
    pub(super) fn arm(&self, socket: c_int, wanted: Interests) -> io::Result<()> {
        let is_wanted = wanted.readable || wanted.writable;
        if !is_wanted {
            // SAFETY: the instance is open; a socket not registered is refused, which is no failure here.
            let _ = unsafe { libc::epoll_ctl(self.epoll, libc::EPOLL_CTL_DEL, socket, ptr::null_mut()) };
            return Ok(());
        }
        let readable = if wanted.readable {
            libc::EPOLLIN | libc::EPOLLRDHUP
        } else {
            0
        };
        let writable = if wanted.writable {
            libc::EPOLLOUT
        } else {
            0
        };
        let events = (libc::EPOLLONESHOT | readable | writable).cast_unsigned();
        let data = u64::try_from(socket).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut event = libc::epoll_event { events, u64: data };
        // SAFETY: the instance is open, and `event` outlives the call.
        let modified = unsafe { libc::epoll_ctl(self.epoll, libc::EPOLL_CTL_MOD, socket, &raw mut event) };
        if modified == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        let is_unregistered = error.raw_os_error() == Some(libc::ENOENT);
        if !is_unregistered {
            return Err(error);
        }
        // SAFETY: as for the modification.
        let added = unsafe { libc::epoll_ctl(self.epoll, libc::EPOLL_CTL_ADD, socket, &raw mut event) };
        if added < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    /// Waits as `sleep` says, or until a socket armed is ready or [`notify`](Self::notify) is called, pushing each
    /// report onto `reports`.
    pub(super) fn wait(&self, sleep: Sleep, reports: &mut Vec<Report>) {
        let timeout = match sleep {
            Sleep::Forever => -1,
            // epoll counts whole milliseconds, so a part of one is waited as a whole one, and the thread never wakes
            // before its timer.
            Sleep::Within(within) => {
                let millis = within.as_nanos().div_ceil(1_000_000);
                c_int::try_from(millis).unwrap_or(c_int::MAX)
            }
        };
        let mut events = [libc::epoll_event { events: 0, u64: 0 }; REPORTS];
        let room = c_int::try_from(events.len()).unwrap_or(c_int::MAX);
        // SAFETY: the instance is open, and `events` holds `room` entries.
        let count = unsafe { libc::epoll_wait(self.epoll, events.as_mut_ptr(), room, timeout) };
        // An interrupted wait reports nothing, and the thread looks again.
        let count = usize::try_from(count).unwrap_or(0);
        for event in &events[..count] {
            let (flags, data) = (event.events, event.u64);
            if data == NOTIFY {
                self.drain();
                continue;
            }
            let closed = flags & (libc::EPOLLHUP | libc::EPOLLERR).cast_unsigned() != 0;
            let readable = closed || flags & (libc::EPOLLIN | libc::EPOLLRDHUP).cast_unsigned() != 0;
            let writable = closed || flags & libc::EPOLLOUT.cast_unsigned() != 0;
            let socket = c_int::try_from(data).unwrap_or(c_int::MAX);
            reports.push(Report { socket, readable, writable });
        }
    }

    /// Wakes the thread sleeping in [`wait`](Self::wait), or the next to wait: the eventfd stays readable until a wait
    /// drains it.
    pub(super) fn notify(&self) {
        let one: u64 = 1;
        // SAFETY: the eventfd is open, and `one` is eight bytes that outlive the call. A count already at its most
        // refuses the write, and wakes the thread all the same.
        let _ = unsafe { libc::write(self.notify, (&raw const one).cast(), size_of::<u64>()) };
    }

    /// Empties the eventfd, so the next wait sleeps.
    fn drain(&self) {
        let mut count: u64 = 0;
        // SAFETY: the eventfd is open and does not block, and `count` is eight bytes that outlive the call.
        let _ = unsafe { libc::read(self.notify, (&raw mut count).cast(), size_of::<u64>()) };
    }
}

impl Drop for System {
    fn drop(&mut self) {
        // SAFETY: both descriptors are open, and closed only here.
        let _ = unsafe { libc::close(self.notify) };
        // SAFETY: as for the eventfd.
        let _ = unsafe { libc::close(self.epoll) };
    }
}
