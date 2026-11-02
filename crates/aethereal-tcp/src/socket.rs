//! The sockets the capability's actors own (ADR-0010, ADR-0033).

use aethereal_actor::Resource;
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::fd::AsRawFd;

/// A connection's socket, set not to block: owned by the connection actor that reads and writes it.
pub struct Socket(pub TcpStream);

impl Resource for Socket {}

/// A listener's socket, bound and set not to block: owned by the listener that accepts on it.
pub struct BoundSocket(pub TcpListener);

impl Resource for BoundSocket {}

impl Socket {
    /// Its descriptor, as a [`Ready`](crate::ready::Ready) names it.
    pub(crate) fn descriptor(&self) -> u64 {
        descriptor(&self.0)
    }
}

impl BoundSocket {
    /// Its descriptor, as a [`Ready`](crate::ready::Ready) names it.
    pub(crate) fn descriptor(&self) -> u64 {
        descriptor(&self.0)
    }
}

/// The descriptor of `socket`: on a host whose sockets are file descriptors, its own.
#[cfg(unix)]
fn descriptor(socket: &impl AsRawFd) -> u64 {
    // A descriptor is never negative; one that were would name no socket, and be waited on as none.
    u64::try_from(socket.as_raw_fd()).unwrap_or(u64::MAX)
}

/// The descriptor of `socket`, on a host whose sockets are not file descriptors: none a poller knows, so a wait for it
/// is ready at once, and the host serves no poller to wait in anyway.
#[cfg(not(unix))]
const fn descriptor<T>(_: &T) -> u64 {
    u64::MAX
}
