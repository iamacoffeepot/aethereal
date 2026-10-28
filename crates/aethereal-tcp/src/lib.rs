//! The TCP capability (ADR-0033): one root, [`Tcp`], whose actors own the sockets.
//!
//! An actor asks the root to [`Listen`] or to [`Connect`], at its typed path,
//! `path::<Tcp>(())`. A listener is a [`Listener`] beneath the root, which
//! hands out the connections it accepts as [`Accepted`] actors beneath it,
//! through an [`Accepting`] stream; a connection dialed out is a [`Dialed`]
//! actor beneath the root. Every connection is read through a [`Reading`]
//! stream, whose credit holds the socket back, and written by asking it to
//! [`Write`]. Each one polls its socket, which never blocks, on every `Tick`.
//!
//! The headless and desktop hosts' crates bootstrap the root, and serve the
//! host's [`Dialer`] into the services its runners hand every job (ADR-0034).

mod address;
mod connection;
mod dial;
mod dialer;
mod framing;
mod listener;
mod settings;
mod socket;
mod tcp;

pub use address::SocketAddress;
pub use connection::{Accepted, ConnectionConfig, Dialed, Frame, HungUp, Read, Reading, Shut, Write, Writing, Wrote};
pub use dialer::{Dialer, Dialing, Undialed};
pub use framing::Framing;
pub use listener::{Accept, Accepting, Incoming, Listener, ListenerConfig, Unbound};
pub use settings::TcpSettings;
pub use socket::{BoundSocket, Socket};
pub use tcp::{Connect, Connected, Dialing as DialingContext, Listen, Listening, Tcp, TcpConfig};
