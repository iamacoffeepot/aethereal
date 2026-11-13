//! The TCP capability (ADR-0033): one root, [`Tcp`], whose actors own the sockets.
//!
//! An actor asks the root to [`Listen`], to [`Connect`], or to [`ConnectUnix`]
//! to a socket on this machine, at its typed path, `path::<Tcp>(())`. A
//! listener is a [`Listener`] beneath the root, which hands out the
//! connections it accepts as [`Accepted`] actors beneath it, through an
//! [`Accepting`] stream; a connection dialed out, over TCP or to a Unix
//! socket, is a [`Dialed`] actor beneath the root. Every connection is read
//! through a [`Reading`] stream, whose credit holds the socket back, and
//! written by asking it to [`Write`], whichever kind of socket it holds. Each
//! makes each call on its socket, which never blocks, in the handler that
//! makes it possible, and waits for the socket through the host's poller
//! when a call would wait.
//!
//! The headless and desktop hosts' crates bootstrap the root, and serve the
//! host's [`Dialer`] and [`Readiness`] into the services its runners hand
//! every job (ADR-0034).

mod address;
mod connection;
mod dial;
mod dialer;
mod framing;
mod listener;
mod readiness;
mod ready;
mod settings;
mod socket;
mod tcp;

pub use address::{SocketAddress, SocketPath, Unaddressable};
pub use connection::{Accepted, ConnectionConfig, Dialed, Frame, HungUp, Read, Reading, Shut, Write, Writing, Wrote};
#[cfg(unix)]
pub use dialer::UnixDialing;
pub use dialer::{Dialer, Dialing, Undialed};
pub use framing::Framing;
pub use listener::{Accept, Accepting, Incoming, Listener, ListenerConfig, Unbound};
pub use readiness::{Interest, Readiness, Readying};
pub use ready::Readied;
pub use settings::TcpSettings;
pub use socket::{BoundSocket, Socket};
pub use tcp::{Connect, ConnectUnix, Connected, ConnectedUnix, Listen, Listening, Tcp, TcpConfig};
