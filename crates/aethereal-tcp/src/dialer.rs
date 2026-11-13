//! The host's dialer, which the capability's dials connect through: a service the host serves its jobs (ADR-0033,
//! ADR-0034).

use std::future::Future;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;
use std::pin::Pin;

/// Why a dial made no connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undialed {
    /// The host name found no address, or no socket is at the path.
    Unresolved,
    /// Every address refused, or none answered in time; or nothing listens on the socket at the path, or the host
    /// refused the connect.
    Refused,
}

/// A dial, as its dialer does it: the connection, set not to block, or why there is none.
pub type Dialing<'a> = Pin<Box<dyn Future<Output = Result<TcpStream, Undialed>> + 'a>>;

/// A dial to a Unix socket, as its dialer does it: the connection, set not to block, or why there is none.
#[cfg(unix)]
pub type UnixDialing<'a> = Pin<Box<dyn Future<Output = Result<UnixStream, Undialed>> + 'a>>;

/// The host's dialer (ADR-0033): threads that look a name up and connect natively, over TCP or to a Unix socket. A host
/// serves it into the services its runners hand every job (ADR-0034); a host that serves none, as a browser, which
/// cannot open a socket, leaves every dial unavailable.
///
/// A dial never blocks the thread that polls it: a dialer whose dials block does them on threads of its own, and wakes
/// the dial once it has connected or given up.
pub trait Dialer: Send + Sync {
    /// Connects to `port` on `host`, a name or an address written out, trying each address the name has in turn.
    fn dial<'a>(&'a self, host: &'a str, port: u16) -> Dialing<'a>;

    /// Connects to the Unix socket at `path`, which the root has checked is absolute and fits a socket's address. A
    /// connect to a listener whose backlog is full waits for room, so it blocks as a dial over TCP does.
    #[cfg(unix)]
    fn dial_unix<'a>(&'a self, path: &'a Path) -> UnixDialing<'a>;
}
