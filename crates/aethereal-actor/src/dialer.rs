//! The host's dialer, which a running job connects to another machine through, as it reads files through the host's
//! storage (ADR-0033).

use alloc::boxed::Box;
use core::future::{self, Future};
use core::pin::Pin;
use std::net::TcpStream;

/// Why a dial made no connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undialed {
    /// The host name found no address.
    Unresolved,
    /// Every address refused, or none answered in time.
    Refused,
    /// The host has no dialer.
    Unavailable,
}

/// A dial, as its dialer does it: the connection, set not to block, or why there is none.
pub type Dialing<'a> = Pin<Box<dyn Future<Output = Result<TcpStream, Undialed>> + 'a>>;

/// The host's dialer, which a running job connects through (ADR-0033): threads that look a name up and connect
/// natively, and none in a browser, which cannot open a socket. It is handed to every running job beside the host's
/// timer and storage.
///
/// A dial never blocks the thread that polls it: a host whose dials block does them on threads of its own, and wakes
/// the dial once it has connected or given up.
pub trait Dialer: Send + Sync {
    /// Connects to `port` on `host`, a name or an address written out, trying each address the name has in turn.
    fn dial<'a>(&'a self, host: &'a str, port: u16) -> Dialing<'a>;
}

/// The dialer of a host that cannot connect out: every dial is [`Undialed::Unavailable`].
pub struct NoDialer;

impl Dialer for NoDialer {
    fn dial<'a>(&'a self, _: &'a str, _: u16) -> Dialing<'a> {
        Box::pin(future::ready(Err(Undialed::Unavailable)))
    }
}
