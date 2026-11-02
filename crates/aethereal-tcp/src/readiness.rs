//! The host's poller, which says when a socket the capability's actors own is ready: a service the host serves its jobs
//! (ADR-0033, ADR-0034).

use aethereal_data::Plain;
use std::future::Future;
use std::pin::Pin;

/// What a socket is waited on for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Plain)]
#[kind(name = "aether.tcp.interest")]
pub enum Interest {
    /// It may be read, or accepted on: bytes have arrived, a connection waits, or the peer has closed.
    Readable,
    /// It may be written: the kernel has room for more.
    Writable,
}

/// A wait for a socket, as its poller does it: ready once the socket is.
pub type Readying<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// The host's poller (ADR-0033): a native host waits in its system's (kqueue, epoll) on the thread that runs the jobs
/// that wait. A host serves it into the services its runners hand every job (ADR-0034); a host that serves none leaves
/// its sockets unwatched.
///
/// A wait never blocks the thread that polls it. It reports once, and may report a socket that is not ready after all:
/// the call made then answers `WouldBlock`, and the actor waits again, as it does when a socket's number has been given
/// to another since the wait began. Dropping a wait forgets it.
pub trait Readiness: Send + Sync {
    /// Ready once the socket whose descriptor is `socket` is ready for `interest`.
    fn ready(&self, socket: u64, interest: Interest) -> Readying<'_>;
}
